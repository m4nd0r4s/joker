//! AUR-424, ADR 0072: every envelope key of ref/01 §5.10 takes effect in
//! jokerd, or the manifest is refused. `deadband`, `min_interval`,
//! `max_actuations` and `verify` are run through the real cycle loop;
//! `keepalive` and `pulse` through the cycle loop and the io task of a
//! real device. The last test enumerates the keys of ref/01 §5.10 and
//! proves that none is silently ignored.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use joker_core::image::{Proposal, Source};
use joker_core::time::{ClockQuality, Instant};
use joker_core::value::NO_TS_SRC;
use joker_core::{Outcome, Quality, QualityCode, QualityReason, RejectReason, SignalId};
use joker_hal::testing::VirtualHost;
use joker_hal::{
    BatchFailure, ConnectOut, Cx, Driver, DriverError, Link, OpenArgs, RawSink, RawValue,
    ReadBatch, WriteAcks, WriteKind, WriteStatus,
};
use joker_manifest::OsFileSystem;
use jokerd::cycle::{RunOptions, run};
use jokerd::drivers::run_driver;
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Pacer, VirtualPacer};
use jokerd::shared::Shared;

const MS: u64 = 1_000_000;

const CYCLE_TOML: &str = r#"
[project]
name  = "output-keys"
joker = "1.0"
mode  = "dev"

[devices.pi]
driver = "sim"
poll   = "20ms"

[devices.pi.signals]
banded   = { type = "f64", direction = "output", unit = "kW" }
spaced   = { type = "f64", direction = "output", unit = "kW" }
budgeted = { type = "f64", direction = "output", unit = "kW" }
checked  = { type = "f64", direction = "output", unit = "kW" }
rb       = { type = "f64", direction = "input", unit = "W" }

[outputs."pi.banded"]
safe     = "0 kW"
group    = "main"
deadband = "500 W"

[outputs."pi.spaced"]
safe         = "0 kW"
group        = "main"
min_interval = "100ms"

[outputs."pi.budgeted"]
safe           = "0 kW"
group          = "main"
max_actuations = "2/h"

[outputs."pi.checked"]
safe   = "0 kW"
group  = "main"
verify = { readback = "pi.rb", tolerance = "0.1 kW", within = "60ms" }

[cycles.main]
period = "20ms"
"#;

fn loaded(text: &str) -> Loaded {
    let fs = common::fs(&[("joker.toml", text)]);
    match load(&fs, Path::new("joker.toml"), &NativeTasks) {
        Ok(l) => l,
        Err(e) => panic!("{e}"),
    }
}

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

/// A virtual pacer that calls `hook(k, shared)` right before tick `k`.
struct Scripted<F> {
    inner: VirtualPacer,
    shared: Arc<Shared>,
    tick: u64,
    hook: F,
}

impl<F: FnMut(u64, &Shared) + Send> Pacer for Scripted<F> {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    fn wait_until(&mut self, mono_ns: u64) {
        self.inner.wait_until(mono_ns);
        (self.hook)(self.tick, &self.shared);
        self.tick += 1;
    }
}

fn manual(plan: &jokerd::plan::Plan, signal: SignalId, value: f64) -> Proposal {
    Proposal {
        writer: plan.writer("user:manual:4").unwrap(),
        signal,
        value: value.to_bits(),
        source: Source::Async,
        ttl_ns: 3_600_000 * MS,
        ..Proposal::EMPTY
    }
}

/// What tick `k` left on one output: (outcome, applied value, quality of
/// its slot).
type Seen = (Outcome, f64, QualityCode, QualityReason);

/// Runs `ticks` ticks of the `main` group, writing `value(k)` to `output`
/// before tick `k` (none when `None`) and calling `inputs(k, shared)`, and
/// returns what each tick left on `output`.
fn drive(
    output: &str,
    ticks: u64,
    value: impl Fn(u64) -> Option<f64> + Send + 'static,
    inputs: impl Fn(u64, &Shared) + Send + 'static,
) -> Vec<Seen> {
    let l = loaded(CYCLE_TOML);
    let plan = &l.plan;
    let id = plan.id(output).unwrap();
    let shared = Arc::new(Shared::new(plan));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let writes: Vec<Option<Proposal>> = (0..ticks)
        .map(|k| value(k).map(|v| manual(plan, id, v)))
        .collect();
    let mut pacer = Scripted {
        inner: VirtualPacer::new(start()),
        shared: Arc::clone(&shared),
        tick: 0,
        hook: move |k: u64, s: &Shared| {
            if k > 0 {
                let (applied, meta, _) = s.output(id).unwrap().read().unwrap();
                let quality = s.slots[id.index()].read(8).unwrap().quality();
                record.lock().unwrap().push((
                    Outcome::from_code(meta.outcome, meta.detail).unwrap(),
                    f64::from_bits(applied),
                    quality.code(),
                    quality.reason(),
                ));
            }
            if let Some(Some(p)) = writes.get(k as usize) {
                s.inboxes[0].push(*p).unwrap();
            }
            inputs(k, s);
        },
    };
    run(
        plan,
        &shared,
        vec![Vec::new()],
        &mut pacer,
        RunOptions {
            max_ticks: Some(ticks + 1),
            ..RunOptions::default()
        },
    )
    .unwrap();
    seen.lock().unwrap().clone()
}

fn outcomes(seen: &[Seen]) -> Vec<(Outcome, f64)> {
    seen.iter().map(|s| (s.0, s.1)).collect()
}

#[test]
fn deadband_suppresses_a_change_smaller_than_the_band() {
    let values = [5.0, 5.3, 5.6, 4.0];
    let seen = drive(
        "pi.banded",
        4,
        move |k| values.get(k as usize).copied(),
        |_, _| {},
    );
    assert_eq!(
        outcomes(&seen),
        vec![
            (Outcome::Accepted, 5.0),
            (Outcome::Suppressed, 5.0),
            (Outcome::Accepted, 5.6),
            (Outcome::Accepted, 4.0),
        ],
        "a 0.3 kW change is inside the 500 W band; 0.6 kW is not"
    );
}

#[test]
fn min_interval_suppresses_actuations_closer_than_the_interval() {
    let seen = drive("pi.spaced", 7, |k| Some(1.0 + k as f64), |_, _| {});
    assert_eq!(
        outcomes(&seen),
        vec![
            (Outcome::Accepted, 1.0),
            (Outcome::Suppressed, 1.0),
            (Outcome::Suppressed, 1.0),
            (Outcome::Suppressed, 1.0),
            (Outcome::Suppressed, 1.0),
            (Outcome::Accepted, 6.0),
            (Outcome::Suppressed, 6.0),
        ],
        "with a 20 ms period, 100 ms lets one actuation in every five ticks"
    );
}

#[test]
fn max_actuations_rejects_with_budget_once_exhausted() {
    let seen = drive("pi.budgeted", 4, |k| Some(1.0 + k as f64), |_, _| {});
    assert_eq!(
        outcomes(&seen),
        vec![
            (Outcome::Accepted, 1.0),
            (Outcome::Accepted, 2.0),
            (Outcome::Rejected(RejectReason::Budget), 2.0),
            (Outcome::Rejected(RejectReason::Budget), 2.0),
        ],
        "two actuations per hour, then Rejected(Budget)"
    );
}

#[test]
fn verify_marks_an_output_whose_readback_disagrees_not_confirmed() {
    let rb = loaded(CYCLE_TOML).plan.id("pi.rb").unwrap();
    let seen = drive(
        "pi.checked",
        12,
        |k| (k == 0).then_some(5.0),
        move |k, s: &Shared| {
            let readback: f64 = if k < 8 { 0.0 } else { 4_950.0 };
            s.slots[rb.index()].write(readback.to_bits(), Quality::GOOD, NO_TS_SRC, 0);
        },
    );
    let quality = |k: usize| (seen[k].2, seen[k].3);
    assert_eq!(seen[0].1, 5.0);
    for k in 0..3 {
        assert_eq!(
            quality(k),
            (QualityCode::Good, QualityReason::None),
            "tick {k} is inside `within`"
        );
    }
    for k in 3..8 {
        assert_eq!(
            quality(k),
            (QualityCode::Uncertain, QualityReason::NotConfirmed),
            "tick {k}: the readback of 0 W has disagreed for 60 ms"
        );
    }
    for k in 8..12 {
        assert_eq!(
            quality(k),
            (QualityCode::Good, QualityReason::None),
            "tick {k}: 4950 W is within 0.1 kW of 5 kW"
        );
    }
    assert_eq!(seen[11].1, 5.0, "verify never changes the applied value");
}

const RELAY_PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/relay"
version = "1.0.0"
driver  = "modbus"

[signals.led]
at        = 0
fc        = 5
type      = "bool"
direction = "output"

[signals.horn]
at        = 1
fc        = 5
type      = "bool"
direction = "output"

[fallback]
timeout = "2s"
value   = false
"#;

const RELAY_TOML: &str = r#"
[project]
name  = "output-stage"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.io]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/relay@1"
timeout = "1s"
unit_id = 1

[outputs."io.led"]
safe      = false
keepalive = "100ms"

[outputs."io.horn"]
safe  = false
pulse = "200ms"
"#;

/// One write the driver received: when, which output, its value and why.
type Write = (u64, String, Option<bool>, WriteKind);

/// A driver that reads nothing and records every write.
struct Recorder {
    points: Vec<String>,
    log: Arc<Mutex<Vec<Write>>>,
}

impl Driver for Recorder {
    fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        Ok(Self {
            points: Vec::new(),
            log: Arc::default(),
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        Ok(())
    }

    async fn read(
        &mut self,
        _cx: &mut Cx<'_>,
        _batch: &ReadBatch<'_>,
        _out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        Ok(())
    }

    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[joker_hal::CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        let now = cx.now().mono_ns;
        let mut log = self.log.lock().unwrap();
        for (i, w) in writes.iter().enumerate() {
            let value = match w.value {
                RawValue::Bool(b) => Some(b),
                _ => None,
            };
            let point = self.points[w.point as usize].clone();
            log.push((now, point, value, w.kind));
            acks.set(i, WriteStatus::Ok, Some(now));
        }
    }
}

/// Commits `true` on both outputs of [`RELAY_TOML`] through the cycle
/// loop, then runs the device's io task for `until_ns` of virtual time and
/// returns every write it sent.
fn relay_writes(until_ns: u64) -> Vec<Write> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), RELAY_TOML).unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
    std::fs::write(
        dir.path().join("profiles/acme/relay@1.profile.toml"),
        RELAY_PROFILE,
    )
    .unwrap();
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    for path in ["io.led", "io.horn"] {
        let mut p = manual(plan, plan.id(path).unwrap(), 0.0);
        p.value = 1;
        shared.inboxes[0].push(p).unwrap();
    }
    run(
        plan,
        &shared,
        vec![Vec::new()],
        &mut VirtualPacer::new(start()),
        RunOptions {
            max_ticks: Some(1),
            ..RunOptions::default()
        },
    )
    .unwrap();

    let device = plan.devices[0].clone();
    let owned = jokerd::bridge::owned_request(&device);
    let views = owned.views();
    let hal_plan = joker_hal::plan::Plan::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let driver = Recorder {
        points: device.points.iter().map(|p| p.path.clone()).collect(),
        log: Arc::clone(&log),
    };
    let host = Arc::new(VirtualHost::new(0));
    let clock = Arc::clone(&host);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &[],
        &shared,
        0,
        device.fallback_keepalive_ns,
        tokio::sync::mpsc::channel(1).1,
        move || clock.mono() >= until_ns,
    ))
    .unwrap();
    log.lock().unwrap().clone()
}

fn of<'w>(writes: &'w [Write], output: &str) -> Vec<&'w Write> {
    writes.iter().filter(|w| w.1 == output).collect()
}

#[test]
fn an_output_keepalive_renews_it_at_its_own_period() {
    let writes = relay_writes(1_000 * MS);
    let led = of(&writes, "io.led");
    assert_eq!(led[0].2, Some(true));
    assert_eq!(led[0].3, WriteKind::Set);
    let renewals: Vec<u64> = led
        .iter()
        .filter(|w| w.3 == WriteKind::Keepalive)
        .map(|w| w.0)
        .collect();
    assert!(
        renewals.len() >= 9,
        "a 100 ms keepalive renews about ten times a second: {renewals:?}"
    );
    assert!(renewals.windows(2).all(|p| p[1] - p[0] >= 100 * MS));
    assert!(led.iter().all(|w| w.2 == Some(true)), "{led:?}");
    let horn_renewals = of(&writes, "io.horn")
        .iter()
        .filter(|w| w.3 == WriteKind::Keepalive)
        .count();
    assert_eq!(
        horn_renewals, 2,
        "without its own keepalive, the horn keeps the device's: 2 s / 5"
    );
}

#[test]
fn a_pulse_returns_the_output_to_safe_once_it_has_elapsed() {
    let writes = relay_writes(1_000 * MS);
    let horn = of(&writes, "io.horn");
    assert_eq!(
        (horn[0].2, horn[0].3),
        (Some(true), WriteKind::Set),
        "{horn:?}"
    );
    assert_eq!((horn[1].2, horn[1].3), (Some(false), WriteKind::Set));
    let length = horn[1].0 - horn[0].0;
    assert!(
        (200 * MS..300 * MS).contains(&length),
        "the pulse lasts its 200 ms: {length}"
    );
    assert!(
        horn[2..].iter().all(|w| w.2 == Some(false)),
        "a keepalive renews the safe state, never the pulse: {horn:?}"
    );
    assert!(horn[2..].iter().any(|w| w.3 == WriteKind::Keepalive));
}

const KEYS_TOML: &str = r#"
[project]
name  = "keys"
joker = "1.0"
mode  = "dev"

[devices.pi]
driver = "sim"
poll   = "20ms"

[devices.pi.signals]
p  = { type = "f64", direction = "output", unit = "kW" }
rb = { type = "f64", direction = "input", unit = "W" }

[cycles.main]
period = "20ms"

[outputs."pi.p"]
"#;

/// How a key of the envelope shows that it was not ignored.
#[derive(Debug, Clone, Copy)]
enum Effect {
    /// The output's plan differs from the plan without the key.
    Changes,
    /// The manifest does not load, with this code.
    Refused(&'static str),
    /// The check reports this code, which it does not without the key.
    Reports(&'static str),
    /// The check stops reporting this code, which it does without the key.
    Silences(&'static str),
}

/// A key, the manifest it is tried on (`true`: the relay of
/// [`RELAY_TOML`], whose device has a fallback), the lines that declare it,
/// and its effect.
type KeyCase = (&'static str, bool, &'static str, Effect);

const KEY_CASES: &[KeyCase] = &[
    ("safe", false, "", Effect::Refused("J0201")),
    (
        "safer",
        false,
        "safer = \"lower\"",
        Effect::Reports("J0238"),
    ),
    (
        "safe_ramp",
        false,
        "safe_ramp = \"1 kW/s\"",
        Effect::Changes,
    ),
    (
        "interlock",
        false,
        "interlock = \"pi.rb > 0 W\"",
        Effect::Changes,
    ),
    (
        "permit_uncertain",
        false,
        "permit_uncertain = true",
        Effect::Changes,
    ),
    (
        "on_interlock",
        false,
        "on_interlock = \"hold\"",
        Effect::Changes,
    ),
    (
        "requires_good",
        false,
        "requires_good = [\"pi.rb\"]",
        Effect::Changes,
    ),
    ("grace", false, "grace = \"2s\"", Effect::Changes),
    (
        "range",
        false,
        "range = [\"-5 kW\", \"5 kW\"]",
        Effect::Changes,
    ),
    ("on_range", false, "on_range = \"reject\"", Effect::Changes),
    ("rate", false, "rate = \"1 kW/s\"", Effect::Changes),
    ("deadband", false, "deadband = \"1 kW\"", Effect::Changes),
    (
        "min_interval",
        false,
        "min_interval = \"1s\"",
        Effect::Changes,
    ),
    (
        "max_actuations",
        false,
        "max_actuations = \"10/h\"",
        Effect::Changes,
    ),
    ("max_lease", false, "max_lease = \"10m\"", Effect::Changes),
    ("max_manual", false, "max_manual = \"1h\"", Effect::Changes),
    (
        "verify",
        false,
        "verify = { readback = \"pi.rb\", tolerance = \"1 kW\" }",
        Effect::Changes,
    ),
    (
        "reset_when",
        false,
        "reset_when = \"pi.rb > 0 W\"",
        Effect::Changes,
    ),
    ("group", false, "group = \"main\"", Effect::Changes),
    ("keepalive", true, "keepalive = \"100ms\"", Effect::Changes),
    ("pulse", true, "pulse = \"200ms\"", Effect::Changes),
    (
        "remote_levels",
        false,
        "remote_levels = [3]",
        Effect::Refused("J0145"),
    ),
    (
        "accept_missing_fallback",
        false,
        "accept_missing_fallback = \"bench output, nothing behind it\"",
        Effect::Silences("J0205"),
    ),
    (
        "widen",
        false,
        "widen = \"range\"",
        Effect::Refused("J0167"),
    ),
    (
        "schedule_depth",
        false,
        "schedule_depth = 4",
        Effect::Refused("J0290"),
    ),
    ("timed", false, "timed = true", Effect::Refused("J0290")),
    (
        "on_late",
        false,
        "on_late = \"drop\"",
        Effect::Refused("J0290"),
    ),
    (
        "late_margin",
        false,
        "late_margin = \"10ms\"",
        Effect::Refused("J0290"),
    ),
];

/// Loads the keys manifest (or the relay one) with `lines` in the
/// envelope of its output, and `safe` unless `with_safe` is false.
fn try_keys(relay: bool, lines: &str, with_safe: bool) -> (Result<Loaded, String>, &'static str) {
    let dir = tempfile::tempdir().unwrap();
    let (text, output) = if relay {
        let base = RELAY_TOML.replace("keepalive = \"100ms\"\n", "");
        (
            base.replace(
                "[outputs.\"io.led\"]\nsafe      = false\n",
                &format!("[outputs.\"io.led\"]\nsafe = false\n{lines}\n"),
            ),
            "io.led",
        )
    } else {
        let safe = if with_safe { "safe = \"0 kW\"\n" } else { "" };
        (format!("{KEYS_TOML}{safe}{lines}\n"), "pi.p")
    };
    std::fs::write(dir.path().join("joker.toml"), text).unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
    std::fs::write(
        dir.path().join("profiles/acme/relay@1.profile.toml"),
        RELAY_PROFILE,
    )
    .unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .map_err(|e| e.to_string());
    (loaded, output)
}

fn reference_output_keys() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/ref/01-manifest.md");
    let text = std::fs::read_to_string(path).unwrap();
    let section = text
        .split("### 5.10 `[outputs.\"<path>\"]`: the envelope")
        .nth(1)
        .and_then(|s| s.split("**`verify`:**").next())
        .unwrap();
    section
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let key = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            let phase = cells.get(cells.len().checked_sub(2)?)?;
            Some((key.to_owned(), phase.trim_matches('*').to_owned()))
        })
        .collect()
}

#[test]
fn no_output_key_of_the_reference_is_silently_ignored() {
    let keys = reference_output_keys();
    let documented: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
    let cased: Vec<&str> = KEY_CASES.iter().map(|c| c.0).collect();
    assert_eq!(documented, cased, "every key of ref/01 §5.10 has a case");
    let schema: Vec<&str> = joker_manifest::schema::ENVELOPE
        .iter()
        .map(|k| k.name)
        .collect();
    assert_eq!(schema, cased, "the manifest schema has the same keys");
    for ((key, phase), case) in keys.iter().zip(KEY_CASES) {
        if phase != "1.0" {
            assert!(
                matches!(case.3, Effect::Refused("J0290")),
                "`{key}` ({phase}) is reserved in this runtime"
            );
        }
    }

    let (base, _) = try_keys(false, "", true);
    let base = base.unwrap();
    let (relay_base, _) = try_keys(true, "", true);
    let relay_base = relay_base.unwrap();
    for (key, relay, lines, effect) in KEY_CASES {
        let (with, output) = try_keys(*relay, lines, *key != "safe");
        let before = if *relay { &relay_base } else { &base };
        match (effect, with) {
            (Effect::Changes, Ok(l)) => assert_ne!(
                l.plan.output(output),
                before.plan.output(output),
                "`{key}` does not change the plan of `{output}`"
            ),
            (Effect::Refused(code), Err(e)) => {
                assert!(e.contains(code), "`{key}` is refused without {code}: {e}")
            }
            (Effect::Reports(code), Ok(l)) => {
                assert!(l.report.codes().contains(code), "`{key}` without {code}");
                assert!(!before.report.codes().contains(code));
            }
            (Effect::Silences(code), Ok(l)) => {
                assert!(!l.report.codes().contains(code), "`{key}` keeps {code}");
                assert!(before.report.codes().contains(code));
            }
            (effect, result) => panic!(
                "`{key}`: expected {effect:?}, got {:?}",
                result.map(|l| l.report.codes())
            ),
        }
    }
}
