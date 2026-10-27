//! `jokerd::drivers::run_driver`'s action-call channel (AUR-405): a
//! compiled `DeviceActionPlan`'s steps run against the device's real
//! driver instance from inside the io-domain task's own loop, the only
//! place that owns it.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use joker_hal::testing::VirtualHost;
use joker_hal::{
    AddrWrite, BatchFailure, ConnectOut, Cx, Driver, DriverError, Link, OpenArgs, RawSink,
    ReadBatch,
};

use jokerd::action_exec::{ActionProgram, FailureKind};
use jokerd::bridge::{DeviceStep, DeviceStepValue};
use jokerd::drivers::{ActionCall, ActionOutcome, PointTarget, run_driver};
use jokerd::shared::Shared;

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

    async fn write_addr(
        &mut self,
        _cx: &mut Cx<'_>,
        _req: &AddrWrite<'_>,
    ) -> Result<(), BatchFailure> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn empty_shared(
    bindings: Vec<joker_hal::OutputBinding>,
    boxes: Vec<joker_hal::CommitBox>,
) -> Shared {
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
        confirmations: crossbeam_queue::ArrayQueue::new(16),
        confirmations_dropped: Default::default(),
        alarm_events: crossbeam_queue::ArrayQueue::new(1),
        alarm_events_dropped: Default::default(),
        alarm_commands: crossbeam_queue::ArrayQueue::new(1),
        alarm_replies: crossbeam_queue::ArrayQueue::new(1),
        unconfirmed: Vec::new().into_boxed_slice(),
    }
}

#[test]
fn an_action_call_runs_its_steps_against_the_real_driver() {
    let shared = Arc::new(empty_shared(Vec::new(), Vec::new()));

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
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(1);
    let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();

    let addr = joker_hal::cbor::Value::Map(vec![("at".to_owned(), joker_hal::cbor::Value::Int(1))])
        .encode();
    let steps = vec![
        DeviceStep::Write {
            addr: addr.clone(),
            type_name: Some("u16".to_owned()),
            value: DeviceStepValue::Literal(joker_hal::acquire::OwnedRaw::Uint(1)),
        },
        DeviceStep::Write {
            addr,
            type_name: Some("u16".to_owned()),
            value: DeviceStepValue::Literal(joker_hal::acquire::OwnedRaw::Uint(0)),
        },
    ];
    action_tx
        .try_send(ActionCall::new(
            ActionProgram::Steps(steps),
            1_000_000_000,
            reply_tx,
        ))
        .unwrap();

    let stop_at = 10_000_000u64;
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
        None,
        action_rx,
        move || host_for_stop.mono() >= stop_at,
    ))
    .unwrap();

    let outcome = reply_rx.try_recv().expect("the call was answered");
    assert_eq!(outcome, ActionOutcome::Ok(serde_json::json!({})));
    assert_eq!(writes_seen.load(Ordering::SeqCst), 2);
}

/// A driver whose `read_addr` never returns the value an `until` step
/// waits for (ref/06 §2.10): the interpreter's own
/// `Failure::TimedOut` (already proven at `joker_hal::interpreter`'s
/// level) must reach the action's caller as a distinct outcome, not a
/// generic `Failed` string (AUR-265: this is what the API maps to the
/// documented `J-E1406`, instead of the catch-all `J-E1408`).
struct NeverMatchingDriver;

impl Driver for NeverMatchingDriver {
    fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
        Ok(Self)
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

    async fn read_addr(
        &mut self,
        _cx: &mut Cx<'_>,
        _req: &joker_hal::AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> Result<(), BatchFailure> {
        let _ = out.push(joker_hal::RawSample::good(
            0u32,
            joker_hal::RawValue::Uint(0),
        ));
        Ok(())
    }
}

#[test]
fn an_action_call_that_times_out_reports_timed_out() {
    let shared = Arc::new(empty_shared(Vec::new(), Vec::new()));
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
    let targets: Vec<PointTarget> = Vec::new();
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(1);
    let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();

    let addr = joker_hal::cbor::Value::Map(vec![("at".to_owned(), joker_hal::cbor::Value::Int(1))])
        .encode();
    let steps = vec![DeviceStep::Until {
        addr,
        type_name: Some("u16".to_owned()),
        equals: joker_hal::acquire::OwnedRaw::Uint(42),
        timeout: joker_units::Duration::from_millis(50),
    }];
    action_tx
        .try_send(ActionCall::new(
            ActionProgram::Steps(steps),
            1_000_000_000,
            reply_tx,
        ))
        .unwrap();

    let stop_at = 10_000_000_000u64;
    let host_for_stop = Arc::clone(&host);
    host.run(run_driver(
        NeverMatchingDriver,
        &*host,
        views.request(),
        hal_plan.view(),
        1_000_000_000,
        &targets,
        &shared,
        0,
        None,
        action_rx,
        move || host_for_stop.mono() >= stop_at,
    ))
    .unwrap();

    let outcome = reply_rx.try_recv().expect("the call was answered");
    let ActionOutcome::Failed { kind, .. } = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert_eq!(kind, FailureKind::Timeout);
}
