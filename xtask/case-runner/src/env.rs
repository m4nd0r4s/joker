//! The fixture as the expression checker sees it (the `Env` of
//! `joker-expr`): every path a case can name, with its type and unit, and
//! the selectors of ref/01 §3.9.

use joker_core::ValueType;
use joker_core::class::ARG_SIGNAL_BASE;
use joker_core::registry::Registry;
use joker_expr::{
    CheckOptions, CompileOptions, Diagnostic, EnumId, Env, Program, SignalInfo, SignalType,
};
use joker_units::{Duration, Unit};

use crate::model::{Fixture, ParamSpec};

/// The expression type of a signal type.
pub fn signal_type(ty: ValueType) -> SignalType {
    match ty {
        ValueType::Bool => SignalType::Bool,
        ValueType::I64 => SignalType::I64,
        ValueType::U64 => SignalType::U64,
        ValueType::F64 => SignalType::F64,
        ValueType::F32 => SignalType::F32,
        ValueType::Enum => SignalType::Enum(EnumId(0)),
        ValueType::Bits => SignalType::Bits,
        ValueType::String => SignalType::String,
        ValueType::Bytes => SignalType::Bytes,
        ValueType::Record => SignalType::Record,
    }
}

/// The unit of a declared unit text, `None` when dimensionless or not in
/// the catalogue (the loader reports the latter).
pub fn unit_of(text: &str) -> Option<Unit> {
    match text {
        "" | "1" => None,
        _ => Unit::parse(text)
            .ok()
            .filter(|u| !u.is_dimensionless() || u.kind().is_some()),
    }
}

/// True when the path matches the selector (ref/01 §3.9): `*` inside a
/// segment matches characters of that segment, and a `**` segment matches
/// one or more segments.
pub fn selects(selector: &str, path: &str) -> bool {
    segments_match(selector, path)
}

fn split(text: &str) -> (&str, Option<&str>) {
    match text.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (text, None),
    }
}

fn segments_match(pattern: &str, path: &str) -> bool {
    let (first, rest) = split(pattern);
    if first == "**" {
        let mut tail = Some(path);
        while let Some(t) = tail {
            let (_, next) = split(t);
            let matched = match (rest, next) {
                (None, None) => true,
                (Some(r), Some(n)) => segments_match(r, n),
                _ => false,
            };
            if matched {
                return true;
            }
            tail = next;
        }
        return false;
    }
    let (seg, next) = split(path);
    if !segment_matches(first, seg) {
        return false;
    }
    match (rest, next) {
        (None, None) => true,
        (Some(r), Some(n)) => segments_match(r, n),
        _ => false,
    }
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == segment,
        Some((head, tail)) => {
            let Some(rest) = segment.strip_prefix(head) else {
                return false;
            };
            (0..=rest.len())
                .filter(|&i| rest.is_char_boundary(i))
                .any(|i| rest.get(i..).is_some_and(|r| segment_matches(tail, r)))
        }
    }
}

/// The signals of a fixture: inputs, outputs, derived signals, the
/// `state` and `active` signals of the alarms and the commit registers.
/// With a registry, each signal has its `SignalId`; without one, its index
/// in this enumeration.
#[derive(Copy, Clone, Debug)]
pub struct FixtureEnv<'f> {
    fixture: &'f Fixture<'f>,
    registry: Option<&'f Registry<'f>>,
}

impl<'f> FixtureEnv<'f> {
    /// The environment of `fixture`, with the ids of `registry` if given.
    pub fn new(fixture: &'f Fixture<'f>, registry: Option<&'f Registry<'f>>) -> Self {
        Self { fixture, registry }
    }

    /// Every signal, in fixture order: (path, type, unit text).
    fn each(&self, mut f: impl FnMut(&'f str, ValueType, &'f str) -> bool) {
        let fx = self.fixture;
        for s in fx.signals {
            if !f(s.path, s.ty, s.unit) {
                return;
            }
        }
        for o in fx.outputs {
            if !f(o.path, o.ty, o.unit) {
                return;
            }
        }
        for c in fx.calcs {
            if !f(c.path, c.ty, c.unit) {
                return;
            }
        }
        for a in fx.alarms {
            if !f(a.state_path, ValueType::Enum, "1") || !f(a.active_path, ValueType::Bool, "1") {
                return;
            }
        }
        for g in fx.atomic {
            if let Some(c) = g.commit
                && !f(c.path, c.ty, "1")
            {
                return;
            }
        }
    }

    fn info(&self, path: &str) -> Option<SignalInfo> {
        let mut index = 0u32;
        let mut found = None;
        self.each(|p, ty, unit| {
            if p == path {
                found = Some((index, ty, unit));
                return false;
            }
            index += 1;
            true
        });
        let (index, ty, unit) = found?;
        let id = match self.registry {
            Some(r) => r.resolve(path)?.0,
            None => index,
        };
        Some(SignalInfo {
            id,
            ty: signal_type(ty),
            unit: unit_of(unit),
        })
    }

    /// Checks and compiles `src` for a group of period `period_ns`.
    pub fn compile(&self, src: &str, period_ns: u64) -> Result<Program, Diagnostic> {
        let checked = joker_expr::check(src, self, CheckOptions::default())?;
        joker_expr::compile(
            &checked,
            CompileOptions {
                period: Duration::from_nanos(period_ns),
            },
        )
    }

    /// Checks and compiles the expression `src` of a class method whose
    /// parameters are `params`: parameter `i` is read as the signal
    /// `ARG_SIGNAL_BASE + i` (ADR 0041), and `own()` is allowed.
    pub fn compile_method(
        &self,
        src: &str,
        period_ns: u64,
        params: &[ParamSpec<'_>],
    ) -> Result<Program, Diagnostic> {
        let env = MethodEnv { base: self, params };
        let checked = joker_expr::check(
            src,
            &env,
            CheckOptions {
                in_class_method: true,
            },
        )?;
        joker_expr::compile(
            &checked,
            CompileOptions {
                period: Duration::from_nanos(period_ns),
            },
        )
    }

    /// Calls `found` with every path that `selector` matches, in the
    /// lexicographic order of the paths, each once.
    pub fn select_paths(&self, selector: &str, mut found: impl FnMut(&'f str)) {
        let mut last: Option<&str> = None;
        loop {
            let mut next: Option<&'f str> = None;
            self.each(|p, _, _| {
                if selects(selector, p) && last.is_none_or(|l| p > l) && next.is_none_or(|n| p < n)
                {
                    next = Some(p);
                }
                true
            });
            match next {
                Some(p) => {
                    found(p);
                    last = Some(p);
                }
                None => return,
            }
        }
    }
}

impl Env for FixtureEnv<'_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        self.info(path)
    }

    fn enum_variant(&self, _enum_name: &str, _variant: &str) -> Option<(EnumId, u64)> {
        None
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        self.select_paths(selector, |path| {
            if let Some(info) = self.info(path) {
                found(info);
            }
        });
    }
}

struct MethodEnv<'e, 'f> {
    base: &'e FixtureEnv<'f>,
    params: &'e [ParamSpec<'e>],
}

impl Env for MethodEnv<'_, '_> {
    fn signal(&self, path: &str) -> Option<SignalInfo> {
        match self.params.iter().position(|p| p.name == path) {
            Some(i) => {
                let p = self.params.get(i)?;
                Some(SignalInfo {
                    id: ARG_SIGNAL_BASE + u32::try_from(i).ok()?,
                    ty: signal_type(p.ty),
                    unit: unit_of(p.unit),
                })
            }
            None => self.base.info(path),
        }
    }

    fn enum_variant(&self, enum_name: &str, variant: &str) -> Option<(EnumId, u64)> {
        self.base.enum_variant(enum_name, variant)
    }

    fn select(&self, selector: &str, found: &mut dyn FnMut(SignalInfo)) {
        self.base.select(selector, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_follow_ref01_3_9() {
        assert!(selects("x.s*", "x.s1"));
        assert!(selects("x.s*", "x.s"));
        assert!(!selects("x.s*", "x.a"));
        assert!(!selects("x.s*", "x.s1.v"));
        assert!(selects("inv*.ac.p", "inv12.ac.p"));
        assert!(selects("p1.acc.*", "p1.acc.x"));
        assert!(selects("meter.**", "meter.l1.v"));
        assert!(selects("meter.**", "meter.f"));
        assert!(!selects("meter.**", "meter"));
        assert!(selects("**", "a.b.c"));
        assert!(selects("x.f", "x.f"));
        assert!(!selects("x.f", "x.f.g"));
    }
}
