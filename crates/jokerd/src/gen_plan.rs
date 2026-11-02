//! The plan of a generation (docs/01 §15, ref/09 §5.13): what changes
//! from the generation it replaces, the effect of each change, the
//! disruption of the whole swap and the reasons that make a confirmation
//! window mandatory in `prod` (ADR 0015).
//!
//! The plan compares the two effective manifests key by key. An envelope
//! (`[outputs.*]`) or an alarm swaps hot; the parameters of an app are hot;
//! the address of a device or its bus restarts that driver only, and the
//! rest of a device reconfigures it; an app's other keys reload it; a cycle
//! group restarts. `[security.api]` and `[security.roles]` are the reason
//! `api` (the API itself, ref/09 §5.13), the rest of `[security]` is
//! `security`, `[net]` is `network` and `[telemetry.uplink]` is `uplinks`.

use serde_json::{Value, json};
use toml::{Table, Value as Toml};

/// What a change does to the running node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    /// Nothing runs differently.
    None,
    /// Swapped without stopping anything.
    Hot,
    /// The part is reloaded.
    Reload,
    /// The part is restarted.
    Restart,
}

impl Effect {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hot => "hot",
            Self::Reload => "reload",
            Self::Restart => "restart",
        }
    }
}

/// One change of the plan (`PlanChange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The kind (`driver_restart`, `envelope`, ...).
    pub kind: &'static str,
    /// What it changes (`devices.pcs`, `outputs.pcs.p_set`, ...).
    pub target: String,
    /// What it does.
    pub effect: Effect,
    /// The leaf keys it changes, each with its value before and after
    /// (`None` where the key is absent), in key order.
    pub fields: Vec<Field>,
}

/// One leaf key of a change (`PlanChange.fields`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The full key (`apps.light.params.half_period`).
    pub key: String,
    /// Its value in the base, rendered.
    pub from: Option<String>,
    /// Its value in the target, rendered.
    pub to: Option<String>,
}

/// The plan of a generation against the one it replaces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenPlan {
    /// The changes, in key order.
    pub changes: Vec<Change>,
    /// The reasons that make a window mandatory in `prod`, without repeats.
    pub confirm_reasons: Vec<&'static str>,
    /// There is no generation to replace.
    pub first: bool,
    /// The target generation runs in `prod`.
    pub prod: bool,
}

/// The keys of a device whose change restarts its driver.
const ADDRESS_KEYS: [&str; 9] = [
    "driver", "bus", "addr", "address", "host", "port", "unit", "path", "profile",
];

type Classified = Vec<(&'static str, String, Effect)>;

impl GenPlan {
    /// Plans `target` (an effective manifest) against `base`, or as a first
    /// generation without one. A manifest that does not parse is planned as
    /// empty.
    pub fn new(base: Option<&str>, target: &str) -> Self {
        let parse = |text: &str| text.parse::<Table>().unwrap_or_default();
        let to = parse(target);
        let prod = to
            .get("project")
            .and_then(|p| p.get("mode"))
            .and_then(Toml::as_str)
            == Some("prod");
        let mut plan = Self {
            first: base.is_none(),
            prod,
            ..Self::default()
        };
        let from = base.map(parse).unwrap_or_default();
        let mut keys: Vec<&String> = from.keys().chain(to.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            plan.section(key, from.get(key), to.get(key));
        }
        plan
    }

    fn push(&mut self, kind: &'static str, target: String, effect: Effect) {
        self.changes.push(Change {
            kind,
            target,
            effect,
            fields: Vec::new(),
        });
    }

    fn push_diff(
        &mut self,
        kind: &'static str,
        target: String,
        effect: Effect,
        from: Option<&Toml>,
        to: Option<&Toml>,
    ) {
        let mut fields = Vec::new();
        leaves(&target, from, to, &mut fields);
        self.changes.push(Change {
            kind,
            target,
            effect,
            fields,
        });
    }

    fn reason(&mut self, reason: &'static str) {
        if !self.confirm_reasons.contains(&reason) {
            self.confirm_reasons.push(reason);
        }
    }

    fn section(&mut self, key: &str, from: Option<&Toml>, to: Option<&Toml>) {
        if from == to {
            return;
        }
        match key {
            "devices" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("device_add", name, Effect::Restart)],
                (_, None) => vec![("device_remove", name, Effect::Restart)],
                (Some(a), Some(b)) => {
                    let restart = ADDRESS_KEYS.iter().any(|k| a.get(k) != b.get(k));
                    if restart {
                        vec![("driver_restart", name, Effect::Restart)]
                    } else {
                        vec![("driver_reconfigure", name, Effect::Reload)]
                    }
                }
            }),
            "buses" => self.entries(key, from, to, |name, _, _| {
                vec![("driver_restart", name, Effect::Restart)]
            }),
            "outputs" => self.entries(key, from, to, |name, _, _| {
                vec![("envelope", name, Effect::Hot)]
            }),
            "apps" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("app_add", name, Effect::Reload)],
                (_, None) => vec![("app_remove", name, Effect::Reload)],
                (Some(a), Some(b)) => {
                    if without(a, "params") == without(b, "params") {
                        vec![("app_params", name, Effect::Hot)]
                    } else {
                        vec![("app_update", name, Effect::Reload)]
                    }
                }
            }),
            "cycles" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("group_add", name, Effect::Restart)],
                (_, None) => vec![("group_remove", name, Effect::Restart)],
                _ => vec![("group_change", name, Effect::Restart)],
            }),
            "alarms" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("alarm_add", name, Effect::Hot)],
                (_, None) => vec![("alarm_remove", name, Effect::Hot)],
                _ => vec![("alarm_change", name, Effect::Hot)],
            }),
            "security" => {
                for part in ["api", "roles"] {
                    let (a, b) = (child(from, part), child(to, part));
                    if a != b {
                        self.reason("api");
                        self.push_diff(
                            "api",
                            format!("security.{part}"),
                            Effect::Reload,
                            a.as_ref(),
                            b.as_ref(),
                        );
                    }
                }
                if rest(from, &["api", "roles"]) != rest(to, &["api", "roles"]) {
                    self.reason("security");
                    self.push("security", "security".into(), Effect::Reload);
                }
            }
            "net" => {
                self.reason("network");
                self.push_diff("network", "net".into(), Effect::Reload, from, to);
            }
            "telemetry" => {
                let (a, b) = (child(from, "uplink"), child(to, "uplink"));
                if a != b {
                    self.reason("uplinks");
                    self.push_diff(
                        "uplink",
                        "telemetry.uplink".into(),
                        Effect::Reload,
                        a.as_ref(),
                        b.as_ref(),
                    );
                }
                if rest(from, &["uplink"]) != rest(to, &["uplink"]) {
                    self.push("telemetry", "telemetry".into(), Effect::Reload);
                }
            }
            "signals" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("signal_add", name, Effect::Reload)],
                (_, None) => vec![("signal_remove", name, Effect::Reload)],
                _ => vec![("signal_retype", name, Effect::Reload)],
            }),
            "instances" => self.entries(key, from, to, |name, a, b| match (a, b) {
                (None, _) => vec![("instance_add", name, Effect::Reload)],
                (_, None) => vec![("instance_remove", name, Effect::Reload)],
                _ => vec![("instance_change", name, Effect::Reload)],
            }),
            "calc" => self.push_diff("calc", key.into(), Effect::Reload, from, to),
            "bridges" => self.entries(key, from, to, |name, _, _| {
                vec![("bridge", name, Effect::Reload)]
            }),
            "clock" => self.push_diff("clock", key.into(), Effect::Reload, from, to),
            "resources" => self.push_diff("resources", key.into(), Effect::Restart, from, to),
            _ => self.push_diff("other", segment(key), Effect::Reload, from, to),
        }
    }

    fn entries(
        &mut self,
        section: &str,
        from: Option<&Toml>,
        to: Option<&Toml>,
        classify: impl Fn(String, Option<&Toml>, Option<&Toml>) -> Classified,
    ) {
        let table = |v: Option<&Toml>| v.and_then(Toml::as_table).cloned().unwrap_or_default();
        let (a, b) = (table(from), table(to));
        let mut names: Vec<&String> = a.keys().chain(b.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            let (x, y) = (a.get(name), b.get(name));
            if x == y {
                continue;
            }
            for (kind, target, effect) in classify(format!("{section}.{}", segment(name)), x, y) {
                self.push_diff(kind, target, effect, x, y);
            }
        }
    }

    /// The disruption of the swap: `full` for a first generation, `none`
    /// without changes, `hot` when nothing reloads or restarts, `partial`
    /// otherwise.
    pub fn disruption(&self) -> &'static str {
        if self.first {
            return "full";
        }
        match self.changes.iter().map(|c| c.effect).max() {
            None => "none",
            Some(Effect::None | Effect::Hot) => "hot",
            Some(_) => "partial",
        }
    }

    /// A confirmation window is mandatory: the target runs in `prod` and
    /// the plan has a confirmation reason (ADR 0015).
    pub fn requires_confirm(&self) -> bool {
        self.prod && !self.confirm_reasons.is_empty()
    }

    /// The `Plan` of ref/09 §5.13, against `base` (`gen`, hash) and for the
    /// generation `target` (its hash).
    pub fn json(&self, base: (u64, &str), target: &str) -> Value {
        let changes: Vec<Value> = self
            .changes
            .iter()
            .map(|c| {
                let mut change =
                    json!({"kind": c.kind, "target": c.target, "effect": c.effect.name()});
                if !c.fields.is_empty() {
                    change["fields"] = c
                        .fields
                        .iter()
                        .map(|f| {
                            let mut field = json!({"key": f.key});
                            if let Some(from) = &f.from {
                                field["from"] = json!(from);
                            }
                            if let Some(to) = &f.to {
                                field["to"] = json!(to);
                            }
                            field
                        })
                        .collect();
                }
                change
            })
            .collect();
        json!({
            "base": {"gen": base.0, "hash": base.1},
            "target": target,
            "requires_confirm": self.requires_confirm(),
            "confirm_reasons": self.confirm_reasons,
            "disruption": self.disruption(),
            "changes": changes,
        })
    }
}

/// A key segment as it is written in a dotted key: quoted when it is not
/// a bare key (`outputs."pcs.p_set"`).
fn segment(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

/// A value as a plan shows it: a string without its quotes, anything else
/// as TOML.
fn render(value: &Toml) -> String {
    match value {
        Toml::String(s) => s.clone(),
        other => inline(other),
    }
}

/// A value as inline TOML.
fn inline(value: &Toml) -> String {
    match value {
        Toml::String(s) => format!("{s:?}"),
        Toml::Integer(i) => i.to_string(),
        Toml::Float(f) => f.to_string(),
        Toml::Boolean(b) => b.to_string(),
        Toml::Datetime(d) => d.to_string(),
        Toml::Array(items) => format!(
            "[{}]",
            items.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Toml::Table(t) => format!(
            "{{ {} }}",
            t.iter()
                .map(|(k, v)| format!("{} = {}", segment(k), inline(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The leaf keys under `key` whose value differs between `from` and `to`,
/// in key order; a table on both sides is walked, anything else is a leaf.
fn leaves(key: &str, from: Option<&Toml>, to: Option<&Toml>, out: &mut Vec<Field>) {
    if from == to {
        return;
    }
    if let (Some(Toml::Table(a)), Some(Toml::Table(b))) = (from, to) {
        let mut names: Vec<&String> = a.keys().chain(b.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            leaves(
                &format!("{key}.{}", segment(name)),
                a.get(name),
                b.get(name),
                out,
            );
        }
        return;
    }
    out.push(Field {
        key: key.to_owned(),
        from: from.map(render),
        to: to.map(render),
    });
}

fn child(section: Option<&Toml>, key: &str) -> Option<Toml> {
    section.and_then(|s| s.get(key)).cloned()
}

fn rest(section: Option<&Toml>, keys: &[&str]) -> Option<Table> {
    let mut t = section.and_then(Toml::as_table).cloned()?;
    for k in keys {
        t.remove(*k);
    }
    (!t.is_empty()).then_some(t)
}

fn without(value: &Toml, key: &str) -> Option<Table> {
    value.as_table().map(|t| {
        let mut t = t.clone();
        t.remove(key);
        t
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
[project]
name = "site"
mode = "prod"

[devices.pcs]
driver = "modbus-tcp"
host = "10.0.0.5"
poll = "100ms"

[outputs."pcs.p_set"]
safe = "0 kW"
range = ["-100 kW", "100 kW"]

[apps.shaver]
path = "apps/shaver"
params = { limit = "400 kW" }

[cycles.main]
period = "100ms"
"#;

    fn with(from: &str, to: &str) -> String {
        BASE.replace(from, to)
    }

    #[test]
    fn the_same_manifest_changes_nothing() {
        let plan = GenPlan::new(Some(BASE), BASE);
        assert!(plan.changes.is_empty());
        assert_eq!(plan.disruption(), "none");
        assert!(!plan.requires_confirm());
    }

    #[test]
    fn a_first_generation_is_a_full_swap() {
        assert_eq!(GenPlan::new(None, BASE).disruption(), "full");
    }

    #[test]
    fn an_envelope_swaps_hot_and_restarts_no_driver() {
        let plan = GenPlan::new(Some(BASE), &with("\"100 kW\"]", "\"80 kW\"]"));
        assert_eq!(
            plan.changes,
            vec![Change {
                kind: "envelope",
                target: "outputs.\"pcs.p_set\"".into(),
                effect: Effect::Hot,
                fields: vec![Field {
                    key: "outputs.\"pcs.p_set\".range".into(),
                    from: Some("[\"-100 kW\", \"100 kW\"]".into()),
                    to: Some("[\"-100 kW\", \"80 kW\"]".into()),
                }],
            }]
        );
        assert_eq!(plan.disruption(), "hot");
    }

    #[test]
    fn a_device_address_restarts_that_driver_only() {
        let plan = GenPlan::new(Some(BASE), &with("10.0.0.5", "10.0.0.6"));
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].kind, "driver_restart");
        assert_eq!(plan.changes[0].target, "devices.pcs");
        assert_eq!(plan.disruption(), "partial");
        let poll = GenPlan::new(Some(BASE), &with("poll = \"100ms\"", "poll = \"50ms\""));
        assert_eq!(poll.changes[0].kind, "driver_reconfigure");
    }

    #[test]
    fn app_parameters_are_hot_and_the_rest_of_an_app_reloads_it() {
        let params = GenPlan::new(Some(BASE), &with("400 kW", "300 kW"));
        assert_eq!(params.changes[0].kind, "app_params");
        assert_eq!(params.changes[0].effect, Effect::Hot);
        assert_eq!(
            params.changes[0].fields,
            vec![Field {
                key: "apps.shaver.params.limit".into(),
                from: Some("400 kW".into()),
                to: Some("300 kW".into()),
            }]
        );
        let path = GenPlan::new(Some(BASE), &with("apps/shaver\"", "apps/shaver2\""));
        assert_eq!(path.changes[0].kind, "app_update");
    }

    #[test]
    fn the_api_and_the_uplinks_require_a_window_in_prod() {
        let target = format!(
            "{BASE}\n[security.api]\nlisten = [\"0.0.0.0:8443\"]\n\n[telemetry.uplink.cloud]\nkind = \"mqtt\"\n"
        );
        let plan = GenPlan::new(Some(BASE), &target);
        assert_eq!(plan.confirm_reasons, vec!["api", "uplinks"]);
        assert!(plan.requires_confirm());
        let dev = target.replace("mode = \"prod\"", "mode = \"dev\"");
        assert!(!GenPlan::new(Some(BASE), &dev).requires_confirm());
    }
}
