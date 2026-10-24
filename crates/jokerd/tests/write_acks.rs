//! Write acks of a real driver (ref/06 §2.9, AGENTS.md invariant 8): a
//! write the driver does not confirm reaches the decision log as a
//! `write_ack` entry, with its status and code.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use joker_hal::testing::{OutputHarness, VirtualHost};
use joker_hal::{
    BatchFailure, CommitBox, CommitTx, ConnectOut, Cx, Driver, DriverError, Link, OpenArgs,
    RawEncoding, RawSink, ReadBatch, WriteAcks, WriteKind, WriteStatus,
};
use joker_manifest::OsFileSystem;

use jokerd::decisions::{DecisionLog, drain};
use jokerd::drivers::{PointTarget, run_driver};
use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::{DeviceStats, Shared, WriteAckRecord};

struct RejectingDriver;

impl Driver for RejectingDriver {
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

    async fn write(
        &mut self,
        _cx: &mut Cx<'_>,
        writes: &[joker_hal::CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) {
        for i in 0..writes.len() {
            acks.set(i, WriteStatus::DeviceRejected, None);
        }
    }
}

fn shared(bindings: Vec<joker_hal::OutputBinding>, boxes: Vec<CommitBox>) -> Shared {
    Shared {
        slots: Vec::new().into_boxed_slice(),
        inboxes: Vec::new().into_boxed_slice(),
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
        keepalive: vec![vec![true; boxes.len()]],
        commit_boxes: vec![boxes],
        devices: vec![DeviceStats::default()].into_boxed_slice(),
        write_acks: crossbeam_queue::ArrayQueue::new(8),
        write_acks_dropped: Default::default(),
    }
}

#[test]
fn a_rejected_write_is_queued_for_the_decision_log() {
    let mut harness = OutputHarness::new(&[("d.out", joker_core::ValueType::Bool, 0, 0)]).unwrap();
    let bindings = harness.bindings(&[RawEncoding::Bool]);
    let output = bindings[0].signal;
    let boxes = vec![CommitBox::new()];
    let mut last = vec![None];
    let mut tx = CommitTx::new(harness.outputs(), &bindings, &boxes, &mut last).unwrap();
    harness.manual("d.out", 1);
    tx.publish(harness.outputs());
    let shared = Arc::new(shared(bindings, boxes));

    let host = Arc::new(VirtualHost::new(0));
    let request = joker_hal::spec::owned::OwnedRequest {
        bus: None,
        devices: vec![joker_hal::spec::owned::OwnedDevice::new(
            "d",
            Vec::new(),
            1_000_000_000,
        )],
        points: Vec::new(),
    };
    let views = request.views();
    let plan = joker_hal::plan::Plan::new();
    let targets: Vec<PointTarget> = Vec::new();
    let host_for_stop = Arc::clone(&host);
    host.run(run_driver(
        RejectingDriver,
        &*host,
        views.request(),
        plan.view(),
        1_000_000_000,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || host_for_stop.mono() >= 50_000_000,
    ))
    .unwrap();

    let ack = shared
        .write_acks
        .pop()
        .expect("the rejected write is queued");
    assert_eq!(ack.output, output);
    assert_eq!(ack.device, 0);
    assert_eq!(ack.kind, WriteKind::Set);
    assert_eq!(ack.status, WriteStatus::DeviceRejected);
    assert!(shared.write_acks.pop().is_none(), "one write, one ack");
}

const JOKER_TOML: &str = r#"
[project]
name  = "write-acks-test"
joker = "1.0"
mode  = "dev"

[outputs."io.relay"]
safe = false

[devices.io]
driver = "gpio"
chip   = "gpiochip0"

[devices.io.signals]
relay = { type = "bool", direction = "output", line = 22 }
"#;

#[test]
fn a_queued_write_ack_becomes_a_decision_log_entry_with_its_code() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let plan = loaded.plan;
    let shared = Shared::new(&plan);
    let relay = plan.id("io.relay").unwrap();
    shared
        .write_acks
        .push(WriteAckRecord {
            device: 0,
            output: relay,
            seq: 7,
            kind: WriteKind::Set,
            status: WriteStatus::DeviceRejected,
            at_mono_ns: 1_000,
        })
        .unwrap();
    shared.write_acks_dropped.bump();

    let path = dir.path().join("decisions.jsonl");
    let log = Arc::new(std::sync::Mutex::new(DecisionLog::open(&path).unwrap()));
    let mut reported = 0;
    drain(&plan, &shared, &log, 3, 0, &mut reported).unwrap();
    jokerd::decisions::lock(&log).sync().unwrap();

    let entries = DecisionLog::read_all(&path).unwrap();
    let ack = entries
        .iter()
        .find(|e| e["record"] == "write_ack")
        .expect("a write_ack entry");
    assert_eq!(ack["output"], "io.relay");
    assert_eq!(ack["device"], "io");
    assert_eq!(ack["status"], "device_rejected");
    assert_eq!(ack["code"], "J0877");
    assert_eq!(ack["kind"], "set");
    assert_eq!(ack["write_seq"], 7);
    assert_eq!(ack["gen"], 3);
    let gap = entries
        .iter()
        .find(|e| e["record"] == "gap")
        .expect("the dropped ack is declared");
    assert_eq!(gap["dropped"], 1);
}
