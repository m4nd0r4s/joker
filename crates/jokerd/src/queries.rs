//! The read side of the historian and of the alarm journal in the API
//! (ref/09 §5.5, §5.8, ADR 0082).

use joker_api::{Problem, Query};
use joker_core::Quality;
use joker_store::{Aggregate, SeriesId, Store, Tier, ValueKind, aggregate, reaggregate};
use serde_json::{Map, Value, json};

use crate::plan::{Plan, SignalPlan};
use crate::wire::{self, duration, parse_duration, timestamp};

/// Points per page by default (ref/09 §5.5).
pub const DEFAULT_POINTS: usize = 10_000;
/// The most points one page may ask for.
pub const MAX_POINTS: usize = 100_000;
/// Selectors per request (ref/09 §3.11).
pub const MAX_SELECTORS: usize = 64;
/// Signals a request may resolve (ref/09 §3.11).
pub const MAX_RESOLVED: usize = 10_000;
/// Default `from` of a history query: one hour before `to`.
pub const DEFAULT_SPAN_NS: u64 = 3_600 * 1_000_000_000;
const MAX_RAW_FOR_STEP: usize = 5_000_000;

fn bad(code: &'static str, detail: impl Into<String>) -> Problem {
    Problem::new(code, 400, detail)
}

/// Every value of a parameter, comma lists split.
pub fn values_of(query: &Query, key: &str) -> Vec<String> {
    query
        .iter()
        .filter(|(k, _)| k == key)
        .flat_map(|(_, v)| v.split(',').map(str::to_owned).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .collect()
}

/// The last value of a parameter.
pub fn one(query: &Query, key: &str) -> Option<String> {
    query
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// An instant parameter (ref/09 §3.3): RFC 3339, `now`, or `-<duration>`.
pub fn instant(text: &str, now_ns: i64, param: &str) -> Result<i64, Problem> {
    let invalid = || {
        bad("J-E1034", format!("`{param}`: `{text}` is not an instant")).with("param", json!(param))
    };
    if text == "now" {
        return Ok(now_ns);
    }
    if let Some(back) = text.strip_prefix('-') {
        let ns = parse_duration(back).map_err(|_| invalid())?;
        return Ok(now_ns.saturating_sub(i64::try_from(ns).unwrap_or(i64::MAX)));
    }
    let t = humantime::parse_rfc3339_weak(text)
        .or_else(|_| humantime::parse_rfc3339(text))
        .map_err(|_| invalid())?;
    let d = t
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid())?;
    i64::try_from(d.as_nanos()).map_err(|_| invalid())
}

/// `from`, `to` and `since` (ref/09 §3.7); `default_back` is how far
/// before `to` an absent `from` is (`None`: unbounded).
pub fn range(query: &Query, now_ns: i64, default_back: Option<u64>) -> Result<(i64, i64), Problem> {
    let to = match one(query, "to") {
        Some(t) => instant(&t, now_ns, "to")?,
        None => now_ns,
    };
    let from_text = one(query, "from");
    let since = one(query, "since");
    let from = match (from_text, since) {
        (Some(_), Some(_)) => {
            return Err(bad("J-E1034", "`since` and `from` together").with("param", json!("since")));
        }
        (Some(f), None) => instant(&f, now_ns, "from")?,
        (None, Some(s)) => {
            let ns =
                parse_duration(&s).map_err(|e| bad("J-E1034", e).with("param", json!("since")))?;
            to.saturating_sub(i64::try_from(ns).unwrap_or(i64::MAX))
        }
        (None, None) => match default_back {
            Some(back) => to.saturating_sub(i64::try_from(back).unwrap_or(i64::MAX)),
            None => i64::MIN,
        },
    };
    if from > to {
        return Err(bad("J-E1034", "`from` is after `to`").with("param", json!("from")));
    }
    Ok((from, to))
}

fn limit(query: &Query, default: usize, max: usize, code: &'static str) -> Result<usize, Problem> {
    let Some(text) = one(query, "limit") else {
        return Ok(default);
    };
    let n: usize = text
        .parse()
        .map_err(|_| bad("J-E1012", "bad `limit`").with("param", json!("limit")))?;
    if n == 0 {
        return Err(bad("J-E1012", "bad `limit`").with("param", json!("limit")));
    }
    if n > max {
        return Err(
            Problem::new(code, 422, format!("`limit` above {max}")).with("param", json!("limit"))
        );
    }
    Ok(n)
}

fn valid_selector(sel: &str) -> bool {
    let parts: Vec<&str> = sel.split('.').collect();
    sel.len() <= 288
        && parts.len() <= 8
        && parts.iter().all(|p| {
            *p == "**"
                || (!p.is_empty()
                    && p.len() <= 32
                    && p.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_lowercase() || c == '*')
                    && p.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '*'
                    }))
        })
}

fn selectors(query: &Query, required: bool) -> Result<Vec<String>, Problem> {
    let sel = values_of(query, "sel");
    if required && sel.is_empty() {
        return Err(bad("J-E1013", "`sel` is required").with("param", json!("sel")));
    }
    if sel.len() > MAX_SELECTORS {
        return Err(
            bad("J-E1030", format!("more than {MAX_SELECTORS} selectors"))
                .with("param", json!("sel")),
        );
    }
    if let Some(s) = sel.iter().find(|s| !valid_selector(s)) {
        return Err(bad("J-E1030", format!("malformed selector `{s}`")).with("param", json!("sel")));
    }
    Ok(sel)
}

const AGGREGATES: [&str; 7] = ["avg", "min", "max", "first", "last", "count", "quality"];
const DEFAULT_AGG: [&str; 5] = ["avg", "min", "max", "count", "quality"];

fn quality_name(code: u32) -> &'static str {
    match code {
        0 => "good",
        1 => "uncertain",
        _ => "bad",
    }
}

fn number(v: f64) -> Value {
    if v.is_finite() { json!(v) } else { Value::Null }
}

fn raw_point(signal: &SignalPlan, s: &joker_store::Sample) -> Value {
    let q = Quality::from_bits(s.quality);
    let mut m = Map::new();
    m.insert("ts".into(), json!(timestamp(s.ts_ns)));
    if let Some(src) = s.ts_src_ns {
        m.insert("ts_src".into(), json!(timestamp(src)));
    }
    let value = if q.is_bad() {
        Value::Null
    } else {
        wire::value(signal, s.bits)
    };
    m.insert("value".into(), value);
    m.insert("quality".into(), json!(wire::quality_name(q)));
    m.insert("reason".into(), json!(wire::reason_name(q)));
    m.insert("seq".into(), json!(s.seq));
    Value::Object(m)
}

fn agg_point(signal: &SignalPlan, a: &Aggregate, aggs: &[&str]) -> Value {
    let mut m = Map::new();
    m.insert("ts".into(), json!(timestamp(a.ts_ns)));
    for name in aggs {
        let v = match *name {
            "avg" => number(a.mean),
            "min" => number(a.min),
            "max" => number(a.max),
            "first" if a.valid > 0 => wire::value(signal, a.first),
            "last" if a.valid > 0 => wire::value(signal, a.last),
            "first" | "last" => Value::Null,
            "count" => json!(a.count),
            _ => continue,
        };
        m.insert((*name).to_owned(), v);
    }
    m.insert("quality".into(), json!(quality_name(a.quality)));
    Value::Object(m)
}

fn empty_bucket(ts: i64) -> Aggregate {
    Aggregate {
        ts_ns: ts,
        count: 0,
        valid: 0,
        min: f64::NAN,
        max: f64::NAN,
        mean: f64::NAN,
        first: 0,
        last: 0,
        quality: 2,
    }
}

fn fill(points: Vec<Aggregate>, from: i64, to: i64, step: u64, mode: &str) -> Vec<Aggregate> {
    if mode == "none" {
        return points;
    }
    let step_i = i64::try_from(step).unwrap_or(i64::MAX).max(1);
    let mut out = Vec::new();
    let mut it = points.into_iter().peekable();
    let mut previous: Option<Aggregate> = None;
    let mut ts = from.div_euclid(step_i) * step_i;
    let mut guard = 0usize;
    while ts <= to && guard < MAX_POINTS * 10 {
        guard += 1;
        match it.peek() {
            Some(p) if p.ts_ns == ts => {
                let p = *p;
                it.next();
                previous = Some(p);
                out.push(p);
            }
            _ => {
                let filler = match (mode, previous) {
                    ("previous", Some(p)) => Aggregate { ts_ns: ts, ..p },
                    _ => empty_bucket(ts),
                };
                out.push(filler);
            }
        }
        ts = ts.saturating_add(step_i);
    }
    out
}

fn tier_param(query: &Query) -> Result<Option<Tier>, Problem> {
    match one(query, "tier").as_deref() {
        None | Some("auto") => Ok(None),
        Some(t) => Tier::from_name(t).map(Some).ok_or_else(|| {
            bad("J-E1039", format!("unknown tier `{t}`")).with("param", json!("tier"))
        }),
    }
}

/// Where a page starts: series index, and the key after which points
/// follow (a store sequence number for raw points, an instant for
/// aggregates).
fn cursor(query: &Query) -> Result<(usize, i64), Problem> {
    let Some(c) = one(query, "cursor") else {
        return Ok((0, i64::MIN));
    };
    let invalid = || Problem::new("J-E1032", 400, "invalid cursor").with("param", json!("cursor"));
    let (s, k) = c.split_once('.').ok_or_else(invalid)?;
    let s = s.strip_prefix('h').ok_or_else(invalid)?;
    Ok((
        s.parse().map_err(|_| invalid())?,
        k.parse().map_err(|_| invalid())?,
    ))
}

fn choose_tier(store: &Store, id: SeriesId, step: u64, from: i64) -> (Tier, bool) {
    let candidates: Vec<Tier> = Tier::ALL
        .into_iter()
        .filter(|t| t.resolution_ns() <= step)
        .collect();
    for t in candidates.iter().rev() {
        if store.coverage(id, *t).is_some_and(|(lo, _)| lo <= from) {
            return (*t, true);
        }
    }
    for t in &candidates {
        if store.coverage(id, *t).is_some() {
            return (*t, false);
        }
    }
    (Tier::Raw, false)
}

/// `GET /v1/history` (ref/09 §5.5) over the signals of `plan`.
pub fn history(
    store: &Store,
    plan: &Plan,
    generation: Value,
    query: &Query,
    now_ns: i64,
) -> Result<Value, Problem> {
    let sel = selectors(query, true)?;
    let signals = wire::select(plan, &sel);
    if signals.len() > MAX_RESOLVED {
        return Err(Problem::new(
            "J-E1031",
            422,
            format!("the selectors resolve to more than {MAX_RESOLVED} signals"),
        ));
    }
    let (from, to) = range(query, now_ns, Some(DEFAULT_SPAN_NS))?;
    let fixed = tier_param(query)?;
    let mut step = one(query, "step")
        .map(|s| parse_duration(&s).map_err(|e| bad("J-E1039", e).with("param", json!("step"))))
        .transpose()?;
    if step.is_some_and(|s| s < 1_000_000) {
        return Err(bad("J-E1039", "`step` below 1ms").with("param", json!("step")));
    }
    if step.is_none()
        && let Some(t) = fixed.filter(|t| *t != Tier::Raw)
    {
        step = Some(t.resolution_ns());
    }
    if let (Some(s), Some(t)) = (step, fixed)
        && s < t.resolution_ns()
    {
        return Err(bad(
            "J-E1039",
            format!("`step` below the resolution of `{}`", t.name()),
        )
        .with("param", json!("step")));
    }
    let asked: Vec<String> = values_of(query, "agg");
    if step.is_none() && !asked.is_empty() {
        return Err(bad("J-E1039", "`agg` needs a `step`").with("param", json!("agg")));
    }
    if let Some(a) = asked.iter().find(|a| !AGGREGATES.contains(&a.as_str())) {
        return Err(bad("J-E1039", format!("unknown aggregate `{a}`")).with("param", json!("agg")));
    }
    let fill_mode = one(query, "fill").unwrap_or_else(|| "none".into());
    if !["none", "previous", "null"].contains(&fill_mode.as_str()) {
        return Err(
            bad("J-E1012", format!("unknown fill `{fill_mode}`")).with("param", json!("fill"))
        );
    }
    let limit = limit(query, DEFAULT_POINTS, MAX_POINTS, "J-E1038")?;
    let (start_series, after) = cursor(query)?;

    let mut series_out = Vec::new();
    let mut coverage = Vec::new();
    let mut warnings = Vec::new();
    let mut used_tier = fixed.unwrap_or(Tier::Raw);
    let mut left = limit;
    let mut next_cursor = Value::Null;
    for (index, signal) in signals.iter().enumerate() {
        if index < start_series {
            continue;
        }
        let Some(id) = store.series(&signal.path) else {
            continue;
        };
        let kind = store.value_kind(id).unwrap_or(ValueKind::Other);
        let after_here = if index == start_series {
            after
        } else {
            i64::MIN
        };
        let mut points = Vec::new();
        let mut last_key = None;
        match step {
            None => {
                let after_n = u64::try_from(after_here).unwrap_or(0);
                let rows = store
                    .read_raw(id, from, to, after_n, left.saturating_add(1))
                    .map_err(|e| Problem::new("J-E1000", 500, format!("historian: {e}")))?;
                for (n, s) in rows {
                    if points.len() == left {
                        next_cursor = json!(format!("h{index}.{}", last_key.unwrap_or(0)));
                        break;
                    }
                    points.push(raw_point(signal, &s));
                    last_key = Some(i64::try_from(n).unwrap_or(i64::MAX));
                }
            }
            Some(step) => {
                let aggs: Vec<&str> = if asked.is_empty() {
                    DEFAULT_AGG
                        .iter()
                        .copied()
                        .filter(|a| kind != ValueKind::Other || !["avg", "min", "max"].contains(a))
                        .collect()
                } else {
                    if kind == ValueKind::Other
                        && let Some(a) = asked
                            .iter()
                            .find(|a| ["avg", "min", "max"].contains(&a.as_str()))
                    {
                        return Err(bad(
                            "J-E1039",
                            format!("`{a}` on `{}`, which is not numeric", signal.path),
                        )
                        .with("param", json!("agg")));
                    }
                    asked.iter().map(String::as_str).collect()
                };
                let (tier, covers) = match fixed {
                    Some(t) => (t, store.coverage(id, t).is_some_and(|(lo, _)| lo <= from)),
                    None => choose_tier(store, id, step, from),
                };
                used_tier = tier;
                let step_i = i64::try_from(step).unwrap_or(i64::MAX).max(1);
                let first = from.div_euclid(step_i) * step_i;
                let buckets = if tier == Tier::Raw {
                    let rows = store
                        .read_raw(id, first, to, 0, MAX_RAW_FOR_STEP)
                        .map_err(|e| Problem::new("J-E1000", 500, format!("historian: {e}")))?;
                    aggregate(kind, &rows, step)
                } else {
                    let rows = store
                        .read_agg(id, tier, first, to, MAX_RAW_FOR_STEP)
                        .map_err(|e| Problem::new("J-E1000", 500, format!("historian: {e}")))?;
                    reaggregate(&rows, step)
                };
                if let Some((lo, hi)) = store.coverage(id, tier) {
                    coverage.push(json!({
                        "path": signal.path,
                        "tier": tier.name(),
                        "from": timestamp(lo),
                        "to": timestamp(hi),
                    }));
                }
                if !covers {
                    warnings.push(json!({
                        "code": "J-E1039",
                        "severity": "warning",
                        "message": format!("`{}`: no tier covers the whole range; the response carries what there is", signal.path),
                    }));
                }
                let buckets: Vec<Aggregate> = buckets
                    .into_iter()
                    .filter(|a| a.ts_ns >= first && a.ts_ns <= to)
                    .collect();
                for a in fill(buckets, from, to, step, &fill_mode)
                    .into_iter()
                    .filter(|a| a.ts_ns > after_here)
                {
                    if points.len() == left {
                        next_cursor = json!(format!("h{index}.{}", last_key.unwrap_or(i64::MIN)));
                        break;
                    }
                    points.push(agg_point(signal, &a, &aggs));
                    last_key = Some(a.ts_ns);
                }
            }
        }
        left -= points.len();
        let mut s = Map::new();
        s.insert("path".into(), json!(signal.path));
        s.insert("type".into(), json!(wire::type_name(signal.ty)));
        if signal.unit != "1" && !signal.unit.is_empty() {
            s.insert("unit".into(), json!(signal.unit));
        }
        s.insert("points".into(), Value::Array(points));
        series_out.push(Value::Object(s));
        if !next_cursor.is_null() {
            break;
        }
        if left == 0 && index + 1 < signals.len() {
            next_cursor = json!(format!("h{}.{}", index + 1, i64::MIN));
            break;
        }
    }
    let mut out = Map::new();
    out.insert("generation".into(), generation);
    out.insert("from".into(), json!(timestamp(from.max(0))));
    out.insert("to".into(), json!(timestamp(to)));
    if let Some(s) = step {
        out.insert("step".into(), json!(duration(s)));
    }
    out.insert("tier".into(), json!(used_tier.name()));
    if !coverage.is_empty() {
        out.insert("coverage".into(), Value::Array(coverage));
    }
    if !warnings.is_empty() {
        out.insert("warnings".into(), Value::Array(warnings));
    }
    out.insert("series".into(), Value::Array(series_out));
    out.insert("next_cursor".into(), next_cursor);
    Ok(Value::Object(out))
}

/// `GET /v1/events` (ref/09 §5.5). Signals with shape `event` are not
/// recorded in 1.0 (ADR 0082): the list is empty once the query is valid.
pub fn events(query: &Query, now_ns: i64) -> Result<Value, Problem> {
    selectors(query, true)?;
    range(query, now_ns, None)?;
    limit(query, 100, 1000, "J-E1012")?;
    Ok(json!({"items": [], "next_cursor": null}))
}

/// `GET /v1/alarm-events` (ref/09 §5.8): the alarm journal, newest first
/// by default, with `seq` the journal's sequence number.
pub fn alarm_events(
    store: &Store,
    journal: SeriesId,
    query: &Query,
    now_ns: i64,
) -> Result<Value, Problem> {
    let sel = selectors(query, false)?;
    let min = match one(query, "severity") {
        Some(s) => Some(crate::alarms::severity_of(&s).ok_or_else(|| {
            bad("J-E1012", format!("unknown severity `{s}`")).with("param", json!("severity"))
        })?),
        None => None,
    };
    let (from, to) = range(query, now_ns, None)?;
    let limit = limit(query, 100, 1000, "J-E1012")?;
    let asc = one(query, "order").is_some_and(|o| o == "asc");
    let cursor = one(query, "cursor")
        .map(|c| {
            c.parse::<u64>().map_err(|_| {
                Problem::new("J-E1032", 400, "invalid cursor").with("param", json!("cursor"))
            })
        })
        .transpose()?;
    let rows = store
        .read_events(journal, from, to, 0, usize::MAX)
        .map_err(|e| Problem::new("J-E1000", 500, format!("historian: {e}")))?;
    let mut items: Vec<(u64, Value)> = rows
        .into_iter()
        .filter_map(|(n, e)| {
            let mut v: Value = serde_json::from_slice(&e.payload).ok()?;
            let id = v["id"].as_str().unwrap_or("").to_owned();
            if !sel.is_empty() && !sel.iter().any(|s| wire::matches(s, &id)) {
                return None;
            }
            if let Some(min) = min {
                let sev = v["severity"]
                    .as_str()
                    .and_then(crate::alarms::severity_of)?;
                if sev < min {
                    return None;
                }
            }
            if let Value::Object(m) = &mut v {
                m.remove("shelved");
                m.insert("seq".into(), json!(n));
            }
            Some((n, v))
        })
        .collect();
    if !asc {
        items.reverse();
    }
    if let Some(c) = cursor {
        items.retain(|(n, _)| if asc { *n > c } else { *n < c });
    }
    let next = (items.len() > limit).then(|| items[limit - 1].0.to_string());
    let page: Vec<Value> = items.into_iter().take(limit).map(|(_, v)| v).collect();
    Ok(json!({"items": page, "next_cursor": next}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn q(pairs: &[(&str, &str)]) -> Query {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn ranges_follow_ref_09_section_3_7() {
        let now = 1_800_000_000_000_000_000;
        let (from, to) = range(&q(&[]), now, Some(DEFAULT_SPAN_NS)).unwrap();
        assert_eq!((to - from) as u64, DEFAULT_SPAN_NS);
        let (from, _) = range(&q(&[("since", "10m")]), now, None).unwrap();
        assert_eq!(now - from, 600_000_000_000);
        let (from, to) = range(&q(&[("from", "-2h"), ("to", "now")]), now, None).unwrap();
        assert_eq!((to, now - from), (now, 7_200_000_000_000));
        let (from, _) = range(&q(&[("from", "2026-09-24T10:00:00Z")]), now, None).unwrap();
        assert_eq!(from, 1_790_244_000_000_000_000);
        for bad in [
            q(&[("from", "-1h"), ("since", "1h")]),
            q(&[("from", "now"), ("to", "-1h")]),
            q(&[("from", "yesterday")]),
        ] {
            assert_eq!(range(&bad, now, None).unwrap_err().code, "J-E1034");
        }
    }

    #[test]
    fn selectors_follow_ref_09_section_3_6() {
        assert!(valid_selector("meter.**"));
        assert!(valid_selector("inv*.ac.p"));
        assert!(!valid_selector("Meter.p"));
        assert!(!valid_selector("a..b"));
        let many: Vec<(&str, &str)> = (0..65).map(|_| ("sel", "a")).collect();
        assert_eq!(selectors(&q(&many), true).unwrap_err().code, "J-E1030");
        assert_eq!(selectors(&q(&[]), true).unwrap_err().code, "J-E1013");
    }

    #[test]
    fn fill_previous_and_null_cover_every_step() {
        let a = Aggregate {
            ts_ns: 10,
            count: 1,
            valid: 1,
            min: 1.0,
            max: 1.0,
            mean: 1.0,
            first: 0,
            last: 0,
            quality: 0,
        };
        let filled = fill(vec![a], 0, 40, 10, "previous");
        assert_eq!(
            filled.iter().map(|a| a.ts_ns).collect::<Vec<_>>(),
            vec![0, 10, 20, 30, 40]
        );
        assert_eq!(filled[0].count, 0);
        assert_eq!(filled[3].mean, 1.0);
        assert!(fill(vec![a], 0, 40, 10, "null")[3].mean.is_nan());
    }
}
