//! Decoding the merged tree into the typed model: steps 1–3 of ref/01 §8.1
//! (unknown and reserved keys, types, enums, required keys, combinations and
//! value grammars). Every finding is collected; decoding never stops at the
//! first one.

use std::collections::BTreeMap;

use joker_units::Duration;

use crate::diag::{Diagnostic, Diagnostics};
use crate::grammar;
use crate::model::{
    App, Binding, BoardRef, Bus, BusKind, Cycle, Device, Envelope, Kernel, Manifest, Mode, Project,
    SafeValue, Spanned, Target, Writer,
};
use crate::source::Span;
use crate::value::{Entry, Node, Table, Value};

/// The v1.0 root sections outside the M0 subset, kept untyped.
pub const PENDING_SECTIONS: [&str; 11] = [
    "resources",
    "clock",
    "drivers",
    "signals",
    "instances",
    "calc",
    "alarms",
    "atomic",
    "telemetry",
    "bridges",
    "security",
];

/// Root sections reserved for 1.1 (ref/01 §6).
pub const RESERVED_SECTIONS: [&str; 4] = ["models", "streams", "pipelines", "training"];

/// Root sections that only exist in a Joker Micro manifest (ref/01 §5.22).
pub const MICRO_SECTIONS: [&str; 2] = ["link", "net"];

const MAX_DURATION: Duration = Duration::from_secs(3650 * 86_400);

struct Reader<'t> {
    table: &'t Table,
    known: Vec<&'static str>,
}

impl<'t> Reader<'t> {
    fn new(table: &'t Table) -> Self {
        Self {
            table,
            known: Vec::new(),
        }
    }

    fn get(&mut self, key: &'static str) -> Option<&'t Entry> {
        self.known.push(key);
        self.table.get(key)
    }

    fn unread(&self) -> impl Iterator<Item = (&'t String, &'t Entry)> + '_ {
        self.table
            .iter()
            .filter(|(k, _)| !self.known.contains(&k.as_str()))
    }

    fn finish(self, d: &mut Decoder<'_>, what: &str) -> BTreeMap<String, Node> {
        let mut extensions = BTreeMap::new();
        for (key, entry) in self.unread() {
            if key.starts_with("x_") {
                extensions.insert(key.clone(), entry.node.clone());
            } else {
                d.unknown_key(key, entry, what, &self.known);
            }
        }
        extensions
    }
}

/// Turns the merged tree into a [`Manifest`].
#[derive(Debug)]
pub struct Decoder<'a> {
    diags: &'a mut Diagnostics,
}

impl<'a> Decoder<'a> {
    /// A decoder that records into `diags`.
    pub fn new(diags: &'a mut Diagnostics) -> Self {
        Self { diags }
    }

    fn push(&mut self, diagnostic: Diagnostic) {
        self.diags.push(diagnostic);
    }

    fn unknown_key(&mut self, key: &str, entry: &Entry, what: &str, known: &[&str]) {
        if key.starts_with('_') {
            self.push(
                Diagnostic::new("J0141", format!("reserved key `{key}`"))
                    .at(entry.key)
                    .label("keys starting with `_` are reserved"),
            );
            return;
        }
        let mut d = Diagnostic::new("J0140", format!("unknown key `{key}` in {what}"))
            .at(entry.key)
            .label("unknown key");
        if let Some(suggestion) = grammar::suggest(key, known.iter().copied()) {
            d = d.help(format!("did you mean `{suggestion}`?"));
        }
        self.push(d);
    }

    fn wrong_type(&mut self, entry: &Entry, expected: &str) {
        self.push(
            Diagnostic::new("J0143", "wrong value type")
                .at(entry.node.span)
                .label(format!(
                    "expected {expected}, found {}",
                    entry.node.value.type_name()
                )),
        );
    }

    fn table<'t>(&mut self, entry: &'t Entry) -> Option<&'t Table> {
        let table = entry.node.as_table();
        if table.is_none() {
            self.wrong_type(entry, "a table");
        }
        table
    }

    fn string(&mut self, entry: &Entry) -> Option<Spanned<String>> {
        match &entry.node.value {
            Value::String(s) => Some(Spanned::new(s.clone(), entry.node.span)),
            _ => {
                self.wrong_type(entry, "a string");
                None
            }
        }
    }

    fn boolean(&mut self, entry: &Entry) -> Option<Spanned<bool>> {
        match entry.node.value {
            Value::Bool(b) => Some(Spanned::new(b, entry.node.span)),
            _ => {
                self.wrong_type(entry, "a boolean");
                None
            }
        }
    }

    fn integer(
        &mut self,
        entry: &Entry,
        range: std::ops::RangeInclusive<i64>,
    ) -> Option<Spanned<i64>> {
        match entry.node.value {
            Value::Integer(i) if range.contains(&i) => Some(Spanned::new(i, entry.node.span)),
            Value::Integer(_) => {
                self.push(
                    Diagnostic::new("J0144", "value outside the allowed range")
                        .at(entry.node.span)
                        .label(format!("expected {}–{}", range.start(), range.end())),
                );
                None
            }
            _ => {
                self.wrong_type(entry, "an integer");
                None
            }
        }
    }

    fn choice(&mut self, entry: &Entry, options: &[&str]) -> Option<Spanned<String>> {
        let value = self.string(entry)?;
        if options.contains(&value.value.as_str()) {
            Some(value)
        } else {
            let list = options
                .iter()
                .map(|o| format!("`{o}`"))
                .collect::<Vec<_>>()
                .join(", ");
            self.push(
                Diagnostic::new("J0144", "value outside the allowed set")
                    .at(entry.node.span)
                    .label(format!("expected one of {list}")),
            );
            None
        }
    }

    fn choice_or(&mut self, entry: Option<&Entry>, options: &[&str], default: &str) -> String {
        entry
            .and_then(|e| self.choice(e, options))
            .map_or_else(|| default.to_owned(), |s| s.value)
    }

    fn duration(
        &mut self,
        entry: &Entry,
        allow_zero: bool,
        max: Duration,
    ) -> Option<Spanned<Duration>> {
        let span = entry.node.span;
        let text = match &entry.node.value {
            Value::String(s) => s,
            Value::Integer(_) | Value::Float(_) => {
                self.push(
                    Diagnostic::new(
                        "J0151",
                        "number without a unit where a duration is required",
                    )
                    .at(span)
                    .label("a duration needs a unit")
                    .help("write the duration as a string with a unit, such as \"200ms\""),
                );
                return None;
            }
            _ => {
                self.wrong_type(entry, "a duration string");
                return None;
            }
        };
        match Duration::parse(text) {
            Ok(d) if d.is_zero() && !allow_zero => {
                self.push(
                    Diagnostic::new("J0152", "invalid duration")
                        .at(span)
                        .label("zero is not allowed here"),
                );
                None
            }
            Ok(d) if d.as_nanos() > max.as_nanos() => {
                self.push(
                    Diagnostic::new("J0152", "invalid duration")
                        .at(span)
                        .label(format!("above the maximum of {max}")),
                );
                None
            }
            Ok(d) => Some(Spanned::new(d, span)),
            Err(_) => {
                self.push(
                    Diagnostic::new("J0152", "invalid duration")
                        .at(span)
                        .label("expected a number and one of ns, us, µs, ms, s, m, min, h, d")
                        .help("compound durations are not allowed: write \"90m\", not \"1h30m\""),
                );
                None
            }
        }
    }

    fn quantity_text(&mut self, entry: &Entry) -> Option<Spanned<String>> {
        match &entry.node.value {
            Value::String(s) => Some(Spanned::new(s.clone(), entry.node.span)),
            Value::Integer(i) => Some(Spanned::new(i.to_string(), entry.node.span)),
            Value::Float(f) => Some(Spanned::new(f.to_string(), entry.node.span)),
            _ => {
                self.wrong_type(entry, "a quantity");
                None
            }
        }
    }

    fn name_key(&mut self, key: &str, entry: &Entry, what: &str) -> bool {
        if grammar::is_name(key) {
            return true;
        }
        self.push(
            Diagnostic::new("J0150", format!("invalid {what} name `{key}`"))
                .at(entry.key)
                .label("names follow `[a-z][a-z0-9_]*`, up to 32 characters"),
        );
        false
    }

    fn reserved(&mut self, entry: &Entry, what: &str) {
        self.push(
            Diagnostic::new("J0290", format!("{what} is reserved for a later version"))
                .at(entry.key)
                .label("reserved for 1.1"),
        );
    }

    fn micro_only(&mut self, entry: &Entry, what: &str) {
        self.push(
            Diagnostic::new(
                "J0145",
                format!("{what} only applies in a Joker Micro manifest"),
            )
            .at(entry.key)
            .label("not valid on a Linux node"),
        );
    }

    fn string_list(&mut self, entry: &Entry) -> Vec<Spanned<String>> {
        match &entry.node.value {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match &item.value {
                    Value::String(s) => Some(Spanned::new(s.clone(), item.span)),
                    other => {
                        self.push(
                            Diagnostic::new("J0143", "wrong value type")
                                .at(item.span)
                                .label(format!("expected a string, found {}", other.type_name())),
                        );
                        None
                    }
                })
                .collect(),
            _ => {
                self.wrong_type(entry, "a list of strings");
                Vec::new()
            }
        }
    }

    /// Decodes the merged tree.
    pub fn manifest(&mut self, root: &Table, root_span: Span) -> Manifest {
        let mut reader = Reader::new(root);
        let mut manifest = Manifest {
            project: match reader.get("project") {
                Some(entry) => self.project(entry),
                None => {
                    self.push(
                        Diagnostic::new("J0142", "missing required section `[project]`")
                            .at(Span::new(root_span.file, 0, 0)),
                    );
                    Project::default()
                }
            },
            ..Manifest::default()
        };
        manifest.target = reader.get("target").and_then(|e| self.target(e));
        if let Some(entry) = reader.get("buses")
            && let Some(table) = self.table(entry)
        {
            for (name, entry) in table {
                if self.name_key(name, entry, "bus")
                    && let Some(bus) = self.bus(entry)
                {
                    manifest.buses.insert(name.clone(), bus);
                }
            }
        }
        if let Some(entry) = reader.get("devices")
            && let Some(table) = self.table(entry)
        {
            for (name, entry) in table {
                if !self.name_key(name, entry, "device") {
                    continue;
                }
                if name == "joker" || name == "board" {
                    self.push(
                        Diagnostic::new("J0141", format!("reserved device name `{name}`"))
                            .at(entry.key),
                    );
                    continue;
                }
                if let Some(device) = self.device(entry) {
                    manifest.devices.insert(name.clone(), device);
                }
            }
        }
        if let Some(entry) = reader.get("outputs")
            && let Some(table) = self.table(entry)
        {
            for (key, entry) in table {
                if let Some(envelope) = self.envelope(key, entry) {
                    manifest.outputs.insert(key.clone(), envelope);
                }
            }
        }
        if let Some(entry) = reader.get("cycles")
            && let Some(table) = self.table(entry)
        {
            for (name, entry) in table {
                if !self.name_key(name, entry, "cycle group") {
                    continue;
                }
                if name.starts_with("default_") {
                    self.push(
                        Diagnostic::new("J0288", format!("reserved group name `{name}`"))
                            .at(entry.key)
                            .label("`default_<period>` groups are implicit and cannot be declared"),
                    );
                    continue;
                }
                if let Some(cycle) = self.cycle(entry) {
                    manifest.cycles.insert(name.clone(), cycle);
                }
            }
        }
        if let Some(entry) = reader.get("apps")
            && let Some(table) = self.table(entry)
        {
            for (name, entry) in table {
                if !self.name_key(name, entry, "app") {
                    continue;
                }
                if name.starts_with("joker") {
                    self.push(
                        Diagnostic::new("J0141", format!("reserved app name `{name}`"))
                            .at(entry.key)
                            .label("app names cannot start with `joker`"),
                    );
                    continue;
                }
                if let Some(app) = self.app(entry) {
                    manifest.apps.insert(name.clone(), app);
                }
            }
        }
        for section in PENDING_SECTIONS {
            if let Some(entry) = reader.get(section) {
                manifest
                    .pending
                    .insert(section.to_owned(), entry.node.clone());
            }
        }
        for section in RESERVED_SECTIONS {
            if let Some(entry) = reader.get(section) {
                self.reserved(entry, &format!("`[{section}]`"));
            }
        }
        for section in MICRO_SECTIONS {
            if let Some(entry) = reader.get(section) {
                self.micro_only(entry, &format!("`[{section}]`"));
            }
        }
        manifest.extensions = reader.finish(self, "the manifest root");
        manifest
    }

    fn project(&mut self, entry: &Entry) -> Project {
        let Some(table) = self.table(entry) else {
            return Project::default();
        };
        let mut r = Reader::new(table);
        let name = r.get("name").and_then(|e| {
            let name = self.string(e)?;
            if grammar::is_dns_name(&name.value) {
                Some(name)
            } else {
                self.push(
                    Diagnostic::new("J0150", "invalid project name")
                        .at(name.span)
                        .label("a lowercase DNS label of up to 63 characters"),
                );
                None
            }
        });
        if r.table.get("name").is_none() {
            self.push(
                Diagnostic::new("J0142", "missing required key `[project] name`").at(entry.key),
            );
        }
        let joker = r.get("joker").and_then(|e| {
            let text = self.string(e)?;
            match grammar::schema_version(&text.value) {
                Some(version) => Some(Spanned::new(version, text.span)),
                None => {
                    self.push(
                        Diagnostic::new("J0147", "invalid `[project] joker` version")
                            .at(text.span)
                            .label("expected \"MAJOR.MINOR\""),
                    );
                    None
                }
            }
        });
        let mode = match r.get("mode").and_then(|e| self.choice(e, &["dev", "prod"])) {
            Some(m) if m.value == "dev" => Mode::Dev,
            _ => Mode::Prod,
        };
        r.finish(self, "`[project]`");
        Project { name, joker, mode }
    }

    fn target(&mut self, entry: &Entry) -> Option<Target> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let board = r.get("board").and_then(|e| {
            let text = self.string(e)?;
            let (name, major) = match text.value.split_once('@') {
                Some((name, major)) => (name, Some(major)),
                None => (text.value.as_str(), None),
            };
            let major = match major.map(str::parse::<u32>) {
                Some(Ok(m)) => Some(m),
                Some(Err(_)) => {
                    self.push(
                        Diagnostic::new("J0172", "board not found")
                            .at(text.span)
                            .label("expected \"<board>\" or \"<board>@<major>\""),
                    );
                    return None;
                }
                None => None,
            };
            Some(Spanned::new(
                BoardRef {
                    name: name.to_owned(),
                    major,
                },
                text.span,
            ))
        });
        let arch = r.get("arch").and_then(|e| self.string(e));
        let kernel = match r
            .get("kernel")
            .and_then(|e| self.choice(e, &["preempt_rt", "standard"]))
        {
            Some(k) if k.value == "preempt_rt" => Kernel::PreemptRt,
            _ => Kernel::Standard,
        };
        for key in ["capacity", "extern", "micro"] {
            if let Some(e) = r.get(key) {
                self.micro_only(e, &format!("`[target.{key}]`"));
            }
        }
        r.finish(self, "`[target]`");
        Some(Target {
            board,
            arch,
            kernel,
            span: entry.node.span,
        })
    }

    fn bus(&mut self, entry: &Entry) -> Option<Bus> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let kind_entry = r.get("kind");
        let kinds: Vec<&str> = BusKind::ALL.iter().map(|k| k.as_str()).collect();
        let kind = match kind_entry {
            Some(e) => {
                let text = self.choice(e, &kinds)?;
                let kind = BusKind::ALL
                    .into_iter()
                    .find(|k| k.as_str() == text.value)?;
                Spanned::new(kind, text.span)
            }
            None => {
                self.push(Diagnostic::new("J0142", "missing required key `kind`").at(entry.key));
                return None;
            }
        };
        let keys: &[&str] = match kind.value {
            BusKind::Ethernet => &["iface"],
            BusKind::Serial => &[
                "port",
                "baud",
                "parity",
                "stop",
                "data_bits",
                "turnaround",
                "dtr",
                "rts",
                "duplex",
            ],
            BusKind::Can => &["iface", "bitrate", "fd", "data_bitrate"],
            BusKind::Ethercat => &["iface", "cycle", "dc"],
            BusKind::I2c => &["dev"],
            BusKind::Spi => &["dev", "mode", "speed"],
        };
        let required: &[&str] = match kind.value {
            BusKind::Ethernet => &["iface"],
            BusKind::Serial => &["port", "baud"],
            BusKind::Can => &["iface", "bitrate"],
            BusKind::Ethercat => &["iface", "cycle"],
            BusKind::I2c | BusKind::Spi => &["dev"],
        };
        let mut bus = Bus {
            kind,
            iface: None,
            port: None,
            dev: None,
            baud: None,
            parity: "none".to_owned(),
            stop: 1,
            data_bits: 8,
            turnaround: None,
            dtr: "low".to_owned(),
            rts: "low".to_owned(),
            duplex: "full".to_owned(),
            bitrate: None,
            fd: false,
            data_bitrate: None,
            cycle: None,
            dc: false,
            spi_mode: 0,
            speed: None,
            span: entry.node.span,
        };
        const ALL_KEYS: [&str; 17] = [
            "iface",
            "port",
            "dev",
            "baud",
            "parity",
            "stop",
            "data_bits",
            "turnaround",
            "dtr",
            "rts",
            "duplex",
            "bitrate",
            "fd",
            "data_bitrate",
            "cycle",
            "dc",
            "mode",
        ];
        for key in ALL_KEYS.into_iter().chain(["speed"]) {
            let Some(e) = r.get(key) else {
                if required.contains(&key) {
                    self.push(
                        Diagnostic::new(
                            "J0142",
                            format!(
                                "missing required key `{key}` for a `{}` bus",
                                kind.value.as_str()
                            ),
                        )
                        .at(entry.key),
                    );
                }
                continue;
            };
            if !keys.contains(&key) {
                self.push(
                    Diagnostic::new(
                        "J0145",
                        format!("`{key}` does not apply to a `{}` bus", kind.value.as_str()),
                    )
                    .at(e.key),
                );
                continue;
            }
            match key {
                "iface" => bus.iface = self.string(e),
                "port" => {
                    bus.port = self.string(e);
                    if let Some(port) = &bus.port
                        && !port.value.starts_with('/')
                    {
                        self.push(
                            Diagnostic::new("J0145", "a serial port on Linux is an absolute path")
                                .at(port.span)
                                .label("expected a path such as `/dev/serial/by-id/…`"),
                        );
                    }
                }
                "dev" => bus.dev = self.string(e),
                "baud" => bus.baud = self.integer(e, 300..=4_000_000),
                "parity" => bus.parity = self.choice_or(Some(e), &["none", "even", "odd"], "none"),
                "stop" => bus.stop = self.integer(e, 1..=2).map_or(1, |v| v.value),
                "data_bits" => bus.data_bits = self.integer(e, 7..=8).map_or(8, |v| v.value),
                "turnaround" => bus.turnaround = self.duration(e, false, MAX_DURATION),
                "dtr" => bus.dtr = self.choice_or(Some(e), &["keep", "low", "high"], "low"),
                "rts" => bus.rts = self.choice_or(Some(e), &["keep", "low", "high"], "low"),
                "duplex" => bus.duplex = self.choice_or(Some(e), &["full", "half"], "full"),
                "bitrate" => bus.bitrate = self.integer(e, 1..=i64::from(u32::MAX)),
                "fd" => bus.fd = self.boolean(e).is_some_and(|b| b.value),
                "data_bitrate" => bus.data_bitrate = self.integer(e, 1..=i64::from(u32::MAX)),
                "cycle" => {
                    bus.cycle = self.duration(e, false, MAX_DURATION).filter(|c| {
                        let ok = c.value.as_nanos() >= 100_000;
                        if !ok {
                            self.push(
                                Diagnostic::new("J0152", "invalid duration")
                                    .at(c.span)
                                    .label("an EtherCAT cycle is at least 100µs"),
                            );
                        }
                        ok
                    });
                }
                "dc" => bus.dc = self.boolean(e).is_some_and(|b| b.value),
                "mode" => bus.spi_mode = self.integer(e, 0..=3).map_or(0, |v| v.value),
                "speed" => bus.speed = self.quantity_text(e),
                _ => {}
            }
        }
        if let Some(data_bitrate) = &bus.data_bitrate
            && !bus.fd
        {
            self.push(
                Diagnostic::new("J0145", "`data_bitrate` needs `fd = true`").at(data_bitrate.span),
            );
        }
        r.finish(self, &format!("a `{}` bus", kind.value.as_str()));
        Some(bus)
    }

    fn device(&mut self, entry: &Entry) -> Option<Device> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let driver = r.get("driver").and_then(|e| self.string(e));
        if r.table.get("driver").is_none() {
            self.push(Diagnostic::new("J0142", "missing required key `driver`").at(entry.key));
        }
        if let Some(d) = &driver {
            if d.value == "extern" {
                self.push(
                    Diagnostic::new(
                        "J0145",
                        "`driver = \"extern\"` only applies in a Joker Micro manifest",
                    )
                    .at(d.span),
                );
            } else {
                let name = d
                    .value
                    .strip_prefix("w:")
                    .or_else(|| d.value.strip_prefix("x:"))
                    .unwrap_or(&d.value);
                if !grammar::is_driver_name(name) {
                    self.push(
                        Diagnostic::new("J0150", "invalid driver name")
                            .at(d.span)
                            .label("driver names follow `[a-z][a-z0-9-]{0,31}`"),
                    );
                }
            }
        }
        let bus = r.get("bus").and_then(|e| self.string(e));
        let profile = r.get("profile").and_then(|e| {
            let text = self.string(e)?;
            match grammar::profile_ref(&text.value) {
                Some(full) => Some(Spanned::new(full, text.span)),
                None => {
                    self.push(
                        Diagnostic::new("J0172", "profile without a major")
                            .at(text.span)
                            .label("expected \"<vendor>/<model>@<major>\""),
                    );
                    None
                }
            }
        });
        let poll = r
            .get("poll")
            .and_then(|e| self.duration(e, false, MAX_DURATION));
        let timeout = r
            .get("timeout")
            .and_then(|e| self.duration(e, false, MAX_DURATION));
        let retries = r
            .get("retries")
            .and_then(|e| self.integer(e, 0..=10))
            .map_or(1, |v| v.value);
        let align = r.get("align").and_then(|e| self.string(e));
        let discover = r.get("discover").and_then(|e| self.boolean(e));
        let mut fallback_configure = None;
        let mut fallback_keepalive = None;
        if let Some(e) = r.get("fallback")
            && let Some(t) = self.table(e)
        {
            let mut f = Reader::new(t);
            fallback_configure = f.get("configure").and_then(|e| self.boolean(e));
            fallback_keepalive = f
                .get("keepalive")
                .and_then(|e| self.duration(e, false, MAX_DURATION));
            f.finish(self, "`fallback`");
        }
        let mut signals = BTreeMap::new();
        if let Some(e) = r.get("signals")
            && let Some(t) = self.table(e)
        {
            for (name, entry) in t {
                if !self.name_key(name, entry, "signal") {
                    continue;
                }
                if let Some(table) = self.table(entry) {
                    let keys = table
                        .iter()
                        .map(|(k, v)| (k.clone(), v.node.clone()))
                        .collect();
                    signals.insert(name.clone(), Spanned::new(keys, entry.key));
                }
            }
        }
        let actions = r
            .get("actions")
            .and_then(|e| self.table(e))
            .map(|t| t.iter().map(|(k, v)| (k.clone(), v.node.clone())).collect())
            .unwrap_or_default();
        let config = r
            .unread()
            .map(|(k, v)| (k.clone(), v.node.clone()))
            .collect();
        Some(Device {
            driver,
            bus,
            profile,
            poll,
            timeout,
            retries,
            align,
            discover,
            fallback_configure,
            fallback_keepalive,
            signals,
            actions,
            config,
            span: entry.node.span,
            key: entry.key,
        })
    }

    fn envelope(&mut self, key: &str, entry: &Entry) -> Option<Envelope> {
        if !grammar::is_path(key) {
            if !grammar::is_selector(key) {
                self.push(
                    Diagnostic::new("J0154", format!("invalid output path or selector `{key}`"))
                        .at(entry.key),
                );
                return None;
            }
            if !grammar::is_wildcard(key) {
                self.push(
                    Diagnostic::new("J0153", format!("invalid output path `{key}`")).at(entry.key),
                );
                return None;
            }
        }
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let safe = r.get("safe").and_then(|e| {
            let value = match &e.node.value {
                Value::Bool(b) => SafeValue::Bool(*b),
                Value::Integer(i) => SafeValue::Integer(*i),
                Value::Float(f) => SafeValue::Float(*f),
                Value::String(s) if s == "relinquish" => SafeValue::Relinquish,
                Value::String(s) => SafeValue::Text(s.clone()),
                _ => {
                    self.push(
                        Diagnostic::new("J0232", "`safe` must be a constant")
                            .at(e.node.span)
                            .label(format!("found {}", e.node.value.type_name())),
                    );
                    return None;
                }
            };
            Some(Spanned::new(value, e.node.span))
        });
        let max_day = Duration::from_secs(86_400);
        let mut envelope = Envelope {
            safe,
            safer: {
                let e = r.get("safer");
                self.choice_or(e, &["lower", "higher", "equal"], "equal")
            },
            safe_ramp: r.get("safe_ramp").and_then(|e| self.quantity_text(e)),
            interlock: r.get("interlock").and_then(|e| self.string(e)),
            permit_uncertain: r
                .get("permit_uncertain")
                .and_then(|e| self.boolean(e))
                .is_some_and(|b| b.value),
            on_interlock: {
                let e = r.get("on_interlock");
                self.choice_or(e, &["safe", "hold"], "safe")
            },
            requires_good: r
                .get("requires_good")
                .map(|e| self.string_list(e))
                .unwrap_or_default(),
            grace: r
                .get("grace")
                .and_then(|e| self.duration(e, true, MAX_DURATION))
                .map_or(Duration::ZERO, |d| d.value),
            range: None,
            on_range: {
                let e = r.get("on_range");
                self.choice_or(e, &["clamp", "reject", "trip"], "clamp")
            },
            rate: r.get("rate").and_then(|e| self.quantity_text(e)),
            deadband: r.get("deadband").and_then(|e| self.quantity_text(e)),
            min_interval: r
                .get("min_interval")
                .and_then(|e| self.duration(e, true, MAX_DURATION))
                .map_or(Duration::ZERO, |d| d.value),
            max_actuations: r.get("max_actuations").and_then(|e| {
                let text = self.string(e)?;
                match grammar::budget(&text.value) {
                    Some(b) => Some(Spanned::new(b, text.span)),
                    None => {
                        self.push(
                            Diagnostic::new("J0159", "invalid actuation budget")
                                .at(text.span)
                                .label("expected \"<count>/<period>\", period between 1s and 7d"),
                        );
                        None
                    }
                }
            }),
            max_lease: self.limited(
                r.get("max_lease"),
                Duration::from_secs(15 * 60),
                max_day,
                "24h",
            ),
            max_manual: self.limited(
                r.get("max_manual"),
                Duration::from_secs(8 * 3600),
                Duration::from_secs(7 * 86_400),
                "7d",
            ),
            verify: r.get("verify").and_then(|e| self.verify(e)),
            reset_when: r.get("reset_when").and_then(|e| self.string(e)),
            group: r.get("group").and_then(|e| self.string(e)),
            keepalive: r
                .get("keepalive")
                .and_then(|e| self.duration(e, false, MAX_DURATION)),
            pulse: r
                .get("pulse")
                .and_then(|e| self.duration(e, false, MAX_DURATION)),
            accept_missing_fallback: r.get("accept_missing_fallback").and_then(|e| {
                let text = self.string(e)?;
                self.justification(&text);
                Some(text)
            }),
            key: entry.key,
            span: entry.node.span,
        };
        if let Some(e) = r.get("range") {
            envelope.range = match &e.node.value {
                Value::Array(items) if items.len() == 2 => {
                    let bounds: Vec<Spanned<String>> = items
                        .iter()
                        .filter_map(|item| {
                            self.quantity_text(&Entry {
                                key: e.key,
                                node: item.clone(),
                            })
                        })
                        .collect();
                    (bounds.len() == 2).then(|| Spanned::new(bounds, e.node.span))
                }
                Value::Array(_) => {
                    self.push(
                        Diagnostic::new("J0230", "`range` must have two elements").at(e.node.span),
                    );
                    None
                }
                _ => {
                    self.wrong_type(e, "a list of two bounds");
                    None
                }
            };
        }
        if let Some(e) = r.get("widen") {
            match &e.node.value {
                Value::String(s) => self.justification(&Spanned::new(s.clone(), e.node.span)),
                Value::Table(t) => {
                    for item in t.values() {
                        if let Some(s) = self.string(item) {
                            self.justification(&s);
                        }
                    }
                }
                _ => self.wrong_type(e, "a justification or a table of justifications"),
            }
        }
        if let Some(e) = r.get("remote_levels") {
            self.micro_only(e, "`remote_levels`");
        }
        for key in ["schedule_depth", "timed", "on_late", "late_margin"] {
            if let Some(e) = r.get(key) {
                self.reserved(e, &format!("`{key}`"));
            }
        }
        r.finish(self, &format!("`[outputs.\"{key}\"]`"));
        Some(envelope)
    }

    fn limited(
        &mut self,
        entry: Option<&Entry>,
        default: Duration,
        max: Duration,
        max_text: &str,
    ) -> Duration {
        let Some(e) = entry else {
            return default;
        };
        match self.duration(e, false, MAX_DURATION) {
            Some(d) if d.value.as_nanos() > max.as_nanos() => {
                self.push(
                    Diagnostic::new("J0255", "lease limit above its maximum")
                        .at(d.span)
                        .label(format!("at most {max_text}")),
                );
                default
            }
            Some(d) => d.value,
            None => default,
        }
    }

    fn justification(&mut self, text: &Spanned<String>) {
        if text.value.chars().count() < 10 {
            self.push(
                Diagnostic::new("J0166", "justification too short")
                    .at(text.span)
                    .label("at least 10 characters"),
            );
        }
    }

    fn verify(&mut self, entry: &Entry) -> Option<Node> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        if r.get("readback").and_then(|e| self.string(e)).is_none()
            && !table.contains_key("readback")
        {
            self.push(Diagnostic::new("J0235", "`verify` without `readback`").at(entry.key));
        }
        r.get("tolerance").and_then(|e| self.quantity_text(e));
        r.get("within")
            .and_then(|e| self.duration(e, false, MAX_DURATION));
        r.finish(self, "`verify`");
        Some(entry.node.clone())
    }

    fn cycle(&mut self, entry: &Entry) -> Option<Cycle> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let period = r
            .get("period")
            .and_then(|e| self.duration(e, false, MAX_DURATION));
        if !table.contains_key("period") {
            self.push(Diagnostic::new("J0142", "missing required key `period`").at(entry.key));
        }
        let resources = r.get("resources").and_then(|e| self.string(e));
        let overrun = {
            let e = r.get("overrun");
            self.choice_or(e, &["skip", "catch_up", "degrade", "trip"], "skip")
        };
        let trip_after = r
            .get("trip_after")
            .and_then(|e| self.integer(e, 1..=i64::from(u32::MAX)))
            .map(|v| v.value)
            .or((overrun == "trip").then_some(1));
        let alarm_after = r
            .get("alarm_after")
            .and_then(|e| self.integer(e, 1..=i64::from(u32::MAX)))
            .map_or(10, |v| v.value);
        let recover_after = r
            .get("recover_after")
            .and_then(|e| self.integer(e, 1..=i64::from(u32::MAX)))
            .map_or(10, |v| v.value);
        let order = r
            .get("order")
            .map(|e| self.string_list(e))
            .unwrap_or_default();
        for (i, item) in order.iter().enumerate() {
            if order[..i].iter().any(|o| o.value == item.value) {
                self.push(
                    Diagnostic::new("J0164", "repeated element in an ordered list")
                        .at(item.span)
                        .label("already in `order`"),
                );
            }
        }
        let mut trace_continuous = None;
        let mut trace_async = false;
        if let Some(e) = r.get("trace")
            && let Some(t) = self.table(e)
        {
            let mut tr = Reader::new(t);
            trace_continuous = tr.get("continuous").and_then(|e| {
                let d = self.duration(e, false, MAX_DURATION)?;
                if d.value.as_nanos() > Duration::from_secs(7 * 86_400).as_nanos() {
                    self.push(Diagnostic::new("J0285", "`trace.continuous` above 7d").at(d.span));
                    return None;
                }
                Some(d)
            });
            trace_async = tr
                .get("async")
                .and_then(|e| self.boolean(e))
                .is_some_and(|b| b.value);
            tr.finish(self, "`trace`");
        }
        let cpu = r.get("cpu").and_then(|e| self.integer(e, 0..=4095));
        let priority = r.get("priority").and_then(|e| self.integer(e, 1..=98));
        r.finish(self, "a cycle group");
        Some(Cycle {
            period,
            resources,
            overrun,
            trip_after,
            alarm_after,
            recover_after,
            order,
            trace_continuous,
            trace_async,
            cpu,
            priority,
            key: entry.key,
            span: entry.node.span,
        })
    }

    fn app(&mut self, entry: &Entry) -> Option<App> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let source = r.get("source").and_then(|e| {
            let s = self.string(e)?;
            if grammar::oci_ref(&s.value) {
                Some(s)
            } else {
                self.push(
                    Diagnostic::new("J0157", "invalid reference")
                        .at(s.span)
                        .label("expected \"oci://<registry>/<repo>:<tag>\" or \"…@sha256:<hex>\""),
                );
                None
            }
        });
        let path = r.get("path").and_then(|e| self.string(e));
        let digest = r.get("digest").and_then(|e| {
            let s = self.string(e)?;
            if grammar::digest(&s.value) {
                Some(s)
            } else {
                self.push(
                    Diagnostic::new("J0157", "invalid reference")
                        .at(s.span)
                        .label("expected \"sha256:<64 hex>\""),
                );
                None
            }
        });
        let kind = {
            let e = r.get("kind");
            self.choice_or(e, &["wasm", "native", "extern"], "wasm")
        };
        if kind == "extern"
            && let Some(e) = table.get("kind")
        {
            self.micro_only(e, "`kind = \"extern\"`");
        }
        let has_source = table.contains_key("source");
        let has_path = table.contains_key("path");
        match (kind.as_str(), has_source, has_path) {
            ("native", false, false) | ("wasm", true, false) | ("wasm", false, true) => {}
            ("native", _, _) => self.push(
                Diagnostic::new("J0187", "a `native` app has no `source` or `path`").at(entry.key),
            ),
            ("wasm", true, true) => self.push(
                Diagnostic::new("J0180", "app with both `source` and `path`")
                    .at(entry.key)
                    .help("keep exactly one of `source` and `path`"),
            ),
            ("wasm", false, false) => {
                self.push(Diagnostic::new("J0180", "app without `source` or `path`").at(entry.key))
            }
            _ => {}
        }
        let writer = r.get("writer").and_then(|e| self.writer(e));
        let mut bind = BTreeMap::new();
        if let Some(e) = r.get("bind")
            && let Some(t) = self.table(e)
        {
            for (port, entry) in t {
                if let Some(binding) = self.binding(entry) {
                    bind.insert(port.clone(), Spanned::new(binding, entry.node.span));
                }
            }
        }
        let params = r
            .get("params")
            .and_then(|e| self.table(e))
            .map(|t| {
                t.iter()
                    .map(|(k, v)| (k.clone(), Spanned::new(v.node.clone(), v.key)))
                    .collect()
            })
            .unwrap_or_default();
        let mut cycles = BTreeMap::new();
        if let Some(e) = r.get("cycles")
            && let Some(t) = self.table(e)
        {
            for (task, entry) in t {
                if let Some(group) = self.string(entry) {
                    cycles.insert(task.clone(), group);
                }
            }
        }
        let period_tolerance = r.get("period_tolerance").and_then(|e| self.string(e));
        let resources = r.get("resources").and_then(|e| self.app_resources(e));
        let mut net_allow = Vec::new();
        if let Some(e) = r.get("net")
            && let Some(t) = self.table(e)
        {
            let mut n = Reader::new(t);
            if let Some(allow) = n.get("allow") {
                net_allow = self.string_list(allow);
                for host in &net_allow {
                    if !grammar::host_port(&host.value) {
                        self.push(
                            Diagnostic::new("J0184", "invalid `net.allow` entry")
                                .at(host.span)
                                .label("expected \"<host>:<port>\" without wildcards"),
                        );
                    }
                }
            }
            n.finish(self, "`net`");
        }
        let mut secrets = BTreeMap::new();
        if let Some(e) = r.get("secrets")
            && let Some(t) = self.table(e)
        {
            for (name, entry) in t {
                if let Some(value) = self.string(entry) {
                    if value
                        .value
                        .strip_prefix("secret:")
                        .is_some_and(grammar::is_name)
                    {
                        secrets.insert(name.clone(), value);
                    } else {
                        self.push(
                            Diagnostic::new("J0294", "plain secret in a secret key")
                                .at(value.span)
                                .label("expected \"secret:<name>\"")
                                .help("store the value in the node vault and refer to it by name"),
                        );
                    }
                }
            }
        }
        let requires_clock = {
            let e = r.get("requires_clock");
            self.choice_or(e, &["any", "holdover", "synced"], "any")
        };
        let keep_previous = r
            .get("keep_previous")
            .and_then(|e| self.duration(e, true, MAX_DURATION))
            .map_or(Duration::from_secs(600), |d| d.value);
        let crashloop = r.get("crashloop").and_then(|e| {
            let t = self.table(e)?;
            let mut c = Reader::new(t);
            c.get("traps")
                .and_then(|e| self.integer(e, 1..=i64::from(u32::MAX)));
            c.get("window")
                .and_then(|e| self.duration(e, false, MAX_DURATION));
            c.get("max_backoff")
                .and_then(|e| self.duration(e, false, MAX_DURATION));
            c.finish(self, "`crashloop`");
            Some(e.node.clone())
        });
        let shadow = r.get("shadow").map(|e| e.node.clone());
        for key in ["writes", "reads"] {
            if let Some(e) = r.get(key) {
                self.micro_only(e, &format!("`{key}`"));
            }
        }
        if let Some(e) = r.get("fallback") {
            self.reserved(e, "`[apps.*.fallback]`");
        }
        r.finish(self, "an app");
        Some(App {
            source,
            path,
            digest,
            kind,
            writer,
            bind,
            params,
            cycles,
            period_tolerance,
            resources,
            net_allow,
            secrets,
            requires_clock,
            keep_previous,
            crashloop,
            shadow,
            key: entry.key,
            span: entry.node.span,
        })
    }

    fn app_resources(&mut self, entry: &Entry) -> Option<Node> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        r.get("cycle").and_then(|e| self.choice(e, &["rt"]));
        if let Some(e) = r.get("async")
            && let Some(class) = self.string(e)
            && class.value == "rt"
        {
            self.push(
                Diagnostic::new("J0186", "async tasks cannot use the `rt` class").at(class.span),
            );
        }
        for key in ["memory", "cycle_memory", "async_memory"] {
            if let Some(e) = r.get(key)
                && let Some(text) = self.string(e)
                && grammar::size(&text.value).is_none()
            {
                self.push(
                    Diagnostic::new("J0159", "invalid size")
                        .at(text.span)
                        .label("expected a size with an IEC prefix, such as \"32 MiB\""),
                );
            }
        }
        r.finish(self, "`resources`");
        Some(entry.node.clone())
    }

    fn writer(&mut self, entry: &Entry) -> Option<Writer> {
        let table = self.table(entry)?;
        let mut r = Reader::new(table);
        let level_text = r.get("level").and_then(|e| self.string(e));
        let priority = r.get("priority").and_then(|e| self.integer(e, 1..=16));
        let lease = r
            .get("lease")
            .and_then(|e| self.duration(e, false, MAX_DURATION));
        if let Some(e) = r.get("kind")
            && let Some(kind) = self.choice(e, &["direct", "proposal"])
            && kind.value == "proposal"
        {
            self.reserved(e, "a `proposal` writer");
        }
        r.finish(self, "`writer`");
        let text = level_text.as_ref().map_or("control", |l| l.value.as_str());
        let span = level_text.as_ref().map_or(entry.node.span, |l| l.span);
        let mut level = match grammar::level(text) {
            Ok(level) => level,
            Err(grammar::LevelError::Band) => {
                self.push(
                    Diagnostic::new("J0250", "invalid level")
                        .at(span)
                        .label("expected a band (`safety`, `supervisor`, `control`, `optimizer`, `schedule`) or `<band>:<priority>`"),
                );
                return None;
            }
            Err(grammar::LevelError::Priority(band)) => {
                self.push(self.priority_out_of_band(band, span));
                return None;
            }
        };
        if let Some(priority) = priority {
            let compact = text.contains(':');
            let p = u8::try_from(priority.value).unwrap_or(0);
            if compact && p != level.priority {
                self.push(
                    Diagnostic::new("J0250", "compact level disagrees with `priority`")
                        .at(priority.span)
                        .label(format!("the level already says {}", level.priority)),
                );
            } else if !level.band.priorities().contains(&p) {
                self.push(self.priority_out_of_band(level.band, priority.span));
            } else {
                level.priority = p;
            }
        }
        if level.band == crate::model::Band::Manual {
            self.push(
                Diagnostic::new("J0252", "an app cannot write in the `manual` band")
                    .at(span)
                    .label("the manual band belongs to users and approved intents"),
            );
        }
        Some(Writer {
            level,
            lease,
            span: entry.node.span,
        })
    }

    fn priority_out_of_band(&self, band: crate::model::Band, span: Span) -> Diagnostic {
        let range = band.priorities();
        Diagnostic::new("J0251", "`priority` outside the band")
            .at(span)
            .label(format!(
                "`{}` takes priorities {}–{}",
                band.as_str(),
                range.start(),
                range.end()
            ))
    }

    fn binding(&mut self, entry: &Entry) -> Option<Binding> {
        match &entry.node.value {
            Value::String(s) => Some(Binding::Target(Spanned::new(s.clone(), entry.node.span))),
            Value::Array(_) => Some(Binding::List(self.string_list(entry))),
            Value::Table(t) if t.contains_key("each") => {
                let mut r = Reader::new(t);
                r.get("each").and_then(|e| self.string(e));
                r.get("map").and_then(|e| self.table(e));
                r.finish(self, "a `{ each, map }` binding");
                Some(Binding::Each(entry.node.clone()))
            }
            Value::Table(t) => {
                let mut r = Reader::new(t);
                let signal = r.get("signal").and_then(|e| self.string(e));
                let conversion = r
                    .get("as")
                    .and_then(|e| self.choice(e, &["fraction", "percent"]));
                let optional = r
                    .get("optional")
                    .and_then(|e| self.boolean(e))
                    .is_some_and(|b| b.value);
                r.finish(self, "a `{ signal, as, optional }` binding");
                match signal {
                    Some(signal) => Some(Binding::Signal {
                        signal,
                        conversion,
                        optional,
                    }),
                    None => {
                        if !t.contains_key("signal") {
                            self.push(
                                Diagnostic::new("J0142", "missing required key `signal`")
                                    .at(entry.node.span),
                            );
                        }
                        None
                    }
                }
            }
            _ => {
                self.push(
                    Diagnostic::new("J0181", "invalid binding form")
                        .at(entry.node.span)
                        .label(format!("found {}", entry.node.value.type_name())),
                );
                None
            }
        }
    }
}
