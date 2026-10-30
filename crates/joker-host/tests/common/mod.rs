//! Shared helpers of the joker-host tests: building the guest fixtures,
//! writing their `joker:meta`, and a one-group core bench to run them on.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use joker_core::commit::OutputState;
use joker_core::output::ClockRequirement;
use joker_core::registry::{Alias, GroupMeta, SignalMeta, UnitRef};
use joker_core::slot::SeqExtender;
use joker_core::time::Instant;
use joker_core::{
    ClockQuality, CommitSink, DecisionRecord, ImageStorage, IntakeSlot, Level, OutputConfig,
    OutputResult, Outputs, OutputsStorage, ProcessImage, Proposal, Quality, QualityCode,
    QualityReason, Registry, RegistryBuilder, SafeValue, Sample, SignalId, Slot, ValueType,
    WriteResult, WriterConfig, WriterId, WriterKind,
};
use joker_host::{
    CycleInstance, CycleReport, CycleSetup, InstanceLimits, PortBinding, PortSource, RunMode,
};

/// Builds a workspace guest crate for `wasm32-wasip2` and returns the
/// component bytes. The guests are workspace members, so this uses the
/// workspace `Cargo.lock`; they build in a target directory of their own,
/// apart from the one running the tests. Equivalent by hand:
///
/// ```sh
/// cargo build --locked --release --target wasm32-wasip2 -p joker-host-fixture-echo
/// ```
///
/// With `JOKER_GUEST_DIR` set, the component is read from
/// `<dir>/<package with _>.wasm` instead: a test binary cross-built for a
/// node runs there with the guests built on the laptop, since a node never
/// compiles (docs/26 E5).
pub fn guest(package: &str) -> Vec<u8> {
    if let Some(dir) = std::env::var_os("JOKER_GUEST_DIR") {
        let artifact = PathBuf::from(dir).join(format!("{}.wasm", package.replace('-', "_")));
        return std::fs::read(&artifact).unwrap_or_else(|e| panic!("{}: {e}", artifact.display()));
    }
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

/// The echo fixture, without `joker:meta`, built once per test binary.
pub fn echo_raw() -> &'static [u8] {
    static ECHO: OnceLock<Vec<u8>> = OnceLock::new();
    ECHO.get_or_init(|| guest("joker-host-fixture-echo"))
}

/// A CBOR item, for writing `joker:meta` by hand.
#[derive(Clone, Debug)]
pub enum C {
    U(u64),
    T(String),
    F(f64),
    B(bool),
    A(Vec<C>),
    M(Vec<(u64, C)>),
}

pub fn t(s: &str) -> C {
    C::T(s.to_owned())
}

fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    match n {
        0..=23 => out.push(m | n as u8),
        24..=0xff => out.extend_from_slice(&[m | 24, n as u8]),
        0x100..=0xffff => {
            out.push(m | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(m | 26);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        _ => {
            out.push(m | 27);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

impl C {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            C::U(n) => head(out, 0, *n),
            C::T(s) => {
                head(out, 3, s.len() as u64);
                out.extend_from_slice(s.as_bytes());
            }
            C::F(f) => {
                out.push(0xfb);
                out.extend_from_slice(&f.to_be_bytes());
            }
            C::B(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            C::A(items) => {
                head(out, 4, items.len() as u64);
                for i in items {
                    i.encode(out);
                }
            }
            C::M(entries) => {
                head(out, 5, entries.len() as u64);
                for (k, v) in entries {
                    head(out, 0, *k);
                    v.encode(out);
                }
            }
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }
}

/// The canonical `joker:meta` of the echo fixture, with `world` and
/// `features` overridable.
pub fn echo_meta(world: &str, features: &[&str]) -> C {
    let port = |name: &str, kind: u64, ty: u64, unit: &str| {
        C::M(vec![
            (0, t(name)),
            (1, C::U(kind)),
            (2, C::U(ty)),
            (3, t(unit)),
        ])
    };
    let task = |name: &str| {
        C::M(vec![
            (0, t(name)),
            (1, C::U(0)),
            (2, C::U(20_000_000)),
            (3, C::U(5_000)),
        ])
    };
    C::M(vec![
        (0, C::U(1)),
        (1, t("echo")),
        (2, t("0.1.0")),
        (3, t(world)),
        (4, C::M(vec![(0, t("rust")), (1, t("0.1.0")), (2, C::U(0))])),
        (5, C::U(1)),
        (
            6,
            C::A(vec![
                port("x", 0, 3, "kW"),
                C::M(vec![
                    (0, t("gain")),
                    (1, C::U(2)),
                    (2, C::U(3)),
                    (3, t("1")),
                    (8, C::F(1.0)),
                ]),
                port("y", 1, 3, "kW"),
                port("nan", 1, 2, "1"),
                port("r", 1, 2, "1"),
            ]),
        ),
        (
            7,
            C::A(["bad", "echo", "empty", "panic", "spin"].map(task).to_vec()),
        ),
        (8, C::A(vec![])),
        (10, C::A(features.iter().map(|f| t(f)).collect())),
        (11, C::A(vec![])),
    ])
}

/// Appends a component-level custom section.
pub fn with_section(component: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    leb(&mut body, name.len() as u64);
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(payload);
    let mut out = component.to_vec();
    out.push(0);
    leb(&mut out, body.len() as u64);
    out.extend_from_slice(&body);
    out
}

fn leb(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The echo component with its canonical `joker:meta`.
pub fn echo() -> Vec<u8> {
    with_section(
        echo_raw(),
        "joker:meta",
        &echo_meta("joker:runtime@0.1.0", &[]).bytes(),
    )
}

pub const PERIOD: u64 = 20_000_000;
pub const TASK_BAD: u32 = 0;
pub const TASK_ECHO: u32 = 1;
pub const TASK_EMPTY: u32 = 2;
pub const TASK_PANIC: u32 = 3;
pub const TASK_SPIN: u32 = 4;
pub const GAIN: f64 = 1.5;

#[derive(Default)]
pub struct Log {
    pub writes: Vec<WriteResult>,
    pub outputs: Vec<OutputResult>,
    pub decisions: Vec<DecisionRecord>,
    pub causes: Vec<Vec<SignalId>>,
}

impl CommitSink for Log {
    fn write(&mut self, r: &WriteResult) {
        self.writes.push(*r);
    }
    fn output(&mut self, r: &OutputResult) {
        self.outputs.push(*r);
    }
    fn decision(&mut self, r: &DecisionRecord) {
        self.decisions.push(*r);
    }
    fn causal(&mut self, _r: &DecisionRecord, inputs: &joker_core::image::CausalSet<'_>) {
        self.causes.push(inputs.iter().map(|(s, _)| s).collect());
    }
}

pub fn leak<T>(v: Vec<T>) -> &'static mut [T] {
    Box::leak(v.into_boxed_slice())
}

/// One cycle group `main` of 20 ms with the four signals the echo app binds.
pub struct Bench {
    pub registry: Registry<'static>,
    pub slots: &'static [Slot],
    pub image: ProcessImage<'static>,
    pub outputs: Outputs<'static>,
    pub x: SignalId,
    pub y: SignalId,
    pub nan: SignalId,
    pub r: SignalId,
}

impl Bench {
    pub fn new() -> Self {
        let mut b = RegistryBuilder::new(
            leak(vec![SignalMeta::EMPTY; 4]),
            leak(vec![Alias::EMPTY; 0]),
            leak(vec![GroupMeta::EMPTY; 1]),
        );
        b.group(GroupMeta {
            name: "main",
            period_ns: PERIOD,
        })
        .unwrap();
        b.signal(SignalMeta::input("dev.x", ValueType::F64, UnitRef("kW")))
            .unwrap();
        b.signal(SignalMeta::output(
            "dev.y",
            ValueType::F64,
            UnitRef("kW"),
            "main",
        ))
        .unwrap();
        b.signal(SignalMeta::output(
            "dev.nan",
            ValueType::U64,
            UnitRef("1"),
            "main",
        ))
        .unwrap();
        b.signal(SignalMeta::output(
            "dev.r",
            ValueType::U64,
            UnitRef("1"),
            "main",
        ))
        .unwrap();
        let registry = b.build().unwrap();
        let id = |p: &str| registry.resolve(p).unwrap();
        let (x, y, nan, r) = (id("dev.x"), id("dev.y"), id("dev.nan"), id("dev.r"));
        let group = registry.resolve_group("main").unwrap();
        let mut outs: Vec<(SignalId, ValueType)> = vec![
            (y, ValueType::F64),
            (nan, ValueType::U64),
            (r, ValueType::U64),
        ];
        outs.sort();
        let configs: &'static [OutputConfig<'static>] = leak(
            outs.iter()
                .map(|&(s, ty)| OutputConfig::new(s, ty, group, SafeValue::Value(0)))
                .collect(),
        );
        let writers: &'static [WriterConfig] = leak(vec![WriterConfig {
            kind: WriterKind::Cycle,
            level: Level::parse("control:12").unwrap(),
            lease_ns: None,
            requires_clock: ClockRequirement::Any,
        }]);
        let outputs = Outputs::new(
            &registry,
            writers,
            OutputsStorage {
                configs,
                states: leak(vec![OutputState::EMPTY; 3]),
                intakes: leak(vec![IntakeSlot::EMPTY; 64]),
            },
        )
        .unwrap();
        let mut ports = vec![x, y, nan, r];
        ports.sort();
        let image = ProcessImage::new(
            group,
            PERIOD,
            ImageStorage {
                ports: leak(ports),
                snapshot: leak(vec![Sample::INITIAL; 4]),
                extenders: leak(vec![SeqExtender::for_slot(); 4]),
                proposals: leak(vec![Proposal::EMPTY; 64]),
            },
        )
        .unwrap();
        let slots: &'static [Slot] = leak((0..4).map(|_| Slot::new()).collect());
        Self {
            registry,
            slots,
            image,
            outputs,
            x,
            y,
            nan,
            r,
        }
    }

    pub fn instance(
        &self,
        engine: &joker_host::HostEngine,
        app: &joker_host::AppComponent,
    ) -> CycleInstance {
        self.try_instance(engine, app, InstanceLimits::default())
            .unwrap()
    }

    pub fn try_instance(
        &self,
        engine: &joker_host::HostEngine,
        app: &joker_host::AppComponent,
        limits: InstanceLimits,
    ) -> Result<CycleInstance, joker_host::HostError> {
        let ports = [
            PortBinding {
                source: PortSource::Signal(self.x),
                name: "dev.x",
            },
            PortBinding {
                source: PortSource::Param(GAIN.to_bits()),
                name: "app.echo.gain",
            },
            PortBinding {
                source: PortSource::Signal(self.y),
                name: "dev.y",
            },
            PortBinding {
                source: PortSource::Signal(self.nan),
                name: "dev.nan",
            },
            PortBinding {
                source: PortSource::Signal(self.r),
                name: "dev.r",
            },
        ];
        CycleInstance::new(
            engine,
            app,
            &self.registry,
            &self.image,
            &CycleSetup {
                node: "bench",
                mode: RunMode::Test,
                generation: 1,
                generation_hash: [0x5a; 32],
                writer: WriterId(0),
                group: "main",
                ports: &ports,
                tasks: &[TASK_BAD, TASK_ECHO, TASK_EMPTY, TASK_PANIC, TASK_SPIN],
                limits,
            },
        )
    }

    /// One tick `k`: the driver writes `x` (good, or bad with `None`), the
    /// group latches, the app runs `task`, the outputs commit.
    pub fn tick(
        &mut self,
        instance: &mut CycleInstance,
        k: u64,
        x: Option<f64>,
        task: u32,
    ) -> (CycleReport, Log) {
        let quality = match x {
            Some(_) => Quality::GOOD,
            None => Quality::new(QualityCode::Bad, QualityReason::CommFailure),
        };
        let at = k * PERIOD;
        self.slots[self.x.index()].write(
            x.unwrap_or(0.0).to_bits(),
            quality,
            joker_core::value::NO_TS_SRC,
            at,
        );
        self.image
            .latch(
                k,
                Instant {
                    mono_ns: at,
                    utc_ns: 1_790_000_000_000_000_000 + at as i64,
                    clock: ClockQuality::Synced,
                },
                self.slots,
                &self.registry,
            )
            .unwrap();
        let report = instance.run(task, &mut self.image, &self.outputs).unwrap();
        let mut log = Log::default();
        self.outputs
            .commit(&self.image, self.slots, &mut log)
            .unwrap();
        self.image.end_tick().unwrap();
        (report, log)
    }

    pub fn applied(&self, signal: SignalId) -> u64 {
        self.outputs.view(signal).unwrap().applied.bits()
    }
}
