//! End to end: apps written with the SDK compile to components, load in
//! `joker-host` from their own `joker:meta` fragments, and run cycles against
//! a joker-core process image, arbiter and envelope.
//!
//! The fixtures are workspace members; each test builds them for
//! `wasm32-wasip2` with the workspace lock, as this does by hand:
//!
//! ```sh
//! cargo build --locked --release --target wasm32-wasip2 -p joker-sdk-fixture-light
//! ```
//!
//! `fixtures/light` is the app of track part 1 §5, unchanged except that its
//! `//` comments are dropped (AGENTS.md: code has no comments).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

use joker_core::commit::OutputState;
use joker_core::output::{ClockRequirement, OnRange};
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::{
    ClockQuality, ImageStorage, Level, Outcome, OutputConfig, Outputs, OutputsStorage,
    ProcessImage, Proposal, Quality, QualityCode, QualityReason, Registry, RegistryBuilder,
    SafeValue, Sample, SignalId, Slot, ValueType, WriteResult, WriterConfig, WriterId, WriterKind,
};
use joker_host::meta::{PortKind, TaskKind};
use joker_host::{
    AppComponent, CycleInstance, CycleReport, CycleSetup, CycleStatus, HostEngine, InstanceLimits,
    PortBinding, PortSource, RunMode, RuntimeMode,
};

fn guest(package: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("guests");
    let status = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-p",
            package,
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .status()
        .expect("cargo runs");
    assert!(
        status.success(),
        "building {package} for wasm32-wasip2 failed"
    );
    let artifact = target_dir
        .join("wasm32-wasip2/release")
        .join(format!("{}.wasm", package.replace('-', "_")));
    std::fs::read(&artifact).unwrap_or_else(|e| panic!("{}: {e}", artifact.display()))
}

fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

#[derive(Default)]
struct Log {
    writes: Vec<WriteResult>,
}

impl joker_core::CommitSink for Log {
    fn write(&mut self, r: &WriteResult) {
        self.writes.push(*r);
    }
}

const PERIOD_MS: u64 = 20;

struct Bench {
    registry: Registry<'static>,
    slots: &'static [Slot],
    image: ProcessImage<'static>,
    outputs: Outputs<'static>,
    period: u64,
}

impl Bench {
    fn new(
        period_ms: u64,
        signals: &[SignalMeta<'static>],
        range: Option<(&str, f64, f64)>,
    ) -> Self {
        let mut b = RegistryBuilder::new(
            leak(vec![SignalMeta::EMPTY; signals.len()]),
            leak(vec![Alias::EMPTY; 0]),
            leak(vec![GroupMeta::EMPTY; 1]),
        );
        let period = period_ms * 1_000_000;
        b.group(GroupMeta {
            name: "main",
            period_ns: period,
        })
        .unwrap();
        for s in signals {
            b.signal(*s).unwrap();
        }
        let registry = b.build().unwrap();
        let group = registry.resolve_group("main").unwrap();
        let mut ports: Vec<SignalId> = signals
            .iter()
            .map(|s| registry.resolve(s.path).unwrap())
            .collect();
        ports.sort();
        let mut outs: Vec<(SignalId, ValueType, &str)> = signals
            .iter()
            .filter(|s| s.group.is_some())
            .map(|s| (registry.resolve(s.path).unwrap(), s.ty, s.path))
            .collect();
        outs.sort();
        let configs: &'static [OutputConfig<'static>] = leak(
            outs.iter()
                .map(|&(s, ty, path)| {
                    let mut c = OutputConfig::new(s, ty, group, SafeValue::Value(0));
                    if let Some((p, lo, hi)) = range
                        && p == path
                    {
                        c.range = Some((lo, hi));
                        c.on_range = OnRange::Reject;
                    }
                    c
                })
                .collect(),
        );
        let outputs = Outputs::new(
            &registry,
            leak(vec![WriterConfig {
                kind: WriterKind::Cycle,
                level: Level::parse("control:12").unwrap(),
                lease_ns: None,
                requires_clock: ClockRequirement::Any,
            }]),
            OutputsStorage {
                configs,
                states: leak(vec![OutputState::EMPTY; outs.len()]),
                intakes: leak(vec![None; 64]),
            },
        )
        .unwrap();
        let n = ports.len();
        let image = ProcessImage::new(
            group,
            period,
            ImageStorage {
                ports: leak(ports),
                snapshot: leak(vec![Sample::INITIAL; n]),
                extenders: leak(vec![SeqExtender::for_slot(); n]),
                proposals: leak(vec![Proposal::EMPTY; 64]),
            },
        )
        .unwrap();
        Self {
            slots: leak((0..signals.len()).map(|_| Slot::new()).collect()),
            registry,
            image,
            outputs,
            period,
        }
    }

    fn id(&self, path: &str) -> SignalId {
        self.registry.resolve(path).unwrap()
    }

    fn instance(
        &self,
        app: &AppComponent,
        engine: &HostEngine,
        ports: &[PortBinding<'_>],
        tasks: &[u32],
    ) -> CycleInstance {
        CycleInstance::new(
            engine,
            app,
            &self.registry,
            &self.image,
            &CycleSetup {
                node: "bench",
                mode: RunMode::Test,
                generation: 1,
                generation_hash: [1; 32],
                writer: WriterId(0),
                group: "main",
                ports,
                tasks,
                limits: InstanceLimits::default(),
            },
        )
        .unwrap()
    }

    fn input(&self, path: &str, value: f64, quality: Quality, k: u64) {
        self.slots[self.id(path).index()].write(
            value.to_bits(),
            quality,
            joker_core::value::NO_TS_SRC,
            k * self.period,
        );
    }

    fn tick(
        &mut self,
        instance: &mut CycleInstance,
        k: u64,
        tasks: &[u32],
    ) -> (Vec<CycleReport>, Log) {
        self.image
            .latch(
                k,
                Instant {
                    mono_ns: k * self.period,
                    utc_ns: 1_790_000_000_000_000_000,
                    clock: ClockQuality::Synced,
                },
                self.slots,
                &self.registry,
            )
            .unwrap();
        let reports = tasks
            .iter()
            .map(|&t| instance.run(t, &mut self.image, &self.outputs).unwrap())
            .collect();
        let mut log = Log::default();
        self.outputs
            .commit(&self.image, self.slots, &mut log)
            .unwrap();
        self.image.end_tick().unwrap();
        (reports, log)
    }

    fn applied(&self, path: &str) -> u64 {
        self.outputs.view(self.id(path)).unwrap().applied.bits()
    }
}

#[test]
fn the_track_light_app_blinks_at_1_hz() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-light"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let meta = app.meta();
    assert_eq!(
        (meta.name.as_str(), meta.version.as_str()),
        ("light", "0.1.0")
    );
    assert_eq!(meta.world, "joker:runtime@0.1.0");
    assert_eq!(meta.ports.len(), 2);
    assert_eq!(meta.ports[0].kind, PortKind::Output);
    assert_eq!(meta.ports[1].unit.as_deref(), Some("ms"));
    assert_eq!(meta.ports[1].default, Some(500.0));
    assert_eq!(meta.tasks.len(), 1);
    assert_eq!(meta.tasks[0].name, "blink");
    assert_eq!(meta.tasks[0].kind, TaskKind::Cycle);
    assert_eq!(
        (meta.tasks[0].period_ns, meta.tasks[0].wcet_ns),
        (Some(20_000_000), Some(5_000))
    );

    let mut bench = Bench::new(
        PERIOD_MS,
        &[SignalMeta::output(
            "pi.act_led",
            ValueType::Bool,
            UnitRef("1"),
            "main",
        )],
        None,
    );
    let led = bench.id("pi.act_led");
    let ports = [
        PortBinding {
            source: PortSource::Signal(led),
            name: "pi.act_led",
        },
        PortBinding {
            source: PortSource::Param(500f64.to_bits()),
            name: "app.light.half_period",
        },
    ];
    let mut instance = bench.instance(&app, &engine, &ports, &[0]);
    let mut toggles = Vec::new();
    let mut last = 0;
    for k in 0..100 {
        let (reports, log) = bench.tick(&mut instance, k, &[0]);
        assert_eq!(reports[0].status, CycleStatus::Ok);
        assert_eq!(reports[0].staged, 1);
        assert_eq!(log.writes.len(), 1);
        assert_eq!(log.writes[0].outcome, Outcome::Accepted);
        let now = bench.applied("pi.act_led");
        if now != last {
            toggles.push(k);
            last = now;
        }
    }
    assert_eq!(toggles, vec![24, 49, 74, 99]);
}

#[test]
fn reads_return_results_and_substitutions_are_noted() {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-probe"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let meta = app.meta();
    assert_eq!(meta.proposal_capacity, Some(1024));
    let names: Vec<&str> = meta.tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["regulate", "watch"]);
    let mut bench = Bench::new(
        100,
        &[
            SignalMeta::input("room.temp", ValueType::F64, UnitRef("°C")),
            SignalMeta::output("room.alarm", ValueType::Bool, UnitRef("1"), "main"),
            SignalMeta::output("room.heater", ValueType::F64, UnitRef("%"), "main"),
        ],
        Some(("room.heater", 0.0, 50.0)),
    );
    let ports = [
        PortBinding {
            source: PortSource::Signal(bench.id("room.temp")),
            name: "room.temp",
        },
        PortBinding {
            source: PortSource::Param(30f64.to_bits()),
            name: "app.probe.limit",
        },
        PortBinding {
            source: PortSource::Signal(bench.id("room.alarm")),
            name: "room.alarm",
        },
        PortBinding {
            source: PortSource::Signal(bench.id("room.heater")),
            name: "room.heater",
        },
    ];
    let mut instance = bench.instance(&app, &engine, &ports, &[0, 1]);
    let good = Quality::GOOD;

    bench.input("room.temp", 20.0, good, 0);
    let (r, _) = bench.tick(&mut instance, 0, &[0]);
    assert_eq!((r[0].status, r[0].staged), (CycleStatus::Ok, 2));
    assert_eq!(f64::from_bits(bench.applied("room.heater")), 10.0);
    assert_eq!(bench.applied("room.alarm"), 0);

    bench.input("room.temp", 35.0, good, 1);
    bench.tick(&mut instance, 1, &[0, 1]);
    assert_eq!(bench.applied("room.alarm"), 1);
    assert_eq!(f64::from_bits(bench.applied("room.heater")), 0.0);

    bench.input(
        "room.temp",
        35.0,
        Quality::new(QualityCode::Bad, QualityReason::CommFailure),
        2,
    );
    let (r, _) = bench.tick(&mut instance, 2, &[0]);
    assert_eq!((r[0].status, r[0].staged), (CycleStatus::AppError, 1));
    assert_eq!(
        instance.last_error(),
        Some(("J-E0102", "port `temp` is bad(comm_failure)"))
    );
    assert_eq!(instance.notes(), &[0]);
    assert_eq!(bench.applied("room.alarm"), 0);

    bench.input("room.temp", -30.0, good, 3);
    let (_, log) = bench.tick(&mut instance, 3, &[0, 1]);
    assert!(
        log.writes
            .iter()
            .any(|w| matches!(w.outcome, Outcome::Rejected(_)))
    );
    let (r, _) = bench.tick(&mut instance, 4, &[0, 1]);
    assert_eq!(r[1].status, CycleStatus::AppError);
    assert_eq!(
        instance.last_error(),
        Some(("J-E0050", "the heater proposal was not applied"))
    );
}
