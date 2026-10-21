//! Loads a `.profile.toml` file (ref/03 §2.2) into a [`ProfileInfo`], the
//! same type `Context` already uses for the manifest checks.
//!
//! Scoped deliberately to what `ProfileInfo` models: `[profile]` presence,
//! `[signals]` (`type`/`signal_type`/`direction`/`unit`, through `[enums]`),
//! `[defaults]` (`poll`/`timeout`), `[fallback]` (presence, `configurable`,
//! a fixed `timeout`) and `[actions]` (an explicit `timeout`, or derived
//! from `steps`' `wait` budgets the same way ref/03 §2.2.12 does: the sum
//! of the waits plus one second per step). Address schemas (driver-specific,
//! ref/03 §2.2.5), `[coherent]`, `[connect]`, `[relinquish]`, `[events]`,
//! `[implements]`, `[discovery]`, per-signal acquisition keys (`scale`,
//! `limits`, …) and the fallback's register forms
//! (`timeout_register`/`value_register`/`values`/`applies_to`) are not
//! parsed here (AUR-414).

use std::collections::BTreeMap;
use std::path::Path;

use joker_units::{Duration, Unit};
use toml::de::DeTable;

use crate::context::{Direction, ProfileInfo, SignalInfo, SignalType};
use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{FileId, Sources, Span};
use crate::value::{self, Node, Table, Value};

/// The result of loading one profile file.
#[derive(Debug)]
pub struct ProfileReport {
    /// The file, for rendering diagnostics.
    pub sources: Sources,
    /// What went wrong, if anything.
    pub diagnostics: Diagnostics,
    /// The profile, when `[profile]` and every signal parsed.
    pub profile: Option<ProfileInfo>,
}

impl ProfileReport {
    /// The findings, rendered as `jk check`/`jk profile test` would.
    pub fn render(&self) -> String {
        self.diagnostics
            .sorted(&self.sources)
            .iter()
            .map(|d| d.render(&self.sources, crate::Mode::Prod))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Parses `text` (the file at `path`, for diagnostics) into a
/// [`ProfileInfo`].
pub fn parse(path: &Path, text: &str) -> ProfileReport {
    let mut sources = Sources::default();
    let file = sources.add(path.to_path_buf(), text.to_owned());
    let mut diagnostics = Diagnostics::default();
    let table = match DeTable::parse(text) {
        Ok(parsed) => match value::from_toml(file, parsed.get_ref()) {
            Ok(table) => table,
            Err(value::InvalidNumber(span)) => {
                diagnostics.push(Diagnostic::new("J0146", "number out of range").at(span));
                return ProfileReport {
                    sources,
                    diagnostics,
                    profile: None,
                };
            }
        },
        Err(error) => {
            let range = error.span().unwrap_or(0..0);
            diagnostics.push(
                Diagnostic::new("J0146", "file is not valid TOML")
                    .at(Span::new(file, range.start, range.end))
                    .label(error.message().trim().to_owned()),
            );
            return ProfileReport {
                sources,
                diagnostics,
                profile: None,
            };
        }
    };
    let profile = load(&table, file, &mut diagnostics);
    ProfileReport {
        sources,
        diagnostics,
        profile,
    }
}

fn load(table: &Table, file: FileId, diagnostics: &mut Diagnostics) -> Option<ProfileInfo> {
    if table
        .get("profile")
        .and_then(|e| e.node.as_table())
        .is_none()
    {
        diagnostics.push(
            Diagnostic::new("J0142", "missing required section `[profile]`")
                .at(Span::new(file, 0, 0)),
        );
        return None;
    }
    let enums = enums_of(table);
    let signals = signals_of(table, &enums, diagnostics)?;
    let (poll, timeout) = defaults_of(table);
    let (fallback, configurable, fallback_timeout) = fallback_of(table, diagnostics);
    let actions = actions_of(table, diagnostics);
    let latency = table
        .get("profile")
        .and_then(|e| e.node.as_table())
        .and_then(|t| t.get("quirks"))
        .and_then(|e| e.node.as_table())
        .and_then(|t| t.get("latency"))
        .and_then(|e| e.node.as_str())
        .and_then(|s| Duration::parse(s).ok());
    Some(ProfileInfo {
        signals,
        fallback,
        configurable,
        fallback_timeout,
        latency,
        poll,
        timeout,
        actions,
    })
}

fn enums_of(table: &Table) -> BTreeMap<String, Vec<String>> {
    let Some(enums) = table.get("enums").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    enums
        .iter()
        .filter_map(|(name, entry)| {
            let variants = entry.node.as_table()?;
            Some((name.clone(), variants.keys().cloned().collect()))
        })
        .collect()
}

fn raw_type(
    type_name: &str,
    enum_name: Option<&str>,
    enums: &BTreeMap<String, Vec<String>>,
) -> Option<SignalType> {
    if let Some(name) = enum_name {
        let variants = enums.get(name)?;
        return Some(SignalType::Enum {
            name: name.to_owned(),
            variants: variants.clone(),
        });
    }
    match type_name {
        "bool" => Some(SignalType::Bool),
        "u16" | "i16" | "u32" | "i32" | "u64" | "i64" | "bits16" | "bits32" | "bits64" => {
            Some(SignalType::Integer)
        }
        "f32" | "f64" => Some(SignalType::Float),
        "string" => Some(SignalType::Text),
        _ => None,
    }
}

fn signals_of(
    table: &Table,
    enums: &BTreeMap<String, Vec<String>>,
    diagnostics: &mut Diagnostics,
) -> Option<BTreeMap<String, SignalInfo>> {
    let Some(signals) = table.get("signals").and_then(|e| e.node.as_table()) else {
        return Some(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    let mut ok = true;
    for (path, entry) in signals {
        let Some(point) = entry.node.as_table() else {
            ok = false;
            continue;
        };
        let Some(type_name) = point.get("type").and_then(|e| e.node.as_str()) else {
            diagnostics.push(
                Diagnostic::new("J0142", format!("signal `{path}` without `type`"))
                    .at(entry.node.span),
            );
            ok = false;
            continue;
        };
        let enum_name = point.get("enum").and_then(|e| e.node.as_str());
        let Some(ty) = raw_type(type_name, enum_name, enums) else {
            diagnostics.push(
                Diagnostic::new(
                    "J0809",
                    format!("signal `{path}`: unknown raw type `{type_name}`"),
                )
                .at(entry.node.span),
            );
            ok = false;
            continue;
        };
        let direction = match point.get("direction").and_then(|e| e.node.as_str()) {
            Some("output") => Direction::Output,
            Some("param") => Direction::Param,
            _ => Direction::Input,
        };
        let mut info = SignalInfo::new(ty, direction);
        if let Some(unit) = point.get("unit").and_then(|e| e.node.as_str()) {
            match Unit::parse(unit) {
                Ok(u) => info = info.with_unit(u),
                Err(_) => diagnostics.push(
                    Diagnostic::new("J0815", format!("signal `{path}`: invalid unit `{unit}`"))
                        .at(entry.node.span),
                ),
            }
        }
        out.insert(path.clone(), info);
    }
    ok.then_some(out)
}

fn defaults_of(table: &Table) -> (Option<Duration>, Option<Duration>) {
    let Some(defaults) = table.get("defaults").and_then(|e| e.node.as_table()) else {
        return (None, None);
    };
    let duration = |key: &str| {
        defaults
            .get(key)
            .and_then(|e| e.node.as_str())
            .and_then(|s| Duration::parse(s).ok())
    };
    (duration("poll"), duration("timeout"))
}

fn fallback_of(table: &Table, diagnostics: &mut Diagnostics) -> (bool, bool, Option<Duration>) {
    let Some(fallback) = table.get("fallback").and_then(|e| e.node.as_table()) else {
        return (false, true, None);
    };
    let configurable = !matches!(
        fallback.get("configurable").map(|e| &e.node.value),
        Some(Value::Bool(false))
    );
    let timeout = match fallback.get("timeout").and_then(|e| e.node.as_str()) {
        Some(s) => match Duration::parse(s) {
            Ok(d) => Some(d),
            Err(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        "J0820",
                        format!("`[fallback] timeout` is not a duration: `{s}`"),
                    )
                    .at(table["fallback"].node.span),
                );
                None
            }
        },
        None => None,
    };
    (true, configurable, timeout)
}

/// ref/03 §2.2.12: an action's default `timeout` is the sum of its `wait`
/// budgets plus one second per step.
fn steps_default_timeout(steps: &[Node]) -> Duration {
    let mut nanos: u64 = 0;
    for step in steps {
        nanos += Duration::from_secs(1).as_nanos();
        if let Some(wait) = step
            .as_table()
            .and_then(|t| t.get("wait"))
            .and_then(|e| e.node.as_str())
            .and_then(|s| Duration::parse(s).ok())
        {
            nanos += wait.as_nanos();
        }
    }
    Duration::from_nanos(nanos)
}

fn actions_of(table: &Table, diagnostics: &mut Diagnostics) -> BTreeMap<String, Option<Duration>> {
    let Some(actions) = table.get("actions").and_then(|e| e.node.as_table()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, entry) in actions {
        let Some(action) = entry.node.as_table() else {
            continue;
        };
        if let Some(timeout) = action.get("timeout").and_then(|e| e.node.as_str()) {
            match Duration::parse(timeout) {
                Ok(d) => {
                    out.insert(name.clone(), Some(d));
                    continue;
                }
                Err(_) => {
                    diagnostics.push(
                        Diagnostic::new("J0821", format!("action `{name}`: invalid `timeout`"))
                            .at(entry.node.span),
                    );
                    continue;
                }
            }
        }
        let steps = action
            .get("steps")
            .and_then(|e| match &e.node.value {
                Value::Array(items) => Some(items.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if steps.is_empty() {
            out.insert(name.clone(), None);
        } else {
            out.insert(name.clone(), Some(steps_default_timeout(&steps)));
        }
    }
    out
}
