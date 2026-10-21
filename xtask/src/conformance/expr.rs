//! The signals of a fixture as the expression checker sees them (the `Env`
//! of `joker-expr`) while the loader still infers the type and unit of the
//! derived signals (ref/01 §5.14). Once the fixture is complete, the
//! executor compiles against the registry with the `FixtureEnv` of
//! `joker-case-runner`.

use alloc::string::String;
use alloc::vec::Vec;

use joker_case_runner::env::selects;
use joker_core::ValueType;
use joker_expr::{
    CheckOptions, CompileOptions, Diagnostic, EnumId, Env, Program, Repr, SignalInfo, Ty,
};
use joker_units::{Duration, Unit};

/// The signal type that holds the result of an expression of type `ty`.
pub fn value_type(ty: Ty) -> ValueType {
    match ty {
        Ty::Bool => ValueType::Bool,
        Ty::Number {
            repr: Repr::Float, ..
        } => ValueType::F64,
        Ty::Number {
            repr: Repr::Signed, ..
        } => ValueType::I64,
        Ty::Number {
            repr: Repr::Unsigned,
            ..
        } => ValueType::U64,
        Ty::Enum(_) => ValueType::Enum,
    }
}

#[derive(Clone, Debug)]
struct Entry {
    path: String,
    info: SignalInfo,
}

/// The signals an expression may read, sorted by path.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    entries: Vec<Entry>,
}

impl Signals {
    /// Adds a signal; a repeated path keeps the first one.
    pub fn insert(&mut self, path: &str, ty: ValueType, unit: Unit, id: u32) {
        let Err(at) = self.entries.binary_search_by(|e| e.path.as_str().cmp(path)) else {
            return;
        };
        let unit = (!unit.is_dimensionless() || unit.kind().is_some()).then_some(unit);
        self.entries.insert(
            at,
            Entry {
                path: path.into(),
                info: SignalInfo {
                    id,
                    ty: joker_case_runner::env::signal_type(ty),
                    unit,
                },
            },
        );
    }

    /// Checks and compiles `src` for a group of period `period_ns`.
    #[allow(clippy::result_large_err)]
    pub fn compile(&self, src: &str, period_ns: u64) -> Result<Program, Diagnostic> {
        let checked = joker_expr::check(src, self, CheckOptions::default())?;
        joker_expr::compile(
            &checked,
            CompileOptions {
                period: Duration::from_nanos(period_ns),
            },
        )
    }

    /// The type of the expression `src`.
    #[allow(clippy::result_large_err)]
    pub fn type_of(&self, src: &str, period_ns: u64) -> Result<Ty, Diagnostic> {
        self.compile(src, period_ns).map(|p| p.result_type())
    }
}

impl Env for Signals {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| self.entries[i].info)
    }

    fn enum_variant(&self, _enum_name: &str, _variant: &str) -> Option<(EnumId, u64)> {
        None
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        let mut seen: Vec<u32> = Vec::new();
        for e in &self.entries {
            if selects(selector, &e.path) && !seen.contains(&e.info.id) {
                seen.push(e.info.id);
                found(e.info);
            }
        }
    }
}
