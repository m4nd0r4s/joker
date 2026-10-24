//! Semantic checks of the v1.0 sections beyond the first slice (ref/01
//! §5.4–§5.5, §5.7, §5.9, §5.13–§5.20, §5.22), on the manifest as decoded
//! and validated by the schema.

use std::collections::{BTreeMap, BTreeSet};

use joker_units::Duration;

use crate::check::{Checker, Registered};
use crate::context::{ClassInfo, Direction, MemberKind, SignalInfo, SignalType, class_ref};
use crate::diag::Diagnostic;
use crate::expr::{self, Expect, Outcome};
use crate::grammar;
use crate::model::{Band, Envelope, Mode};
use crate::source::Span;
use crate::value::{Entry, Node, Table, Value};

const TRUST_ROLES: [&str; 11] = [
    "config", "runtime", "firmware", "apps", "drivers", "profiles", "boards", "classes", "secrets",
    "deck", "models",
];
const RECORD_LAYERS: [&str; 4] = ["raw", "1s", "1m", "15m"];
const UPLINK_CLASSES: [&str; 5] = ["alarms", "events", "high", "low", "bulk"];
const DEVICE_GROUPS: [&str; 5] = ["dialout", "video", "gpio", "i2c", "spi"];
const FORBIDDEN_GROUPS: [&str; 4] = ["joker", "root", "wheel", "sudo"];

fn table(node: Option<&Node>) -> Option<&Table> {
    node.and_then(Node::as_table)
}

fn get<'t>(table: &'t Table, key: &str) -> Option<&'t Entry> {
    table.get(key)
}

fn text<'t>(table: &'t Table, key: &str) -> Option<&'t str> {
    table.get(key).and_then(|e| e.node.as_str())
}

fn strings(node: &Node) -> Vec<(String, Span)> {
    match &node.value {
        Value::String(s) => vec![(s.clone(), node.span)],
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str().map(|s| (s.to_owned(), i.span)))
            .collect(),
        _ => Vec::new(),
    }
}

fn duration(node: &Node) -> Option<Duration> {
    node.as_str().and_then(|s| Duration::parse(s).ok())
}

impl Checker<'_> {
    fn section(&self, name: &str) -> Option<&Table> {
        table(self.manifest.pending.get(name))
    }

    fn prod(&self) -> bool {
        self.manifest.project.mode == Mode::Prod
    }

    /// The checks that build the rest of the path registry, before the
    /// envelopes are checked against it.
    pub(crate) fn before_outputs(&mut self) {
        self.extern_devices();
        self.resources();
        self.clock();
        self.drivers();
        self.instances();
        self.calc_and_alarms();
        self.signals();
        self.bridge_signals();
    }

    /// The checks that need the envelopes.
    pub(crate) fn after_outputs(&mut self) {
        self.alarm_actions();
        self.atomic();
        self.telemetry();
        self.bridges();
        self.security();
        self.micro_target();
        self.output_rules();
    }

    fn opaque_check(&self) -> impl Fn(&str) -> bool + '_ {
        move |p: &str| self.opaque_path(p)
    }

    /// Checks an expression string and records its finding.
    pub(crate) fn expression(&mut self, source: &str, span: Span, expect: Expect) -> Outcome {
        let (outcome, diagnostic) = {
            let registry = expr::Registry::new(&self.paths);
            let opaque = self.opaque_check();
            expr::check(source, span, &registry, expect, &opaque)
        };
        if let Some(d) = diagnostic {
            self.push(d);
        }
        outcome
    }

    fn matches_any(&self, selector: &str) -> bool {
        if grammar::is_wildcard(selector) {
            self.paths.keys().any(|p| grammar::matches(selector, p))
                || self.selector_may_be_opaque(selector)
        } else {
            self.paths.contains_key(selector) || self.opaque_path(selector)
        }
    }

    fn outputs_of(&self, selector: &str) -> Vec<String> {
        self.paths
            .iter()
            .filter(|(p, r)| {
                r.info.direction == Direction::Output
                    && **p == r.canonical
                    && (if grammar::is_wildcard(selector) {
                        grammar::matches(selector, p)
                    } else {
                        *p == selector
                    })
            })
            .map(|(p, _)| p.clone())
            .collect()
    }

    fn extern_devices(&mut self) {
        if !self.micro {
            return;
        }
        for (name, device) in &self.manifest.devices {
            if device.driver.as_ref().is_none_or(|d| d.value != "extern") {
                continue;
            }
            for (signal, tuning) in &device.signals {
                let has = |k: &str| tuning.value.contains_key(k);
                let numeric = tuning
                    .value
                    .get("type")
                    .and_then(Node::as_str)
                    .is_some_and(|t| t != "bool" && t != "string" && t != "bytes");
                if !has("type") || !has("direction") || (numeric && !has("unit")) {
                    self.push(
                        Diagnostic::new("J0785", format!("`extern` signal `{name}.{signal}` without `type`, `direction` or `unit`"))
                            .at(tuning.span),
                    );
                }
            }
        }
    }

    fn resources(&mut self) {
        let Some(resources) = self.section("resources").cloned() else {
            return;
        };
        let cores = self
            .manifest
            .target
            .as_ref()
            .and_then(|t| t.board.as_ref())
            .and_then(|b| self.context.boards.get(&b.value.name))
            .and_then(|b| b.cpus);
        let mut sets: BTreeMap<String, (BTreeSet<i64>, Span)> = BTreeMap::new();
        for (class, entry) in &resources {
            let Some(spec) = entry.node.as_table() else {
                continue;
            };
            if let Some(cpus) = get(spec, "cpus")
                && let Value::Array(items) = &cpus.node.value
            {
                let mut set = BTreeSet::new();
                for item in items {
                    if let Value::Integer(i) = item.value {
                        if cores.is_some_and(|n| i >= i64::from(n)) {
                            self.push(
                                Diagnostic::new(
                                    "J0144",
                                    format!("CPU {i} does not exist on the board"),
                                )
                                .at(item.span),
                            );
                        }
                        set.insert(i);
                    }
                }
                sets.insert(class.clone(), (set, cpus.node.span));
            }
            if class == "rt"
                && let Some(memory) = get(spec, "memory")
            {
                self.push(
                    Diagnostic::new("J0145", "`memory` does not apply to the `rt` class")
                        .at(memory.key),
                );
            }
            if class != "ml"
                && let Some(accel) = get(spec, "accel")
            {
                self.push(
                    Diagnostic::new("J0145", "`accel` only applies to the `ml` class")
                        .at(accel.key),
                );
            }
        }
        if let Some((rt, span)) = sets.get("rt").cloned() {
            let single = cores == Some(1);
            if rt.is_empty() && !self.manifest.cycles.is_empty() {
                self.push(Diagnostic::new("J0360", "class `rt` without CPUs").at(span));
            }
            for (class, (set, _)) in &sets {
                if class != "rt" && !set.is_disjoint(&rt) && !single {
                    self.push(
                        Diagnostic::new("J0360", format!("class `rt` overlaps class `{class}`"))
                            .at(span)
                            .help("`rt` CPUs are isolated: give the other classes other CPUs"),
                    );
                }
            }
        }
    }

    fn clock(&mut self) {
        let Some(clock) = self.section("clock").cloned() else {
            return;
        };
        let sync = text(&clock, "sync").unwrap_or(if self.micro { "link" } else { "ntp" });
        if let Some(entry) = get(&clock, "sync") {
            if !self.micro && matches!(sync, "link" | "extern") {
                self.push(
                    Diagnostic::new(
                        "J0145",
                        format!("`sync = \"{sync}\"` only applies on a Joker Micro"),
                    )
                    .at(entry.node.span),
                );
            }
            if sync == "none"
                && self
                    .manifest
                    .apps
                    .values()
                    .any(|a| a.requires_clock == "synced")
            {
                self.push(
                    Diagnostic::new(
                        "J0145",
                        "`sync = \"none\"` with apps that require a synced clock",
                    )
                    .at(entry.node.span),
                );
            }
        }
        if sync == "ptp"
            && !clock.contains_key("iface")
            && let Some(entry) = get(&clock, "sync")
        {
            self.push(
                Diagnostic::new("J0142", "`sync = \"ptp\"` without `iface`").at(entry.node.span),
            );
        }
        if let Some(servers) = get(&clock, "servers")
            && sync != "ntp"
        {
            self.push(
                Diagnostic::new("J0145", "`servers` only applies with `sync = \"ntp\"`")
                    .at(servers.key),
            );
        }
        if let Some(holdover) = get(&clock, "max_holdover")
            && duration(&holdover.node)
                .is_some_and(|d| d.as_nanos() > Duration::from_secs(30 * 86_400).as_nanos())
        {
            self.push(
                Diagnostic::new("J0152", "invalid duration")
                    .at(holdover.node.span)
                    .label("at most 30d"),
            );
        }
    }

    fn drivers(&mut self) {
        let Some(drivers) = self.section("drivers").cloned() else {
            return;
        };
        const X_ONLY: [&str; 11] = [
            "exec",
            "args",
            "launch",
            "user",
            "groups",
            "cpu",
            "isolation",
            "net",
            "devices",
            "raw_net",
            "env",
        ];
        const W_ONLY: [&str; 2] = ["cpu_per_call", "plan_fuel"];
        for (name, entry) in &drivers {
            let Some(decl) = entry.node.as_table() else {
                continue;
            };
            let tier = text(decl, "tier").unwrap_or("x");
            for (key, item) in decl {
                let wrong = (tier == "w" && X_ONLY.contains(&key.as_str()))
                    || (tier == "x" && W_ONLY.contains(&key.as_str()));
                if wrong {
                    self.push(
                        Diagnostic::new(
                            "J0145",
                            format!("`{key}` does not apply to a tier `{tier}` driver"),
                        )
                        .at(item.key),
                    );
                }
            }
            match (tier, decl.contains_key("source"), decl.contains_key("exec")) {
                ("w", false, _) => self.push(
                    Diagnostic::new("J0142", format!("tier W driver `{name}` without `source`"))
                        .at(entry.key),
                ),
                ("x", true, true) => self.push(
                    Diagnostic::new(
                        "J0145",
                        format!("driver `{name}` with both `source` and `exec`"),
                    )
                    .at(entry.key),
                ),
                ("x", false, false) => self.push(
                    Diagnostic::new(
                        "J0142",
                        format!("tier X driver `{name}` without `source` or `exec`"),
                    )
                    .at(entry.key),
                ),
                _ => {}
            }
            if tier == "w"
                && let Some(memory) = get(decl, "memory")
                && memory
                    .node
                    .as_str()
                    .and_then(grammar::size)
                    .is_some_and(|b| b > 64 << 20)
            {
                self.push(
                    Diagnostic::new("J0874", "tier W driver memory above 64 MiB")
                        .at(memory.node.span),
                );
            }
            if let Some(cpu) = get(decl, "cpu")
                && let Some(Ok(joker_units::Literal::Quantity(q))) =
                    cpu.node.as_str().map(joker_units::Literal::parse)
                && !(1.0..=400.0).contains(&q.value)
            {
                self.push(Diagnostic::new("J0144", "`cpu` outside 1 %–400 %").at(cpu.node.span));
            }
            if let Some(groups) = get(decl, "groups") {
                for (group, span) in strings(&groups.node) {
                    if FORBIDDEN_GROUPS.contains(&group.as_str())
                        || !DEVICE_GROUPS.contains(&group.as_str())
                    {
                        self.push(
                            Diagnostic::new(
                                "J0865",
                                format!("driver group `{group}` is not a device-access group"),
                            )
                            .at(span),
                        );
                    }
                }
            }
            if let Some(env) = get(decl, "env").and_then(|e| e.node.as_table()) {
                for (key, item) in env {
                    if key.starts_with("JDP_") {
                        self.push(
                            Diagnostic::new("J0141", format!("`{key}` is reserved for the JDP"))
                                .at(item.key),
                        );
                    }
                }
            }
            if let Some(net) = get(decl, "net") {
                for (host, span) in strings(&net.node) {
                    let ip = host.split('/').next().unwrap_or("");
                    if ip.parse::<std::net::IpAddr>().is_err() {
                        self.push(
                            Diagnostic::new("J0184", "invalid `net` entry")
                                .at(span)
                                .label("expected an IP or a CIDR"),
                        );
                    }
                }
            }
            if let Some(restart) = get(decl, "restart").and_then(|e| e.node.as_table())
                && let (Some(min), Some(max)) = (
                    restart.get("min").and_then(|e| duration(&e.node)),
                    restart.get("max").and_then(|e| duration(&e.node)),
                )
                && min.as_nanos() > max.as_nanos()
            {
                self.push(
                    Diagnostic::new("J0145", "`restart.min` above `restart.max`").at(entry.key),
                );
            }
            if tier == "x"
                && self.prod()
                && text(decl, "launch") == Some("spawn")
                && text(decl, "user").is_none()
            {
                self.push(
                    Diagnostic::new(
                        "J0865",
                        format!("driver `{name}` spawned without its own user"),
                    )
                    .at(entry.key),
                );
            }
        }
    }

    fn instances(&mut self) {
        let Some(instances) = self.section("instances").cloned() else {
            return;
        };
        let mut classes: BTreeMap<String, (String, Option<ClassInfo>, BTreeSet<String>)> =
            BTreeMap::new();
        for (name, entry) in &instances {
            let Some(decl) = entry.node.as_table() else {
                continue;
            };
            let Some(class) = text(decl, "class").map(class_ref) else {
                continue;
            };
            let info = self.context.classes.get(&class).cloned();
            if info.is_none() && !self.context.open_world {
                self.push(
                    Diagnostic::new("J0193", format!("unknown class `{class}`"))
                        .at(get(decl, "class").map_or(entry.key, |e| e.node.span)),
                );
            }
            let caps: BTreeSet<String> = get(decl, "capabilities")
                .map(|e| strings(&e.node).into_iter().map(|(s, _)| s).collect())
                .unwrap_or_default();
            if let Some(info) = &info
                && let Some(e) = get(decl, "capabilities")
            {
                for (cap, span) in strings(&e.node) {
                    if !info.capabilities.contains(&cap) {
                        self.push(
                            Diagnostic::new(
                                "J0193",
                                format!("class `{class}` has no capability `{cap}`"),
                            )
                            .at(span),
                        );
                    }
                }
            }
            classes.insert(name.clone(), (class, info, caps));
        }
        for (name, entry) in &instances {
            let Some(decl) = entry.node.as_table() else {
                continue;
            };
            let Some((class, info, caps)) = classes.get(name).cloned() else {
                continue;
            };
            let origins = ["map", "implemented_by", "sim"]
                .iter()
                .filter(|k| match decl.get(**k) {
                    Some(e) if **k == "sim" => !matches!(e.node.value, Value::Bool(false)),
                    Some(_) => true,
                    None => false,
                })
                .count();
            let virtual_instance = decl.contains_key("implemented_by");
            let only_class_members = |info: &Option<ClassInfo>, map: &Table| {
                info.as_ref().is_some_and(|i| {
                    map.keys().all(|m| {
                        i.members
                            .get(m)
                            .is_some_and(|m| m.kind == MemberKind::Class)
                    })
                })
            };
            if origins > 1 {
                let allowed = virtual_instance
                    && !decl.contains_key("sim")
                    && get(decl, "map")
                        .and_then(|e| e.node.as_table())
                        .is_some_and(|m| only_class_members(&info, m));
                if !allowed {
                    self.push(
                        Diagnostic::new("J0192", format!("instance `{name}` has more than one origin"))
                            .at(entry.key)
                            .help("an instance comes from exactly one of `map`, `implemented_by` and `sim`"),
                    );
                }
            }
            if let Some(sim) = get(decl, "sim")
                && self.prod()
                && !matches!(sim.node.value, Value::Bool(false))
            {
                self.push(
                    Diagnostic::new("J0145", "`sim` instance in `mode = \"prod\"`").at(sim.key),
                );
            }
            if let Some(implemented) = get(decl, "implemented_by") {
                let app = match &implemented.node.value {
                    Value::String(s) => s.strip_prefix("app:").map(str::to_owned),
                    Value::Table(t) => text(t, "app").map(str::to_owned),
                    _ => None,
                };
                match app {
                    Some(app) if self.manifest.apps.contains_key(&app) => {
                        if let Some(interface) = self.context.apps.get(&app)
                            && !interface.implements.iter().any(|c| class_ref(c) == class)
                        {
                            self.push(
                                Diagnostic::new(
                                    "J0191",
                                    format!("app `{app}` does not implement `{class}`"),
                                )
                                .at(implemented.node.span),
                            );
                        }
                    }
                    Some(app) => self.push(
                        Diagnostic::new(
                            "J0191",
                            format!("`implemented_by` names unknown app `{app}`"),
                        )
                        .at(implemented.node.span),
                    ),
                    None => self.push(
                        Diagnostic::new("J0157", "invalid reference")
                            .at(implemented.node.span)
                            .label("expected \"app:<app>\""),
                    ),
                }
            }
            let map = get(decl, "map").and_then(|e| e.node.as_table());
            if let Some(info) = &info {
                for (member_name, member) in &info.members {
                    let bound = map.and_then(|m| m.get(member_name));
                    let needed = !member.optional
                        && member.capability.as_ref().is_none_or(|c| caps.contains(c))
                        && !virtual_instance
                        && !decl.contains_key("sim");
                    let class_member_needed = member.kind == MemberKind::Class && !member.optional;
                    if bound.is_none() && (needed || (virtual_instance && class_member_needed)) {
                        let code = if member.capability.is_some() {
                            "J0602"
                        } else {
                            "J0601"
                        };
                        self.push(
                            Diagnostic::new(
                                code,
                                format!("member `{member_name}` of instance `{name}` is not bound"),
                            )
                            .at(entry.key),
                        );
                    }
                }
                if let Some(map) = map {
                    for (member_name, item) in map {
                        let Some(member) = info.members.get(member_name) else {
                            self.push(
                                Diagnostic::new(
                                    "J0601",
                                    format!("class `{class}` has no member `{member_name}`"),
                                )
                                .at(item.key),
                            );
                            continue;
                        };
                        if virtual_instance && member.kind != MemberKind::Class {
                            self.push(
                                Diagnostic::new(
                                    "J0192",
                                    format!(
                                        "scalar member `{member_name}` mapped on a virtual instance"
                                    ),
                                )
                                .at(item.key),
                            );
                            continue;
                        }
                        self.instance_member(name, member_name, member, item, &classes);
                    }
                }
                let path_ty = |m: &crate::context::Member| SignalInfo {
                    ty: m.ty.clone(),
                    unit: m.unit,
                    direction: if m.kind == MemberKind::Out {
                        Direction::Output
                    } else {
                        Direction::Input
                    },
                    type_name: None,
                    raw: BTreeMap::new(),
                    acquisition: crate::context::Acquisition::NONE,
                };
                for (member_name, member) in &info.members {
                    if !matches!(
                        member.kind,
                        MemberKind::In | MemberKind::Out | MemberKind::Param
                    ) {
                        continue;
                    }
                    let alias = format!("{name}.{member_name}");
                    let target =
                        map.and_then(|m| m.get(member_name))
                            .and_then(|e| match &e.node.value {
                                Value::String(s) if !s.starts_with("const:") => Some(s.clone()),
                                Value::Table(t) => text(t, "signal").map(str::to_owned),
                                _ => None,
                            });
                    if let Some(existing) = self.paths.get(&alias) {
                        let canonical = existing.canonical.clone();
                        self.push(
                            Diagnostic::new(
                                "J0604",
                                format!("member path `{alias}` collides with `{canonical}`"),
                            )
                            .at(entry.key),
                        );
                        continue;
                    }
                    let registered = match target.as_deref().and_then(|t| self.paths.get(t)) {
                        Some(r) => Registered {
                            span: Some(entry.key),
                            ..r.clone()
                        },
                        None => Registered {
                            info: path_ty(member),
                            canonical: alias.clone(),
                            device: None,
                            span: Some(entry.key),
                        },
                    };
                    self.paths.insert(alias, registered);
                }
            } else {
                self.opaque.insert(name.clone());
            }
        }
        self.instance_cycles(&instances);
    }

    fn instance_member(
        &mut self,
        instance: &str,
        member_name: &str,
        member: &crate::context::Member,
        item: &Entry,
        classes: &BTreeMap<String, (String, Option<ClassInfo>, BTreeSet<String>)>,
    ) {
        if let Some(class_member) = &member.class {
            let names = strings(&item.node);
            let is_list = matches!(item.node.value, Value::Array(_));
            let count = u32::try_from(names.len()).unwrap_or(u32::MAX);
            if (class_member.fixed && is_list)
                || count < class_member.min
                || count > class_member.max
            {
                self.push(
                    Diagnostic::new(
                        "J0653",
                        format!(
                            "member `{member_name}` takes {}–{} instances",
                            class_member.min, class_member.max
                        ),
                    )
                    .at(item.node.span),
                );
            }
            for (bound, span) in names {
                let Some((class, _, caps)) = classes.get(&bound) else {
                    let mut d =
                        Diagnostic::new("J0650", format!("`{bound}` is not an instance")).at(span);
                    if let Some(owner) = self.paths.get(&bound).and_then(|r| r.device.clone()) {
                        d = d.help(format!("`{bound}` is a signal of `{owner}`"));
                    }
                    self.push(d);
                    continue;
                };
                if *class != class_ref(&class_member.class) {
                    self.push(
                        Diagnostic::new(
                            "J0651",
                            format!("`{bound}` is a `{class}`, not a `{}`", class_member.class),
                        )
                        .at(span),
                    );
                }
                for required in &class_member.requires {
                    if !caps.contains(required) {
                        self.push(
                            Diagnostic::new(
                                "J0652",
                                format!("`{bound}` lacks the capability `{required}`"),
                            )
                            .at(span),
                        );
                    }
                }
            }
            return;
        }
        match &item.node.value {
            Value::String(s) if s.starts_with("const:") => {
                if member.kind == MemberKind::Out {
                    self.push(
                        Diagnostic::new("J0603", format!("`const:` on out member `{member_name}`"))
                            .at(item.node.span),
                    );
                }
            }
            Value::String(s) if member.kind == MemberKind::Action => {
                if s.split_once('.').is_none() {
                    self.push(
                        Diagnostic::new("J0194", "invalid adapter for an action")
                            .at(item.node.span),
                    );
                }
            }
            Value::String(s) => {
                if !self.paths.contains_key(s) {
                    self.report_unknown(s, item.node.span);
                } else if member.kind == MemberKind::Out
                    && self
                        .paths
                        .get(s)
                        .is_some_and(|r| r.info.direction != Direction::Output)
                {
                    self.push(
                        Diagnostic::new(
                            "J0194",
                            format!("out member `{member_name}` mapped to an input"),
                        )
                        .at(item.node.span),
                    );
                }
            }
            Value::Table(t) => {
                if t.contains_key("enum_map") && !matches!(member.ty, SignalType::Enum { .. }) {
                    self.push(
                        Diagnostic::new("J0194", "`enum_map` on a member that is not an enum")
                            .at(item.node.span),
                    );
                }
                if t.contains_key("edge")
                    && !(member.kind == MemberKind::Out && member.ty == SignalType::Bool)
                {
                    self.push(
                        Diagnostic::new("J0194", "`edge` only adapts a boolean out member")
                            .at(item.node.span),
                    );
                }
                if (t.contains_key("on_rise") || t.contains_key("on_change"))
                    && member.kind != MemberKind::Event
                {
                    self.push(
                        Diagnostic::new("J0194", "`on_rise`/`on_change` only adapt an event")
                            .at(item.node.span),
                    );
                }
                if let Some(signal) = text(t, "signal")
                    && !self.paths.contains_key(signal)
                {
                    let span = t.get("signal").map_or(item.node.span, |e| e.node.span);
                    self.report_unknown(signal, span);
                }
                let _ = instance;
            }
            _ => {}
        }
    }

    fn instance_cycles(&mut self, instances: &Table) {
        let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, entry) in instances {
            let Some(map) = entry
                .node
                .as_table()
                .and_then(|d| d.get("map"))
                .and_then(|e| e.node.as_table())
            else {
                continue;
            };
            for item in map.values() {
                for (bound, _) in strings(&item.node) {
                    if instances.contains_key(&bound) {
                        edges.entry(name.clone()).or_default().push(bound);
                    }
                }
            }
        }
        for (start, entry) in instances {
            let mut stack = vec![start.clone()];
            let mut seen = BTreeSet::new();
            while let Some(node) = stack.pop() {
                for next in edges.get(&node).into_iter().flatten() {
                    if next == start {
                        self.push(
                            Diagnostic::new(
                                "J0654",
                                format!("instance `{start}` is a member of itself"),
                            )
                            .at(entry.key),
                        );
                        stack.clear();
                        break;
                    }
                    if seen.insert(next.clone()) {
                        stack.push(next.clone());
                    }
                }
            }
        }
    }

    fn calc_and_alarms(&mut self) {
        let calc = self.section("calc").cloned().unwrap_or_default();
        let alarms = self.section("alarms").cloned().unwrap_or_default();
        for (name, entry) in &calc {
            self.paths.insert(
                format!("calc.{name}"),
                Registered {
                    info: SignalInfo::new(SignalType::Unknown, Direction::Input),
                    canonical: format!("calc.{name}"),
                    device: None,
                    span: Some(entry.key),
                },
            );
            let group = entry.node.as_table().and_then(|t| t.get("group"));
            if let Some(group) = group.and_then(|g| g.node.as_str().map(|s| (s, g.node.span)))
                && !self.group_exists(group.0)
            {
                self.push(
                    Diagnostic::new("J0237", format!("group `{}` does not exist", group.0))
                        .at(group.1),
                );
            }
        }
        for (name, entry) in &alarms {
            if name == "app" {
                self.push(
                    Diagnostic::new("J0196", "alarm named `app`")
                        .at(entry.key)
                        .help("`alarm.app.*` is where app alarms live"),
                );
                continue;
            }
            for (suffix, info) in [
                (
                    "state",
                    SignalInfo::new(
                        SignalType::Enum {
                            name: expr::ALARM_STATE.to_owned(),
                            variants: expr::ALARM_STATES.iter().map(|s| (*s).to_owned()).collect(),
                        },
                        Direction::Input,
                    ),
                ),
                (
                    "active",
                    SignalInfo::new(SignalType::Bool, Direction::Input),
                ),
            ] {
                let path = format!("alarm.{name}.{suffix}");
                if self.paths.contains_key(&path) {
                    self.push(
                        Diagnostic::new("J0196", format!("alarm path `{path}` collides"))
                            .at(entry.key),
                    );
                    continue;
                }
                self.paths.insert(
                    path.clone(),
                    Registered {
                        info,
                        canonical: path,
                        device: None,
                        span: Some(entry.key),
                    },
                );
            }
            if let Some(decl) = entry.node.as_table()
                && let Some(group) = get(decl, "group")
                    .and_then(|g| g.node.as_str().map(|s| (s.to_owned(), g.node.span)))
                && !self.group_exists(&group.0)
            {
                self.push(
                    Diagnostic::new("J0237", format!("group `{}` does not exist", group.0))
                        .at(group.1),
                );
            }
        }
        let mut pending: Vec<(String, String, Span)> = calc
            .iter()
            .filter_map(|(name, entry)| {
                let node = match &entry.node.value {
                    Value::Table(t) => &t.get("expr")?.node,
                    _ => &entry.node,
                };
                Some((name.clone(), node.as_str()?.to_owned(), node.span))
            })
            .collect();
        loop {
            let before = pending.len();
            let mut waiting = Vec::new();
            for (name, source, span) in pending {
                let (outcome, diagnostic, info) = {
                    let registry = expr::Registry::new(&self.paths);
                    let opaque = |p: &str| self.opaque_path(p);
                    let (outcome, diagnostic) =
                        expr::check(&source, span, &registry, Expect::Any, &opaque);
                    let info = match outcome {
                        Outcome::Typed(ty) => Some(expr::signal_of(ty, &registry)),
                        _ => None,
                    };
                    (outcome, diagnostic, info)
                };
                match outcome {
                    Outcome::Typed(_) => {
                        if let (Some(info), Some(r)) =
                            (info, self.paths.get_mut(&format!("calc.{name}")))
                        {
                            r.info = info;
                        }
                    }
                    Outcome::Opaque(at) if self.sources.text(at).starts_with("calc.") => {
                        waiting.push((name, source, span))
                    }
                    Outcome::Opaque(_) => {}
                    Outcome::Failed => {
                        if let Some(d) = diagnostic {
                            self.push(d);
                        }
                    }
                }
            }
            if waiting.len() == before || waiting.is_empty() {
                for (name, _, span) in &waiting {
                    let dependencies: Vec<String> = waiting
                        .iter()
                        .filter(|(other, _, _)| other != name)
                        .map(|(other, _, _)| format!("calc.{other}"))
                        .collect();
                    self.push(
                        Diagnostic::new("J0190", format!("`calc.{name}` depends on itself"))
                            .at(*span)
                            .label(if dependencies.is_empty() {
                                "through its own value".to_owned()
                            } else {
                                format!("through {}", dependencies.join(", "))
                            }),
                    );
                }
                break;
            }
            pending = waiting;
        }
        for (name, entry) in &alarms {
            let Some(decl) = entry.node.as_table() else {
                continue;
            };
            for key in ["when", "suppress_when"] {
                if let Some(e) = get(decl, key)
                    && let Some(source) = e.node.as_str()
                {
                    self.expression(source, e.node.span, Expect::Bool);
                }
            }
            if let Some(message) = get(decl, "message")
                && let Some(text) = message.node.as_str()
            {
                for placeholder in placeholders(text) {
                    if !self.paths.contains_key(&placeholder) && !self.opaque_path(&placeholder) {
                        self.push(
                            Diagnostic::new(
                                "J0302",
                                format!(
                                    "unknown signal `{placeholder}` in the message of `{name}`"
                                ),
                            )
                            .at(message.node.span),
                        );
                    }
                }
            }
        }
    }

    fn alarm_actions(&mut self) {
        let alarms = self.section("alarms").cloned().unwrap_or_default();
        for entry in alarms.values() {
            let Some(trip) = entry
                .node
                .as_table()
                .and_then(|d| d.get("actions"))
                .and_then(|e| e.node.as_table())
                .and_then(|a| a.get("trip"))
            else {
                continue;
            };
            for (target, span) in strings(&trip.node) {
                self.trip_target(&target, span);
            }
        }
    }

    fn trip_target(&mut self, target: &str, span: Span) {
        if self.opaque_path(target) {
            return;
        }
        if grammar::is_wildcard(target) {
            if self.outputs_of(target).is_empty() {
                self.push(
                    Diagnostic::new("J0155", format!("selector `{target}` matches no output"))
                        .at(span),
                );
            }
        } else if !self.paths.contains_key(target) {
            self.report_unknown(target, span);
        } else if self.outputs_of(target).is_empty() {
            self.push(
                Diagnostic::new("J0234", format!("`{target}` is not an output of this node"))
                    .at(span),
            );
        }
    }

    fn signals(&mut self) {
        let Some(signals) = self.section("signals").cloned() else {
            return;
        };
        let mut set: BTreeMap<(String, String), (String, Span)> = BTreeMap::new();
        for (device_name, device) in &self.manifest.devices {
            for (signal, tuning) in &device.signals {
                for (key, node) in &tuning.value {
                    set.insert(
                        (format!("{device_name}.{signal}"), key.clone()),
                        (render(node), node.span),
                    );
                }
            }
        }
        for (key, entry) in &signals {
            let targets: Vec<String> = if grammar::is_wildcard(key) {
                self.paths
                    .iter()
                    .filter(|(p, r)| **p == r.canonical && grammar::matches(key, p))
                    .map(|(p, _)| p.clone())
                    .collect()
            } else if let Some(r) = self.paths.get(key) {
                vec![r.canonical.clone()]
            } else {
                if !self.opaque_path(key) {
                    self.report_unknown(key, entry.key);
                }
                continue;
            };
            if targets.is_empty() {
                self.push(
                    Diagnostic::new("J0155", format!("selector `{key}` matches nothing"))
                        .at(entry.key),
                );
                continue;
            }
            let Some(adjust) = entry.node.as_table() else {
                continue;
            };
            for target in targets {
                let derived = ["calc.", "app.", "alarm."]
                    .iter()
                    .any(|p| target.starts_with(p));
                for (name, item) in adjust {
                    if derived && name != "record" {
                        self.push(
                            Diagnostic::new(
                                "J0176",
                                format!("`{name}` does not apply to the derived signal `{target}`"),
                            )
                            .at(item.key),
                        );
                        continue;
                    }
                    let value = render(&item.node);
                    match set.get(&(target.clone(), name.clone())) {
                        Some((other, span)) if *other != value => {
                            let span = *span;
                            self.push(
                                Diagnostic::new("J0175", format!("`{name}` of `{target}` declared twice with different values"))
                                    .at(item.node.span)
                                    .label(format!("also {other} at another place"))
                                    .help("keep one adjustment per key and signal"),
                            );
                            let _ = span;
                        }
                        _ => {
                            set.insert((target.clone(), name.clone()), (value, item.node.span));
                        }
                    }
                }
            }
        }
    }

    fn bridge_signals(&mut self) {
        let Some(bridges) = self.section("bridges").cloned() else {
            return;
        };
        for (name, entry) in &bridges {
            let Some(subscribe) = entry
                .node
                .as_table()
                .and_then(|b| b.get("subscribe"))
                .and_then(|e| e.node.as_table())
            else {
                continue;
            };
            for item in subscribe.values() {
                let Some(sub) = item.node.as_table() else {
                    continue;
                };
                for (to, _) in sub.get("to").map(|e| strings(&e.node)).unwrap_or_default() {
                    if to.starts_with(&format!("bridge.{name}.")) && !self.paths.contains_key(&to) {
                        self.paths.insert(
                            to.clone(),
                            Registered {
                                info: SignalInfo::new(SignalType::Float, Direction::Input),
                                canonical: to,
                                device: None,
                                span: Some(item.key),
                            },
                        );
                    }
                }
            }
        }
    }

    /// The expressions of an envelope: `interlock`, `reset_when` and the
    /// non-constant `range` bounds (ref/01 §3.7).
    pub(crate) fn envelope_expressions(&mut self, registered: &Registered, envelope: &Envelope) {
        for e in [&envelope.interlock, &envelope.reset_when]
            .into_iter()
            .flatten()
        {
            self.expression(&e.value, e.span, Expect::Bool);
        }
        if let Some(range) = &envelope.range
            && let Some(unit) = registered.info.unit
        {
            for bound in &range.value {
                if joker_units::Literal::parse(&bound.value).is_err() {
                    self.expression(&bound.value, bound.span, Expect::Unit(unit));
                }
            }
        }
    }

    fn output_rules(&mut self) {
        let manifest = self.manifest;
        let alarms = self.section("alarms").cloned().unwrap_or_default();
        let tripped: BTreeSet<String> = alarms
            .values()
            .filter_map(|e| {
                e.node
                    .as_table()?
                    .get("actions")?
                    .node
                    .as_table()?
                    .get("trip")
                    .map(|t| strings(&t.node))
            })
            .flatten()
            .flat_map(|(t, _)| self.outputs_of(&t))
            .collect();
        for (key, envelope) in &manifest.outputs {
            let targets = self.outputs_of(key);
            if let Some(reset) = &envelope.reset_when
                && envelope.on_range != "trip"
                && !targets.iter().any(|t| tripped.contains(t))
            {
                self.push(
                    Diagnostic::new(
                        "J0236",
                        format!("`reset_when` on `{key}`, which never trips"),
                    )
                    .at(reset.span),
                );
            }
            for target in &targets {
                let Some(device) = self.paths.get(target).and_then(|r| r.device.clone()) else {
                    continue;
                };
                if self.opaque.contains(&device) {
                    continue;
                }
                let profile = manifest
                    .devices
                    .get(&device)
                    .and_then(|d| d.profile.as_ref())
                    .and_then(|p| self.context.profiles.get(&p.value));
                let signal = target
                    .strip_prefix(device.as_str())
                    .and_then(|rest| rest.strip_prefix('.'))
                    .unwrap_or(target);
                let has_fallback = profile.is_some_and(|p| p.fallback_covers(signal));
                if has_fallback {
                    if let (Some(value), Some(safe)) = (
                        profile.and_then(|p| p.fallback_value.as_ref()),
                        &envelope.safe,
                    ) {
                        let unit = self.paths.get(target).and_then(|r| r.info.unit);
                        if !fallback_as_safe(value, &safe.value, &envelope.safer, unit.as_ref()) {
                            self.push(
                                Diagnostic::new(
                                    "J0205",
                                    format!(
                                        "the device fallback of `{target}` is not as safe as its `safe` (`safer = \"{}\"`)",
                                        envelope.safer
                                    ),
                                )
                                .at(safe.span)
                                .label("the profile's `[fallback] value` is applied when the node stops"),
                            );
                        }
                    }
                    if let Some(accept) = &envelope.accept_missing_fallback {
                        self.push(
                            Diagnostic::new(
                                "J0277",
                                format!(
                                    "`accept_missing_fallback` on `{target}`, which has a fallback"
                                ),
                            )
                            .at(accept.span),
                        );
                    }
                } else {
                    if envelope.safer != "equal" {
                        self.push(
                            Diagnostic::new(
                                "J0238",
                                format!(
                                    "`safer` on `{target}` without a device fallback (ignored)"
                                ),
                            )
                            .at(envelope.key),
                        );
                    }
                    if envelope.accept_missing_fallback.is_none() {
                        self.push(
                            Diagnostic::new("J0205", format!("output `{target}` has no device fallback"))
                                .at(envelope.key)
                                .label("if the node stops, the output stays where it was")
                                .help("declare a fallback in the profile, or justify with `accept_missing_fallback`"),
                        );
                    }
                }
            }
        }
    }

    fn atomic(&mut self) {
        let Some(atomic) = self.section("atomic").cloned() else {
            return;
        };
        let mut member_of: BTreeMap<String, String> = BTreeMap::new();
        for (name, entry) in &atomic {
            let Some(decl) = entry.node.as_table() else {
                continue;
            };
            let outputs: Vec<(String, Span)> = decl
                .get("outputs")
                .map(|e| strings(&e.node))
                .unwrap_or_default();
            if outputs.len() < 2 {
                self.push(
                    Diagnostic::new(
                        "J0270",
                        format!("atomic group `{name}` needs at least two outputs"),
                    )
                    .at(entry.key),
                );
            }
            let mut devices = BTreeSet::new();
            let mut groups = BTreeSet::new();
            for (output, span) in &outputs {
                if !self.owner.contains_key(output) {
                    if self.paths.contains_key(output) || !self.opaque_path(output) {
                        self.push(
                            Diagnostic::new(
                                "J0201",
                                format!("atomic output `{output}` has no envelope"),
                            )
                            .at(*span),
                        );
                    }
                    continue;
                }
                if let Some(other) = member_of.insert(output.clone(), name.clone()) {
                    self.push(
                        Diagnostic::new(
                            "J0270",
                            format!("`{output}` is in atomic groups `{other}` and `{name}`"),
                        )
                        .at(*span),
                    );
                }
                if let Some(device) = self.paths.get(output).and_then(|r| r.device.clone()) {
                    devices.insert(device);
                }
                let group = self
                    .owner
                    .get(output)
                    .and_then(|k| self.manifest.outputs.get(k))
                    .and_then(|e| e.group.as_ref())
                    .map_or_else(|| "default".to_owned(), |g| g.value.clone());
                groups.insert(group);
            }
            if devices.len() > 1 {
                let buses: BTreeSet<Option<String>> = devices
                    .iter()
                    .map(|d| {
                        self.manifest
                            .devices
                            .get(d)
                            .and_then(|d| d.bus.as_ref())
                            .map(|b| b.value.clone())
                    })
                    .collect();
                let ethercat = buses.len() == 1
                    && buses.iter().all(Option::is_some)
                    && buses.iter().flatten().all(|b| {
                        self.manifest
                            .buses
                            .get(b)
                            .is_some_and(|b| b.kind.value == crate::model::BusKind::Ethercat)
                    });
                if !ethercat {
                    self.push(
                        Diagnostic::new(
                            "J0271",
                            format!(
                                "atomic group `{name}` spans devices without a common transport"
                            ),
                        )
                        .at(entry.key),
                    );
                }
            }
            if groups.len() > 1 {
                self.push(
                    Diagnostic::new(
                        "J0272",
                        format!("atomic group `{name}` has outputs of different owner groups"),
                    )
                    .at(entry.key),
                );
            }
            if let Some(order) = decl.get("order") {
                let mut a: Vec<String> = strings(&order.node).into_iter().map(|(s, _)| s).collect();
                let mut b: Vec<String> = outputs.iter().map(|(s, _)| s.clone()).collect();
                a.sort();
                b.sort();
                if a != b {
                    self.push(
                        Diagnostic::new("J0273", "`order` is not a permutation of `outputs`")
                            .at(order.node.span),
                    );
                }
            }
            if let Some(commit) = decl.get("commit").and_then(|e| e.node.as_table())
                && let Some(signal) = commit.get("signal")
                && let Some(path) = signal.node.as_str()
            {
                let device = self.paths.get(path).and_then(|r| r.device.clone());
                let ok = self
                    .paths
                    .get(path)
                    .is_some_and(|r| r.info.direction == Direction::Output)
                    && device.is_some()
                    && devices.iter().all(|d| Some(d) == device.as_ref())
                    && !outputs.iter().any(|(o, _)| o == path)
                    && !self.owner.contains_key(path);
                if !ok && !self.opaque_path(path) {
                    self.push(
                        Diagnostic::new("J0274", "invalid `commit.signal`")
                            .at(signal.node.span)
                            .label("an output of the same device, outside `outputs`, without its own envelope"),
                    );
                }
            }
        }
    }

    fn telemetry(&mut self) {
        let Some(telemetry) = self.section("telemetry").cloned() else {
            return;
        };
        if let Some(record) = telemetry.get("record").and_then(|e| e.node.as_table()) {
            let mut claimed: BTreeMap<String, (u32, usize, String)> = BTreeMap::new();
            for (selector, entry) in record {
                let Some(rule) = entry.node.as_table() else {
                    continue;
                };
                let known = self.paths.iter().any(|(p, r)| {
                    **p == r.canonical
                        && r.info.ty != SignalType::Unknown
                        && (if grammar::is_wildcard(selector) {
                            grammar::matches(selector, p)
                        } else {
                            *p == *selector
                        })
                });
                if known
                    && !rule.contains_key("blocks")
                    && rule.contains_key("every") == rule.contains_key("on_change")
                {
                    self.push(
                        Diagnostic::new(
                            "J0195",
                            format!("`{selector}` needs exactly one of `every` and `on_change`"),
                        )
                        .at(entry.key),
                    );
                }
                if let Some(every) = rule.get("every")
                    && duration(&every.node)
                        .is_some_and(|d| d.as_nanos() < Duration::from_millis(100).as_nanos())
                {
                    self.push(
                        Diagnostic::new("J0152", "invalid duration")
                            .at(every.node.span)
                            .label("at least 100ms"),
                    );
                }
                if let Some(keep) = rule.get("keep").and_then(|e| e.node.as_table()) {
                    for (layer, item) in keep {
                        if !RECORD_LAYERS.contains(&layer.as_str()) {
                            self.push(
                                Diagnostic::new(
                                    "J0197",
                                    format!("unknown layer `{layer}` in `keep`"),
                                )
                                .at(item.key),
                            );
                        }
                    }
                }
                let matched: Vec<String> = self
                    .paths
                    .iter()
                    .filter(|(p, r)| {
                        **p == r.canonical
                            && (if grammar::is_wildcard(selector) {
                                grammar::matches(selector, p)
                            } else {
                                *p == selector
                            })
                    })
                    .map(|(p, _)| p.clone())
                    .collect();
                if matched.is_empty() && !self.opaque_path(selector) {
                    self.push(
                        Diagnostic::new(
                            "J0156",
                            format!("telemetry selector `{selector}` matches nothing"),
                        )
                        .at(entry.key),
                    );
                }
                let literal = selector.split('.').filter(|s| !s.contains('*')).count() as u32;
                let prefix = selector.find('*').unwrap_or(selector.len());
                for path in matched {
                    let rank = if grammar::is_wildcard(selector) {
                        (literal, prefix)
                    } else {
                        (u32::MAX, usize::MAX)
                    };
                    match claimed.get(&path) {
                        Some((l, p, other)) if (*l, *p) == rank => {
                            self.push(
                                Diagnostic::new("J0195", format!("`{path}` recorded by `{other}` and `{selector}` with the same specificity")).at(entry.key),
                            );
                        }
                        Some((l, p, _)) if (*l, *p) > rank => {}
                        _ => {
                            claimed.insert(path, (rank.0, rank.1, selector.clone()));
                        }
                    }
                }
            }
        }
        if let Some(store) = telemetry.get("store").and_then(|e| e.node.as_table())
            && let Some(interval) = store.get("fsync_interval")
            && text(store, "fsync").unwrap_or("batch") != "batch"
        {
            self.push(
                Diagnostic::new(
                    "J0145",
                    "`fsync_interval` only applies with `fsync = \"batch\"`",
                )
                .at(interval.key),
            );
        }
        if let Some(uplinks) = telemetry.get("uplink").and_then(|e| e.node.as_table()) {
            for (name, entry) in uplinks {
                if let Some(uplink) = entry.node.as_table() {
                    self.uplink(name, entry, uplink);
                }
            }
        }
    }

    fn uplink(&mut self, name: &str, entry: &Entry, uplink: &Table) {
        const COMMON: [&str; 8] = [
            "kind", "classes", "budget", "live", "history", "username", "password", "token",
        ];
        let kind = text(uplink, "kind").unwrap_or("");
        let (own, required): (&[&str], &[&str]) = match kind {
            "mqtt" => (&["broker", "topic", "encoding", "qos"], &["broker"]),
            "mqtt-sparkplug" => (&["broker", "group_id", "node_id"], &["broker", "group_id"]),
            "zenoh" => (&["endpoints", "key_prefix", "mode"], &["endpoints"]),
            "otlp" => (&["endpoint", "signals", "every"], &["endpoint"]),
            "http" => (&["url", "events"], &["url"]),
            _ => return,
        };
        for (key, item) in uplink {
            if !COMMON.contains(&key.as_str())
                && !own.contains(&key.as_str())
                && !key.starts_with("x_")
            {
                self.push(
                    Diagnostic::new(
                        "J0145",
                        format!("`{key}` does not apply to a `{kind}` uplink"),
                    )
                    .at(item.key),
                );
            }
        }
        for key in required {
            if !uplink.contains_key(*key) {
                self.push(
                    Diagnostic::new("J0142", format!("uplink `{name}` without `{key}`"))
                        .at(entry.key),
                );
            }
        }
        if kind == "otlp" {
            for key in ["classes", "live", "history"] {
                if let Some(item) = uplink.get(key) {
                    self.push(
                        Diagnostic::new(
                            "J0198",
                            format!("`{key}` does not apply to an `otlp` uplink"),
                        )
                        .at(item.key),
                    );
                }
            }
        }
        if let Some(classes) = uplink.get("classes").and_then(|e| e.node.as_table()) {
            for (class, item) in classes {
                if !UPLINK_CLASSES.contains(&class.as_str()) {
                    self.push(
                        Diagnostic::new("J0198", format!("unknown uplink class `{class}`"))
                            .at(item.key),
                    );
                }
                for (selector, span) in strings(&item.node) {
                    if !self.matches_any(&selector) {
                        self.push(
                            Diagnostic::new(
                                "J0156",
                                format!("uplink selector `{selector}` matches nothing"),
                            )
                            .at(span),
                        );
                    }
                }
            }
        }
        for key in ["broker", "endpoint", "url"] {
            if let Some(item) = uplink.get(key)
                && let Some(url) = item.node.as_str()
                && self.prod()
                && (url.starts_with("mqtt://") || url.starts_with("http://"))
            {
                self.push(Diagnostic::new("J0296", "connection without TLS").at(item.node.span));
            }
        }
    }

    fn bridges(&mut self) {
        let Some(bridges) = self.section("bridges").cloned() else {
            return;
        };
        for (name, entry) in &bridges {
            let Some(bridge) = entry.node.as_table() else {
                continue;
            };
            let kind = text(bridge, "kind").unwrap_or("");
            let own: &[&str] = match kind {
                "opcua-server" => &["listen", "security"],
                "modbus-server" => &["listen", "bus", "unit_id", "profile"],
                "zenoh-ros2" => &["subscribe", "endpoints"],
                _ => continue,
            };
            for (key, item) in bridge {
                if !["kind", "expose", "writer"].contains(&key.as_str())
                    && !own.contains(&key.as_str())
                    && !key.starts_with("x_")
                {
                    self.push(
                        Diagnostic::new(
                            "J0145",
                            format!("`{key}` does not apply to a `{kind}` bridge"),
                        )
                        .at(item.key),
                    );
                }
            }
            let writer = bridge.get("writer").and_then(|e| e.node.as_table());
            let writable: Vec<(String, Span)> = writer
                .and_then(|w| w.get("writable"))
                .map(|e| strings(&e.node))
                .unwrap_or_default();
            if let Some(writer) = writer {
                if let Some(level) = writer
                    .get("level")
                    .and_then(|e| e.node.as_str().map(|s| (s, e.node.span)))
                {
                    match grammar::level(level.0) {
                        Ok(l) if l.band == Band::Safety => self.push(
                            Diagnostic::new("J0253", "a bridge in the `safety` band").at(level.1),
                        ),
                        Ok(l) if l.band == Band::Manual => self.push(
                            Diagnostic::new("J0252", "a bridge in the `manual` band").at(level.1),
                        ),
                        Ok(_) => {}
                        Err(grammar::LevelError::Band) => {
                            self.push(Diagnostic::new("J0250", "invalid level").at(level.1))
                        }
                        Err(grammar::LevelError::Priority(_)) => self.push(
                            Diagnostic::new("J0251", "`priority` outside the band").at(level.1),
                        ),
                    }
                } else if !writable.is_empty() {
                    self.push(
                        Diagnostic::new(
                            "J0142",
                            format!("bridge `{name}` writes without `writer.level`"),
                        )
                        .at(entry.key),
                    );
                }
            }
            for (target, span) in &writable {
                let outputs = self.outputs_of(target);
                if self.opaque_path(target) {
                    continue;
                }
                if outputs.is_empty() {
                    self.push(
                        Diagnostic::new("J0155", format!("`{target}` matches no output")).at(*span),
                    );
                } else if outputs.iter().any(|o| !self.owner.contains_key(o)) {
                    self.push(
                        Diagnostic::new(
                            "J0199",
                            format!("`{target}` has an output without an envelope"),
                        )
                        .at(*span),
                    );
                }
            }
            if let Some(expose) = bridge.get("expose") {
                let selectors: Vec<(String, Span)> = match &expose.node.value {
                    Value::Table(t) => t.iter().map(|(k, e)| (k.clone(), e.key)).collect(),
                    _ => strings(&expose.node),
                };
                for (selector, span) in selectors {
                    if !self.matches_any(&selector) {
                        self.push(
                            Diagnostic::new("J0156", format!("`{selector}` exposes nothing"))
                                .at(span),
                        );
                    }
                }
            }
            match kind {
                "opcua-server" => {
                    let security = bridge.get("security").and_then(|e| e.node.as_table());
                    for key in ["policy", "mode"] {
                        if let Some(item) = security.and_then(|s| s.get(key))
                            && item.node.as_str() == Some("None")
                            && self.prod()
                        {
                            self.push(
                                Diagnostic::new("J0296", "OPC UA without security")
                                    .at(item.node.span),
                            );
                        }
                    }
                    if !writable.is_empty() && security.is_none_or(|s| !s.contains_key("trust")) {
                        self.push(
                            Diagnostic::new(
                                "J0142",
                                format!("bridge `{name}` writes without `security.trust`"),
                            )
                            .at(entry.key),
                        );
                    }
                    self.listen_all(bridge, &writable);
                }
                "modbus-server" => {
                    if bridge.contains_key("listen") == bridge.contains_key("bus") {
                        self.push(
                            Diagnostic::new(
                                "J0199",
                                format!("bridge `{name}` needs exactly one of `listen` and `bus`"),
                            )
                            .at(entry.key),
                        );
                    }
                    if !bridge.contains_key("profile") {
                        self.push(
                            Diagnostic::new("J0142", format!("bridge `{name}` without `profile`"))
                                .at(entry.key),
                        );
                    }
                    if let Some(bus) = bridge
                        .get("bus")
                        .and_then(|e| e.node.as_str().map(|s| (s, e.node.span)))
                        && self
                            .manifest
                            .buses
                            .get(bus.0)
                            .is_none_or(|b| b.kind.value != crate::model::BusKind::Serial)
                    {
                        self.push(
                            Diagnostic::new("J0171", format!("`{}` is not a serial bus", bus.0))
                                .at(bus.1),
                        );
                    }
                    self.listen_all(bridge, &writable);
                }
                _ => {
                    for item in bridge
                        .get("subscribe")
                        .and_then(|e| e.node.as_table())
                        .into_iter()
                        .flat_map(|t| t.values())
                    {
                        let Some(sub) = item.node.as_table() else {
                            continue;
                        };
                        let to = sub.get("to").map(|e| strings(&e.node)).unwrap_or_default();
                        if let Some(fields) = sub.get("fields")
                            && strings(&fields.node).len() != to.len()
                        {
                            self.push(
                                Diagnostic::new("J0199", "`fields` and `to` of different lengths")
                                    .at(fields.node.span),
                            );
                        }
                        for (path, span) in to {
                            let own = path.starts_with(&format!("bridge.{name}."));
                            let output = !self.outputs_of(&path).is_empty();
                            if !own && !output && !self.opaque_path(&path) {
                                self.push(
                                    Diagnostic::new(
                                        "J0199",
                                        format!(
                                            "`{path}` is neither `bridge.{name}.…` nor an output"
                                        ),
                                    )
                                    .at(span),
                                );
                            }
                            if output && writer.is_none() {
                                self.push(
                                    Diagnostic::new(
                                        "J0142",
                                        format!("bridge `{name}` writes `{path}` without `writer`"),
                                    )
                                    .at(span),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn listen_all(&mut self, bridge: &Table, writable: &[(String, Span)]) {
        if let Some(listen) = bridge.get("listen")
            && let Some(address) = listen.node.as_str()
            && (address.starts_with("0.0.0.0:") || address.starts_with("[::]:"))
            && !writable.is_empty()
            && self.prod()
        {
            self.push(
                Diagnostic::new("J0297", "bridge with writes listening on every interface")
                    .at(listen.node.span),
            );
        }
    }

    fn security(&mut self) {
        let Some(security) = self.section("security").cloned() else {
            return;
        };
        if let Some(trust) = security.get("trust").and_then(|e| e.node.as_table()) {
            for (role, item) in trust {
                if role != "revoked"
                    && !TRUST_ROLES.contains(&role.as_str())
                    && !role.starts_with("x_")
                {
                    self.push(
                        Diagnostic::new("J0144", format!("unknown trust role `{role}`"))
                            .at(item.key),
                    );
                }
            }
            if let Some(revoked) = trust.get("revoked") {
                for (id, span) in strings(&revoked.node) {
                    let ok = id
                        .strip_prefix("ed25519:")
                        .is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()));
                    if !ok {
                        self.push(
                            Diagnostic::new("J0144", "invalid key id")
                                .at(span)
                                .label("expected \"ed25519:<32 hex>\""),
                        );
                    }
                }
            }
        }
        if let Some(listen) = security
            .get("api")
            .and_then(|e| e.node.as_table())
            .and_then(|a| a.get("listen"))
        {
            for (address, span) in strings(&listen.node) {
                let ok = address
                    .strip_prefix("unix:")
                    .is_some_and(|p| p.starts_with('/'))
                    || address.strip_prefix("tcp:").is_some_and(grammar::host_port);
                if !ok {
                    self.push(
                        Diagnostic::new("J0157", "invalid listener")
                            .at(span)
                            .label("expected \"unix:<path>\" or \"tcp:<host>:<port>\""),
                    );
                }
            }
        }
        if let Some(roles) = security.get("roles").and_then(|e| e.node.as_table()) {
            for item in roles.values() {
                for (principal, span) in strings(&item.node) {
                    match principal_ok(&principal) {
                        None => self.push(
                            Diagnostic::new("J0295", format!("invalid principal `{principal}`"))
                                .at(span),
                        ),
                        Some(true) => self.push(
                            Diagnostic::new(
                                "J0293",
                                format!("`{principal}` grants the role to any certificate"),
                            )
                            .at(span),
                        ),
                        Some(false) => {}
                    }
                }
            }
        }
    }

    fn micro_target(&mut self) {
        let micro = self.micro;
        let Some(target) = &self.manifest.target else {
            return;
        };
        if micro && let Some(arch) = &target.arch {
            self.push(
                Diagnostic::new("J0145", "`arch` does not apply to a Joker Micro target")
                    .at(arch.span),
            );
        }
        let link = self.section("link").cloned();
        let wifi = table(self.manifest.pending.get("net"))
            .and_then(|n| n.get("wifi"))
            .and_then(|e| e.node.as_table())
            .cloned();
        if !micro {
            return;
        }
        if let Some(link) = &link {
            let ip = link.get("address").and_then(|e| e.node.as_str());
            if link.contains_key("bus") == link.contains_key("address") {
                self.push(
                    Diagnostic::new("J0145", "`[link]` needs exactly one of `bus` and `address`")
                        .at(link.values().next().map_or(target.span, |e| e.key)),
                );
            }
            if let Some(address) = link.get("address")
                && let Some(text) = ip
                && !(text
                    .strip_prefix("udp:")
                    .or_else(|| text.strip_prefix("tcp:"))
                    .is_some_and(grammar::host_port))
            {
                self.push(Diagnostic::new("J0157", "invalid link address").at(address.node.span));
            }
            if ip.is_some() && link.get("auth").and_then(|e| e.node.as_str()) == Some("none") {
                self.push(
                    Diagnostic::new(
                        "J0179",
                        "Joker Link without authentication on an IP transport",
                    )
                    .at(link.get("auth").map_or(target.span, |e| e.node.span)),
                );
            }
            if ip.is_some() && wifi.is_none() {
                self.push(
                    Diagnostic::new(
                        "J0241",
                        "IP transport on a Joker Micro without `[net.wifi]`",
                    )
                    .at(link.get("address").map_or(target.span, |e| e.node.span)),
                );
            }
        }
        if let Some(wifi) = &wifi {
            if let Some(ssid) = wifi.get("ssid")
                && ssid
                    .node
                    .as_str()
                    .is_some_and(|s| s.is_empty() || s.len() > 32)
            {
                self.push(
                    Diagnostic::new("J0240", "invalid `ssid`")
                        .at(ssid.node.span)
                        .label("1–32 bytes"),
                );
            }
            if let Some(country) = wifi.get("country")
                && !country
                    .node
                    .as_str()
                    .is_some_and(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()))
            {
                self.push(
                    Diagnostic::new("J0240", "invalid `country`")
                        .at(country.node.span)
                        .label("an ISO 3166-1 alpha-2 code"),
                );
            }
            if let Some(hostname) = wifi.get("hostname")
                && !hostname.node.as_str().is_some_and(grammar::is_dns_name)
            {
                self.push(Diagnostic::new("J0240", "invalid `hostname`").at(hostname.node.span));
            }
            if !wifi.contains_key("password") {
                self.push(
                    Diagnostic::new(
                        "J0242",
                        "open Wi-Fi network: `[net.wifi]` without `password`",
                    )
                    .at(target.span),
                );
            }
        }
    }
}

/// One writer of one output.
struct WriterUse {
    who: String,
    level: crate::model::Level,
    explicit: bool,
    span: Span,
}

impl Checker<'_> {
    fn group_period(&self, group: &str) -> Duration {
        self.manifest
            .cycles
            .get(group)
            .and_then(|c| c.period.as_ref())
            .map_or(Duration::from_millis(100), |p| p.value)
    }

    fn app_outputs(&self, name: &str, app: &crate::model::App) -> Vec<(String, Span)> {
        let interface = self.context.apps.get(name);
        let mut out = Vec::new();
        for (port, binding) in &app.bind {
            let writes = match interface.and_then(|i| i.ports.get(port)) {
                Some(p) => matches!(
                    p.kind,
                    crate::context::PortKind::Output | crate::context::PortKind::OutputMany
                ),
                None => true,
            };
            if !writes {
                continue;
            }
            let targets: Vec<(String, Span)> = match &binding.value {
                crate::model::Binding::Target(t) => vec![(t.value.clone(), t.span)],
                crate::model::Binding::Signal { signal, .. } => {
                    vec![(signal.value.clone(), signal.span)]
                }
                crate::model::Binding::List(items) => {
                    items.iter().map(|i| (i.value.clone(), i.span)).collect()
                }
                crate::model::Binding::Each(_) => Vec::new(),
            };
            for (target, span) in targets {
                for output in self.outputs_of(&target) {
                    out.push((output, span));
                }
            }
        }
        out
    }

    /// The writer rules of ref/01 §5.12 and §3.11, the task periods, and
    /// the cycle group rules that need the apps (`J0130`, `J0210`–`J0213`,
    /// `J0256`, `J0257`, `J0284`, `J0286`, `J0287`).
    pub(crate) fn writers_and_groups(&mut self) {
        let manifest = self.manifest;
        let mut writers: BTreeMap<String, Vec<WriterUse>> = BTreeMap::new();
        for (name, app) in &manifest.apps {
            let outputs = self.app_outputs(name, app);
            let level = app.writer.as_ref().map_or(
                crate::model::Level {
                    band: Band::Control,
                    priority: 12,
                },
                |w| w.level,
            );
            let lease = app
                .writer
                .as_ref()
                .and_then(|w| w.lease.as_ref())
                .map(|l| (l.value, l.span));
            let explicit = app.writer.is_some();
            if let Some(writer) = &app.writer {
                if writer.proposal
                    && let Some((_, span)) = outputs.first()
                {
                    self.push(
                        Diagnostic::new(
                            "J0212",
                            format!("proposal-only writer `{name}` writes directly"),
                        )
                        .at(*span),
                    );
                }
                let has_output_ports = self.context.apps.get(name).map(|i| {
                    i.ports.values().any(|p| {
                        matches!(
                            p.kind,
                            crate::context::PortKind::Output | crate::context::PortKind::OutputMany
                        )
                    })
                });
                if has_output_ports == Some(false) {
                    self.push(
                        Diagnostic::new(
                            "J0257",
                            format!("`writer` on app `{name}`, which has no outputs"),
                        )
                        .at(writer.span),
                    );
                }
            }
            let group = app.cycles.values().next().map(|g| g.value.clone());
            for (output, span) in outputs {
                writers.entry(output.clone()).or_default().push(WriterUse {
                    who: format!("app:{name}"),
                    level,
                    explicit,
                    span,
                });
                if let Some((lease, lease_span)) = lease {
                    let envelope = self
                        .owner
                        .get(&output)
                        .and_then(|k| manifest.outputs.get(k));
                    if let Some(envelope) = envelope
                        && lease.as_nanos() > envelope.max_lease.as_nanos()
                    {
                        self.push(
                            Diagnostic::new(
                                "J0213",
                                format!("lease of `{name}` above the `max_lease` of `{output}`"),
                            )
                            .at(lease_span),
                        );
                    }
                    let owner = envelope
                        .and_then(|e| e.group.as_ref().map(|g| g.value.clone()))
                        .or_else(|| group.clone())
                        .unwrap_or_else(|| "default".to_owned());
                    if lease.as_nanos() < self.group_period(&owner).as_nanos() {
                        self.push(
                            Diagnostic::new(
                                "J0256",
                                format!(
                                    "lease of `{name}` shorter than the period of group `{owner}`"
                                ),
                            )
                            .at(lease_span),
                        );
                    }
                }
            }
            if let Some(interface) = self.context.apps.get(name) {
                for (task, group) in &app.cycles {
                    let Some(Some(requested)) = interface.tasks.get(task) else {
                        continue;
                    };
                    let tolerance = app
                        .period_tolerance
                        .as_ref()
                        .and_then(|p| joker_units::Literal::parse(&p.value).ok())
                        .map_or(50.0, |l| match l {
                            joker_units::Literal::Quantity(q) => q.value,
                            joker_units::Literal::Number(n) => n,
                        });
                    let actual = self.group_period(&group.value).as_nanos() as f64;
                    let wanted = requested.as_nanos() as f64;
                    if wanted > 0.0 && (actual - wanted).abs() > wanted * tolerance / 100.0 {
                        self.push(
                            Diagnostic::new(
                                "J0130",
                                format!(
                                    "group `{}` is outside the period tolerance of `{name}.{task}`",
                                    group.value
                                ),
                            )
                            .at(group.span),
                        );
                    }
                }
            }
        }
        for (output, list) in &writers {
            for (i, w) in list.iter().enumerate() {
                if list[..i]
                    .iter()
                    .any(|o| o.who != w.who && o.level == w.level)
                {
                    self.push(
                        Diagnostic::new(
                            "J0210",
                            format!(
                                "two writers of `{output}` at {}:{}",
                                w.level.band.as_str(),
                                w.level.priority
                            ),
                        )
                        .at(w.span),
                    );
                }
            }
            if list.len() > 1 {
                for w in list.iter().filter(|w| !w.explicit) {
                    self.push(
                        Diagnostic::new(
                            "J0211",
                            format!(
                                "`{output}` has several writers and `{}` declares no `writer`",
                                w.who
                            ),
                        )
                        .at(w.span),
                    );
                }
            }
        }
        let rt: BTreeSet<i64> = self
            .section("resources")
            .and_then(|r| r.get("rt"))
            .and_then(|e| e.node.as_table())
            .and_then(|t| t.get("cpus"))
            .map(|e| match &e.node.value {
                Value::Array(items) => items
                    .iter()
                    .filter_map(|i| match i.value {
                        Value::Integer(n) => Some(n),
                        _ => None,
                    })
                    .collect(),
                _ => BTreeSet::new(),
            })
            .unwrap_or_default();
        for (group, cycle) in &manifest.cycles {
            if let Some(cpu) = &cycle.cpu
                && !rt.contains(&cpu.value)
            {
                self.push(
                    Diagnostic::new(
                        "J0284",
                        format!("`cpu` of group `{group}` is outside `[resources.rt]`"),
                    )
                    .at(cpu.span),
                );
            }
            let tasks: Vec<(String, String)> = manifest
                .apps
                .iter()
                .flat_map(|(a, app)| {
                    app.cycles
                        .iter()
                        .filter(|(_, g)| g.value == *group)
                        .map(move |(t, _)| (a.clone(), t.clone()))
                })
                .collect();
            if cycle.overrun == "degrade"
                && !tasks.iter().any(|(a, t)| {
                    self.context
                        .apps
                        .get(a)
                        .is_some_and(|i| i.optional_tasks.contains(t))
                })
            {
                self.push(
                    Diagnostic::new(
                        "J0286",
                        format!("`overrun = \"degrade\"` in `{group}` without optional tasks"),
                    )
                    .at(cycle.key),
                );
            }
            if cycle.overrun == "catch_up" {
                let timed = manifest.outputs.values().any(|e| {
                    e.group.as_ref().is_some_and(|g| g.value == *group)
                        && (e.rate.is_some()
                            || e.max_actuations.is_some()
                            || e.min_interval.as_nanos() > 0)
                });
                if timed {
                    self.push(Diagnostic::new("J0287", format!("`overrun = \"catch_up\"` in `{group}`, whose outputs depend on time")).at(cycle.key));
                }
            }
        }
    }

    /// `J0188`, `J0189`, `J0243`, `J0276` and `J0298`.
    pub(crate) fn app_and_device_rules(&mut self) {
        let manifest = self.manifest;
        for (name, app) in &manifest.apps {
            if let Some(of) = app
                .shadow
                .as_ref()
                .and_then(Node::as_table)
                .and_then(|s| s.get("of"))
                && let Some(text) = of.node.as_str()
            {
                let ok = text == "none"
                    || text.split_once('@').is_some_and(|(a, v)| {
                        grammar::is_name(a)
                            && v.split('.').count() == 3
                            && v.split('.').all(|p| p.parse::<u32>().is_ok())
                    });
                if !ok {
                    self.push(
                        Diagnostic::new(
                            "J0188",
                            format!(
                                "`shadow.of` of `{name}` is not \"<app>@<version>\" or \"none\""
                            ),
                        )
                        .at(of.node.span),
                    );
                }
            }
        }
        if let Some(instances) = self.section("instances").cloned() {
            for entry in instances.values() {
                let Some(implemented) = entry
                    .node
                    .as_table()
                    .and_then(|d| d.get("implemented_by"))
                    .and_then(|e| e.node.as_table())
                else {
                    continue;
                };
                let (Some(app), Some(bind)) = (
                    text(implemented, "app"),
                    implemented.get("bind").and_then(|e| e.node.as_table()),
                ) else {
                    continue;
                };
                for (port, item) in bind {
                    if manifest
                        .apps
                        .get(app)
                        .is_some_and(|a| a.bind.contains_key(port))
                    {
                        self.push(
                            Diagnostic::new(
                                "J0189",
                                format!("port `{port}` of `{app}` bound in two places"),
                            )
                            .at(item.key),
                        );
                    }
                }
            }
        }
        if let Some(board) = manifest.target.as_ref().and_then(|t| t.board.as_ref())
            && self
                .context
                .boards
                .get(&board.value.name)
                .is_some_and(|b| b.no_wasm)
            && manifest.apps.values().any(|a| a.kind == "wasm")
        {
            self.push(
                Diagnostic::new(
                    "J0243",
                    "the board has no Wasm engine for its architecture: only native apps",
                )
                .at(board.span),
            );
        }
        for device in manifest.devices.values() {
            let Some(timeout) = device
                .profile
                .as_ref()
                .and_then(|p| self.context.profiles.get(&p.value))
                .and_then(|p| p.fallback_timeout)
            else {
                continue;
            };
            let keepalive = device.fallback_keepalive.as_ref();
            let value = keepalive.map_or(timeout.as_nanos() / 5, |k| k.value.as_nanos());
            if value >= timeout.as_nanos() {
                let span = keepalive.map_or(device.key, |k| k.span);
                self.push(
                    Diagnostic::new(
                        "J0276",
                        "`keepalive` not below the timeout of the device fallback",
                    )
                    .at(span)
                    .label(format!("the equipment falls back after {timeout}")),
                );
            }
        }
        let mut actions: Vec<(Span, Option<String>)> = Vec::new();
        let mut permits: Vec<(Span, String)> = Vec::new();
        for device in manifest.devices.values() {
            let profile = device
                .profile
                .as_ref()
                .and_then(|p| self.context.profiles.get(&p.value));
            for (name, action) in &device.actions {
                if let Some(role) = action.as_table().and_then(|t| t.get("role")) {
                    actions.push((role.node.span, role.node.as_str().map(str::to_owned)));
                }
                if let Some(permit) = action.as_table().and_then(|t| t.get("permit"))
                    && let Some(source) = permit.node.as_str()
                {
                    permits.push((permit.node.span, source.to_owned()));
                }
                if let Some(profile) = profile {
                    match profile.actions.get(name) {
                        None => self.push(
                            Diagnostic::new("J0101", format!("unknown action `{name}`"))
                                .at(action.span),
                        ),
                        Some(default_timeout) => {
                            let has_timeout =
                                action.as_table().and_then(|t| t.get("timeout")).is_some();
                            if !has_timeout && default_timeout.is_none() {
                                self.push(
                                    Diagnostic::new(
                                        "J0142",
                                        format!(
                                            "missing required key `timeout` for action `{name}`"
                                        ),
                                    )
                                    .at(action.span),
                                );
                            }
                        }
                    }
                }
            }
        }
        for entry in self
            .section("instances")
            .cloned()
            .unwrap_or_default()
            .values()
        {
            for action in entry
                .node
                .as_table()
                .and_then(|d| d.get("actions"))
                .and_then(|e| e.node.as_table())
                .into_iter()
                .flat_map(|t| t.values())
            {
                if let Some(role) = action.node.as_table().and_then(|t| t.get("role")) {
                    actions.push((role.node.span, role.node.as_str().map(str::to_owned)));
                }
                if let Some(permit) = action.node.as_table().and_then(|t| t.get("permit"))
                    && let Some(source) = permit.node.as_str()
                {
                    permits.push((permit.node.span, source.to_owned()));
                }
            }
        }
        for (span, source) in permits {
            self.expression(&source, span, Expect::Bool);
        }
        for (span, role) in actions {
            if role.as_deref() == Some("viewer") {
                self.push(
                    Diagnostic::new("J0298", "action with `role = \"viewer\"`: it counts as `operator`, the minimum of the API").at(span),
                );
            }
        }
    }
}

fn placeholders(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = message.replace("{{", "").replace("}}", "");
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        out.push(rest[start + 1..start + end].trim().to_owned());
        rest = rest[start + end + 1..].to_owned();
    }
    out
}

/// Whether a principal is valid (`Some`), and whether it grants the role to
/// any certificate (`Some(true)`), ref/01 §5.20.
fn principal_ok(principal: &str) -> Option<bool> {
    if let Some(rest) = principal.strip_prefix("cert:") {
        let (attribute, pattern) = rest.split_once('=')?;
        if !["CN", "OU", "O", "SAN"].contains(&attribute) || pattern.is_empty() {
            return None;
        }
        return Some(pattern == "*");
    }
    if let Some(name) = principal
        .strip_prefix("local:user=")
        .or_else(|| principal.strip_prefix("local:group="))
    {
        return (!name.is_empty()).then_some(false);
    }
    if let Some(rest) = principal.strip_prefix("deck:") {
        let (deck, who) = rest.split_once('/')?;
        let ok = !deck.is_empty()
            && (who.strip_prefix("user:").is_some_and(|u| !u.is_empty())
                || who.strip_prefix("group:").is_some_and(|g| !g.is_empty()));
        return ok.then_some(false);
    }
    None
}

fn render(node: &Node) -> String {
    match &node.value {
        Value::String(s) => format!("{s:?}"),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Datetime(d) => d.clone(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        Value::Table(t) => format!(
            "{{{}}}",
            t.iter()
                .map(|(k, e)| format!("{k}={}", render(&e.node)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// A number of a fallback value or of a `safe`, in `unit` when it is a
/// quantity.
fn safe_number(value: &Value, unit: Option<&joker_units::Unit>) -> Option<f64> {
    match value {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::String(text) => match joker_units::Literal::parse(text).ok()? {
            joker_units::Literal::Number(n) => Some(n),
            joker_units::Literal::Quantity(q) => q.value_in(unit?).ok(),
        },
        _ => None,
    }
}

/// Whether the device fallback `value` is as safe as `safe` (docs/01 §8,
/// `J0205`): equal with `safer = "equal"`, not above it with `"lower"`,
/// not below it with `"higher"`. Two values that cannot be compared are
/// not as safe.
fn fallback_as_safe(
    value: &Value,
    safe: &crate::model::SafeValue,
    safer: &str,
    unit: Option<&joker_units::Unit>,
) -> bool {
    use crate::model::SafeValue;
    let safe = match safe {
        SafeValue::Bool(b) => Value::Bool(*b),
        SafeValue::Integer(i) => Value::Integer(*i),
        SafeValue::Float(f) => Value::Float(*f),
        SafeValue::Text(t) => Value::String(t.clone()),
        SafeValue::Relinquish => return true,
    };
    if let (Value::String(a), Value::String(b)) = (value, &safe)
        && safe_number(value, unit).is_none()
    {
        return safer == "equal" && a == b;
    }
    let (Some(fallback), Some(safe)) = (safe_number(value, unit), safe_number(&safe, unit)) else {
        return false;
    };
    match safer {
        "lower" => fallback <= safe,
        "higher" => fallback >= safe,
        _ => fallback == safe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principals() {
        assert_eq!(principal_ok("cert:CN=ops-*"), Some(false));
        assert_eq!(principal_ok("cert:CN=*"), Some(true));
        assert_eq!(principal_ok("local:group=joker-ops"), Some(false));
        assert_eq!(principal_ok("deck:d1/user:42"), Some(false));
        assert_eq!(principal_ok("cert:XX=a"), None);
        assert_eq!(principal_ok("ops"), None);
    }

    #[test]
    fn message_placeholders() {
        assert_eq!(
            placeholders("CPU at {calc.temp} ({{raw}}) {meter.f}"),
            ["calc.temp", "meter.f"]
        );
    }
}
