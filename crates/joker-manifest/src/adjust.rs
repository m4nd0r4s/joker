//! The manifest's adjustments of a device signal (ref/01 §5.8.1, §5.9)
//! composed with what the profile says (ref/03 §2.2.4, ADR 0075).
//!
//! One function, [`adjust`], is used by the check (to type the signal and
//! report what does not fit) and by the runtime (to acquire and write it),
//! so both read the same composition:
//!
//! - `scale` and `offset` apply after the profile's, in the profile's unit;
//!   the two-point `scale = { from, to }` maps `from` (in the profile's
//!   unit) onto `to`, whose unit becomes the signal's;
//! - `unit` converts the result to another unit of the same dimension;
//! - the profile's `raw × scale + offset` and all of the above fold into a
//!   single `raw × scale + offset` in the signal's unit; an integer signal
//!   that gains a `scale` or `offset` becomes a float (ref/03 §2.2.7);
//! - `limits`, `deadband`, `stuck_detect` and `debounce` are in the
//!   signal's final unit and replace the profile's; a profile `deadband`
//!   follows the signal's unit when the manifest changes it.

use std::collections::BTreeMap;

use joker_units::{Conversion, Duration, Literal, Unit};

use crate::context::{SignalInfo, SignalType};
use crate::diag::Diagnostic;
use crate::value::{Node, Value};

/// A signal with its adjustments applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Adjusted {
    /// The signal: its type, its unit and its acquisition steps, composed.
    pub info: SignalInfo,
    /// `max_age`, when the manifest declares one.
    pub max_age: Option<Duration>,
}

fn number(node: &Node) -> Option<f64> {
    match node.value {
        Value::Integer(i) => Some(i as f64),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

fn quantity(node: &Node, unit: Option<&Unit>) -> Option<f64> {
    let dimensionless = unit.is_none_or(Unit::is_dimensionless);
    match (&node.value, node.as_str().map(Literal::parse)) {
        (Value::Integer(_) | Value::Float(_), _) if dimensionless => number(node),
        (_, Some(Ok(Literal::Number(n)))) if dimensionless => Some(n),
        (_, Some(Ok(Literal::Quantity(q)))) => unit.and_then(|u| q.value_in(u).ok()),
        _ => None,
    }
}

fn duration(node: &Node) -> Option<Duration> {
    node.as_str().and_then(|s| Duration::parse(s).ok())
}

fn pair(node: &Node) -> Option<[&Node; 2]> {
    match &node.value {
        Value::Array(items) if items.len() == 2 => Some([&items[0], &items[1]]),
        _ => None,
    }
}

fn quantity_unit(node: &Node) -> Option<Unit> {
    match node.as_str().map(Literal::parse) {
        Some(Ok(Literal::Quantity(q))) => Some(q.unit),
        _ => None,
    }
}

/// Applies `tuning` (the merged `[devices.<d>.signals.<s>]` and
/// `[signals."<path>"]` keys of one signal) to `base`, the signal as its
/// profile or its declaration gives it. `from_profile` is true for a
/// profile signal, whose `unit` may only change within its dimension. The
/// errors carry the codes of ref/01 §5.8.1.
pub fn adjust(
    base: &SignalInfo,
    from_profile: bool,
    tuning: &BTreeMap<String, Node>,
) -> Result<Adjusted, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut fail = |code: &'static str, message: String, node: &Node| {
        errors.push(Diagnostic::new(code, message).at(node.span));
    };
    let numeric = matches!(base.ty, SignalType::Integer | SignalType::Float);
    let get = |key: &str| tuning.get(key);
    let mut info = base.clone();
    let profile_unit = base.unit;

    let mut map = Conversion::IDENTITY;
    let mut mapped_unit = profile_unit;
    let mut rescaled = false;
    if let Some(node) = get("scale") {
        rescaled = true;
        if !numeric {
            fail(
                "J0176",
                "`scale` only applies to a numeric signal".into(),
                node,
            );
        } else if let Some(k) = number(node) {
            if k == 0.0 || !k.is_finite() {
                fail(
                    "J0176",
                    "`scale` must be a number other than 0".into(),
                    node,
                );
            } else {
                map.scale = k;
            }
        } else if let Some(table) = node.as_table() {
            let from = table.get("from").and_then(|e| pair(&e.node));
            let to = table.get("to").and_then(|e| pair(&e.node));
            let to_unit = to.and_then(|t| quantity_unit(t[0]));
            let from_values = from.map(|f| f.map(|n| quantity(n, profile_unit.as_ref())));
            let to_values = to.map(|t| t.map(|n| quantity(n, to_unit.as_ref())));
            match (from_values, to_values) {
                (Some([Some(f0), Some(f1)]), Some([Some(t0), Some(t1)])) if f1 != f0 => {
                    map.scale = (t1 - t0) / (f1 - f0);
                    map.offset = t0 - f0 * map.scale;
                    mapped_unit = to_unit.or(profile_unit);
                    if map.scale == 0.0 {
                        fail("J0176", "`scale.to` maps everything to one value".into(), node);
                    }
                }
                _ => fail(
                    "J0107",
                    "`scale.from` must be two different quantities in the profile's unit, `scale.to` two quantities of one unit".into(),
                    node,
                ),
            }
            if let Some(offset) = get("offset") {
                fail(
                    "J0145",
                    "`offset` does not combine with `scale = { from, to }`".into(),
                    offset,
                );
            }
        } else {
            fail(
                "J0143",
                "`scale` is a number or `{ from, to }`".into(),
                node,
            );
        }
    }
    if let Some(node) = get("offset") {
        rescaled = true;
        if !numeric {
            fail(
                "J0176",
                "`offset` only applies to a numeric signal".into(),
                node,
            );
        } else {
            let difference = profile_unit.map(|u| u.difference());
            match quantity(node, difference.as_ref()) {
                Some(o) if o.is_finite() => map.offset += o,
                _ => fail(
                    "J0107",
                    "`offset` must be a number or a quantity in the dimension of the signal".into(),
                    node,
                ),
            }
        }
    }

    let mut final_unit = mapped_unit;
    let mut convert = Conversion::IDENTITY;
    if let Some(node) = get("unit") {
        match node.as_str().map(Unit::parse) {
            Some(Ok(u)) => match mapped_unit {
                Some(from) => match from.conversion_to(&u) {
                    Ok(c) => {
                        convert = c;
                        final_unit = Some(u);
                    }
                    Err(_) => fail(
                        "J0107",
                        format!(
                            "`unit` `{}` is not in the dimension of `{}`",
                            u.symbol(),
                            from.symbol()
                        ),
                        node,
                    ),
                },
                None if !from_profile => final_unit = Some(u),
                None => fail(
                    "J0107",
                    "the profile signal has no unit to convert".into(),
                    node,
                ),
            },
            _ => fail("J0143", "`unit` is not a unit".into(), node),
        }
    }

    if numeric {
        let acq = &mut info.acquisition;
        let s1 = acq.scale.unwrap_or(1.0);
        let o1 = acq.offset.unwrap_or(0.0);
        let scale = s1 * map.scale * convert.scale;
        let offset = (o1 * map.scale + map.offset) * convert.scale + convert.offset;
        let changed = map != Conversion::IDENTITY || !convert.is_identity();
        if changed {
            acq.scale = (scale != 1.0 || acq.scale.is_some()).then_some(scale);
            acq.offset = (offset != 0.0 || acq.offset.is_some()).then_some(offset);
            if let Some(d) = acq.deadband {
                acq.deadband = Some((d * map.scale * convert.scale).abs());
            }
        }
        if rescaled && base.ty == SignalType::Integer {
            info.ty = SignalType::Float;
        }
    }
    info.unit = final_unit;

    if let Some(node) = get("limits") {
        let bounds = pair(node).map(|b| b.map(|n| quantity(n, final_unit.as_ref())));
        match bounds {
            _ if !numeric => fail(
                "J0176",
                "`limits` only apply to a numeric signal".into(),
                node,
            ),
            Some([Some(lo), Some(hi)]) if lo < hi => info.acquisition.limits = Some((lo, hi)),
            _ => fail(
                "J0176",
                "`limits` are two quantities of the signal, the minimum first".into(),
                node,
            ),
        }
    }
    if let Some(node) = get("deadband") {
        let difference = final_unit.map(|u| u.difference());
        match quantity(node, difference.as_ref()) {
            _ if !numeric => fail(
                "J0176",
                "`deadband` only applies to a numeric signal".into(),
                node,
            ),
            Some(d) if d >= 0.0 && d.is_finite() => {
                info.acquisition.deadband = (d > 0.0).then_some(d);
            }
            _ => fail(
                "J0107",
                "`deadband` is a difference in the dimension of the signal".into(),
                node,
            ),
        }
    }
    if let Some(node) = get("stuck_detect") {
        match duration(node) {
            _ if base.ty == SignalType::Bool => fail(
                "J0176",
                "`stuck_detect` does not apply to a `bool`".into(),
                node,
            ),
            Some(d) if d.as_nanos() > 0 => info.acquisition.stuck_detect = Some(d),
            _ => fail(
                "J0816",
                "`stuck_detect` is a positive duration".into(),
                node,
            ),
        }
    }
    if let Some(node) = get("debounce") {
        match duration(node) {
            _ if base.ty != SignalType::Bool => {
                fail("J0176", "`debounce` only applies to a `bool`".into(), node)
            }
            Some(d) => info.acquisition.debounce = (d.as_nanos() > 0).then_some(d),
            None => fail("J0152", "invalid duration".into(), node),
        }
    }
    let max_age = match get("max_age") {
        Some(node) => match duration(node) {
            Some(d) if d.as_nanos() > 0 => Some(d),
            _ => {
                fail("J0816", "`max_age` is a positive duration".into(), node);
                None
            }
        },
        None => None,
    };
    if errors.is_empty() {
        Ok(Adjusted { info, max_age })
    } else {
        Err(errors)
    }
}

/// The adjustment keys `[signals."<path>"]` may set (ref/01 §5.9).
const SECTION_KEYS: [&str; 5] = ["debounce", "deadband", "max_age", "limits", "stuck_detect"];

/// The merged adjustments of `<device>.<signal>`: its
/// `[devices.<d>.signals.<s>]` table, and the keys of every
/// `[signals."<path>"]` entry whose path, alias or selector reaches it (the
/// check reports two different values for one key, `J0175`).
pub fn tuning(
    manifest: &crate::model::Manifest,
    paths: &crate::check::PathRegistry,
    device: &str,
    signal: &str,
) -> BTreeMap<String, Node> {
    let canonical = format!("{device}.{signal}");
    let mut merged: BTreeMap<String, Node> = manifest
        .devices
        .get(device)
        .and_then(|d| d.signals.get(signal))
        .map(|t| t.value.clone())
        .unwrap_or_default();
    let section = manifest.pending.get("signals").and_then(Node::as_table);
    for (key, entry) in section.into_iter().flatten() {
        let reaches = if crate::grammar::is_wildcard(key) {
            crate::grammar::matches(key, &canonical)
        } else {
            paths.get(key).is_some_and(|r| r.canonical == canonical)
        };
        if !reaches {
            continue;
        }
        for (name, item) in entry.node.as_table().into_iter().flatten() {
            if SECTION_KEYS.contains(&name.as_str()) {
                merged
                    .entry(name.clone())
                    .or_insert_with(|| item.node.clone());
            }
        }
    }
    merged
}
