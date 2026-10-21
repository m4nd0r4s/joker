//! Envelopes only tighten (ref/01 §7.4): each layer's `[outputs]` is
//! compared, key by key, with the envelope accumulated from the layers
//! before it, and every widening needs a `widen` justification, which the
//! lock records in `[accepted]` (ref/02 §2.1.14).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use joker_units::{Duration, Literal};

use crate::diag::{Diagnostic, Diagnostics};
use crate::grammar;
use crate::layers::{self, Layer};
use crate::schema;
use crate::source::Span;
use crate::value::{Node, Table, Value};

/// A widening covered by `widen`, to be approved in the lock.
#[derive(Debug, Clone, PartialEq)]
pub struct Widening {
    /// The literal key of `[outputs]`.
    pub output: String,
    /// The envelope key that widens.
    pub key: String,
    /// The justification.
    pub reason: String,
    /// The widened value, as written; `None` when the key was removed.
    pub value: Option<Node>,
    /// Where the `widen` is.
    pub span: Span,
}

/// An `accept_missing_fallback` of the manifest, to be approved in the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    /// The literal key of `[outputs]`.
    pub output: String,
    /// The justification.
    pub reason: String,
    /// Where it is written.
    pub span: Span,
}

enum Widen {
    All(String, Span),
    Keys(BTreeMap<String, (String, Span)>),
}

fn duration(node: Option<&Node>, default: Option<Duration>) -> Option<u64> {
    match node {
        Some(n) => n
            .as_str()
            .and_then(|s| Duration::parse(s).ok())
            .map(Duration::as_nanos),
        None => default.map(Duration::as_nanos),
    }
}

fn quantity(old: &Node, new: &Node) -> Option<Ordering> {
    let parse = |n: &Node| match &n.value {
        Value::String(s) => Literal::parse(s).ok(),
        Value::Integer(i) => Some(Literal::Number(*i as f64)),
        Value::Float(f) => Some(Literal::Number(*f)),
        _ => None,
    };
    match (parse(old)?, parse(new)?) {
        (Literal::Quantity(a), Literal::Quantity(b)) => {
            b.value_in(&a.unit).ok()?.partial_cmp(&a.value)
        }
        (Literal::Number(a), Literal::Number(b)) => b.partial_cmp(&a),
        _ => None,
    }
}

fn constant(node: &Node) -> Option<Literal> {
    match &node.value {
        Value::String(s) => Literal::parse(s).ok(),
        Value::Integer(i) => Some(Literal::Number(*i as f64)),
        Value::Float(f) => Some(Literal::Number(*f)),
        _ => None,
    }
}

fn same(a: Option<&Node>, b: Option<&Node>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => layers::same(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn strings(node: Option<&Node>) -> BTreeSet<String> {
    match node.map(|n| &n.value) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_owned))
            .collect(),
        Some(Value::String(s)) => [s.clone()].into_iter().collect(),
        _ => BTreeSet::new(),
    }
}

fn rank(node: Option<&Node>, order: &[&str], default: &str) -> Option<usize> {
    let text = node.map_or(Some(default), Node::as_str)?;
    order.iter().position(|o| *o == text)
}

fn per_hour(node: &Node) -> Option<f64> {
    let budget = grammar::budget(node.as_str()?)?;
    Some(f64::from(budget.count) * 3600e9 / budget.window.as_nanos() as f64)
}

/// Whether the change of one key, from `old` to `new`, widens the envelope.
fn widens(key: &str, old: Option<&Node>, new: Option<&Node>) -> bool {
    if same(old, new) {
        return false;
    }
    let smaller = |old: Option<&Node>, new: Option<&Node>| match (old, new) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(o), Some(n)) => quantity(o, n).is_some_and(|c| c != Ordering::Greater),
    };
    match key {
        "range" => match (old, new) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(o), Some(n)) => {
                let bounds = |node: &Node| match &node.value {
                    Value::Array(items) if items.len() == 2 => {
                        Some((items[0].clone(), items[1].clone()))
                    }
                    _ => None,
                };
                match (bounds(o), bounds(n)) {
                    (Some((olo, ohi)), Some((nlo, nhi)))
                        if [&olo, &ohi, &nlo, &nhi]
                            .iter()
                            .all(|b| constant(b).is_some()) =>
                    {
                        let lo_ok = quantity(&olo, &nlo).is_some_and(|c| c != Ordering::Less);
                        let hi_ok = quantity(&ohi, &nhi).is_some_and(|c| c != Ordering::Greater);
                        !(lo_ok && hi_ok)
                    }
                    _ => true,
                }
            }
        },
        "rate" | "safe_ramp" => !smaller(old, new),
        "min_interval" => match (duration(old, Some(Duration::ZERO)), duration(new, None)) {
            (_, None) => true,
            (Some(o), Some(n)) => n < o,
            _ => true,
        },
        "max_actuations" => match (old, new) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(o), Some(n)) => match (per_hour(o), per_hour(n)) {
                (Some(o), Some(n)) => n > o,
                _ => true,
            },
        },
        "interlock" => match (old.and_then(Node::as_str), new.and_then(Node::as_str)) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(o), Some(n)) => !n.starts_with(&format!("({}) && ", o.trim())),
        },
        "permit_uncertain" => matches!(new.map(|n| &n.value), Some(Value::Bool(true))),
        "requires_good" => !strings(old).is_subset(&strings(new)),
        "grace" => duration(new, Some(Duration::ZERO)) > duration(old, Some(Duration::ZERO)),
        "max_lease" => {
            duration(new, Some(Duration::from_secs(15 * 60)))
                > duration(old, Some(Duration::from_secs(15 * 60)))
        }
        "max_manual" => {
            duration(new, Some(Duration::from_secs(8 * 3600)))
                > duration(old, Some(Duration::from_secs(8 * 3600)))
        }
        "on_range" => {
            rank(new, &["clamp", "reject", "trip"], "clamp")
                < rank(old, &["clamp", "reject", "trip"], "clamp")
        }
        "on_interlock" => {
            rank(new, &["hold", "safe"], "safe") < rank(old, &["hold", "safe"], "safe")
        }
        "verify" => match (old.and_then(Node::as_table), new.and_then(Node::as_table)) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(o), Some(n)) => {
                !same(
                    o.get("readback").map(|e| &e.node),
                    n.get("readback").map(|e| &e.node),
                ) || (o.contains_key("tolerance")
                    && !smaller(
                        o.get("tolerance").map(|e| &e.node),
                        n.get("tolerance").map(|e| &e.node),
                    ))
                    || duration(n.get("within").map(|e| &e.node), None)
                        > duration(o.get("within").map(|e| &e.node), None)
            }
        },
        "reset_when" => new.is_some(),
        "safe" | "safer" => true,
        _ => false,
    }
}

fn widen_of(table: &Table, diags: &mut Diagnostics) -> Option<Widen> {
    let entry = table.get("widen")?;
    match &entry.node.value {
        Value::String(s) => Some(Widen::All(s.clone(), entry.node.span)),
        Value::Table(t) => {
            let mut keys = BTreeMap::new();
            for (key, item) in t {
                if key == "accept_missing_fallback" {
                    diags.push(
                        Diagnostic::new("J0167", "`widen` names `accept_missing_fallback`, which is never a widening")
                            .at(item.key)
                            .help("the key itself is the justification; the lock approves it in `[accepted]`"),
                    );
                    continue;
                }
                if !schema::ENVELOPE.iter().any(|k| k.name == key) || key == "widen" {
                    diags.push(
                        Diagnostic::new(
                            "J0166",
                            format!("`widen` on `{key}`, which is not an envelope key"),
                        )
                        .at(item.key),
                    );
                    continue;
                }
                if let Some(reason) = item.node.as_str() {
                    keys.insert(key.clone(), (reason.to_owned(), item.node.span));
                }
            }
            Some(Widen::Keys(keys))
        }
        _ => None,
    }
}

/// The outputs that a `joker-link` device governs in the merged manifest:
/// removing their envelope from the Linux manifest widens nothing.
fn governed_by_micro(merged: &Table) -> BTreeSet<String> {
    merged
        .get("devices")
        .and_then(|e| e.node.as_table())
        .into_iter()
        .flatten()
        .filter(|(_, e)| {
            e.node
                .as_table()
                .and_then(|d| d.get("driver"))
                .and_then(|d| d.node.as_str())
                == Some("joker-link")
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Checks that each layer only tightens the envelopes of the layers before
/// it, and returns the widenings that `widen` covers.
pub fn check(layers: &[Layer], merged: &Table, diags: &mut Diagnostics) -> Vec<Widening> {
    let micro = governed_by_micro(merged);
    let on_micro = |output: &str| output.split('.').next().is_some_and(|d| micro.contains(d));
    let mut accumulated: BTreeMap<String, Table> = BTreeMap::new();
    let mut widenings = Vec::new();
    for layer in layers {
        let Some(outputs) = layer.table.get("outputs").and_then(|e| e.node.as_table()) else {
            continue;
        };
        for (output, entry) in outputs {
            let previous = accumulated.get(output).cloned();
            if layers::is_unset(&entry.node) {
                if previous.is_some() && !on_micro(output) {
                    diags.push(
                        Diagnostic::new(
                            "J0220",
                            format!("overlay removes the envelope of `{output}`"),
                        )
                        .at(entry.key)
                        .help("a removal widens every key: it needs `widen`"),
                    );
                }
                accumulated.remove(output);
                continue;
            }
            let Some(table) = entry.node.as_table() else {
                continue;
            };
            let mut scratch = Diagnostics::default();
            let mut next = previous.clone().unwrap_or_default();
            layers::merge_table(&mut next, table, &mut scratch);
            next.remove("widen");
            let widen = widen_of(table, diags);
            let Some(previous) = previous else {
                if let Some(w) = &widen {
                    let span = match w {
                        Widen::All(_, span) => *span,
                        Widen::Keys(keys) => keys.values().next().map_or(entry.key, |(_, s)| *s),
                    };
                    diags.push(
                        Diagnostic::new(
                            "J0167",
                            format!("`widen` on `{output}`, which this layer declares first"),
                        )
                        .at(span),
                    );
                }
                accumulated.insert(output.clone(), next);
                continue;
            };
            let keys: BTreeSet<&String> = previous.keys().chain(next.keys()).collect();
            let mut widened = Vec::new();
            for key in keys {
                let old = previous.get(key).map(|e| &e.node);
                let new = next.get(key).map(|e| &e.node);
                if widens(key, old, new) {
                    widened.push(key.clone());
                }
            }
            for key in &widened {
                let span = table.get(key).map_or(entry.key, |e| e.key);
                let reason = match &widen {
                    Some(Widen::All(reason, s)) => Some((reason.clone(), *s)),
                    Some(Widen::Keys(keys)) => keys.get(key).cloned(),
                    None => None,
                };
                match reason {
                    Some((reason, span)) => widenings.push(Widening {
                        output: output.clone(),
                        key: key.clone(),
                        reason,
                        value: next.get(key).map(|e| e.node.clone()),
                        span,
                    }),
                    None => diags.push(
                        Diagnostic::new("J0220", format!("overlay widens `{key}` of `{output}` without `widen`"))
                            .at(span)
                            .help(format!("justify it with `widen = {{ {key} = \"…\" }}`, or keep the previous limit")),
                    ),
                }
            }
            match &widen {
                Some(Widen::All(_, span)) if widened.is_empty() => {
                    diags.push(
                        Diagnostic::new(
                            "J0167",
                            format!("`widen` on `{output}` in a layer that widens nothing"),
                        )
                        .at(*span),
                    );
                }
                Some(Widen::Keys(keys)) => {
                    for (key, (_, span)) in keys {
                        if !widened.contains(key) {
                            diags.push(Diagnostic::new("J0167", format!("`widen` of `{key}` on `{output}`, which this layer does not widen")).at(*span));
                        }
                    }
                }
                _ => {}
            }
            accumulated.insert(output.clone(), next);
        }
    }
    widenings
}

/// The `accept_missing_fallback` keys of the merged manifest.
pub fn acceptances(merged: &Table) -> Vec<Acceptance> {
    merged
        .get("outputs")
        .and_then(|e| e.node.as_table())
        .into_iter()
        .flatten()
        .filter_map(|(output, e)| {
            let item = e.node.as_table()?.get("accept_missing_fallback")?;
            Some(Acceptance {
                output: output.clone(),
                reason: item.node.as_str()?.to_owned(),
                span: item.node.span,
            })
        })
        .collect()
}
