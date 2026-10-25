//! Version compatibility (docs/17 §9, ADR 0059): the change from one
//! version of a class to the next, classified, and `JC0601` when the new
//! version does not bump what the change requires.

use std::collections::BTreeMap;

use crate::contract::{ContractFile, Param, Scalar};

/// What a change requires of the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    /// Nothing changed.
    None,
    /// Documentation or HMI annotations only.
    Patch,
    /// A compatible addition.
    Minor,
    /// A breaking change.
    Major,
}

/// One classified difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What it requires.
    pub bump: Bump,
    /// What changed, for `jk class diff`.
    pub what: String,
}

fn change(bump: Bump, what: impl Into<String>) -> Change {
    Change {
        bump,
        what: what.into(),
    }
}

fn by_name<T>(items: &[T], name: impl Fn(&T) -> &str) -> BTreeMap<&str, &T> {
    items.iter().map(|i| (name(i), i)).collect()
}

fn dimension(unit: Option<&String>) -> Option<joker_units::Dimension> {
    unit.and_then(|u| joker_units::Unit::parse(u).ok())
        .map(|u| u.dimension())
}

fn fields(kind: &str, name: &str, old: &[Param], new: &[Param], out: &mut Vec<Change>) {
    let common = old.len().min(new.len());
    let same_prefix = old.iter().zip(new.iter()).take(common).all(|(a, b)| a == b);
    if !same_prefix || new.len() < old.len() {
        out.push(change(
            Bump::Major,
            format!("{kind} `{name}`: fields reordered, removed or changed"),
        ));
    } else if new.len() > old.len() {
        out.push(change(
            Bump::Minor,
            format!("{kind} `{name}`: fields added at the end"),
        ));
    }
}

fn scalar(old: &Scalar, new: &Scalar, out: &mut Vec<Change>) {
    let name = &new.name;
    if old.direction != new.direction || old.ty != new.ty {
        out.push(change(
            Bump::Major,
            format!("member `{name}`: direction or type changed"),
        ));
    }
    if old.unit != new.unit {
        if dimension(old.unit.as_ref()) == dimension(new.unit.as_ref()) {
            out.push(change(
                Bump::Minor,
                format!("member `{name}`: unit of the same dimension"),
            ));
        } else {
            out.push(change(
                Bump::Major,
                format!("member `{name}`: dimension changed"),
            ));
        }
    }
    if old.capability != new.capability {
        out.push(change(
            Bump::Major,
            format!("member `{name}`: moved between capabilities"),
        ));
    }
    if old.optional && !new.optional {
        out.push(change(
            Bump::Major,
            format!("member `{name}`: optional made required"),
        ));
    } else if !old.optional && new.optional {
        out.push(change(
            Bump::Minor,
            format!("member `{name}`: made optional"),
        ));
    }
    if old.default != new.default
        || old.safe_hint != new.safe_hint
        || old.range_hint != new.range_hint
    {
        out.push(change(
            Bump::Minor,
            format!("member `{name}`: default or hints changed"),
        ));
    }
}

/// The changes from `old` to `new`.
pub fn diff(old: &ContractFile, new: &ContractFile) -> Vec<Change> {
    let mut out = Vec::new();
    let (o, n) = (&old.contract, &new.contract);
    for cap in &o.capabilities {
        if !n.capabilities.contains(cap) {
            out.push(change(Bump::Major, format!("capability `{cap}` removed")));
        }
    }
    for cap in &n.capabilities {
        if !o.capabilities.contains(cap) {
            out.push(change(Bump::Minor, format!("capability `{cap}` added")));
        }
    }
    let new_caps: Vec<&String> = n
        .capabilities
        .iter()
        .filter(|c| !o.capabilities.contains(c))
        .collect();
    let added_required = |cap: &Option<String>, optional: bool| {
        !optional && cap.as_ref().is_none_or(|c| !new_caps.contains(&c))
    };
    let om = by_name(&o.members, |m| &m.name);
    let nm = by_name(&n.members, |m| &m.name);
    for (name, old_m) in &om {
        match nm.get(name) {
            Some(new_m) => scalar(old_m, new_m, &mut out),
            None => {
                let renamed = n.members.iter().find(|m| {
                    m.renamed_from.as_deref() == Some(name)
                        && m.direction == old_m.direction
                        && m.ty == old_m.ty
                });
                match renamed {
                    Some(m) => out.push(change(
                        Bump::Minor,
                        format!("member `{name}` renamed to `{}`", m.name),
                    )),
                    None => out.push(change(Bump::Major, format!("member `{name}` removed"))),
                }
            }
        }
    }
    for (name, m) in &nm {
        if om.contains_key(name)
            || m.renamed_from
                .as_deref()
                .is_some_and(|r| om.contains_key(r))
        {
            continue;
        }
        let bump = if added_required(&m.capability, m.optional) {
            Bump::Major
        } else {
            Bump::Minor
        };
        out.push(change(bump, format!("member `{name}` added")));
    }
    let oc = by_name(&o.class_members, |m| &m.name);
    let nc = by_name(&n.class_members, |m| &m.name);
    for (name, old_c) in &oc {
        match nc.get(name) {
            Some(new_c) if new_c == old_c => {}
            Some(new_c) => {
                let bump = if new_c.class == old_c.class
                    && new_c.capability == old_c.capability
                    && new_c.min <= old_c.min
                    && new_c.max >= old_c.max
                    && new_c.requires == old_c.requires
                    && (new_c.optional || !old_c.optional)
                {
                    Bump::Minor
                } else {
                    Bump::Major
                };
                out.push(change(bump, format!("class member `{name}` changed")));
            }
            None => out.push(change(
                Bump::Major,
                format!("class member `{name}` removed"),
            )),
        }
    }
    for (name, c) in &nc {
        if !oc.contains_key(name) {
            let bump = if added_required(&c.capability, c.optional) {
                Bump::Major
            } else {
                Bump::Minor
            };
            out.push(change(bump, format!("class member `{name}` added")));
        }
    }
    macro_rules! items {
        ($field:ident, $kind:literal, $compare:expr) => {
            let oi = by_name(&o.$field, |x| &x.name);
            let ni = by_name(&n.$field, |x| &x.name);
            for (name, old_i) in &oi {
                match ni.get(name) {
                    Some(new_i) => {
                        let compare: &dyn Fn(&_, &_, &mut Vec<Change>) = &$compare;
                        compare(*old_i, *new_i, &mut out);
                    }
                    None => {
                        let renamed = ni.values().any(|x| x.renamed_from.as_deref() == Some(name));
                        let bump = if renamed { Bump::Minor } else { Bump::Major };
                        out.push(change(
                            bump,
                            format!("{} `{name}` removed or renamed", $kind),
                        ));
                    }
                }
            }
            for (name, new_i) in &ni {
                if !oi.contains_key(name)
                    && !new_i
                        .renamed_from
                        .as_deref()
                        .is_some_and(|r| oi.contains_key(r))
                {
                    out.push(change(Bump::Minor, format!("{} `{name}` added", $kind)));
                }
            }
        };
    }
    items!(
        queries,
        "query",
        |a: &crate::contract::QueryDef, b: &crate::contract::QueryDef, out: &mut Vec<Change>| {
            if a.ty != b.ty || a.expr != b.expr || a.capability != b.capability {
                out.push(change(
                    Bump::Major,
                    format!("query `{}`: type or body changed", b.name),
                ));
            } else if a.unit != b.unit {
                let bump = if dimension(a.unit.as_ref()) == dimension(b.unit.as_ref()) {
                    Bump::Minor
                } else {
                    Bump::Major
                };
                out.push(change(bump, format!("query `{}`: unit changed", b.name)));
            }
        }
    );
    items!(
        methods,
        "method",
        |a: &crate::contract::MethodDef, b: &crate::contract::MethodDef, out: &mut Vec<Change>| {
            if a.params != b.params
                || a.body != b.body
                || a.atomic != b.atomic
                || a.order != b.order
                || a.capability != b.capability
            {
                out.push(change(
                    Bump::Major,
                    format!("method `{}`: signature or body changed", b.name),
                ));
            }
        }
    );
    items!(
        events,
        "event",
        |a: &crate::contract::EventDef, b: &crate::contract::EventDef, out: &mut Vec<Change>| {
            if a.capability != b.capability {
                out.push(change(
                    Bump::Major,
                    format!("event `{}`: moved between capabilities", b.name),
                ));
            }
            fields("event", &b.name, &a.fields, &b.fields, out);
        }
    );
    items!(
        actions,
        "action",
        |a: &crate::contract::ActionDef, b: &crate::contract::ActionDef, out: &mut Vec<Change>| {
            if a.result != b.result || a.capability != b.capability || a.long != b.long {
                out.push(change(
                    Bump::Major,
                    format!("action `{}`: result, capability or `@long` changed", b.name),
                ));
            }
            fields("action", &b.name, &a.params, &b.params, out);
            if a.timeout != b.timeout
                || a.permit_hint != b.permit_hint
                || a.role_hint != b.role_hint
            {
                out.push(change(
                    Bump::Minor,
                    format!("action `{}`: hints changed", b.name),
                ));
            }
        }
    );
    if o.health != n.health {
        out.push(change(Bump::Major, "`health required` changed"));
    }
    let oe = by_name(&o.enums, |e| &e.name);
    for e in &n.enums {
        if let Some(old_e) = oe.get(e.name.as_str()) {
            let kept = old_e.variants.iter().all(|v| e.variants.contains(v));
            if !kept {
                out.push(change(
                    Bump::Major,
                    format!("enum `{}`: variant removed or renumbered", e.name),
                ));
            } else if e.variants.len() > old_e.variants.len() {
                out.push(change(
                    Bump::Minor,
                    format!("enum `{}`: variants added", e.name),
                ));
            }
        }
    }
    let or = by_name(&o.records, |r| &r.name);
    for r in &n.records {
        if let Some(old_r) = or.get(r.name.as_str()) {
            fields("record", &r.name, &old_r.fields, &r.fields, &mut out);
        }
    }
    if old.docs != new.docs {
        out.push(change(Bump::Patch, "documentation or HMI annotations"));
    }
    out
}

/// The largest bump the changes require.
pub fn required(changes: &[Change]) -> Bump {
    changes.iter().map(|c| c.bump).max().unwrap_or(Bump::None)
}

/// `JC0601` when `new` does not bump the version enough over `old`, with
/// the reason; `Ok` with the changes otherwise. A different class or major
/// is not compared.
pub fn check(old: &ContractFile, new: &ContractFile) -> Result<Vec<Change>, String> {
    let (ov, nv) = (old.contract.version, new.contract.version);
    if old.contract.class != new.contract.class || nv.major > ov.major {
        return Ok(Vec::new());
    }
    let changes = diff(old, new);
    let need = required(&changes);
    let given = if nv.major < ov.major || nv < ov {
        return Err(format!("version {nv} is older than {ov}"));
    } else if nv.minor > ov.minor {
        Bump::Minor
    } else if nv.patch > ov.patch {
        Bump::Patch
    } else {
        Bump::None
    };
    if need > given {
        let first = changes
            .iter()
            .filter(|c| c.bump == need)
            .map(|c| c.what.as_str())
            .next()
            .unwrap_or("");
        let what = match need {
            Bump::Major => "a new major",
            Bump::Minor => "at least a new minor",
            Bump::Patch => "at least a new patch",
            Bump::None => "nothing",
        };
        return Err(format!("{ov} → {nv} needs {what}: {first}"));
    }
    Ok(changes)
}
