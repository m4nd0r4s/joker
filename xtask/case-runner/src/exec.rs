//! Runs one case against `joker-core` with a virtual clock and hands the
//! observer, for every tick the case covers, the result of every output,
//! the latched sample of every input, the sample of every derived signal
//! and commit register, and the state of every alarm (ref/03 §2.6.4).
//!
//! The initial state (clock `synced`), the step where each writer kind's
//! proposals enter, the order of the actions at a tick boundary and the
//! generation swap follow ADR 0042. Inside a tick, the derived signals and
//! then the alarms are evaluated in step 3, after the async proposals of
//! step 2 and before the cycle proposals of step 4 (docs/16 §8, ADR 0040,
//! ADR 0041 §5); a new generation starts its expressions and alarms from
//! their initial state.
//!
//! All the storage of the core lives on the stack, in arrays whose lengths
//! are the const parameters of [`run`]: `S` signals (inputs, outputs,
//! derived, alarm and commit signals), `O` outputs, `W` writers and `P`
//! staged proposals per tick. The compiled expressions and the state arena
//! are slices the caller provides, sized with [`needs`]. Each tick is split
//! in two: the core part (inputs into slots, latch, step 3, staging,
//! commit), which runs inside the [`TickGuard`], and the copy of the results
//! into the [`TickView`].

use joker_core::alarm::{AlarmConfig, AlarmRuntime, Alarms, Severity};
use joker_core::class::{Classes, DerivedDef, InstanceDef, MethodDef, Stmt};
use joker_core::commit::{CommitError, OutputState};
use joker_core::image::{ImageStorage, MethodCall, RELINQUISH, Source};
use joker_core::output::{AtomicGroup, Bound, Budget, OnRate, OutputConfig};
use joker_core::quality::{Quality, QualityCode, QualityReason};
use joker_core::registry::{Alias, Direction, GroupId, GroupMeta, Registry, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::{Clock, ClockQuality, Instant, VirtualClock};
use joker_core::value::NO_TS_SRC;
use joker_core::{
    Applied, CommitSink, IntakeSlot, Level, Outcome, OutputResult, Outputs, OutputsStorage,
    ProcessImage, Proposal, RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType,
    WriterConfig, WriterId, WriterKind,
};
use joker_expr::Program;

use crate::env::FixtureEnv;
use crate::model::{ActionKind, Fixture, Item, LimitSpec, ParamSpec, SafeDef, Script, StmtSpec};

/// The name of the only cycle group of a core suite.
pub const GROUP: &str = "case";

/// The capacity of the staging buffer of one tick, in proposals, on every
/// target: a proposal past it is dropped by the core, so the host and the
/// firmware use the same value.
pub const STAGING: usize = 256;

/// Why a case could not run: the fixture does not build in the core.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SetupError {
    /// The code the core returned (`J0203`, `J0210`, …), the code of an
    /// expression that does not compile (`J03xx`), `J0805` for the
    /// registry, or `J0844`.
    pub code: &'static str,
    /// What failed.
    pub message: &'static str,
}

/// What one output did in one tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputTick {
    /// The result of the core.
    pub result: OutputResult,
    /// The sample of the output slot after the commit.
    pub sample: Sample,
}

impl OutputTick {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        result: OutputResult {
            tick: 0,
            signal: SignalId(0),
            outcome: Outcome::None,
            applied: Applied::Relinquish,
            level: None,
            tripped: false,
        },
        sample: Sample::INITIAL,
    };
}

/// What one alarm is in one tick, after step 3.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AlarmTick {
    /// The published state, as its code (ref/00 §6).
    pub state: u8,
    /// The value of `alarm.<a>.active`.
    pub active: bool,
}

impl AlarmTick {
    /// Placeholder used to initialise fixed-capacity storage.
    pub const EMPTY: Self = Self {
        state: 0,
        active: false,
    };
}

/// What the case saw in one tick.
#[derive(Copy, Clone, Debug)]
pub struct TickView<'a> {
    /// Per output, in the order of [`Fixture::outputs`].
    pub outputs: &'a [OutputTick],
    /// Per input, the latched sample, in the order of [`Fixture::signals`].
    pub inputs: &'a [Sample],
    /// Per derived signal, its sample after the commit, in the order of
    /// [`Fixture::calcs`].
    pub calcs: &'a [Sample],
    /// Per alarm, in the order of [`Fixture::alarms`].
    pub alarms: &'a [AlarmTick],
    /// Per atomic group, the sample of its commit register (the initial
    /// sample when it has none), in the order of [`Fixture::atomic`].
    pub commits: &'a [Sample],
}

/// Receives every tick of a case, in order, outside the guard.
pub trait Observer {
    /// Tick `tick` (from 0) has committed.
    fn tick(&mut self, tick: u64, view: &TickView<'_>);
}

/// Runs code that must not allocate: the core part of every tick. The host
/// runner installs a guard that counts allocator calls; a firmware with no
/// allocator linked runs [`Unguarded`].
pub trait TickGuard {
    /// Runs `f` inside the guard.
    fn run<R>(&self, f: impl FnOnce() -> R) -> R;

    /// Allocator calls seen inside guarded code on this thread so far.
    fn violations(&self) -> u32 {
        0
    }
}

/// The identity guard, for a target where nothing can allocate.
#[derive(Copy, Clone, Debug, Default)]
pub struct Unguarded;

impl TickGuard for Unguarded {
    fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        f()
    }
}

#[derive(Debug)]
struct Sink<'a> {
    results: &'a mut [Option<OutputResult>],
    len: usize,
}

impl CommitSink for Sink<'_> {
    fn output(&mut self, result: &OutputResult) {
        if let Some(slot) = self.results.get_mut(self.len) {
            *slot = Some(*result);
            self.len += 1;
        }
    }
}

const fn setup(code: &'static str, message: &'static str) -> SetupError {
    SetupError { code, message }
}

const CAPACITY: SetupError = setup("J0844", "the fixture exceeds the capacity of the runner");

fn quality_of(code: u8, reason: Option<u8>) -> Quality {
    let code = match code {
        0 => QualityCode::Good,
        1 => QualityCode::Uncertain,
        _ => QualityCode::Bad,
    };
    Quality::new(code, QualityReason::from_u8(reason.unwrap_or(0)))
}

/// What a case needs beyond the const capacities of [`run`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    /// Compiled expressions: the length of the `programs` slice.
    pub programs: usize,
    /// Words of state arena: the length of the `arena` slice.
    pub arena: usize,
}

/// Visits every expression of the fixture in the order [`run`] compiles
/// them: per output `interlock`, the low and high `range` limits and
/// `reset_when`; then the derived signals; then per alarm `when` and
/// `suppress_when`; then, per instance and method, each assignment and
/// each call argument, with the parameters of its method.
fn expressions<'f>(fixture: &Fixture<'f>, mut f: impl FnMut(&'f str, Option<&'f [ParamSpec<'f>]>)) {
    for o in fixture.outputs {
        if let Some(src) = o.interlock {
            f(src, None);
        }
        if let Some((low, high)) = o.range {
            for limit in [low, high] {
                if let LimitSpec::Expr(src) = limit {
                    f(src, None);
                }
            }
        }
        if let Some(src) = o.reset_when {
            f(src, None);
        }
    }
    for c in fixture.calcs {
        f(c.expr, None);
    }
    for a in fixture.alarms {
        f(a.when, None);
        if let Some(src) = a.suppress_when {
            f(src, None);
        }
    }
    for i in fixture.instances {
        for m in i.methods {
            for stmt in m.stmts {
                match *stmt {
                    StmtSpec::Assign { expr, .. } => f(expr, Some(m.params)),
                    StmtSpec::Call { args, .. } => {
                        for arg in args {
                            f(arg, Some(m.params));
                        }
                    }
                }
            }
        }
    }
}

/// The number of compiled expressions and the words of state arena that a
/// fixture needs.
///
/// # Errors
///
/// The code of the first expression that does not compile.
pub fn needs(fixture: &Fixture<'_>) -> Result<Needs, SetupError> {
    let env = FixtureEnv::new(fixture, None);
    let mut out = Needs::default();
    let mut error = None;
    expressions(fixture, |src, params| {
        match compile(&env, src, fixture.period_ns, params) {
            Ok(p) => {
                out.programs += 1;
                out.arena += p.state_words();
            }
            Err(e) => {
                error.get_or_insert(setup(e.code(), "an expression does not compile"));
            }
        }
    });
    if let Some(e) = error {
        return Err(e);
    }
    out.arena += fixture
        .outputs
        .iter()
        .filter_map(|o| o.max_actuations)
        .map(|(count, _)| 1 + count as usize)
        .sum::<usize>();
    Ok(out)
}

/// The parts of a fixture that the core keeps for a whole generation, built
/// once per case.
#[derive(Debug)]
struct Plan<'p> {
    registry: Registry<'p>,
    group: GroupId,
    writers: &'p [WriterConfig],
    configs: &'p [OutputConfig<'p>],
    output_ids: &'p [SignalId],
    input_ids: &'p [SignalId],
    calc_ids: &'p [SignalId],
    alarm_active_ids: &'p [SignalId],
    commit_ids: &'p [Option<SignalId>],
    ports: &'p [SignalId],
    slots: &'p [Slot],
    derived: &'p [DerivedDef<'p>],
    instances: &'p [InstanceDef<'p>],
    alarms: &'p [AlarmConfig<'p>],
    atomic: &'p [AtomicGroup<'p>],
    arena_words: usize,
}

fn bound<'a>(spec: LimitSpec<'_>, program: Option<&'a Program>) -> Result<Bound<'a>, SetupError> {
    match spec {
        LimitSpec::Const(x) => Ok(Bound::Const(x)),
        LimitSpec::Expr(_) => program.map(Bound::Expr).ok_or(CAPACITY),
    }
}

fn compile(
    env: &FixtureEnv<'_>,
    src: &str,
    period_ns: u64,
    params: Option<&[ParamSpec<'_>]>,
) -> Result<Program, joker_expr::Diagnostic> {
    match params {
        Some(params) => env.compile_method(src, period_ns, params),
        None => env.compile(src, period_ns),
    }
}

fn registry_error<T>(_: T) -> SetupError {
    setup("J0805", "the registry does not build")
}

fn compile_error(e: joker_expr::Diagnostic) -> SetupError {
    setup(e.code(), "an expression does not compile")
}

/// Appends `id` to `buffer[start..*len]` unless it is already there.
/// Returns false when the buffer is full.
fn push_unique(buffer: &mut [SignalId], start: usize, len: &mut usize, id: SignalId) -> bool {
    if buffer.get(start..*len).is_some_and(|b| b.contains(&id)) {
        return true;
    }
    match buffer.get_mut(*len) {
        Some(slot) => {
            *slot = id;
            *len += 1;
            true
        }
        None => false,
    }
}

/// Runs a case and hands every tick to `observer`. `programs` and `arena`
/// hold at least what [`needs`] returns for the fixture.
///
/// # Errors
///
/// A [`SetupError`] when the fixture does not fit the capacities, when an
/// expression does not compile, when the core refuses it (`J0203`, `J0210`,
/// …), or when a tick fails in the core.
#[allow(clippy::too_many_lines)]
pub fn run<const S: usize, const O: usize, const W: usize, const P: usize>(
    fixture: &Fixture<'_>,
    script: &Script<'_>,
    programs: &mut [Option<Program>],
    arena: &mut [u64],
    guard: &impl TickGuard,
    observer: &mut impl Observer,
) -> Result<(), SetupError> {
    let n_in = fixture.signals.len();
    let n_out = fixture.outputs.len();
    let n_calc = fixture.calcs.len();
    let n_alarm = fixture.alarms.len();
    let n_atomic = fixture.atomic.len();
    let n_writers = fixture.writers.len();
    let n_commit = fixture.atomic.iter().filter(|g| g.commit.is_some()).count();
    if n_in + n_out + n_calc + 2 * n_alarm + n_commit > S
        || fixture.instances.len() > S
        || n_out > O
        || n_atomic > O
        || n_writers > W
    {
        return Err(CAPACITY);
    }
    let mut metas = [SignalMeta::EMPTY; S];
    let mut aliases: [Alias<'_>; 0] = [];
    let mut groups = [GroupMeta::EMPTY; 1];
    let mut builder = RegistryBuilder::new(&mut metas, &mut aliases, &mut groups);
    let derived = |path, ty, unit| SignalMeta {
        path,
        ty,
        unit: UnitRef(unit),
        direction: Direction::Derived,
        group: Some(GROUP),
        group_id: None,
        max_age_ns: None,
    };
    for s in fixture.signals {
        let mut meta = SignalMeta::input(s.path, s.ty, UnitRef(s.unit));
        meta.max_age_ns = s.max_age_ns;
        builder.signal(meta).map_err(registry_error)?;
    }
    for o in fixture.outputs {
        builder
            .signal(SignalMeta::output(o.path, o.ty, UnitRef(o.unit), GROUP))
            .map_err(registry_error)?;
    }
    for c in fixture.calcs {
        builder
            .signal(derived(c.path, c.ty, c.unit))
            .map_err(registry_error)?;
    }
    for a in fixture.alarms {
        builder
            .signal(derived(a.state_path, ValueType::Enum, "1"))
            .map_err(registry_error)?;
        builder
            .signal(derived(a.active_path, ValueType::Bool, "1"))
            .map_err(registry_error)?;
    }
    for c in fixture.atomic.iter().filter_map(|g| g.commit) {
        builder
            .signal(SignalMeta::output(c.path, c.ty, UnitRef::ONE, GROUP))
            .map_err(registry_error)?;
    }
    builder
        .group(GroupMeta {
            name: GROUP,
            period_ns: fixture.period_ns,
        })
        .map_err(registry_error)?;
    let registry = builder.build().map_err(registry_error)?;
    let missing = || setup("J0844", "a path is not in the registry");
    let group = registry
        .resolve_group(GROUP)
        .ok_or_else(|| setup("J0844", "group missing from the registry"))?;
    let resolve = |path: &str| registry.resolve(path).ok_or_else(missing);

    let mut input_ids = [SignalId(0); S];
    for (id, s) in input_ids.iter_mut().zip(fixture.signals) {
        *id = resolve(s.path)?;
    }
    let mut output_ids = [SignalId(0); O];
    for (id, o) in output_ids.iter_mut().zip(fixture.outputs) {
        *id = resolve(o.path)?;
    }
    let mut calc_ids = [SignalId(0); S];
    for (id, c) in calc_ids.iter_mut().zip(fixture.calcs) {
        *id = resolve(c.path)?;
    }
    let mut active_ids = [SignalId(0); S];
    let mut state_ids = [SignalId(0); S];
    for (i, a) in fixture.alarms.iter().enumerate() {
        active_ids[i] = resolve(a.active_path)?;
        state_ids[i] = resolve(a.state_path)?;
    }
    let mut commit_ids = [None; O];
    for (id, g) in commit_ids.iter_mut().zip(fixture.atomic) {
        if let Some(c) = g.commit {
            *id = Some(resolve(c.path)?);
        }
    }

    let env = FixtureEnv::new(fixture, Some(&registry));
    let mut compiled = 0usize;
    let mut failure = None;
    expressions(fixture, |src, params| {
        if failure.is_some() {
            return;
        }
        match (
            compile(&env, src, fixture.period_ns, params),
            programs.get_mut(compiled),
        ) {
            (Ok(p), Some(slot)) => {
                *slot = Some(p);
                compiled += 1;
            }
            (Ok(_), None) => failure = Some(CAPACITY),
            (Err(e), _) => failure = Some(compile_error(e)),
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    let dummy = env
        .compile("false", fixture.period_ns)
        .map_err(compile_error)?;
    let programs: &[Option<Program>] = programs;
    let mut next = 0usize;
    let mut take = || -> Result<&Program, SetupError> {
        let p = programs
            .get(next)
            .and_then(Option::as_ref)
            .ok_or(CAPACITY)?;
        next += 1;
        Ok(p)
    };

    let mut good = [SignalId(0); S];
    let mut good_spans = [(0usize, 0usize); O];
    let mut good_len = 0usize;
    for (i, span) in good_spans.iter_mut().enumerate().take(n_out) {
        let start = good_len;
        let mut fits = true;
        for link in fixture.requires_good.iter().filter(|l| l.owner == i) {
            let Item::Path(selector) = link.item else {
                continue;
            };
            env.select_paths(selector, |path| {
                if let Some(id) = registry.resolve(path) {
                    fits &= push_unique(&mut good, start, &mut good_len, id);
                }
            });
        }
        if !fits {
            return Err(CAPACITY);
        }
        *span = (start, good_len);
    }

    let mut per_output: [[Option<&Program>; 4]; O] = [[None; 4]; O];
    for (slots, o) in per_output.iter_mut().zip(fixture.outputs) {
        if o.interlock.is_some() {
            slots[0] = Some(take()?);
        }
        if let Some((low, high)) = o.range {
            for (k, limit) in [low, high].into_iter().enumerate() {
                if matches!(limit, LimitSpec::Expr(_)) {
                    slots[1 + k] = Some(take()?);
                }
            }
        }
        if o.reset_when.is_some() {
            slots[3] = Some(take()?);
        }
    }

    let mut order = [0usize; O];
    for (i, slot) in order.iter_mut().enumerate() {
        *slot = i;
    }
    order[..n_out].sort_unstable_by_key(|&i| output_ids[i]);
    let mut configs =
        [OutputConfig::new(SignalId(0), ValueType::F64, group, SafeValue::Relinquish); O];
    for (config, &i) in configs.iter_mut().zip(&order[..n_out]) {
        let o = &fixture.outputs[i];
        let safe = match o.safe {
            SafeDef::Value(bits) => SafeValue::Value(bits),
            SafeDef::Relinquish => SafeValue::Relinquish,
        };
        let [interlock, low, high, reset_when] = per_output[i];
        let mut c = OutputConfig::new(output_ids[i], o.ty, group, safe);
        c.safe_ramp = o.safe_ramp;
        match o.range {
            None => {}
            Some((LimitSpec::Const(a), LimitSpec::Const(b))) => c.range = Some((a, b)),
            Some((lo, hi)) => {
                c.range_exprs = Some((bound(lo, low)?, bound(hi, high)?));
            }
        }
        c.on_range = o.on_range;
        c.rate = o.rate;
        c.on_interlock = o.on_interlock;
        if let Some(ns) = o.max_lease_ns {
            c.max_lease_ns = ns;
        }
        if let Some(ns) = o.max_manual_ns {
            c.max_manual_ns = ns;
        }
        c.interlock = interlock;
        c.permit_uncertain = o.permit_uncertain;
        let (start, end) = good_spans[i];
        c.requires_good = &good[start..end];
        c.grace_ns = o.grace_ns;
        c.reset_when = reset_when;
        c.deadband = o.deadband;
        c.min_interval_ns = o.min_interval_ns;
        c.max_actuations = o
            .max_actuations
            .map(|(count, window_ns)| Budget { count, window_ns });
        *config = c;
    }

    let mut derived_defs = [DerivedDef {
        signal: SignalId(0),
        group,
        expr: &dummy,
    }; S];
    for (def, id) in derived_defs.iter_mut().zip(&calc_ids[..n_calc]) {
        *def = DerivedDef {
            signal: *id,
            group,
            expr: take()?,
        };
    }

    let mut trip = [SignalId(0); S];
    let mut trip_len = 0usize;
    let mut trip_spans = [(0usize, 0usize); S];
    for (a, span) in trip_spans.iter_mut().enumerate().take(n_alarm) {
        let start = trip_len;
        for link in fixture.trips.iter().filter(|l| l.owner == a) {
            if let Item::Output(o) = link.item {
                let id = *output_ids[..n_out].get(o).ok_or_else(missing)?;
                if !push_unique(&mut trip, start, &mut trip_len, id) {
                    return Err(CAPACITY);
                }
            }
        }
        *span = (start, trip_len);
    }
    let mut alarm_configs = [AlarmConfig::new(0, group, &dummy, Severity::Low); S];
    for (i, a) in fixture.alarms.iter().enumerate() {
        let when = take()?;
        let suppress = match a.suppress_when {
            Some(_) => Some(take()?),
            None => None,
        };
        let mut c = AlarmConfig::new(
            u32::try_from(i).map_err(|_| CAPACITY)?,
            group,
            when,
            a.severity,
        );
        c.suppress_when = suppress;
        c.delay_on_ns = a.delay_on_ns;
        c.delay_off_ns = a.delay_off_ns;
        if let Some(ns) = a.max_shelve_ns {
            c.max_shelve_ns = ns;
        }
        let (start, end) = trip_spans[i];
        c.trip = &trip[start..end];
        c.state_signal = Some(state_ids[i]);
        c.active_signal = Some(active_ids[i]);
        alarm_configs[i] = c;
    }

    let n_inst = fixture.instances.len();
    let mut assign_programs: [&Program; S] = [&dummy; S];
    let mut arg_refs: [&Program; S] = [&dummy; S];
    let mut arg_len = 0usize;
    let mut stmt_args = [(0usize, 0usize); S];
    let mut stmt_count = 0usize;
    for m in fixture.instances.iter().flat_map(|i| i.methods) {
        for stmt in m.stmts {
            let k = stmt_count;
            match *stmt {
                StmtSpec::Assign { .. } => {
                    *assign_programs.get_mut(k).ok_or(CAPACITY)? = take()?;
                }
                StmtSpec::Call { args, .. } => {
                    let first = arg_len;
                    for _ in args {
                        *arg_refs.get_mut(arg_len).ok_or(CAPACITY)? = take()?;
                        arg_len += 1;
                    }
                    *stmt_args.get_mut(k).ok_or(CAPACITY)? = (first, arg_len);
                }
            }
            stmt_count += 1;
        }
    }
    let arg_refs = arg_refs;
    let mut stmts = [Stmt::Assign {
        target: SignalId(0),
        expr: &dummy,
    }; S];
    let mut k = 0usize;
    for m in fixture.instances.iter().flat_map(|i| i.methods) {
        for stmt in m.stmts {
            let def = match *stmt {
                StmtSpec::Assign { output, .. } => Stmt::Assign {
                    target: *output_ids[..n_out].get(output).ok_or_else(missing)?,
                    expr: assign_programs[k],
                },
                StmtSpec::Call {
                    instance, method, ..
                } => {
                    let (first, end) = stmt_args[k];
                    Stmt::Call {
                        instance: u32::try_from(instance).map_err(|_| CAPACITY)?,
                        method,
                        args: &arg_refs[first..end],
                    }
                }
            };
            *stmts.get_mut(k).ok_or(CAPACITY)? = def;
            k += 1;
        }
    }
    let stmts = stmts;
    let mut methods = [MethodDef {
        params: 0,
        stmts: &[],
    }; S];
    let mut method_spans = [(0usize, 0usize); S];
    let mut n_methods = 0usize;
    let mut k = 0usize;
    for (ii, i) in fixture.instances.iter().enumerate() {
        let first = n_methods;
        for m in i.methods {
            let n = m.stmts.len();
            *methods.get_mut(n_methods).ok_or(CAPACITY)? = MethodDef {
                params: u8::try_from(m.params.len()).map_err(|_| CAPACITY)?,
                stmts: stmts.get(k..k + n).ok_or(CAPACITY)?,
            };
            k += n;
            n_methods += 1;
        }
        *method_spans.get_mut(ii).ok_or(CAPACITY)? = (first, n_methods);
    }
    let methods = methods;
    let mut instance_defs = [InstanceDef { methods: &[] }; S];
    for (def, (first, end)) in instance_defs.iter_mut().zip(&method_spans[..n_inst]) {
        *def = InstanceDef {
            methods: &methods[*first..*end],
        };
    }
    let method_words = assign_programs[..stmt_count]
        .iter()
        .chain(&arg_refs[..arg_len])
        .map(|p| p.state_words())
        .max()
        .unwrap_or(0);

    let mut members = [SignalId(0); O];
    let mut members_len = 0usize;
    let mut bus = [SignalId(0); O];
    let mut bus_len = 0usize;
    let mut spans = [((0usize, 0usize), (0usize, 0usize)); O];
    for (g, span) in spans.iter_mut().enumerate().take(n_atomic) {
        let (m0, b0) = (members_len, bus_len);
        for (list, buffer, len) in [
            (fixture.members, &mut members, &mut members_len),
            (fixture.order, &mut bus, &mut bus_len),
        ] {
            for link in list.iter().filter(|l| l.owner == g) {
                if let Item::Output(o) = link.item {
                    let id = *output_ids[..n_out].get(o).ok_or_else(missing)?;
                    let slot = buffer.get_mut(*len).ok_or(CAPACITY)?;
                    *slot = id;
                    *len += 1;
                }
            }
        }
        *span = ((m0, members_len), (b0, bus_len));
    }
    let mut atomic = [AtomicGroup {
        outputs: &[],
        order: &[],
        on_rate: OnRate::Independent,
        commit: None,
    }; O];
    for (g, spec) in fixture.atomic.iter().enumerate() {
        let ((m0, m1), (b0, b1)) = spans[g];
        atomic[g] = AtomicGroup {
            outputs: &members[m0..m1],
            order: &bus[b0..b1],
            on_rate: spec.on_rate,
            commit: match (spec.commit, commit_ids[g]) {
                (Some(c), Some(id)) => Some((id, c.value)),
                _ => None,
            },
        };
    }

    let mut writers = [WriterConfig::EMPTY; W];
    for (config, w) in writers.iter_mut().zip(fixture.writers) {
        *config = WriterConfig {
            kind: w.kind,
            level: Level::new(w.level)
                .ok_or_else(|| setup("J0844", "a writer level is outside 1-16"))?,
            lease_ns: w.lease_ns,
            requires_clock: w.requires_clock,
        };
    }
    let mut ports = input_ids;
    ports[..n_in].sort_unstable();
    let slots: [Slot; S] = core::array::from_fn(|_| Slot::new());
    let arena_words = configs[..n_out]
        .iter()
        .map(OutputConfig::state_words)
        .sum::<usize>()
        + alarm_configs[..n_alarm]
            .iter()
            .map(|a| a.when.state_words() + a.suppress_when.map_or(0, Program::state_words))
            .sum::<usize>()
        + derived_defs[..n_calc]
            .iter()
            .map(|d| d.expr.state_words())
            .sum::<usize>()
        + method_words;
    let arena = arena.get_mut(..arena_words).ok_or(CAPACITY)?;
    let plan = Plan {
        registry,
        group,
        writers: &writers[..n_writers],
        configs: &configs[..n_out],
        output_ids: &output_ids[..n_out],
        input_ids: &input_ids[..n_in],
        calc_ids: &calc_ids[..n_calc],
        alarm_active_ids: &active_ids[..n_alarm],
        commit_ids: &commit_ids[..n_atomic],
        ports: &ports[..n_in],
        slots: &slots[..registry.len()],
        derived: &derived_defs[..n_calc],
        instances: &instance_defs[..n_inst],
        alarms: &alarm_configs[..n_alarm],
        atomic: &atomic[..n_atomic],
        arena_words,
    };
    let clock = VirtualClock::new(Instant {
        mono_ns: 0,
        utc_ns: 0,
        clock: ClockQuality::Synced,
    });
    let segment = Segment {
        fixture,
        script,
        plan: &plan,
        clock: &clock,
    };
    let mut inherited = [Applied::Relinquish; O];
    let mut has_inherited = false;
    let mut start = 0;
    loop {
        let next = script
            .actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Generation) && a.tick > start)
            .map(|a| a.tick)
            .min();
        let end = match next {
            Some(g) => g,
            None if script.ticks > start => script.ticks,
            None => break,
        };
        let from = has_inherited.then_some(&inherited[..n_out]);
        let mut last = [Applied::Relinquish; O];
        segment.run::<S, O, P>(
            start,
            end,
            from,
            &mut last[..n_out],
            &mut *arena,
            guard,
            observer,
        )?;
        inherited = last;
        has_inherited = true;
        start = end;
        if next.is_none() {
            break;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Segment<'r> {
    fixture: &'r Fixture<'r>,
    script: &'r Script<'r>,
    plan: &'r Plan<'r>,
    clock: &'r VirtualClock,
}

/// The core state of one generation.
struct Generation<'g> {
    image: ProcessImage<'g>,
    outputs: Outputs<'g>,
    alarms: Alarms<'g>,
    classes: Classes<'g>,
}

impl Segment<'_> {
    /// Runs the ticks `start..end` of one generation and writes the last
    /// applied value of every output, in the order of the configs, to
    /// `last`.
    #[allow(clippy::too_many_arguments)]
    fn run<const S: usize, const O: usize, const P: usize>(
        &self,
        start: u64,
        end: u64,
        inherited: Option<&[Applied]>,
        last: &mut [Applied],
        arena: &mut [u64],
        guard: &impl TickGuard,
        observer: &mut impl Observer,
    ) -> Result<(), SetupError> {
        let plan = self.plan;
        let n_out = plan.configs.len();
        let n_in = plan.ports.len();
        let n_calc = plan.calc_ids.len();
        let n_alarm = plan.alarms.len();
        let n_atomic = plan.atomic.len();
        let refused = |e| match e {
            CommitError::Config(c) => setup(c.code, "the core refuses the output configuration"),
            _ => setup("J0844", "the outputs do not build"),
        };
        let mut states = [OutputState::EMPTY; O];
        let mut intakes = [IntakeSlot::EMPTY; P];
        let mut outputs = Outputs::new(
            &plan.registry,
            plan.writers,
            OutputsStorage {
                configs: plan.configs,
                states: &mut states[..n_out],
                intakes: &mut intakes,
            },
        )
        .map_err(refused)?;
        let (output_arena, rest) = arena.split_at_mut(outputs.state_words().min(plan.arena_words));
        outputs.set_state_arena(output_arena).map_err(refused)?;
        outputs.set_atomic_groups(plan.atomic).map_err(refused)?;
        if let Some(values) = inherited {
            for (config, value) in plan.configs.iter().zip(values) {
                outputs.inherit(config.signal, *value);
            }
        }
        let mut alarm_states = [AlarmRuntime::EMPTY; S];
        let mut alarms = Alarms::new(plan.alarms, &mut alarm_states[..n_alarm]).map_err(refused)?;
        let (alarm_arena, rest) = rest.split_at_mut(alarms.state_words().min(rest.len()));
        alarms.set_state_arena(alarm_arena).map_err(refused)?;
        let mut scratch = [Proposal::EMPTY; P];
        let mut classes = Classes::new(plan.instances, plan.derived, &mut scratch);
        classes.set_state_arena(rest).map_err(refused)?;
        let mut snapshot = [Sample::INITIAL; S];
        let mut extenders = [SeqExtender::for_slot(); S];
        let mut proposals = [Proposal::EMPTY; P];
        let mut method_buffer = [MethodCall::EMPTY; P];
        let mut image = ProcessImage::new(
            plan.group,
            self.fixture.period_ns,
            ImageStorage {
                ports: plan.ports,
                snapshot: &mut snapshot[..n_in],
                extenders: &mut extenders[..n_in],
                proposals: &mut proposals,
            },
        )
        .map_err(|_| setup("J0844", "the process image does not build"))?;
        image.set_method_buffer(&mut method_buffer);
        let mut generation = Generation {
            image,
            outputs,
            alarms,
            classes,
        };
        let mut results = [None; O];
        let mut record = [OutputTick::EMPTY; O];
        let mut inputs = [Sample::INITIAL; S];
        let mut calcs = [Sample::INITIAL; S];
        let mut alarm_view = [AlarmTick::EMPTY; S];
        let mut commits = [Sample::INITIAL; O];
        let read = |id: SignalId| {
            plan.slots
                .get(id.index())
                .and_then(|s| s.try_read().ok())
                .unwrap_or(Sample::INITIAL)
        };
        for tick in start..end {
            let mut sink = Sink {
                results: &mut results[..n_out],
                len: 0,
            };
            let core = guard.run(|| self.tick(tick, &mut generation, &mut sink));
            core.map_err(|message| setup("J0844", message))?;
            let results = &sink.results[..sink.len];
            for (out, id) in record.iter_mut().zip(plan.output_ids) {
                let result = results
                    .iter()
                    .flatten()
                    .find(|r| r.signal == *id)
                    .copied()
                    .ok_or_else(|| setup("J0844", "an output has no result in a tick"))?;
                *out = OutputTick {
                    result,
                    sample: read(*id),
                };
            }
            for (sample, id) in inputs.iter_mut().zip(plan.input_ids) {
                *sample = generation
                    .image
                    .sample(*id)
                    .copied()
                    .unwrap_or(Sample::INITIAL);
            }
            for (sample, id) in calcs.iter_mut().zip(plan.calc_ids) {
                *sample = read(*id);
            }
            for (i, (view, id)) in alarm_view.iter_mut().zip(plan.alarm_active_ids).enumerate() {
                let state = u32::try_from(i)
                    .ok()
                    .and_then(|a| generation.alarms.state(a))
                    .map_or(0, |(s, _)| s as u8);
                *view = AlarmTick {
                    state,
                    active: read(*id).transport_bits() != 0,
                };
            }
            for (sample, id) in commits.iter_mut().zip(plan.commit_ids) {
                *sample = id.map_or(Sample::INITIAL, read);
            }
            observer.tick(
                tick,
                &TickView {
                    outputs: &record[..n_out],
                    inputs: &inputs[..n_in],
                    calcs: &calcs[..n_calc],
                    alarms: &alarm_view[..n_alarm],
                    commits: &commits[..n_atomic],
                },
            );
        }
        for (value, config) in last.iter_mut().zip(plan.configs) {
            let view = generation.outputs.view(config.signal);
            *value = view.map_or(Applied::from_safe(config.safe), |v| v.applied);
        }
        Ok(())
    }

    fn tick(
        &self,
        tick: u64,
        generation: &mut Generation<'_>,
        sink: &mut Sink<'_>,
    ) -> Result<(), &'static str> {
        let plan = self.plan;
        let Generation {
            image,
            outputs,
            alarms,
            classes,
        } = generation;
        let now = tick.saturating_mul(self.fixture.period_ns);
        let quality = self.clock.now().clock;
        self.clock.set(Instant {
            mono_ns: now,
            utc_ns: i64::try_from(now).unwrap_or(i64::MAX),
            clock: quality,
        });
        let at_tick = || self.script.actions.iter().filter(move |a| a.tick == tick);
        for action in at_tick() {
            match action.kind {
                ActionKind::Clock(q) => self.clock.set_quality(q),
                ActionKind::Input {
                    signal,
                    bits,
                    quality,
                } => {
                    let id = plan.input_ids.get(signal).ok_or("unknown input")?;
                    let slot = plan.slots.get(id.index()).ok_or("missing slot")?;
                    slot.write(
                        bits,
                        quality_of(quality.code, quality.reason),
                        NO_TS_SRC,
                        now,
                    );
                }
                ActionKind::Reset { output } => {
                    let id = plan.output_ids.get(output).ok_or("unknown output")?;
                    outputs.reset(*id);
                }
                _ => {}
            }
        }
        image
            .latch(tick, self.clock.now(), plan.slots, &plan.registry)
            .map_err(|_| "latch failed")?;
        for stage_async in [true, false] {
            if !stage_async {
                classes
                    .evaluate_derived(image, plan.slots)
                    .map_err(|_| "derived signals failed")?;
                alarms
                    .evaluate(image, plan.slots, outputs, sink)
                    .map_err(|_| "alarms failed")?;
            }
            for action in at_tick() {
                let (output, writer, bits, flags) = match action.kind {
                    ActionKind::Propose {
                        output,
                        writer,
                        bits,
                    } => (output, writer, bits, 0),
                    ActionKind::Release { output, writer } => (output, writer, 0, RELINQUISH),
                    _ => continue,
                };
                let def = self.fixture.writers.get(writer).ok_or("unknown writer")?;
                if def.is_async() != stage_async {
                    continue;
                }
                let proposal = Proposal {
                    writer: WriterId(u32::try_from(writer).map_err(|_| "too many writers")?),
                    signal: *plan.output_ids.get(output).ok_or("unknown output")?,
                    value: bits,
                    flags,
                    ttl_ns: if def.kind == WriterKind::Manual {
                        def.lease_ns.unwrap_or(0)
                    } else {
                        0
                    },
                    ..Proposal::EMPTY
                };
                let staged = if stage_async {
                    image.stage_async(proposal)
                } else {
                    image.stage_cycle(proposal)
                };
                staged.map_err(|_| "staging failed")?;
            }
            for action in at_tick() {
                let ActionKind::Method {
                    instance,
                    method,
                    writer,
                    args,
                    n_args,
                } = action.kind
                else {
                    continue;
                };
                let def = self.fixture.writers.get(writer).ok_or("unknown writer")?;
                if def.is_async() != stage_async {
                    continue;
                }
                image
                    .stage_method(MethodCall {
                        writer: WriterId(u32::try_from(writer).map_err(|_| "too many writers")?),
                        instance: u32::try_from(instance).map_err(|_| "too many instances")?,
                        method,
                        args,
                        n_args,
                        source: if stage_async {
                            Source::Async
                        } else {
                            Source::Cycle
                        },
                        ..MethodCall::EMPTY
                    })
                    .map_err(|_| "staging failed")?;
            }
        }
        classes
            .expand(image, plan.slots, outputs, sink)
            .map_err(|_| "method expansion failed")?;
        outputs
            .commit(image, plan.slots, sink)
            .map_err(|_| "commit failed")?;
        image.end_tick().map_err(|_| "end of tick failed")?;
        Ok(())
    }
}
