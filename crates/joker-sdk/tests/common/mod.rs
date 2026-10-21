//! Shared helpers of the SDK tests: building the fixtures for
//! `wasm32-wasip2`, and a one-group core bench to run them on.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

use joker_core::commit::OutputState;
use joker_core::output::{ClockRequirement, OnRange};
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::{
    ClockQuality, CommitSink, ImageStorage, Level, OutputConfig, Outputs, OutputsStorage,
    ProcessImage, Proposal, Quality, Registry, RegistryBuilder, SafeValue, Sample, SignalId, Slot,
    ValueType, WriteResult, WriterConfig, WriterId, WriterKind,
};
use joker_host::{
    AppComponent, CycleInstance, CycleReport, CycleSetup, HostEngine, InstanceLimits, PortBinding,
    PortSource, RunMode, RuntimeMode,
};

pub fn guest(package: &str) -> Vec<u8> {
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

pub fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

#[derive(Default)]
pub struct Log {
    pub writes: Vec<WriteResult>,
}

impl CommitSink for Log {
    fn write(&mut self, r: &WriteResult) {
        self.writes.push(*r);
    }
}

pub const PERIOD_MS: u64 = 20;

pub struct Bench {
    pub registry: Registry<'static>,
    pub slots: &'static [Slot],
    pub image: ProcessImage<'static>,
    pub outputs: Outputs<'static>,
    pub period: u64,
}

impl Bench {
    pub fn new(
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

    pub fn id(&self, path: &str) -> SignalId {
        self.registry.resolve(path).unwrap()
    }

    pub fn instance(
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

    pub fn input(&self, path: &str, value: f64, quality: Quality, k: u64) {
        self.slots[self.id(path).index()].write(
            value.to_bits(),
            quality,
            joker_core::value::NO_TS_SRC,
            k * self.period,
        );
    }

    pub fn tick(
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

    pub fn applied(&self, path: &str) -> u64 {
        self.outputs.view(self.id(path)).unwrap().applied.bits()
    }
}

/// A commit sink that only counts, so a tick through it does not allocate.
#[derive(Default)]
pub struct Counts {
    pub writes: u64,
    pub accepted: u64,
}

impl CommitSink for Counts {
    fn write(&mut self, r: &WriteResult) {
        self.writes += 1;
        if r.outcome == joker_core::Outcome::Accepted {
            self.accepted += 1;
        }
    }
}

/// The track light app (`fixtures/light`), loaded and initialized on a
/// one-output bench.
pub fn light() -> (Bench, CycleInstance) {
    let engine = HostEngine::native().unwrap();
    let app = AppComponent::compile(
        &engine,
        &guest("joker-sdk-fixture-light"),
        RuntimeMode::Prod,
    )
    .unwrap();
    let bench = Bench::new(
        PERIOD_MS,
        &[SignalMeta::output(
            "pi.act_led",
            ValueType::Bool,
            UnitRef("1"),
            "main",
        )],
        None,
    );
    let ports = [
        PortBinding {
            source: PortSource::Signal(bench.id("pi.act_led")),
            name: "pi.act_led",
        },
        PortBinding {
            source: PortSource::Param(500f64.to_bits()),
            name: "app.light.half_period",
        },
    ];
    let instance = bench.instance(&app, &engine, &ports, &[0]);
    (bench, instance)
}
