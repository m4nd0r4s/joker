//! The io-domain half of a real driver: the async task that opens a
//! `Driver`, polls it on a real clock, and feeds `Shared`'s slots and a
//! device's `CommitRx` (docs/01 §3.4, ref/06 §2.1).
//!
//! Scoped deliberately: [`raw_to_bits`] only covers the scalar raw kinds
//! (`Bool`/`Int`/`Uint`/`Float`) a settled value can already carry
//! directly as the core's transport bits. `Regs` (modbus's register
//! words) and `Bytes` (i2c's byte buffers) need the raw-to-engineering
//! decode of ref/06 §2.7 (word/byte order, scale, offset,
//! `round_ties_even`) — not implemented anywhere in this codebase yet,
//! by any driver or by the core. A point using either kind reads as
//! `bad(config_error)` until that decode exists.

use std::sync::Arc;

use joker_core::value::NO_TS_SRC;
use joker_core::{SignalId, ValueType};
use joker_hal::acquire::{OwnedRaw, SampleBuffer, settle};
use joker_hal::cx::Host;
use joker_hal::driver::{AckSlot, ConnectOut, Driver, RawSink, ReadBatch, WriteAcks};
use joker_hal::plan::PlanView;
use joker_hal::spec::PlanRequest;
use joker_hal::types::BatchKind;
use joker_hal::{CommitRx, Cx, RxState};

use crate::bridge::DeviceStep;
use crate::plan::Plan;
use crate::shared::Shared;

/// A request to run a device action's compiled program against the
/// device's real driver, from inside the io-domain task's own loop — the
/// only place that owns the driver instance (ref/06 §2.10, ADR 0058).
#[derive(Debug)]
pub struct ActionCall {
    /// The action's compiled steps (`jokerd::bridge::DeviceStep`).
    pub steps: Vec<DeviceStep>,
    /// Where the outcome goes; dropped silently if the caller stopped
    /// waiting.
    pub reply: tokio::sync::oneshot::Sender<ActionOutcome>,
}

/// What an action invocation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    /// Every step completed.
    Ok,
    /// A step failed; the message is `joker_hal::interpreter::Failure`'s
    /// own `Debug` rendering.
    Failed(String),
}

/// The core's transport bits for a settled scalar raw value, or `None`
/// when `value`'s kind cannot become `ty` without scale/word decoding
/// this crate does not implement yet (see the module doc).
pub fn raw_to_bits(value: &OwnedRaw, ty: ValueType) -> Option<u64> {
    match (value, ty) {
        (OwnedRaw::Bool(b), ValueType::Bool) => Some(u64::from(*b)),
        (OwnedRaw::Int(i), ValueType::I64) => Some(*i as u64),
        (OwnedRaw::Uint(u), ValueType::U64 | ValueType::Enum | ValueType::Bits) => Some(*u),
        (OwnedRaw::Float(f), ValueType::F64) => Some(f.to_bits()),
        (OwnedRaw::Float(f), ValueType::F32) => Some(u64::from((*f as f32).to_bits())),
        _ => None,
    }
}

/// One point's signal id and core type, for [`run_driver`] to write a
/// settled read into the right slot.
#[derive(Clone, Copy, Debug)]
pub struct PointTarget {
    /// The point's id in the request (matches `OwnedPoint::id`).
    pub point: joker_hal::types::PointId,
    /// The core signal to write (an input), or read (an output's applied
    /// value is never read here — writes reach the driver through
    /// `CommitRx`, not this list).
    pub signal: SignalId,
    /// The core type, for [`raw_to_bits`].
    pub ty: ValueType,
}

/// The [`PointTarget`]s of `plan.devices[device_index]`'s read points
/// (input and param; output points never reach a `RawSink`, they only
/// reach the driver as `CommittedWrite`s).
pub fn point_targets(plan: &Plan, device_index: usize) -> Vec<PointTarget> {
    let Some(device) = plan.devices.get(device_index) else {
        return Vec::new();
    };
    device
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if p.direction == joker_hal::Direction::Output {
                return None;
            }
            let signal = plan.id(&p.path)?;
            let ty = plan.signals.iter().find(|s| s.path == p.path)?.ty;
            Some(PointTarget {
                point: i as u32,
                signal,
                ty,
            })
        })
        .collect()
}

/// Runs one real device's driver on `host`'s clock until `stop` returns
/// true: connects, then loops writes-before-reads like
/// `joker_hal::testing::Bench::run_until`, but for real (`host` is a
/// `TokioHost` in production; a test can still use `VirtualHost` plus its
/// own `run`, since this is a plain `async fn`). Reads land in
/// `shared.slots` through `targets` (only the scalar raw kinds
/// `raw_to_bits` covers settle into a value; everything else is dropped,
/// not guessed at). Writes are drained from the `CommitRx` on
/// `shared.commit_bindings[device_index]`/`commit_boxes[device_index]`.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is a distinct, independently-sourced piece (the driver, the clock, the plan request/view, a deadline, the slot targets, the shared commit channel, which device, and the stop condition); a wrapper struct would only rename this list, not shrink it"
)]
pub async fn run_driver<D: Driver>(
    mut driver: D,
    host: &dyn Host,
    request: PlanRequest<'_>,
    plan: PlanView<'_>,
    timeout_ns: u64,
    targets: &[PointTarget],
    shared: &Arc<Shared>,
    device_index: usize,
    fallback_keepalive_ns: Option<u64>,
    mut action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
    mut stop: impl FnMut() -> bool,
) {
    let deadline = host.now().mono_ns + timeout_ns;
    let mut cx = Cx::new(host, deadline);
    let mut connect_sink = ();
    let mut out = ConnectOut::new(&mut connect_sink);
    if driver.connect(&mut cx, &mut out).await.is_err() {
        return;
    }

    let bindings = &shared.commit_bindings[device_index];
    let boxes = &shared.commit_boxes[device_index];
    let mut rx_state = vec![RxState::default(); bindings.len()];
    let Ok(mut rx) = CommitRx::new(bindings, boxes, &mut rx_state) else {
        return;
    };

    // On-demand batches (`fc`/params read only when something asks) are
    // not scheduled here at all: there is no periodic "due" for them to
    // begin with, and treating them as always-due-again spun this loop
    // at the virtual clock's own resolution once already (fixed by
    // scoping the scheduler to `Poll` batches only). A real on-demand
    // read needs its own request-driven trigger, not built yet.
    let mut due: Vec<u64> = vec![host.now().mono_ns; plan.len()];
    let mut buf = SampleBuffer::with_capacity(4096);
    const IDLE_NS: u64 = 100_000_000;
    // docs/01 §8, M1: the device fallback keepalive is renewed regardless
    // of whether the applied value changed; `applies_to` (which outputs
    // it actually covers, ref/03 §2.2.11) is not resolved, so every
    // binding of this device is marked due, same as `resync`.
    let mut next_keepalive = fallback_keepalive_ns.map(|ns| host.now().mono_ns + ns);
    while !stop() {
        if let (Some(ns), Some(next)) = (fallback_keepalive_ns, next_keepalive)
            && host.now().mono_ns >= next
        {
            for i in 0..bindings.len() {
                rx.keepalive(i);
            }
            next_keepalive = Some(host.now().mono_ns + ns);
        }
        let mut writes = Vec::new();
        rx.drain_into(&mut writes);
        if !writes.is_empty() {
            let mut slots = vec![AckSlot::default(); writes.len()];
            if let Some(mut acks) = WriteAcks::new(&writes, &mut slots) {
                let deadline = host.now().mono_ns + timeout_ns;
                let mut cx = Cx::new(host, deadline);
                driver.write(&mut cx, &writes, &mut acks).await;
            }
        }

        let next = plan
            .iter()
            .filter(|b| b.head.kind == BatchKind::Poll)
            .map(|b| due[b.head.id as usize])
            .chain(next_keepalive)
            .min()
            .unwrap_or(host.now().mono_ns + IDLE_NS);
        tokio::select! {
            () = core::future::poll_fn(|task_cx| host.poll_sleep(next, task_cx)) => {}
            call = action_rx.recv() => {
                if let Some(call) = call {
                    let views: Vec<_> = call.steps.iter().map(DeviceStep::view).collect();
                    let deadline = host.now().mono_ns + timeout_ns;
                    let mut acx = Cx::new(host, deadline);
                    let result = joker_hal::interpreter::run(&mut driver, &mut acx, 0, &views).await;
                    let _ = call.reply.send(match result {
                        Ok(()) => ActionOutcome::Ok,
                        Err(e) => ActionOutcome::Failed(format!("{e:?}")),
                    });
                }
                continue;
            }
        }
        if host.now().mono_ns < next {
            continue;
        }
        let ids: Vec<_> = plan
            .iter()
            .filter(|b| {
                b.head.kind == BatchKind::Poll && due[b.head.id as usize] <= host.now().mono_ns
            })
            .map(|b| b.head.id)
            .collect();
        for id in ids {
            let Some(spec) = plan.batch(id) else { continue };
            let deadline = host.now().mono_ns + timeout_ns;
            let batch = ReadBatch::new(&spec, deadline);
            let mut cx = Cx::new(host, deadline);
            buf.clear();
            let mut sink = RawSink::new(&mut buf);
            let result = driver.read(&mut cx, &batch, &mut sink).await;
            let settled = settle(&request, &batch, buf.samples(), result);
            let now = host.now().mono_ns;
            for s in settled {
                let Some(target) = targets.iter().find(|t| t.point == s.point) else {
                    continue;
                };
                let Some(bits) = raw_to_bits(&s.value, target.ty) else {
                    continue;
                };
                if let Some(slot) = shared.slots.get(target.signal.index()) {
                    slot.write(bits, s.status.into(), NO_TS_SRC, s.ts_hw.unwrap_or(now));
                }
            }
            // One step, not a catch-up loop: an on-demand batch's period
            // is 0, and a `while *d <= now { *d += period }` with a near-0
            // period would spin a huge number of times before `now`
            // itself ever advances (`now` only moves on through the
            // `poll_sleep` stall above, not inside this inner loop).
            let period = spec.head.period_ns.max(1);
            let d = &mut due[id as usize];
            if *d <= now {
                *d = now + period;
            }
        }
    }
}
