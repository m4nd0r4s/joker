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
                    if child(from, part) != child(to, part) {
                        self.reason("api");
                        self.push("api", format!("security.{part}"), Effect::Reload);
                    }
                }
                if rest(from, &["api", "roles"]) != rest(to, &["api", "roles"]) {
                    self.reason("security");
                    self.push("security", "security".into(), Effect::Reload);
                }
            }
            "net" => {
                self.reason("network");
                self.push("network", "net".into(), Effect::Reload);
            }
            "telemetry" => {
                if child(from, "uplink") != child(to, "uplink") {
                    self.reason("uplinks");
                    self.push("uplink", "telemetry.uplink".into(), Effect::Reload);
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
            "calc" => self.push("calc", key.into(), Effect::Reload),
            "bridges" => self.entries(key, from, to, |name, _, _| {
                vec![("bridge", name, Effect::Reload)]
            }),
            "clock" => self.push("clock", key.into(), Effect::Reload),
            "resources" => self.push("resources", key.into(), Effect::Restart),
            _ => self.push("other", key.into(), Effect::Reload),
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
            for (kind, target, effect) in classify(format!("{section}.{name}"), x, y) {
                self.push(kind, target, effect);
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
            .map(|c| json!({"kind": c.kind, "target": c.target, "effect": c.effect.name()}))
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
                target: "outputs.pcs.p_set".into(),
                effect: Effect::Hot,
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
