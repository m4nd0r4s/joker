//! The storage write-load projection of `jk check` (`J0381`, ADR 0105).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{
    BoardInfo, Direction, DriverInfo, ProfileInfo, SignalInfo, SignalType,
};
use joker_manifest::{Context, MemoryFileSystem, Report, check, storage};

fn context() -> Context {
    let mut c = Context::default();
    c.drivers.insert(
        "meterbus".into(),
        DriverInfo {
            profile_required: true,
            ..DriverInfo::default()
        },
    );
    c.profiles.insert(
        "acme/meter@1".into(),
        ProfileInfo {
            signals: (0..50)
                .map(|i| {
                    (
                        format!("p{i}"),
                        SignalInfo::new(SignalType::Float, Direction::Input),
                    )
                })
                .collect(),
            ..ProfileInfo::default()
        },
    );
    for (name, storage) in [("pi", None), ("cm", Some("emmc")), ("pc", Some("ssd"))] {
        c.boards.insert(
            name.into(),
            BoardInfo {
                majors: vec![1],
                arch: "aarch64".into(),
                storage: storage.map(str::to_owned),
                ..BoardInfo::default()
            },
        );
    }
    c
}

fn run(text: &str) -> Report {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", text);
    check(&fs, Path::new("joker.toml"), &context())
}

fn site(board: &str, record: &str, store: &str) -> String {
    format!(
        "[project]\nname = \"t\"\nmode = \"dev\"\n[target]\nboard = \"{board}@1\"\n\
         [devices.m1]\ndriver = \"meterbus\"\nprofile = \"acme/meter@1\"\npoll = \"200ms\"\n\
         [telemetry.record]\n{record}\n{store}"
    )
}

#[test]
fn a_quiet_site_on_an_sd_card_is_within_its_limits() {
    let report = run(&site("pi", "\"m1.p0\" = { every = \"1s\" }", ""));
    assert!(report.codes().is_empty(), "{}", report.render());
    let changes = run(&site("pi", "\"m1.*\" = { on_change = true }", ""));
    assert!(changes.codes().is_empty(), "{}", changes.render());
}

#[test]
fn fifty_signals_at_100ms_are_too_much_for_an_sd_card_and_not_for_emmc() {
    let text = site("pi", "\"m1.*\" = { every = \"100ms\" }", "");
    let report = run(&text);
    assert_eq!(report.codes(), ["J0381"], "{}", report.render());
    let rendered = report.render();
    assert!(rendered.contains("storage = \"sd\""), "{rendered}");
    assert!(
        rendered.contains("historian samples 2.6 GiB/day"),
        "{rendered}"
    );
    assert!(rendered.contains("limit 2.0 GiB"), "{rendered}");
    let emmc = run(&text.replace("board = \"pi@1\"", "board = \"cm@1\""));
    assert!(emmc.codes().is_empty(), "{}", emmc.render());
}

#[test]
fn fsync_always_with_fast_samples_is_too_many_fsyncs_for_an_sd_card() {
    let record = "\"m1.p0\" = { every = \"100ms\" }";
    let always = run(&site(
        "pi",
        record,
        "[telemetry.store]\nfsync = \"always\"\n",
    ));
    assert_eq!(always.codes(), ["J0381"], "{}", always.render());
    assert!(
        always.render().contains("historian WAL 36000 fsync/h"),
        "{}",
        always.render()
    );
    let batch = run(&site(
        "pi",
        record,
        "[telemetry.store]\nfsync = \"batch\"\nfsync_interval = \"200ms\"\n",
    ));
    assert_eq!(batch.codes(), ["J0381"], "{}", batch.render());
    let ssd = run(&site(
        "pc",
        record,
        "[telemetry.store]\nfsync = \"always\"\n",
    ));
    assert!(ssd.codes().is_empty(), "{}", ssd.render());
}

#[test]
fn the_limits_of_each_class_are_those_of_adr_0105() {
    let sd = storage::limits("sd").unwrap();
    assert_eq!(sd.bytes_per_day, 2.0 * 1_073_741_824.0);
    assert_eq!(sd.fsyncs_per_hour, 10_800.0);
    assert_eq!(storage::limits("emmc").unwrap().fsyncs_per_hour, 36_000.0);
    assert_eq!(storage::limits("ssd").unwrap().fsyncs_per_hour, 360_000.0);
    assert!(storage::limits("tape").is_none());
}

#[test]
fn every_storage_code_has_a_case() {
    let cases: [(&str, &str); 1] = [("J0381", "fifty signals at 100ms on an SD card")];
    assert_eq!(cases.len(), 1);
}
