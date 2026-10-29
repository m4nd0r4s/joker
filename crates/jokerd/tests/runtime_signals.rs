//! The runtime's own `joker.*` signals (ADR 0090): what `svc/metrics`
//! writes, and that the cycle reads them like any input.

#![allow(clippy::unwrap_used, clippy::panic)]

use joker_core::time::{ClockQuality, Instant};
use joker_core::{QualityCode, QualityReason};
use joker_manifest::OsFileSystem;

use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::pacer::Monotonic;
use jokerd::plan::Plan;
use jokerd::runtime_signals::{Sources, leases_expiring, write_all};
use jokerd::shared::{OutputMeta, Shared};

const JOKER_TOML: &str = r#"
[project]
name  = "runtime-signals"
joker = "1.0"
mode  = "dev"

[cycles.main]
period = "20ms"

[devices.pi]
driver = "sim"

[devices.pi.signals]
led = { type = "bool", direction = "output" }

[outputs."pi.led"]
safe  = false
group = "main"

[alarms.lossy]
when     = "joker.runtime.dropped > 0"
severity = "low"
group    = "main"
"#;

fn plan() -> Plan {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    loaded.plan
}

fn sources(state_dir: Option<std::path::PathBuf>) -> Sources {
    Sources {
        manual: None,
        logs: None,
        state_dir,
        clock: Monotonic::start(),
    }
}

fn read(plan: &Plan, shared: &Shared, path: &str) -> (u64, QualityCode, QualityReason) {
    let id = plan
        .id(path)
        .unwrap_or_else(|| panic!("`{path}` is not in the plan"));
    let s = shared.slots[id.index()].read(8).unwrap();
    (s.transport_bits(), s.quality().code(), s.quality().reason())
}

#[test]
fn svc_metrics_writes_the_counters_and_marks_what_it_cannot_read() {
    let plan = plan();
    let shared = Shared::new(&plan);
    let main = plan.groups.iter().position(|g| g.name == "main").unwrap();
    shared.groups[main].ticks.add(42);
    shared.groups[main].overruns.add(3);
    shared.decisions_dropped.add(2);
    shared.trace_dropped.add(5);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("state")).unwrap();
    std::fs::write(dir.path().join("state/boot_count"), "7\n").unwrap();
    write_all(
        &plan,
        &shared,
        &sources(Some(dir.path().to_path_buf())),
        5_000_000_000,
    );

    let good = |path: &str| {
        let (bits, code, _) = read(&plan, &shared, path);
        assert_eq!(code, QualityCode::Good, "{path}");
        bits
    };
    assert_eq!(good("joker.group.main.ticks"), 42);
    assert_eq!(good("joker.group.main.overruns"), 3);
    assert_eq!(good("joker.runtime.dropped"), 7);
    assert_eq!(good("joker.node.boot_count"), 7);
    assert_eq!(good("joker.output.pi.led.accepted"), 0);
    assert!(good("joker.system.disk_free_bytes") > 0);
    assert!(f64::from_bits(good("joker.node.uptime")) >= 0.0);
    assert_eq!(
        read(&plan, &shared, "joker.group.calc.ticks"),
        (0, QualityCode::Bad, QualityReason::NotConnected),
        "a group the plan does not run"
    );
    assert_eq!(
        read(&plan, &shared, "joker.arbiter.manual_active").1,
        QualityCode::Bad,
        "no manual table"
    );
}

#[test]
fn a_lease_living_on_its_own_is_expiring_and_a_renewed_one_is_not() {
    let plan = plan();
    let shared = Shared::new(&plan);
    let led = shared.output(plan.id("pi.led").unwrap()).unwrap();
    let meta = |level, outcome: joker_core::Outcome| OutputMeta {
        level,
        outcome: outcome.code(),
        detail: outcome.detail(),
        tripped: false,
        relinquish: false,
    };
    led.publish(1, meta(12, joker_core::Outcome::Accepted), 1, 1);
    assert_eq!(leases_expiring(&plan, &shared), 0);
    led.publish(1, meta(12, joker_core::Outcome::None), 2, 2);
    assert_eq!(leases_expiring(&plan, &shared), 1);
    led.publish(0, meta(0, joker_core::Outcome::None), 3, 3);
    assert_eq!(leases_expiring(&plan, &shared), 0, "no active level");
    let manual = plan
        .writers
        .iter()
        .find(|w| w.config.kind == joker_core::output::WriterKind::Manual)
        .map(|w| w.config.level.get())
        .unwrap();
    led.publish(1, meta(manual, joker_core::Outcome::None), 4, 4);
    assert_eq!(
        leases_expiring(&plan, &shared),
        0,
        "manual levels have their own alarm"
    );
}

#[test]
fn a_manifest_alarm_reads_a_runtime_signal_through_the_cycle() {
    let plan = plan();
    let shared = Shared::new(&plan);
    shared.decisions_dropped.bump();
    write_all(&plan, &shared, &sources(None), 999_000_000);
    jokerd::cycle::run(
        &plan,
        &shared,
        plan.groups.iter().map(|_| Vec::new()).collect(),
        &mut jokerd::pacer::VirtualPacer::new(Instant {
            mono_ns: 1_000_000_000,
            utc_ns: 1_760_000_000_000_000_000,
            clock: ClockQuality::Synced,
        }),
        jokerd::cycle::RunOptions {
            max_ticks: Some(2),
            ..jokerd::cycle::RunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(read(&plan, &shared, "alarm.lossy.active").0, 1);
}
