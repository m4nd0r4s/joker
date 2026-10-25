//! `[telemetry]` resolved against a generation's signals (ref/01 §5.17,
//! ADR 0082).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use joker_store::{Fsync, Tier};
use jokerd::load::load;
use jokerd::native::NativeTasks;

const MANIFEST: &str = r#"
[project]
name  = "telemetry"
joker = "1.0"
mode  = "dev"

[outputs."pi.p_set"]
safe = "0 kW"

[telemetry.record]
"pi.**"    = { every = "1s", keep = { raw = "1d", "1m" = "30d" } }
"pi.p_set" = { on_change = true, keep = { raw = "90d" } }

[telemetry.store]
max_disk       = "2 GiB"
fsync          = "batch"
fsync_interval = "500ms"

[devices.pi]
driver = "sim"

[devices.pi.signals]
p_set = { type = "f64", direction = "output", unit = "kW" }
temp  = { type = "f64", direction = "input", unit = "°C" }
name  = { type = "string", direction = "input" }
"#;

#[test]
fn the_most_specific_entry_records_each_signal() {
    let fs = common::fs(&[("joker.toml", MANIFEST)]);
    let loaded = match load(&fs, Path::new("joker.toml"), &NativeTasks) {
        Ok(l) => l,
        Err(e) => panic!("{e}"),
    };
    let t = &loaded.plan.telemetry;
    let rule = |p: &str| t.records.iter().find(|r| r.path == p);
    let p_set = rule("pi.p_set").unwrap();
    assert_eq!(p_set.every_ns, None, "the exact path beats the selector");
    assert_eq!(p_set.keep.get(Tier::Raw), Some(90 * 86_400 * 1_000_000_000));
    assert_eq!(p_set.keep.get(Tier::M1), None);
    let temp = rule("pi.temp").unwrap();
    assert_eq!(temp.every_ns, Some(1_000_000_000));
    assert_eq!(temp.keep.get(Tier::M1), Some(30 * 86_400 * 1_000_000_000));
    assert!(
        rule("pi.name").is_none(),
        "wide signals are not recorded in 1.0"
    );
    assert_eq!(t.store.max_disk, Some(2 << 30));
    assert_eq!(t.store.fsync, Fsync::Batch);
    assert_eq!(t.store.fsync_interval_ns, 500_000_000);
}
