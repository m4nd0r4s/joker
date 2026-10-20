//! Expressions of the manifest (ref/01 §3.7), checked with `joker-expr`
//! against the path registry of the site.

use std::collections::BTreeMap;

use joker_expr::{EnumId, Env, Ty};
use joker_units::Unit;

use crate::check::PathRegistry;
use crate::context::{SignalInfo, SignalType};
use crate::diag::Diagnostic;
use crate::grammar;
use crate::source::Span;

/// The `alarm_state` enum of ref/00 §6, as expressions name it.
pub const ALARM_STATE: &str = "AlarmState";

/// The variants of `alarm_state`, in their numeric order.
pub const ALARM_STATES: [&str; 5] = [
    "normal",
    "active_unacked",
    "active_acked",
    "rtn_unacked",
    "unknown",
];

/// What an expression must produce.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Expect {
    /// A boolean (permissives, alarm conditions, `reset_when`).
    Bool,
    /// A number convertible to a unit (`range` bounds).
    Unit(Unit),
    /// Any scalar (`calc`).
    Any,
}

/// The outcome of checking one expression.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// Checked, with its result type.
    Typed(Ty),
    /// It reads a signal whose type is not known yet (an untyped `calc`, or
    /// a device whose profile is not in the context); only the syntax was
    /// checked.
    Opaque(Span),
    /// It has an error, already reported.
    Failed,
}

/// The registry as `joker-expr` sees it.
#[derive(Debug)]
pub struct Registry<'a> {
    paths: &'a PathRegistry,
    enums: BTreeMap<String, (u32, Vec<String>)>,
    ids: BTreeMap<String, u32>,
}

impl<'a> Registry<'a> {
    /// The expression view of a path registry.
    pub fn new(paths: &'a PathRegistry) -> Self {
        let mut enums: BTreeMap<String, (u32, Vec<String>)> = BTreeMap::new();
        enums.insert(
            ALARM_STATE.to_owned(),
            (0, ALARM_STATES.iter().map(|s| (*s).to_owned()).collect()),
        );
        for registered in paths.values() {
            if let SignalType::Enum { name, variants } = &registered.info.ty {
                let next = u32::try_from(enums.len()).unwrap_or(u32::MAX);
                enums
                    .entry(name.clone())
                    .or_insert_with(|| (next, variants.clone()));
            }
        }
        let ids = paths
            .keys()
            .enumerate()
            .map(|(i, p)| (p.clone(), u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        Self { paths, enums, ids }
    }

    fn info(&self, path: &str) -> Option<joker_expr::SignalInfo> {
        let registered = self.paths.get(path)?;
        let id = *self
            .ids
            .get(&registered.canonical)
            .or_else(|| self.ids.get(path))?;
        to_expr(&registered.info, id, &self.enums)
    }

    /// Whether a path is known but has no usable type yet.
    pub fn is_opaque(&self, path: &str) -> bool {
        self.paths
            .get(path)
            .is_some_and(|r| r.info.ty == SignalType::Unknown)
    }
}

fn to_expr(
    info: &SignalInfo,
    id: u32,
    enums: &BTreeMap<String, (u32, Vec<String>)>,
) -> Option<joker_expr::SignalInfo> {
    let ty = match &info.ty {
        SignalType::Bool => joker_expr::SignalType::Bool,
        SignalType::Integer => joker_expr::SignalType::I64,
        SignalType::Float => joker_expr::SignalType::F64,
        SignalType::Enum { name, .. } => joker_expr::SignalType::Enum(EnumId(enums.get(name)?.0)),
        SignalType::Text => joker_expr::SignalType::String,
        SignalType::Bytes => joker_expr::SignalType::Bytes,
        SignalType::Unknown => return None,
    };
    Some(joker_expr::SignalInfo {
        id,
        ty,
        unit: info.unit,
    })
}

impl Env for Registry<'_> {
    fn signal(&self, path: &str) -> Option<joker_expr::SignalInfo> {
        self.info(path)
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        let (id, variants) = self.enums.get(enum_name)?;
        let index = variants.iter().position(|v| v == variant)?;
        Some((EnumId(*id), u64::try_from(index).ok()?))
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(joker_expr::SignalInfo)) {
        for (path, registered) in self.paths {
            if *path == registered.canonical
                && grammar::matches(selector, path)
                && let Some(info) = self.info(path)
            {
                found(info);
            }
        }
    }
}

/// Where an expression's text starts inside its TOML string: one byte after
/// the opening quote of a basic or literal string.
fn origin(span: Span) -> Span {
    Span::new(
        span.file,
        span.start + 1,
        span.end.saturating_sub(1).max(span.start + 1),
    )
}

/// Checks an expression written at `span` (the TOML string) and returns its
/// type, or a diagnostic to record.
pub fn check(
    source: &str,
    span: Span,
    registry: &Registry<'_>,
    expect: Expect,
    opaque_prefix: &dyn Fn(&str) -> bool,
) -> (Outcome, Option<Diagnostic>) {
    let base = origin(span);
    let at = |s: joker_units::Span| {
        let range = s.range();
        base.slice(range)
    };
    let fail = |d: joker_expr::Diagnostic| {
        (
            Outcome::Failed,
            Some(Diagnostic::new(d.code(), d.kind.to_string()).at(at(d.span))),
        )
    };
    match joker_expr::check(source, registry, joker_expr::CheckOptions::default()) {
        Ok(checked) => {
            let result = match expect {
                Expect::Bool => checked.expect_bool(),
                Expect::Unit(unit) => checked.expect_unit(&unit).map(|_| ()),
                Expect::Any => Ok(()),
            };
            match result {
                Ok(()) => (Outcome::Typed(checked.result_type()), None),
                Err(d) => fail(d),
            }
        }
        Err(d) if d.code() == "J0302" => {
            let path = source.get(d.span.range()).unwrap_or("");
            if registry.is_opaque(path) || opaque_prefix(path) {
                match joker_expr::parse(source) {
                    Ok(_) => (Outcome::Opaque(at(d.span)), None),
                    Err(d) => fail(d),
                }
            } else {
                fail(d)
            }
        }
        Err(d) => fail(d),
    }
}

/// The signal a typed expression produces, for `calc` results.
pub fn signal_of(ty: Ty, enums: &Registry<'_>) -> SignalInfo {
    use crate::context::Direction;
    match ty {
        Ty::Bool => SignalInfo::new(SignalType::Bool, Direction::Input),
        Ty::Number { unit, .. } if unit.is_dimensionless() => {
            SignalInfo::new(SignalType::Float, Direction::Input)
        }
        Ty::Number { unit, .. } => {
            SignalInfo::new(SignalType::Float, Direction::Input).with_unit(unit)
        }
        Ty::Enum(id) => {
            let found = enums.enums.iter().find(|(_, (i, _))| EnumId(*i) == id).map(
                |(name, (_, variants))| SignalType::Enum {
                    name: name.clone(),
                    variants: variants.clone(),
                },
            );
            SignalInfo::new(found.unwrap_or(SignalType::Unknown), Direction::Input)
        }
    }
}
