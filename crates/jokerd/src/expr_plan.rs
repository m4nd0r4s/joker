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
//! Scoped deliberately: only scalar, non-enum signal types resolve
//! (`Bool`/`I64`/`U64`/`F64`/`F32`/`Bits`). `Plan`'s own `SignalPlan`
//! does not carry the manifest's enum *names* (only each signal's own
//! variant list), so `enum_variant` cannot be implemented correctly
//! yet — an enum-typed path type-checks as unresolvable here, not
//! silently wrong.

use joker_core::ValueType;
use joker_expr::{
    CheckOptions, CompileOptions, Diagnostic, Env, EvalError, Inputs, Output, Program, Quality,
    Reading, SignalInfo, SignalType, check, compile, eval,
};
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
        // Enum names aren't carried by `Plan` yet (see the module doc);
        // `String`/`Bytes`/`Record` can't appear in an expression
        // (ref/01 §3.7: the language is scalar-only).
        ValueType::Enum | ValueType::String | ValueType::Bytes | ValueType::Record => None,
    }
}

/// `joker_expr::Env` over a jokerd [`Plan`], so a compiled `Program`'s
/// `Input.signal` is exactly the `SignalId` `Shared.slots` is indexed
/// by (see the module doc).
#[derive(Debug)]
pub struct PlanEnv<'a> {
    plan: &'a Plan,
}

impl<'a> PlanEnv<'a> {
    /// An `Env` over `plan`'s own signals.
    pub fn new(plan: &'a Plan) -> Self {
        Self { plan }
    }
}

impl Env for PlanEnv<'_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        let id = self.plan.id(path)?;
        let canonical = self.plan.path(id)?;
        let s = self.plan.signals.iter().find(|s| s.path == canonical)?;
        let ty = signal_type(s.ty)?;
        let unit = (s.unit != "1")
            .then(|| joker_units::Unit::parse(&s.unit).ok())
            .flatten();
        Some(SignalInfo {
            id: u32::try_from(id.index()).ok()?,
            ty,
            unit,
        })
    }

    fn enum_variant(&self, _enum_name: &str, _variant: &str) -> Option<(joker_expr::EnumId, u64)> {
        // Not resolvable yet; see the module doc.
        None
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
/// `shared.slots` at `now` (monotonic ns). A single, stateless call: a
/// `program` using a windowed function needs its `state` arena to
/// persist across calls, which the caller of this function does not do
/// yet (AUR-417's own open question — out of scope for the id-scheme
/// pinning test this module exists to support).
pub fn eval_bool(program: &Program, shared: &Shared, now: u64) -> Result<Output, EvalError> {
    let mut state = vec![0u64; program.state_words()];
    let inputs = SlotInputs { shared, now };
    eval(program, &inputs, now, &mut state)
}
