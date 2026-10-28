//! AUR-425, ADR 0075: the manifest's adjustments of a device signal
//! compose with its profile and take effect at runtime, and the output
//! stage turns an applied engineering value into the exact raw value its
//! point carries, or the envelope rejects it with `Rejected(Range)`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use joker_core::image::{Proposal, Source};
use joker_core::time::{ClockQuality, Instant};
use joker_core::{Outcome, Quality, QualityCode, QualityReason, RejectReason, ValueType};
use joker_hal::acquire::OwnedRaw;
use joker_hal::testing::VirtualHost;
use joker_hal::{
    BatchFailure, ConnectOut, Cx, Driver, DriverError, Link, OpenArgs, RawSink, ReadBatch,
    WriteAcks, WriteKind, WriteStatus,
};
use joker_manifest::OsFileSystem;
use jokerd::cycle::{RunOptions, run};
use jokerd::drivers::{Debounce, Decode, Stuck, acquire, run_driver};
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;
use jokerd::pacer::VirtualPacer;
use jokerd::shared::Shared;

const MS: u64 = 1_000_000;

const PROFILE: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[enums]
Mode = { off = 0, on = 7, auto = 9 }

[signals.p_ac]
at       = 0
fc       = 4
type     = "i16"
scale    = 0.1
unit     = "kW"
deadband = "0.5 kW"

[signals.level]
at   = 1
fc   = 4
type = "u16"
unit = "V"

[signals.count]
at   = 2
fc   = 4
type = "u16"

[signals.p_set]
at        = 10
fc        = 6
type      = "i16"
scale     = 0.1
unit      = "kW"
direction = "output"

[signals.q_set]
at        = 11
fc        = 6
type      = "u32"
unit      = "kvar"
direction = "output"
word_order = "little"

[signals.run]
at        = 12
fc        = 5
type      = "bool"
invert    = true
direction = "output"

[signals.mode]
at        = 13
fc        = 6
type      = "u16"
enum      = "Mode"
direction = "output"
"#;

const BASE: &str = r#"
[project]
name  = "adjustments"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.pcs]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/pcs@1"
timeout = "1s"
unit_id = 1
poll    = "100ms"

[outputs."pcs.p_set"]
safe                    = "0 kW"
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.q_set"]
safe                    = "0 kvar"
range                   = ["0 kvar", "100000 kvar"]
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.run"]
safe                    = false
accept_missing_fallback = "bench inverter, no load behind it"

[outputs."pcs.mode"]
safe                    = "Mode.off"
accept_missing_fallback = "bench inverter, no load behind it"
"#;

fn try_load(extra: &str) -> Result<Loaded, String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), format!("{BASE}{extra}")).unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
    std::fs::write(dir.path().join("profiles/acme/pcs@1.profile.toml"), PROFILE).unwrap();
    load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks).map_err(|e| e.to_string())
}

fn loaded(extra: &str) -> Loaded {
    try_load(extra).unwrap_or_else(|e| panic!("{e}"))
}

fn point<'p>(l: &'p Loaded, path: &str) -> &'p jokerd::bridge::DevicePointPlan {
    l.plan.devices[0]
        .points
        .iter()
        .find(|p| p.path == path)
        .unwrap()
}

fn read(l: &Loaded, path: &str, raw: OwnedRaw) -> Result<f64, QualityReason> {
    let p = point(l, path);
    let ty = l.plan.signal(path).unwrap().ty;
    acquire(
        &raw,
        &Decode::of_point(&p.type_name, &p.addr),
        &p.acquisition,
        ty,
    )
    .map(|bits| ty.bits_to_f64(bits).unwrap())
}

#[test]
fn manifest_scale_offset_and_unit_compose_after_the_profile() {
    let l = loaded(
        "[devices.pcs.signals]\np_ac = { scale = 2, offset = \"1 kW\", unit = \"W\", max_age = \"3s\" }\n",
    );
    let a = &point(&l, "pcs.p_ac").acquisition;
    assert_eq!(a.scale, Some(200.0), "0.1 × 2 × 1000 W/kW");
    assert_eq!(a.offset, Some(1_000.0), "1 kW after the scale, in W");
    assert_eq!(
        a.deadband,
        Some(1_000.0),
        "the profile's 0.5 kW follows: × 2, in W"
    );
    let signal = l.plan.signal("pcs.p_ac").unwrap();
    assert_eq!(signal.unit, "W");
    assert_eq!(signal.max_age_ns, Some(3_000 * MS));
    assert_eq!(read(&l, "pcs.p_ac", OwnedRaw::Regs(vec![10])), Ok(3_000.0));
}

#[test]
fn a_two_point_scale_maps_from_onto_to_and_takes_its_unit() {
    let l = loaded(
        "[devices.pcs.signals]\nlevel = { scale = { from = [\"0.6 V\", \"3.0 V\"], to = [\"0 m\", \"3 m\"] } }\n",
    );
    assert_eq!(l.plan.signal("pcs.level").unwrap().unit, "m");
    let level = read(&l, "pcs.level", OwnedRaw::Regs(vec![3])).unwrap();
    assert!((level - 3.0).abs() < 1e-12, "{level}");
}

#[test]
fn an_integer_signal_with_a_manifest_scale_becomes_a_float() {
    let before = loaded("");
    assert!(matches!(
        before.plan.signal("pcs.count").unwrap().ty,
        ValueType::I64 | ValueType::U64
    ));
    let l = loaded("[devices.pcs.signals]\ncount = { scale = 0.5 }\n");
    assert_eq!(l.plan.signal("pcs.count").unwrap().ty, ValueType::F64);
    assert_eq!(read(&l, "pcs.count", OwnedRaw::Regs(vec![5])), Ok(2.5));
}

#[test]
fn limits_and_deadband_replace_the_profiles_and_the_signals_section_reaches_the_device() {
    let l = loaded(
        "[devices.pcs.signals]\np_ac = { limits = [\"-10 kW\", \"10 kW\"] }\n[signals.\"pcs.p_ac\"]\ndeadband = \"2 kW\"\nstuck_detect = \"5s\"\n",
    );
    let a = &point(&l, "pcs.p_ac").acquisition;
    assert_eq!(a.limits, Some((-10.0, 10.0)));
    assert_eq!(a.deadband, Some(2.0));
    assert_eq!(a.stuck_detect.map(|d| d.as_nanos()), Some(5_000 * MS));
    assert_eq!(read(&l, "pcs.p_ac", OwnedRaw::Regs(vec![100])), Ok(10.0));
    assert_eq!(
        read(&l, "pcs.p_ac", OwnedRaw::Regs(vec![101])),
        Err(QualityReason::OutOfRange),
        "10.1 kW is outside the limits"
    );
}

#[test]
fn an_adjustment_that_does_not_fit_stops_the_plan_with_its_code() {
    let err = try_load("[signals.\"pcs.p_ac\"]\nstuck_detect = \"0s\"\n").unwrap_err();
    assert!(err.contains("J0816"), "{err}");
    let err = try_load("[devices.pcs.signals]\np_ac = { unit = \"Hz\" }\n").unwrap_err();
    assert!(err.contains("J0107"), "{err}");
}

#[test]
fn stuck_detect_marks_a_value_that_does_not_move_frozen() {
    let mut stuck = Stuck::default();
    let window = Some(1_000 * MS);
    let g = Quality::GOOD;
    assert_eq!(stuck.filter(window, 7, g, 0), g);
    assert_eq!(stuck.filter(window, 7, g, 900 * MS), g);
    assert_eq!(
        stuck.filter(window, 7, g, 1_000 * MS),
        Quality::new(QualityCode::Uncertain, QualityReason::Frozen)
    );
    assert_eq!(
        stuck.filter(window, 8, g, 1_100 * MS),
        g,
        "a new value moves"
    );
}

#[test]
fn stuck_detect_marks_wide_bytes_that_do_not_move_frozen() {
    let mut stuck = jokerd::drivers::WideStuck::default();
    let window = Some(1_000 * MS);
    let g = Quality::GOOD;
    let frozen = Quality::new(QualityCode::Uncertain, QualityReason::Frozen);
    assert_eq!(stuck.filter(window, &[1, 2], g, 0), g);
    assert_eq!(stuck.filter(window, &[1, 2], g, 900 * MS), g);
    assert_eq!(stuck.filter(window, &[1, 2], g, 1_000 * MS), frozen);
    assert_eq!(
        stuck.filter(window, &[1, 2, 0], g, 1_100 * MS),
        g,
        "a longer value with the same prefix moves"
    );
    assert_eq!(stuck.filter(window, &[1, 2, 0], g, 2_100 * MS), frozen);
    let bad = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
    assert_eq!(stuck.filter(window, &[], bad, 2_200 * MS), bad);
    assert_eq!(
        stuck.filter(window, &[1, 2, 0], g, 2_300 * MS),
        g,
        "a bad read restarts the window"
    );
    assert_eq!(
        stuck.filter(None, &[1, 2, 0], g, 9_000 * MS),
        g,
        "off without a window"
    );
}

#[test]
fn debounce_publishes_a_change_once_it_has_lasted() {
    let mut d = Debounce::default();
    let window = Some(100 * MS);
    let g = Quality::GOOD;
    assert_eq!(d.filter(window, 0, g, 0), Some((0, 0)), "D4: first read");
    assert_eq!(d.filter(window, 1, g, 50 * MS), None, "D3: held");
    assert_eq!(d.filter(window, 0, g, 100 * MS), Some((0, 100 * MS)), "D2");
    assert_eq!(d.filter(window, 1, g, 150 * MS), None);
    assert_eq!(d.filter(window, 1, g, 200 * MS), None);
    assert_eq!(
        d.filter(window, 1, g, 250 * MS),
        Some((1, 150 * MS)),
        "D1 and D5: three reads cover 100 ms; the instant is the first's"
    );
    let bad = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
    assert_eq!(
        d.filter(window, 0, bad, 300 * MS),
        Some((0, 300 * MS)),
        "D2: bad"
    );
}

/// One write the driver received: which output and its raw value.
type Write = (String, String, WriteKind);

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
        let mut log = self.log.lock().unwrap();
        for (i, w) in writes.iter().enumerate() {
            let point = self.points[w.point as usize].clone();
            log.push((point, format!("{:?}", w.value), w.kind));
            acks.set(i, WriteStatus::Ok, Some(cx.now().mono_ns));
        }
    }
}

fn start() -> Instant {
    Instant {
        mono_ns: 1_000 * MS,
        utc_ns: 1_760_000_000_000_000_000,
        clock: ClockQuality::Synced,
    }
}

/// Writes each `(output, bits)` manually through one tick of the cycle
/// loop, then runs the device's io task and returns what the driver got
/// and the outcome of each output.
fn through_the_output_stage(
    l: &Loaded,
    writes: &[(&str, u64)],
) -> (Vec<Write>, Vec<(String, Outcome)>) {
    let plan = &l.plan;
    let shared = Arc::new(Shared::new(plan));
    for (path, bits) in writes {
        let p = Proposal {
            writer: plan.writer("user:manual:4").unwrap(),
            signal: plan.id(path).unwrap(),
            value: *bits,
            source: Source::Async,
            ttl_ns: 3_600_000 * MS,
            ..Proposal::EMPTY
        };
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
    let outcomes = writes
        .iter()
        .map(|(path, _)| {
            let (_, meta, _) = shared
                .output(plan.id(path).unwrap())
                .unwrap()
                .read()
                .unwrap();
            (
                (*path).to_owned(),
                Outcome::from_code(meta.outcome, meta.detail).unwrap(),
            )
        })
        .collect();

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
        None,
        tokio::sync::mpsc::channel(1).1,
        move || clock.mono() >= 50 * MS,
    ))
    .unwrap();
    (log.lock().unwrap().clone(), outcomes)
}

fn value_of<'w>(writes: &'w [Write], output: &str) -> &'w str {
    &writes.iter().find(|w| w.0 == output).unwrap().1
}

#[test]
fn the_driver_receives_the_exact_raw_values() {
    let l = loaded("");
    let variants = &l.plan.signal("pcs.mode").unwrap().variants;
    let mode_on = variants.iter().position(|v| v == "on").unwrap() as u64;
    let (writes, outcomes) = through_the_output_stage(
        &l,
        &[
            ("pcs.p_set", 12.34f64.to_bits()),
            ("pcs.q_set", 65_537.0f64.to_bits()),
            ("pcs.run", 1),
            ("pcs.mode", mode_on),
        ],
    );
    assert!(
        outcomes.iter().all(|(_, o)| *o == Outcome::Accepted),
        "{outcomes:?}"
    );
    assert_eq!(
        value_of(&writes, "pcs.p_set"),
        "Regs([123])",
        "12.34 kW / 0.1"
    );
    assert_eq!(
        value_of(&writes, "pcs.q_set"),
        "Regs([1, 1])",
        "65537 in two registers, least significant word first"
    );
    assert_eq!(value_of(&writes, "pcs.run"), "Bool(false)", "invert");
    assert_eq!(value_of(&writes, "pcs.mode"), "Regs([7])", "Mode.on is 7");
}

#[test]
fn rounding_is_to_the_nearest_with_ties_to_even() {
    let l = loaded("");
    let (writes, _) = through_the_output_stage(&l, &[("pcs.p_set", 0.25f64.to_bits())]);
    assert_eq!(
        value_of(&writes, "pcs.p_set"),
        "Regs([2])",
        "2.5 rounds to 2"
    );
    let (writes, _) = through_the_output_stage(&l, &[("pcs.p_set", (-0.25f64).to_bits())]);
    assert_eq!(
        value_of(&writes, "pcs.p_set"),
        "Regs([65534])",
        "-2.5 rounds to -2"
    );
}

#[test]
fn a_manifest_scale_on_an_output_composes_with_the_profile() {
    let l = loaded("[devices.pcs.signals]\np_set = { scale = 2 }\n");
    let (writes, _) = through_the_output_stage(&l, &[("pcs.p_set", 12.34f64.to_bits())]);
    assert_eq!(
        value_of(&writes, "pcs.p_set"),
        "Regs([62])",
        "12.34 / (0.1 × 2) = 61.7"
    );
}

#[test]
fn a_value_the_raw_type_cannot_carry_is_rejected_with_range() {
    let l = loaded("");
    let p_set = l.plan.output("pcs.p_set").unwrap();
    let (lo, hi) = p_set.range.unwrap();
    assert!(
        (lo + 3_276.8).abs() < 1e-9 && (hi - 3_276.7).abs() < 1e-9,
        "{lo} {hi}"
    );
    assert_eq!(p_set.on_range, joker_core::output::OnRange::Reject);
    let (writes, outcomes) = through_the_output_stage(&l, &[("pcs.p_set", 5_000.0f64.to_bits())]);
    assert_eq!(
        outcomes[0].1,
        Outcome::Rejected(RejectReason::Range),
        "5000 kW does not fit an i16 of 0.1 kW"
    );
    assert!(
        writes
            .iter()
            .filter(|w| w.0 == "pcs.p_set")
            .all(|w| w.1 == "Regs([0])"),
        "only the safe value reaches the driver: {writes:?}"
    );
}

#[test]
fn a_safe_or_a_range_the_raw_type_cannot_carry_is_refused() {
    let base_q = "range                   = [\"0 kvar\", \"100000 kvar\"]";
    let text = |s: &str| BASE.replace(base_q, s);
    let check = |manifest: String, code: &str| {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("joker.toml"), manifest).unwrap();
        std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
        std::fs::write(dir.path().join("profiles/acme/pcs@1.profile.toml"), PROFILE).unwrap();
        let err = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains(code), "{err}");
    };
    check(text("range = [\"-1 kvar\", \"100 kvar\"]"), "J0858");
    check(
        BASE.replace("safe                    = \"0 kW\"", "safe = \"4000 kW\""),
        "J0858",
    );
}

#[test]
fn a_scaled_output_is_no_longer_refused_at_plan_time() {
    assert!(
        try_load("").is_ok(),
        "ADR 0064 refused it; ADR 0075 converts it"
    );
}

const GPIO_TOML: &str = r#"
[project]
name  = "debounce"
joker = "1.0"
mode  = "dev"

[devices.io]
driver = "gpio"
chip   = "gpiochip0"

[devices.io.signals]
button = { type = "bool", direction = "input", line = 17, poll = "50ms" }

[signals."io.button"]
debounce = "100ms"
"#;

#[test]
fn a_polled_bool_changes_only_after_its_debounce_through_the_io_task() {
    use joker_driver_gpio::{GpioDriver, GpioPlanner};
    use joker_hal::testing::MemoryGpio;
    use joker_hal::{GpioChip, Planner};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), GPIO_TOML).unwrap();
    let l = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let device = l.plan.devices[0].clone();
    assert_eq!(
        device.points[0].acquisition.debounce.map(|d| d.as_nanos()),
        Some(100 * MS)
    );
    let shared = Arc::new(Shared::new(&l.plan));
    let owned = jokerd::bridge::owned_request(&device);
    let views = owned.views();
    let hal_plan = GpioPlanner::plan(&views.request()).unwrap();
    let (chip, lines) = MemoryGpio::new();
    lines.drive(17, false);
    let driver = GpioDriver::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Gpio(GpioChip::new(chip)),
    )
    .unwrap();
    let targets = jokerd::drivers::point_targets(&l.plan, 0);
    let button = l.plan.id("io.button").unwrap();
    let host = Arc::new(VirtualHost::new(0));
    let clock = Arc::clone(&host);
    let observer = Arc::clone(&shared);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        1_000 * MS,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || {
            let now = clock.mono();
            let high = (120 * MS..170 * MS).contains(&now) || now >= 220 * MS;
            lines.drive(17, high);
            let s = observer.slots[button.index()].read(8).unwrap();
            record.lock().unwrap().push((now, s.transport_bits()));
            now >= 500 * MS
        },
    ))
    .unwrap();
    let seen = seen.lock().unwrap().clone();
    let at = |t: u64| seen.iter().rev().find(|(n, _)| *n <= t).unwrap().1;
    assert_eq!(at(110 * MS), 0);
    assert_eq!(at(160 * MS), 0, "the read at 150 ms is held, not published");
    assert_eq!(at(210 * MS), 0, "a 50 ms pulse is debounced away");
    assert_eq!(
        at(310 * MS),
        0,
        "the second rise has lasted only 50 to 100 ms"
    );
    assert_eq!(
        at(400 * MS),
        1,
        "and is published once three reads cover 100 ms"
    );
}
