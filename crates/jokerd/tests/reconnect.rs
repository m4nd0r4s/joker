//! `jokerd::drivers::run_driver`'s link handling (ref/06 §2.1, §2.12): a
//! failed `connect` or a lost link turns the device's signals
//! `bad(comm_failure)`, and `connect` is retried with a backoff until the
//! link comes back.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use joker_core::{QualityCode, QualityReason, SignalId, Slot, ValueType};
use joker_driver_gpio::GpioPlanner;
use joker_hal::cbor::Value;
use joker_hal::spec::owned::{OwnedDevice, OwnedPoint, OwnedRequest};
use joker_hal::testing::VirtualHost;
use joker_hal::{
    BatchFailure, ConnectOut, Cx, Driver, DriverError, Link, LinkState, OpenArgs, Planner, RawKind,
    RawSample, RawSink, RawValue, ReadBatch, code,
};

use jokerd::drivers::{BACKOFF_MAX_NS, BACKOFF_MIN_NS, Backoff, Decode, PointTarget, run_driver};
use jokerd::shared::{DeviceStats, Shared};

const MS: u64 = 1_000_000;

#[derive(Debug, Default)]
struct Script {
    refuse_connects: u32,
    lost_from: Option<u64>,
    lost_until: u64,
    failure: Option<BatchFailure>,
    fail_every: u32,
    reads: u32,
    connects: u32,
    disconnects: u32,
}

struct FlakyDriver {
    script: Arc<Mutex<Script>>,
}

impl Driver for FlakyDriver {
    fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        Ok(Self {
            script: Arc::default(),
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        let mut s = self.script.lock().unwrap();
        if s.refuse_connects > 0 {
            s.refuse_connects -= 1;
            return Err(DriverError::new(code!("J0876"), "refused"));
        }
        s.connects += 1;
        Ok(())
    }

    async fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let now = cx.now().mono_ns;
        let mut s = self.script.lock().unwrap();
        s.reads += 1;
        if s.fail_every > 0 && s.reads.is_multiple_of(s.fail_every) {
            return Err(BatchFailure::Timeout);
        }
        if let (Some(from), Some(failure)) = (s.lost_from, s.failure)
            && now >= from
            && now < s.lost_until
        {
            return Err(failure);
        }
        for &point in batch.points {
            out.push(RawSample::good(point, RawValue::Bool(true))).ok();
        }
        Ok(())
    }

    async fn disconnect(&mut self, _cx: &mut Cx<'_>) {
        self.script.lock().unwrap().disconnects += 1;
    }
}

fn request() -> OwnedRequest {
    OwnedRequest {
        bus: None,
        devices: vec![OwnedDevice::new(
            "d",
            Value::map([("chip", Value::text("gpiochip0"))]).encode(),
            50 * MS,
        )],
        points: vec![OwnedPoint::input(
            0,
            "d.in",
            RawKind::Bool,
            0,
            "bool",
            Value::map([("line", Value::Uint(4))]).encode(),
            20 * MS,
        )],
    }
}

fn shared() -> Shared {
    Shared {
        slots: vec![Slot::new()].into_boxed_slice(),
        inboxes: Vec::new().into_boxed_slice(),
        method_inboxes: Vec::new().into_boxed_slice(),
        method_results: crossbeam_queue::ArrayQueue::new(1),
        method_results_dropped: Default::default(),
        resets: Vec::new().into_boxed_slice(),
        trips: Vec::new().into_boxed_slice(),
        reset_results: crossbeam_queue::ArrayQueue::new(1),
        reset_results_dropped: Default::default(),
        decisions: crossbeam_queue::ArrayQueue::new(1),
        decisions_dropped: Default::default(),
        results: crossbeam_queue::ArrayQueue::new(1),
        results_dropped: Default::default(),
        trace_free: crossbeam_queue::ArrayQueue::new(1),
        trace_filled: crossbeam_queue::ArrayQueue::new(1),
        trace_dropped: Default::default(),
        outputs: Vec::new().into_boxed_slice(),
        trip_since: Vec::new().into_boxed_slice(),
        output_ids: Vec::new().into_boxed_slice(),
        groups: Vec::new().into_boxed_slice(),
        export_request: Default::default(),
        exports: crossbeam_queue::ArrayQueue::new(1),
        stop: Default::default(),
        running: Default::default(),
        commit_bindings: vec![Vec::new()],
        commit_boxes: vec![Vec::new()],
        stages: vec![Vec::new()],
        devices: vec![DeviceStats::default()].into_boxed_slice(),
        write_acks: crossbeam_queue::ArrayQueue::new(8),
        write_acks_dropped: Default::default(),
        confirmations: crossbeam_queue::ArrayQueue::new(16),
        confirmations_dropped: Default::default(),
        alarm_events: crossbeam_queue::ArrayQueue::new(1),
        alarm_events_dropped: Default::default(),
        alarm_commands: crossbeam_queue::ArrayQueue::new(1),
        alarm_replies: crossbeam_queue::ArrayQueue::new(1),
        unconfirmed: Vec::new().into_boxed_slice(),
    }
}

/// Runs a [`FlakyDriver`] following `script` and calls `probe` with the
/// virtual time and the shared state before every check of `stop`, until
/// `end`.
fn run(script: Script, end: u64, mut probe: impl FnMut(u64, &Shared)) -> (Arc<Shared>, Script) {
    let script = Arc::new(Mutex::new(script));
    let req = request();
    let views = req.views();
    let plan = GpioPlanner::plan(&views.request()).unwrap();
    let shared = Arc::new(shared());
    let targets = [PointTarget {
        point: 0,
        signal: SignalId(0),
        ty: ValueType::Bool,
        decode: Decode::TYPED,
        acquisition: joker_manifest::context::Acquisition::NONE,
    }];
    let host = Arc::new(VirtualHost::new(0));
    let host_for_stop = Arc::clone(&host);
    let shared_for_stop = Arc::clone(&shared);
    host.run(run_driver(
        FlakyDriver {
            script: Arc::clone(&script),
        },
        &*host,
        views.request(),
        plan.view(),
        50 * MS,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || {
            let now = host_for_stop.mono();
            probe(now, &shared_for_stop);
            now >= end
        },
    ))
    .unwrap();
    let script = std::mem::take(&mut *script.lock().unwrap());
    (shared, script)
}

fn quality(shared: &Shared) -> (QualityCode, QualityReason) {
    let q = shared.slots[0].read(8).unwrap().quality();
    (q.code(), q.reason())
}

fn link(shared: &Shared) -> u64 {
    shared.devices[0].link.get()
}

#[test]
fn a_refused_connect_is_bad_comm_failure_until_a_retry_succeeds() {
    let mut seen_down = false;
    let (shared, script) = run(
        Script {
            refuse_connects: 2,
            ..Script::default()
        },
        2_000 * MS,
        |now, shared| {
            if now > 0 && now < 200 * MS {
                assert_eq!(
                    quality(shared),
                    (QualityCode::Bad, QualityReason::CommFailure)
                );
                assert_eq!(link(shared), LinkState::Down as u64);
                seen_down = true;
            }
        },
    );
    assert!(seen_down);
    assert_eq!(script.connects, 1, "the third attempt connects");
    assert_eq!(quality(&shared).0, QualityCode::Good);
    assert_eq!(link(&shared), LinkState::Up as u64);
    assert_eq!(shared.devices[0].reconnects.get(), 0);
}

#[test]
fn a_lost_link_disconnects_marks_the_signals_bad_and_reconnects() {
    let mut bad_while_lost = false;
    let (shared, script) = run(
        Script {
            lost_from: Some(100 * MS),
            lost_until: 150 * MS,
            failure: Some(BatchFailure::NotConnected),
            ..Script::default()
        },
        1_000 * MS,
        |now, shared| {
            if (100 * MS..250 * MS).contains(&now) {
                bad_while_lost |= quality(shared) == (QualityCode::Bad, QualityReason::CommFailure)
                    && link(shared) == LinkState::Down as u64;
            }
        },
    );
    assert!(bad_while_lost);
    assert_eq!(script.connects, 2);
    assert_eq!(
        script.disconnects, 2,
        "one on the lost link, one at the stop"
    );
    assert_eq!(shared.devices[0].reconnects.get(), 1);
    assert_eq!(quality(&shared).0, QualityCode::Good);
    assert_eq!(link(&shared), LinkState::Up as u64);
}

#[test]
fn three_rounds_of_timeouts_take_the_link_down() {
    let (shared, script) = run(
        Script {
            lost_from: Some(100 * MS),
            lost_until: 160 * MS,
            failure: Some(BatchFailure::Timeout),
            ..Script::default()
        },
        1_000 * MS,
        |_, _| {},
    );
    assert_eq!(script.connects, 2, "three failed rounds force a reconnect");
    assert_eq!(shared.devices[0].reconnects.get(), 1);
    assert_eq!(quality(&shared).0, QualityCode::Good);
}

#[test]
fn a_device_fault_does_not_count_against_the_link() {
    let (shared, script) = run(
        Script {
            lost_from: Some(100 * MS),
            lost_until: 400 * MS,
            failure: Some(BatchFailure::DeviceFault("exception 4")),
            ..Script::default()
        },
        300 * MS,
        |_, _| {},
    );
    assert_eq!(script.connects, 1);
    assert_eq!(
        quality(&shared),
        (QualityCode::Bad, QualityReason::DeviceFault)
    );
    assert_eq!(link(&shared), LinkState::Up as u64);
}

#[test]
fn a_degraded_link_reads_the_affected_batch_as_uncertain_stale() {
    let mut stale_while_degraded = false;
    let (_, script) = run(
        Script {
            fail_every: 4,
            ..Script::default()
        },
        1_000 * MS,
        |_, shared| {
            if link(shared) == LinkState::Degraded as u64 {
                let q = quality(shared);
                assert_ne!(q.0, QualityCode::Good, "a degraded batch never reads good");
                stale_while_degraded |= q == (QualityCode::Uncertain, QualityReason::Stale);
            }
        },
    );
    assert!(stale_while_degraded);
    assert_eq!(script.connects, 1, "a degraded link is not taken down");
}

#[test]
fn the_backoff_doubles_to_thirty_seconds_with_twenty_percent_jitter() {
    let mut backoff = Backoff::new(7);
    let mut base = BACKOFF_MIN_NS;
    for _ in 0..12 {
        let delay = backoff.next_delay_ns();
        assert!(
            delay >= base - base / 5 && delay <= base + base / 5,
            "{delay} outside {base} ± 20 %"
        );
        base = (base * 2).min(BACKOFF_MAX_NS);
    }
    backoff.reset();
    assert!(backoff.next_delay_ns() <= BACKOFF_MIN_NS + BACKOFF_MIN_NS / 5);
}
