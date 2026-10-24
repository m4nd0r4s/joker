//! Binds `joker_expr` (parse/check/compile/eval, already built and
//! `no_std`) to a jokerd [`Plan`]'s own `SignalId` space, instead of
//! `joker-manifest::expr::Registry`'s separate, independent
//! enumeration (AUR-416).
//!
//! `joker-manifest::expr::Registry` assigns its own `u32` ids by
//! enumerating the full path registry (including aliases and `param`
//! signals) — fine for `jk check`'s own type-checking, where no id
//! needs to survive past that one pass, but wrong to reuse for a
//! `Program` that must later read `Shared.slots`: that buffer is
//! indexed by the real, canonical `SignalId` `Plan::new` resolves
//! through `joker_core::registry::Registry` (`plan.tables()?.registry`,
//! committed to by `registry_hash`), which skips aliases/params and is
//! built independently. [`PlanEnv`] closes that gap by sourcing ids
//! from `Plan` itself, so a `Program` compiled against it is
//! guaranteed to read the signal `Shared.slots` actually has it at.
//!
//! Scalar signal types resolve (`Bool`/`I64`/`U64`/`F64`/`F32`/`Bits`),
//! and so do enums (ADR 0060): an enum signal and an `Enum.variant`
//! constant resolve by the enum's declared name and the variant's
//! position in its declared list, exactly as `joker-manifest` resolves
//! them, with `AlarmState` always known. `String`/`Bytes`/`Record` never
//! appear in an expression (ref/01 §3.7: the language is scalar-only).

use std::collections::BTreeMap;

use joker_core::ValueType;
use joker_expr::{
    CheckOptions, CompileOptions, Diagnostic, EnumId, Env, EvalError, Inputs, Output, Program,
    Quality, Reading, SignalInfo, SignalType, check, compile, eval,
};
use joker_manifest::expr::{ALARM_STATE, ALARM_STATES};
use joker_units::Duration;

use crate::plan::Plan;
use crate::shared::Shared;

fn signal_type(ty: ValueType) -> Option<SignalType> {
    match ty {
        ValueType::Bool => Some(SignalType::Bool),
        ValueType::I64 => Some(SignalType::I64),
        ValueType::U64 => Some(SignalType::U64),
        ValueType::F64 => Some(SignalType::F64),
        ValueType::F32 => Some(SignalType::F32),
        ValueType::Bits => Some(SignalType::Bits),
        ValueType::Enum | ValueType::String | ValueType::Bytes | ValueType::Record => None,
    }
}

/// `joker_expr::Env` over a jokerd [`Plan`], so a compiled `Program`'s
/// `Input.signal` is exactly the `SignalId` `Shared.slots` is indexed
/// by (see the module doc).
#[derive(Debug)]
pub struct PlanEnv<'a> {
    plan: &'a Plan,
    enums: BTreeMap<&'a str, (u32, Vec<&'a str>)>,
}

impl<'a> PlanEnv<'a> {
    /// An `Env` over `plan`'s own signals and enums. `AlarmState` is
    /// enum 0; the others follow in name order, so the ids are the same
    /// for the same plan.
    pub fn new(plan: &'a Plan) -> Self {
        let mut enums: BTreeMap<&'a str, (u32, Vec<&'a str>)> = BTreeMap::new();
        enums.insert(ALARM_STATE, (0, ALARM_STATES.to_vec()));
        let mut named: Vec<(&'a str, &'a [String])> = plan
            .signals
            .iter()
            .filter_map(|s| Some((s.enum_name.as_deref()?, s.variants.as_slice())))
            .collect();
        named.sort_by_key(|(name, _)| *name);
        for (name, variants) in named {
            let next = u32::try_from(enums.len()).unwrap_or(u32::MAX);
            enums
                .entry(name)
                .or_insert_with(|| (next, variants.iter().map(String::as_str).collect()));
        }
        Self { plan, enums }
    }
}

impl Env for PlanEnv<'_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        let id = self.plan.id(path)?;
        let canonical = self.plan.path(id)?;
        let s = self.plan.signals.iter().find(|s| s.path == canonical)?;
        let ty = match &s.enum_name {
            Some(name) if s.ty == ValueType::Enum => {
                SignalType::Enum(EnumId(self.enums.get(name.as_str())?.0))
            }
            _ => signal_type(s.ty)?,
        };
        let unit = (s.unit != "1")
            .then(|| joker_units::Unit::parse(&s.unit).ok())
            .flatten();
        Some(SignalInfo {
            id: u32::try_from(id.index()).ok()?,
            ty,
            unit,
        })
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        let (id, variants) = self.enums.get(enum_name)?;
        let index = variants.iter().position(|v| *v == variant)?;
        Some((EnumId(*id), u64::try_from(index).ok()?))
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        let mut paths: Vec<&str> = self
            .plan
            .signals
            .iter()
            .map(|s| s.path.as_str())
            .filter(|p| crate::wire::matches(selector, p))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        for path in paths {
            if let Some(info) = self.signal(path) {
                found(info);
            }
        }
    }
}

/// Parses, type-checks (expecting a boolean result) and compiles
/// `source` against `plan`'s own signals (AUR-416). `period` is the
/// compiled program's window-sizing period (ADR 0034); callers outside
/// a cycle group (e.g. an action's `permit`) that have no single group
/// period of their own pass a representative one — this is a known,
/// separate open question (AUR-417), not resolved here.
#[allow(
    clippy::result_large_err,
    reason = "joker_expr::Diagnostic is already allowed large within its own crate (crates/joker-expr/src/lib.rs); a diagnostic is a cold path, not something to box just for this"
)]
pub fn compile_bool(source: &str, plan: &Plan, period: Duration) -> Result<Program, Diagnostic> {
    let env = PlanEnv::new(plan);
    let checked = check(source, &env, CheckOptions::default())?;
    checked.expect_bool()?;
    compile(&checked, CompileOptions { period })
}

/// Reads `Shared.slots` by the `SignalId` [`PlanEnv`] assigned.
struct SlotInputs<'a> {
    shared: &'a Shared,
    now: u64,
}

impl Inputs for SlotInputs<'_> {
    fn read(&self, signal: u32) -> Reading {
        let bad = Reading {
            raw: 0,
            quality: Quality::new(joker_expr::QualityCode::Bad, joker_expr::Reason::NONE),
            acquired: self.now,
        };
        let Some(slot) = self.shared.slots.get(signal as usize) else {
            return bad;
        };
        match slot.read(8) {
            Ok(sample) => Reading {
                raw: sample.transport_bits(),
                quality: Quality::from_bits(sample.quality().to_bits()),
                acquired: sample.ts_acq(),
            },
            Err(_) => bad,
        }
    }
}

/// Evaluates `program` (compiled by [`compile_bool`]) against
/// `shared.slots` at `now` (monotonic ns), continuing from `state`: the
/// program's arena for windows, `held`, `rose`, `fell` and `hyst` (ADR
/// 0034). The caller keeps `state` between evaluations of the same
/// program (ADR 0060); an empty or wrongly sized `state` is reset to the
/// program's initial state first.
pub fn eval_bool(
    program: &Program,
    shared: &Shared,
    now: u64,
    state: &mut Vec<u64>,
) -> Result<Output, EvalError> {
    if state.len() != program.state_words() {
        state.clear();
        state.resize(program.state_words(), 0);
    }
    let inputs = SlotInputs { shared, now };
    eval(program, &inputs, now, state)
}
