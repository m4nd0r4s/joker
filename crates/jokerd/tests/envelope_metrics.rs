//! The envelope results per output of `GET /v1/metrics` (docs/01 §19,
//! ref/09 §5.2): each published result is counted under the name of its
//! kind, and a tick with no proposal counts nothing.

#![allow(clippy::unwrap_used)]

use joker_core::{Level, Outcome, RejectReason};
use joker_manifest::OsFileSystem;
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::{EnvelopeResults, OutputMeta, Shared};

const JOKER_TOML: &str = r#"
[project]
name  = "envelope-metrics"
joker = "1.0"
mode  = "dev"

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act = { type = "bool", direction = "output", led = "led0" }

[outputs."pi.act"]
safe                    = false
accept_missing_fallback = "indicator LED, no load"
"#;

fn meta(outcome: Outcome) -> OutputMeta {
    OutputMeta {
        level: 4,
        outcome: outcome.code(),
        detail: outcome.detail(),
        tripped: outcome == Outcome::Tripped,
        relinquish: false,
    }
}

#[test]
fn each_published_result_is_counted_under_its_kind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let shared = Shared::new(&loaded.plan);
    let status = shared.output(loaded.plan.id("pi.act").unwrap()).unwrap();
    assert_eq!(status.results(), EnvelopeResults::default());

    let level = Level::new(12).unwrap();
    for (tick, outcome) in [
        Outcome::Accepted,
        Outcome::Accepted,
        Outcome::ClampedRange,
        Outcome::ClampedRate,
        Outcome::Suppressed,
        Outcome::Rejected(RejectReason::Budget),
        Outcome::Tripped,
        Outcome::None,
        Outcome::Overridden(level),
        Outcome::Deferred,
        Outcome::Shadowed,
    ]
    .into_iter()
    .enumerate()
    {
        status.publish(1, meta(outcome), tick as u64, 0);
    }
    assert_eq!(
        status.results(),
        EnvelopeResults {
            accepted: 2,
            clamped: 2,
            rejected: 1,
            suppressed: 1,
            tripped: 1,
        }
    );
}
