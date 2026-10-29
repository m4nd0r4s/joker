//! `jokerd::drivers::run_driver`'s `fallback_keepalive_ns` scheduling
//! (docs/01 §8, M1): the device fallback's keepalive is renewed on its
//! own timer, independent of whether the applied value changed.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use joker_hal::testing::{OutputHarness, VirtualHost};
use joker_hal::{
    BatchFailure, CommitBox, CommitTx, ConnectOut, Cx, Driver, DriverError, Link, OpenArgs,
    RawEncoding, RawSink, ReadBatch, WriteAcks, WriteStatus,
};

use jokerd::drivers::{PointTarget, run_driver};
use jokerd::shared::Shared;

/// A driver that does nothing on read, and counts every write handed to
/// it.
struct SpyDriver {
    writes: Arc<AtomicU32>,
}

impl Driver for SpyDriver {
    fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        Ok(Self {
            writes: Arc::new(AtomicU32::new(0)),
        })
    }

    async fn connect(
        &mut self,
        _cx: &mut Cx<'_>,
        _out: &mut ConnectOut<'_>,
    ) -> Result<(), DriverError> {
        Ok(())
    }

    async fn read(
        &mut self,
        _cx: &mut Cx<'_>,
        _batch: &ReadBatch<'_>,
        _out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        Ok(())
    }

    async fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[joker_hal::CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        self.writes.fetch_add(writes.len() as u32, Ordering::SeqCst);
        for (i, _) in writes.iter().enumerate() {
            acks.set(i, WriteStatus::Ok, Some(cx.now().mono_ns));
        }
    }
}

fn empty_shared(bindings: Vec<joker_hal::OutputBinding>, boxes: Vec<CommitBox>) -> Shared {
    Shared {
        slots: Vec::new().into_boxed_slice(),
        wide_slots: Vec::new().into_boxed_slice(),
        wide_index: Vec::new().into_boxed_slice(),
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
        commit_bindings: vec![bindings],
        stages: vec![vec![jokerd::shared::OutputStage::COVERED; boxes.len()]],
        commit_boxes: vec![boxes],
        devices: vec![jokerd::shared::DeviceStats::default()].into_boxed_slice(),
        write_acks: crossbeam_queue::ArrayQueue::new(8),
        write_acks_dropped: Default::default(),
        confirmations: vec![crossbeam_queue::ArrayQueue::new(16)].into_boxed_slice(),
        confirmations_dropped: Default::default(),
        output_groups: Vec::new().into_boxed_slice(),
        trip_requests_dropped: Default::default(),
        alarm_events: crossbeam_queue::ArrayQueue::new(1),
        alarm_events_dropped: Default::default(),
        alarm_commands: Vec::new().into_boxed_slice(),
        alarm_replies: crossbeam_queue::ArrayQueue::new(1),
        unconfirmed: Vec::new().into_boxed_slice(),
        app_stopped: Vec::new().into_boxed_slice(),
        task_swaps: Vec::new().into_boxed_slice(),
        retired_tasks: crossbeam_queue::ArrayQueue::new(1),
    }
}

#[test]
fn a_keepalive_interval_resends_without_a_new_commit() {
    let mut harness = OutputHarness::new(&[("d.out", joker_core::ValueType::Bool, 0, 0)]).unwrap();
    let bindings = harness.bindings(&[RawEncoding::Bool]);
    let boxes = vec![CommitBox::new()];
    let mut last = vec![None];
    let mut tx = CommitTx::new(harness.outputs(), &bindings, &boxes, &mut last).unwrap();
    harness.manual("d.out", 1);
    tx.publish(harness.outputs());

    let shared = Arc::new(empty_shared(bindings, boxes));

    let host = Arc::new(VirtualHost::new(0));
    let hal_request = joker_hal::spec::owned::OwnedRequest {
        bus: None,
        devices: vec![joker_hal::spec::owned::OwnedDevice::new(
            "d",
            Vec::new(),
            1_000_000_000,
        )],
        points: Vec::new(),
    };
    let views = hal_request.views();
    let hal_plan = joker_hal::plan::Plan::new();

    let writes_seen = Arc::new(AtomicU32::new(0));
    let driver = SpyDriver {
        writes: Arc::clone(&writes_seen),
    };
    let targets: Vec<PointTarget> = Vec::new();
    let stop_at = 250_000_000u64;
    let host_for_stop = Arc::clone(&host);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        1_000_000_000,
        &targets,
        &shared,
        0,
        Some(50_000_000),
        tokio::sync::mpsc::channel(1).1,
        move || host_for_stop.mono() >= stop_at,
    ))
    .unwrap();
    let seen = writes_seen.load(Ordering::SeqCst);
    assert!(seen >= 4, "expected several keepalive resends, got {seen}");
}
