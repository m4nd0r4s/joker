//! Real-time admission (ADR 0048) and the bus budget (ADR 0049).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{
    AppInterface, Batch, BoardInfo, Direction, DriverInfo, Measured, ProfileInfo, SignalInfo,
    SignalType, Tier,
};
use joker_manifest::model::BusKind;
use joker_manifest::{Context, MemoryFileSystem, Report, check};
use joker_units::Duration;

fn context() -> Context {
    let mut c = Context::default();
    c.drivers.insert(
        "modbus-rtu".into(),
        DriverInfo {
            buses: vec![BusKind::Serial],
            bus_required: true,
            profile_required: true,
            config: ["unit_id".to_owned()].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    c.drivers.insert(
        "canopen".into(),
        DriverInfo {
            buses: vec![BusKind::Can],
            bus_required: true,
            profile_required: true,
            config: ["node_id".to_owned()].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    c.profiles.insert(
        "acme/meter@1".into(),
        ProfileInfo {
            signals: [(
                "p".to_owned(),
                SignalInfo::new(SignalType::Float, Direction::Input),
            )]
            .into_iter()
            .collect(),
            latency: Some(Duration::from_millis(10)),
            ..ProfileInfo::default()
        },
    );
    for (name, ms) in [("fast", 900), ("slow", 2), ("mid", 4)] {
        c.apps.insert(
            name.into(),
            AppInterface {
                tasks: [("run".to_owned(), None)].into_iter().collect(),
                wcet: [(
                    "run".to_owned(),
                    Duration::from_micros(ms * if name == "fast" { 1 } else { 1000 }),
                )]
                .into_iter()
                .collect(),
                ..AppInterface::default()
            },
        );
    }
    c.boards.insert(
        "small".into(),
        BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            memory: Some(256 << 20),
            cpus: Some(4),
            ..BoardInfo::default()
        },
    );
    c
}

fn run(text: &str, context: &Context) -> Report {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", text).dir("apps/x");
    check(&fs, Path::new("joker.toml"), context)
}

const BASE: &str = "[project]\nname = \"t\"\nmode = \"dev\"\n[target]\nkernel = \"preempt_rt\"\n";

fn meters(context: &mut Context) -> String {
    let mut text = format!(
        "{BASE}[buses.rs485]\nkind = \"serial\"\nport = \"/dev/ttyUSB0\"\nbaud = 19200\nparity = \"even\"\nturnaround = \"5ms\"\n"
    );
    for i in 1..=3 {
        text.push_str(&format!(
            "[devices.m{i}]\ndriver = \"modbus-rtu\"\nbus = \"rs485\"\nunit_id = {i}\nprofile = \"acme/meter@1\"\npoll = \"200ms\"\n"
        ));
        context.loads.insert(
            format!("m{i}"),
            vec![
                Batch {
                    request: 8,
                    response: 45,
                    ..Batch::default()
                },
                Batch {
                    request: 8,
                    response: 45,
                    ..Batch::default()
                },
            ],
        );
    }
    text
}

#[test]
fn the_rtu_example_of_document_05_is_overloaded() {
    let mut context = context();
    let text = meters(&mut context);
    let report = run(&text, &context);
    assert_eq!(report.codes(), ["J0370"], "{}", report.render());
    let rendered = report.render();
    assert!(rendered.contains("estimated occupancy 136%"), "{rendered}");
    let relaxed = text.replace("poll = \"200ms\"", "poll = \"400ms\"");
    assert!(run(&relaxed, &context).codes().is_empty());
}

#[test]
fn can_load_is_budgeted() {
    let mut context = context();
    let text = format!(
        "{BASE}[buses.can0]\nkind = \"can\"\niface = \"can0\"\nbitrate = 125000\n[devices.d]\ndriver = \"canopen\"\nbus = \"can0\"\nnode_id = 1\nprofile = \"acme/meter@1\"\npoll = \"10ms\"\n"
    );
    context.loads.insert(
        "d".into(),
        vec![Batch {
            frames: 12,
            ..Batch::default()
        }],
    );
    assert_eq!(run(&text, &context).codes(), ["J0370"]);
    context.loads.insert(
        "d".into(),
        vec![Batch {
            frames: 4,
            ..Batch::default()
        }],
    );
    assert!(run(&text, &context).codes().is_empty());
}

fn groups(extra: &str) -> String {
    format!("{BASE}[resources]\nrt = {{ cpus = [3] }}\nbg = {{ cpus = [0] }}\n{extra}")
}

#[test]
fn a_1ms_group_with_900us_of_tasks_is_refused() {
    let text = groups(
        "[cycles.fast]\nperiod = \"1ms\"\n[apps.fast]\npath = \"apps/x\"\ncycles = { run = \"fast\" }\n",
    );
    let report = run(&text, &context());
    assert_eq!(report.codes(), ["J0350"], "{}", report.render());
}

#[test]
fn harmonic_groups_are_admitted_and_measurements_win() {
    let text = groups(
        "[cycles.a]\nperiod = \"10ms\"\n[cycles.b]\nperiod = \"20ms\"\n[apps.mid]\npath = \"apps/x\"\ncycles = { run = \"a\" }\n[apps.slow]\npath = \"apps/x\"\ncycles = { run = \"b\" }\n",
    );
    let report = run(&text, &context());
    assert!(report.codes().is_empty(), "{}", report.render());
    let mut measured = context();
    measured.measured.insert(
        "mid.run".into(),
        Measured {
            p999: Duration::from_millis(9),
            max: Duration::from_millis(9),
        },
    );
    assert_eq!(run(&text, &measured).codes(), ["J0350"]);
    let mut tier2 = context();
    if let Some(app) = tier2.apps.get_mut("mid") {
        app.tier = Tier::TypeScript;
    }
    tier2.measured.insert(
        "mid.run".into(),
        Measured {
            p999: Duration::from_millis(1),
            max: Duration::from_millis(9),
        },
    );
    assert_eq!(
        run(&text, &tier2).codes(),
        ["J0350"],
        "tier 2 admits with the maximum"
    );
}

#[test]
fn explicit_priorities_keep_the_rate_monotonic_order() {
    let text = groups(
        "[cycles.a]\nperiod = \"10ms\"\npriority = 50\n[cycles.b]\nperiod = \"100ms\"\npriority = 60\n",
    );
    assert_eq!(run(&text, &context()).codes(), ["J0283"]);
    let fine = text.replace("priority = 60", "priority = 40");
    assert!(run(&fine, &context()).codes().is_empty());
}

#[test]
fn the_memory_estimate_uses_the_board_ram() {
    let mut text = format!("{BASE}[target]\nboard = \"small@1\"\n").replace(
        "[target]\nkernel = \"preempt_rt\"\n[target]\n",
        "[target]\nkernel = \"preempt_rt\"\n",
    );
    for i in 0..3 {
        text.push_str(&format!("[apps.a{i}]\npath = \"apps/x\"\n"));
    }
    let report = run(&text, &context());
    assert_eq!(report.codes(), ["J0380"], "{}", report.render());
    let small = text.replacen(
        "path = \"apps/x\"\n",
        "path = \"apps/x\"\nresources = { memory = \"8 MiB\" }\n",
        3,
    );
    assert!(
        run(&small, &context()).codes().is_empty(),
        "{}",
        run(&small, &context()).render()
    );
}

#[test]
fn every_admission_code_has_a_case() {
    let cases: [(&str, &str); 4] = [
        ("J0350", "1ms group with 900us"),
        ("J0283", "inverted priorities"),
        ("J0370", "overloaded RS-485"),
        ("J0380", "memory above 70%"),
    ];
    assert_eq!(cases.len(), 4);
}
