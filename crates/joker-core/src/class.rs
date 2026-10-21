//! Class methods and queries in the core (docs/17 §4, ADR 0041): method
//! records expanded in step 5a into ordinary proposals of the caller, and
//! derived signals (class queries, `calc`) evaluated in step 3.
//!
//! Methods are compiled per instance when the generation is built, so an
//! assignment targets a `SignalId` and a nested call targets an
//! (instance, method) pair. Method arguments are read by the expressions as
//! the pseudo-signals `ARG_SIGNAL_BASE + i`.

use joker_expr::{Inputs, Program, Reading};

use crate::commit::{CommitError, CommitSink, Outputs, TickInputs};
use crate::image::{MAX_METHOD_ARGS, MethodCall, ProcessImage, Proposal, WriterId};
use crate::quality::{Quality, QualityCode, QualityReason};
use crate::registry::{GroupId, SignalId};
use crate::slot::Slot;
use crate::value::NO_TS_SRC;

/// Argument `i` of a method is read by its expressions as signal
/// `ARG_SIGNAL_BASE + i`.
pub const ARG_SIGNAL_BASE: u32 = 0xFFFF_FF00;
/// Deepest chain of nested calls (docs/17 §8).
pub const MAX_CALL_DEPTH: usize = 3;

/// One statement of a method body (docs/17 §3.2).
#[derive(Copy, Clone, Debug)]
pub enum Stmt<'a> {
    /// `out = expr`.
    Assign {
        /// The `out` member's signal.
        target: SignalId,
        /// The value.
        expr: &'a Program,
    },
    /// `member.method(args)` on a fixed class member.
    Call {
        /// The member's instance.
        instance: u32,
        /// The method.
        method: u16,
        /// The arguments, evaluated in the caller's context.
        args: &'a [&'a Program],
    },
}

/// A method of one instance.
#[derive(Copy, Clone, Debug)]
pub struct MethodDef<'a> {
    /// Number of parameters.
    pub params: u8,
    /// The body.
    pub stmts: &'a [Stmt<'a>],
}

/// The methods of one instance, indexed by method number.
#[derive(Copy, Clone, Debug)]
pub struct InstanceDef<'a> {
    /// The methods.
    pub methods: &'a [MethodDef<'a>],
}

/// A derived signal: a class query or a `calc` (ADR 0041 §5).
#[derive(Copy, Clone, Debug)]
pub struct DerivedDef<'a> {
    /// The signal it publishes.
    pub signal: SignalId,
    /// The group that owns and evaluates it.
    pub group: GroupId,
    /// The expression.
    pub expr: &'a Program,
}

/// The result of one method record.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MethodResult {
    /// The record.
    pub call: MethodCall,
    /// Writes it expanded to.
    pub writes: u16,
    /// Why it expanded to nothing (`J-E0504`, `J-E0505`, `J-E0907`).
    pub error: Option<&'static str>,
}

/// The classes of a generation: instance methods and derived signals.
#[derive(Debug)]
pub struct Classes<'a> {
    instances: &'a [InstanceDef<'a>],
    derived: &'a [DerivedDef<'a>],
    scratch: &'a mut [Proposal],
    arena: &'a mut [u64],
    derived_words: usize,
    method_words: usize,
}

impl<'a> Classes<'a> {
    /// Classes over caller-provided definitions; `scratch` bounds the writes
    /// that the method records of one tick may expand to.
    pub fn new(
        instances: &'a [InstanceDef<'a>],
        derived: &'a [DerivedDef<'a>],
        scratch: &'a mut [Proposal],
    ) -> Self {
        let derived_words = derived.iter().map(|d| d.expr.state_words()).sum();
        let method_words = instances
            .iter()
            .flat_map(|i| i.methods)
            .flat_map(|m| m.stmts)
            .map(|s| match s {
                Stmt::Assign { expr, .. } => expr.state_words(),
                Stmt::Call { args, .. } => args.iter().map(|p| p.state_words()).max().unwrap_or(0),
            })
            .max()
            .unwrap_or(0);
        Self {
            instances,
            derived,
            scratch,
            arena: &mut [],
            derived_words,
            method_words,
        }
    }

    /// Words of state arena needed: the derived signals' state, plus one
    /// scratch area for method expressions, zeroed for each evaluation.
    pub fn state_words(&self) -> usize {
        self.derived_words + self.method_words
    }

    /// Hands in the state arena and zeroes it.
    pub fn set_state_arena(&mut self, arena: &'a mut [u64]) -> Result<(), CommitError> {
        if arena.len() < self.state_words() {
            return Err(CommitError::StateArenaTooSmall);
        }
        arena.fill(0);
        self.arena = arena;
        Ok(())
    }

    /// Step 3: evaluates the derived signals owned by the image's group and
    /// writes their slots, with the expression's quality.
    pub fn evaluate_derived(
        &mut self,
        image: &ProcessImage<'_>,
        slots: &[Slot],
    ) -> Result<(), CommitError> {
        let tick = image.tick().ok_or(CommitError::NoTick)?;
        if self.arena.len() < self.state_words() {
            return Err(CommitError::StateArenaTooSmall);
        }
        let inputs = TickInputs::new(image, slots);
        let mut offset = 0;
        for d in self.derived {
            let words = d.expr.state_words();
            let state = &mut self.arena[offset..offset + words];
            offset += words;
            if d.group != image.group() {
                continue;
            }
            let (raw, quality) = match joker_expr::eval(d.expr, &inputs, tick.at.mono_ns, state) {
                Ok(o) => (
                    o.get_uncertain().map_or(0, |v| v.to_raw()),
                    Quality::from_bits(o.quality().to_bits()),
                ),
                Err(_) => (
                    0,
                    Quality::new(QualityCode::Bad, QualityReason::ConfigError),
                ),
            };
            slots
                .get(d.signal.index())
                .ok_or(CommitError::MissingSlot(d.signal))?
                .write(raw, quality, NO_TS_SRC, tick.at.mono_ns);
        }
        Ok(())
    }

    /// Step 5a: expands the image's method records, in staging order, into
    /// writes of their callers appended after the direct proposals
    /// (ADR 0041). A call that cannot expand fully expands to nothing, and
    /// every call is reported to `sink`.
    pub fn expand(
        &mut self,
        image: &mut ProcessImage<'_>,
        slots: &[Slot],
        outputs: &Outputs<'_>,
        sink: &mut impl CommitSink,
    ) -> Result<(), CommitError> {
        let tick = image.tick().ok_or(CommitError::NoTick)?;
        if self.arena.len() < self.state_words() {
            return Err(CommitError::StateArenaTooSmall);
        }
        let produced = {
            let view: &ProcessImage<'_> = image;
            let inputs = TickInputs::new(view, slots);
            let scratch_state = &mut self.arena[self.derived_words..];
            let mut ctx = Expansion {
                instances: self.instances,
                inputs: &inputs,
                direct: view.proposals(),
                outputs,
                now: tick.at.mono_ns,
                scratch: &mut *self.scratch,
                len: 0,
                state: scratch_state,
            };
            for call in view.method_calls() {
                let start = ctx.len;
                let result = ctx.call(
                    call.writer,
                    call,
                    call.instance,
                    call.method,
                    call.args(),
                    0,
                );
                if result.is_err() {
                    ctx.len = start;
                }
                sink.method(&MethodResult {
                    call: *call,
                    writes: (ctx.len - start) as u16,
                    error: result.err(),
                });
            }
            ctx.len
        };
        for proposal in &self.scratch[..produced] {
            image
                .push_expanded(*proposal)
                .map_err(|_| CommitError::IntakeBufferTooSmall)?;
        }
        Ok(())
    }
}

struct Expansion<'x, 'i> {
    instances: &'x [InstanceDef<'x>],
    inputs: &'x TickInputs<'i>,
    direct: &'x [Proposal],
    outputs: &'x Outputs<'x>,
    now: u64,
    scratch: &'x mut [Proposal],
    len: usize,
    state: &'x mut [u64],
}

impl Expansion<'_, '_> {
    fn call(
        &mut self,
        writer: WriterId,
        record: &MethodCall,
        instance: u32,
        method: u16,
        args: &[u64],
        depth: usize,
    ) -> Result<(), &'static str> {
        let def = self
            .instances
            .get(instance as usize)
            .ok_or("J-E0907")?
            .methods
            .get(usize::from(method))
            .ok_or("J-E0505")?;
        if args.len() != usize::from(def.params) || depth >= MAX_CALL_DEPTH {
            return Err("J-E0504");
        }
        for stmt in def.stmts {
            match *stmt {
                Stmt::Assign { target, expr } => {
                    let value = self.eval(writer, expr, args)?;
                    let config = self.outputs.config(target).ok_or("J-E0504")?;
                    if !config.accepts(value) {
                        return Err("J-E0504");
                    }
                    let slot = self.scratch.get_mut(self.len).ok_or("J-E0504")?;
                    *slot = Proposal {
                        writer,
                        signal: target,
                        value,
                        source: record.source,
                        ticket: record.ticket,
                        task: record.task,
                        ..Proposal::EMPTY
                    };
                    self.len += 1;
                }
                Stmt::Call {
                    instance,
                    method,
                    args: programs,
                } => {
                    if programs.len() > MAX_METHOD_ARGS {
                        return Err("J-E0504");
                    }
                    let mut values = [0u64; MAX_METHOD_ARGS];
                    for (slot, program) in values.iter_mut().zip(programs) {
                        *slot = self.eval(writer, program, args)?;
                    }
                    self.call(
                        writer,
                        record,
                        instance,
                        method,
                        &values[..programs.len()],
                        depth + 1,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn eval(
        &mut self,
        writer: WriterId,
        program: &Program,
        args: &[u64],
    ) -> Result<u64, &'static str> {
        let words = program.state_words();
        let state = self.state.get_mut(..words).ok_or("J-E0504")?;
        state.fill(0);
        let inputs = MethodInputs {
            base: self.inputs,
            args,
            writer,
            earlier: &self.scratch[..self.len],
            direct: self.direct,
            outputs: self.outputs,
            now: self.now,
        };
        let out = joker_expr::eval(program, &inputs, self.now, state).map_err(|_| "J-E0504")?;
        out.get_uncertain()
            .map(|v| v.to_raw())
            .map_err(|_| "J-E0504")
    }
}

struct MethodInputs<'x, 'i> {
    base: &'x TickInputs<'i>,
    args: &'x [u64],
    writer: WriterId,
    earlier: &'x [Proposal],
    direct: &'x [Proposal],
    outputs: &'x Outputs<'x>,
    now: u64,
}

impl Inputs for MethodInputs<'_, '_> {
    fn read(&self, signal: u32) -> Reading {
        if signal >= ARG_SIGNAL_BASE {
            let i = (signal - ARG_SIGNAL_BASE) as usize;
            return match self.args.get(i) {
                Some(&raw) => Reading {
                    raw,
                    quality: joker_expr::Quality::from_bits(Quality::GOOD.to_bits()),
                    acquired: self.now,
                },
                None => missing(),
            };
        }
        self.base.read(signal)
    }

    fn own(&self, signal: u32) -> Reading {
        let id = SignalId(signal);
        let last = |list: &[Proposal]| {
            list.iter()
                .rev()
                .find(|p| p.writer == self.writer && p.signal == id && !p.is_relinquish())
                .map(|p| p.value)
        };
        match last(self.earlier)
            .or_else(|| last(self.direct))
            .or_else(|| self.outputs.own_value(self.writer, id))
        {
            Some(raw) => Reading {
                raw,
                quality: joker_expr::Quality::from_bits(Quality::GOOD.to_bits()),
                acquired: self.now,
            },
            None => missing(),
        }
    }
}

fn missing() -> Reading {
    Reading {
        raw: 0,
        quality: joker_expr::Quality::from_bits(
            Quality::new(QualityCode::Bad, QualityReason::NotConnected).to_bits(),
        ),
        acquired: 0,
    }
}
