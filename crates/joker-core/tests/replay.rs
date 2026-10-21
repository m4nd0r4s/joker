//! Record a run into a `.jktrace`, replay it from the file alone, and check
//! that the replayed trace is byte for byte the recorded one (docs/16 §7).

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use joker_core::image::Source;
use joker_core::quality::{QualityCode, QualityReason};
use joker_core::trace::{FLAG_UNCOMPRESSED, Header, Item, Items, SegmentWriter, Ticks};
use joker_core::{ClockQuality, Proposal, Quality, WriterId};

const GOLDEN: &str = "tests/golden/arbiter_rate.jktrace";

fn script(k: u64, out: joker_core::SignalId) -> Vec<Proposal> {
    let p = |writer: u32, value: f64, source: Source| Proposal {
        writer: WriterId(writer),
        signal: out,
        value: value.to_bits(),
        source,
        ..Proposal::EMPTY
    };
    match k {
        0..=9 => vec![p(0, 50.0, Source::Cycle)],
        10 => vec![p(1, -20.0, Source::Async), p(0, 50.0, Source::Cycle)],
        11..=15 => vec![p(0, 50.0, Source::Cycle)],
        16 => vec![p(0, f64::NAN, Source::Cycle)],
        17..=24 => vec![p(0, 80.0, Source::Cycle)],
        _ => vec![],
    }
}

fn harness() -> Harness {
    Harness::new(vec![app("optimizer:13", 1_000), manual(400)], |_| {})
}

fn record() -> Vec<u8> {
    let mut h = harness();
    let mut buf = vec![0u8; 64 * 1024];
    let mut segment = SegmentWriter::new(0, &mut buf).unwrap();
    for k in 0..40 {
        match k {
            0 => h.set_input(50.0, Quality::GOOD, k),
            12 => h.set_input(
                49.9,
                Quality::new(QualityCode::Bad, QualityReason::CommFailure),
                k,
            ),
            20 => h.set_input(50.1, Quality::GOOD, k),
            _ => {}
        }
        h.clock = if (30..33).contains(&k) {
            ClockQuality::Holdover
        } else {
            ClockQuality::Synced
        };
        let staged = script(k, h.out());
        h.record(k, &staged, &mut segment);
    }
    let header = Header {
        flags: FLAG_UNCOMPRESSED,
        generation_hash: [0x11; 32],
        registry_hash: h.registry.hash(),
        epoch_mono_ns: 0,
        epoch_utc_ns: 0,
        clock: 0,
    };
    let mut file = header.encode().to_vec();
    file.extend_from_slice(segment.finish());
    file
}

#[test]
fn recording_is_deterministic_and_matches_the_golden_file() {
    let first = record();
    assert_eq!(first, record());
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(GOLDEN, &first).unwrap();
    }
    let golden = std::fs::read(GOLDEN).unwrap();
    assert_eq!(
        first, golden,
        "rerun with UPDATE_GOLDEN=1 after a deliberate format change"
    );
}

#[test]
fn replay_from_the_file_reproduces_the_same_bytes() {
    let file = record();
    let (header, rest) = Header::decode(&file).unwrap();
    let mut h = harness();
    assert_eq!(header.registry_hash, h.registry.hash());
    let mut buf = vec![0u8; 64 * 1024];
    let mut replayed = SegmentWriter::new(0, &mut buf).unwrap();
    let mut last_seq = 0;
    let mut n_segments = 0;
    for item in Items::new(rest) {
        let Item::Ticks {
            group,
            n_ticks,
            payload,
        } = item.unwrap()
        else {
            panic!("unexpected state segment");
        };
        n_segments += 1;
        let ports = h.image.ports().len();
        assert_eq!(group, 0);
        for tick in Ticks::new(payload, n_ticks, ports) {
            let tick = tick.unwrap();
            let input = tick.inputs().next().unwrap();
            if input.seq != last_seq {
                h.set_input(f64::from_bits(input.value), input.quality, tick.index);
                last_seq = input.seq;
            }
            h.clock = joker_core::ClockQuality::from_u8(tick.clock).unwrap();
            let staged: Vec<Proposal> = tick.proposals().map(|p| p.to_proposal()).collect();
            h.record(tick.index, &staged, &mut replayed);
        }
    }
    assert_eq!(n_segments, 1);
    let replayed_file = [&rest[..0], replayed.finish()].concat();
    assert_eq!(replayed_file, rest);
}
