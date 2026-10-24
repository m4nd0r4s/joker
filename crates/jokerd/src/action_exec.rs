//! Runs one admitted action invocation on the device's io task, the only
//! place that owns the driver (ref/06 §2.10, ADR 0058, ADR 0066).
//!
//! A step program goes through `joker_hal::interpreter::run_into`, with
//! the invocation's arguments, the device's `timeout` per driver call and
//! the action's own `timeout` over the whole program (ADR 0066 §4); a
//! native action goes to `Driver::invoke`. Cancellation reaches both
//! through the call's own [`Host`] (ADR 0066 §9). The outcome is projected
//! onto the failure kinds of ADR 0066 §13 and sent back to the caller.

use std::sync::Arc;
use std::task::{Context, Poll};

use joker_core::time::Instant;
use joker_core::{Clock, QualityCode, QualityReason};
use joker_hal::cbor::Cbor;
use joker_hal::cx::{Host, LogLevel};
use joker_hal::driver::{ActionCall as NativeCall, ActionOut, Driver};
use joker_hal::interpreter::{ArgList, Call, Failure, Reading, Results, Scalar};
use joker_hal::types::{ActionFailure, BatchFailure, RawStatus};
use joker_hal::{Cx, MonoNs};
use serde_json::{Map, Value, json};

use crate::actions::Ticket;
use crate::bridge::DeviceStep;

/// What an invocation runs.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionProgram {
    /// A profile's step program, run by the interpreter.
    Steps(Vec<DeviceStep>),
    /// A native action (`invoke`): its `ActionSpec::id` and the CBOR of
    /// its argument record.
    Native {
        /// The action's id among the device's native actions.
        id: u32,
        /// The argument record, as CBOR.
        args: Vec<u8>,
    },
}

/// A request to run an admitted invocation on the device's io task.
#[derive(Debug)]
pub struct ActionCall {
    /// What to run.
    pub program: ActionProgram,
    /// The arguments, converted to their declared types (ADR 0066 §5).
    pub args: Vec<(String, Scalar)>,
    /// The effective timeout of the invocation (ADR 0066 §4).
    pub timeout_ns: u64,
    /// The state the caller shares with this task: cancellation, start
    /// and progress.
    pub ticket: Arc<Ticket>,
    /// Where the outcome goes; dropped silently if the caller is gone.
    pub reply: tokio::sync::oneshot::Sender<ActionOutcome>,
}

impl ActionCall {
    /// A call of `program` without arguments, with `timeout_ns` and a
    /// fresh ticket.
    pub fn new(
        program: ActionProgram,
        timeout_ns: u64,
        reply: tokio::sync::oneshot::Sender<ActionOutcome>,
    ) -> Self {
        Self {
            program,
            args: Vec::new(),
            timeout_ns,
            ticket: Arc::new(Ticket::default()),
            reply,
        }
    }
}

/// Why an invocation failed, as ADR 0066 §13 projects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// The action's timeout, or an `until` that never matched.
    Timeout,
    /// The equipment or the transport failed, or a read could not be
    /// trusted.
    Device,
    /// A `fail_if` matched, or the driver refused the arguments.
    Invalid,
    /// A step the interpreter cannot resolve, or the driver has no such
    /// action.
    Unsupported,
    /// The device is not connected.
    NotConnected,
    /// Cancelled by the core.
    Cancelled,
}

impl FailureKind {
    /// The `error.kind` and `code` of the API (ref/09 §5.10).
    pub fn api(self) -> (&'static str, &'static str) {
        match self {
            Self::Timeout => ("timeout", "J-E1406"),
            Self::Device => ("device", "J-E1407"),
            Self::Invalid => ("invalid", "J-E1408"),
            Self::Unsupported => ("device", "J-E1408"),
            Self::NotConnected => ("device", "J-E1415"),
            Self::Cancelled => ("cancelled", "J-E1409"),
        }
    }
}

/// What an invocation produced.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    /// It completed; the result record (ADR 0066 §12).
    Ok(Value),
    /// It did not.
    Failed {
        /// The projection of ADR 0066 §13.
        kind: FailureKind,
        /// What happened, for humans.
        detail: String,
    },
}

impl ActionOutcome {
    /// A failure of `kind`.
    pub fn failed(kind: FailureKind, detail: impl Into<String>) -> Self {
        Self::Failed {
            kind,
            detail: detail.into(),
        }
    }
}

/// The host of one invocation: the io task's host, with the
/// invocation's own cancellation.
struct CallHost<'h> {
    inner: &'h dyn Host,
    ticket: &'h Ticket,
}

impl Clock for CallHost<'_> {
    fn now(&self) -> Instant {
        self.inner.now()
    }
}

impl Host for CallHost<'_> {
    fn poll_sleep(&self, at: MonoNs, cx: &mut Context<'_>) -> Poll<()> {
        self.inner.poll_sleep(at, cx)
    }

    fn cancelled(&self) -> bool {
        self.ticket.cancelled()
    }

    fn log(&self, level: LogLevel, message: &str) {
        self.inner.log(level, message);
    }
}

struct Record<'a, 't> {
    fields: Vec<(&'a str, Reading)>,
    ticket: &'t Ticket,
}

impl<'a> Results<'a> for Record<'a, '_> {
    fn record(&mut self, into: &'a str, reading: Reading) {
        self.fields.push((into, reading));
    }

    fn progress(&mut self, done: usize, total: usize) {
        let permille = (done * 1000).checked_div(total).unwrap_or(1000);
        self.ticket
            .set_progress(u16::try_from(permille).unwrap_or(1000), "");
    }
}

/// The quality literal of ref/03 §2.1.3 for a raw status.
pub fn status_literal(status: RawStatus) -> String {
    let code = match status.code {
        QualityCode::Good => "good",
        QualityCode::Uncertain => "uncertain",
        QualityCode::Bad => "bad",
    };
    match QualityReason::from_u8(status.reason.0).name() {
        Some(reason) if status.reason.0 != 0 => format!("{code}({reason})"),
        _ => code.to_owned(),
    }
}

fn scalar_json(value: Scalar) -> Value {
    match value {
        Scalar::Bool(b) => json!(b),
        Scalar::Int(i) => json!(i),
        Scalar::Uint(u) => json!(u),
        Scalar::Float(f) => json!(f),
    }
}

fn result_json(fields: &[(&str, Reading)]) -> Value {
    let mut out = Map::new();
    for (into, reading) in fields {
        let value = scalar_json(reading.value);
        let entry = if reading.status.code == QualityCode::Good {
            value
        } else {
            json!({"value": value, "quality": status_literal(reading.status)})
        };
        out.insert((*into).to_owned(), entry);
    }
    Value::Object(out)
}

/// A CBOR item as JSON: maps with text keys, arrays, text, numbers,
/// booleans and null. `None` for anything else.
pub fn cbor_json(item: Cbor<'_>) -> Option<Value> {
    if item.is_null() {
        return Some(Value::Null);
    }
    if let Ok(entries) = item.entries() {
        let mut out = Map::new();
        for entry in entries {
            let (k, v) = entry.ok()?;
            out.insert(k.to_owned(), cbor_json(v)?);
        }
        return Some(Value::Object(out));
    }
    if let Ok(items) = item.array() {
        return items
            .map(|i| i.ok().and_then(cbor_json))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array);
    }
    if let Ok(b) = item.as_bool() {
        return Some(json!(b));
    }
    if let Ok(u) = item.as_u64() {
        return Some(json!(u));
    }
    if let Ok(i) = item.as_i64() {
        return Some(json!(i));
    }
    if let Ok(f) = item.as_f64() {
        return Some(json!(f));
    }
    item.as_str().ok().map(|s| json!(s))
}

fn batch_detail(failure: BatchFailure) -> String {
    match failure {
        BatchFailure::Timeout => "the device did not answer in time".to_owned(),
        BatchFailure::CommFailure(m) => format!("communication failure: {m}"),
        BatchFailure::DeviceFault(m) => format!("the device answered with a fault: {m}"),
        BatchFailure::NotConnected => "the device is not connected".to_owned(),
        BatchFailure::ConfigError(m) => format!("the step cannot be built: {m}"),
    }
}

fn project(failure: Failure<'_>) -> ActionOutcome {
    match failure {
        Failure::TimedOut => {
            ActionOutcome::failed(FailureKind::Timeout, "an `until` step never saw its value")
        }
        Failure::Device(BatchFailure::NotConnected) => ActionOutcome::failed(
            FailureKind::NotConnected,
            batch_detail(BatchFailure::NotConnected),
        ),
        Failure::Device(f @ BatchFailure::ConfigError(_)) => {
            ActionOutcome::failed(FailureKind::Unsupported, batch_detail(f))
        }
        Failure::Device(f) => ActionOutcome::failed(FailureKind::Device, batch_detail(f)),
        Failure::Quality(status) => ActionOutcome::failed(
            FailureKind::Device,
            format!("a step read a value of quality {}", status_literal(status)),
        ),
        Failure::FailedCondition(message) => ActionOutcome::failed(FailureKind::Invalid, message),
        Failure::Unsupported => ActionOutcome::failed(
            FailureKind::Unsupported,
            "a step the interpreter cannot resolve (a non-scalar type, a literal that does not fit its type, or a missing argument)",
        ),
        Failure::Cancelled => ActionOutcome::failed(FailureKind::Cancelled, "cancelled"),
    }
}

fn project_native(failure: ActionFailure) -> ActionOutcome {
    match failure {
        ActionFailure::Timeout => {
            ActionOutcome::failed(FailureKind::Timeout, "the driver did not answer in time")
        }
        ActionFailure::Device(e) => ActionOutcome::failed(FailureKind::Device, format!("{e:?}")),
        ActionFailure::Invalid(e) => ActionOutcome::failed(FailureKind::Invalid, format!("{e:?}")),
        ActionFailure::NotConnected => {
            ActionOutcome::failed(FailureKind::NotConnected, "the device is not connected")
        }
        ActionFailure::Unsupported => {
            ActionOutcome::failed(FailureKind::Unsupported, "the driver has no such action")
        }
        ActionFailure::Cancelled => ActionOutcome::failed(FailureKind::Cancelled, "cancelled"),
    }
}

/// Answers `call` without running it: the device is not connected.
pub fn not_connected(call: ActionCall) {
    let _ = call.reply.send(ActionOutcome::failed(
        FailureKind::NotConnected,
        "the device is not connected",
    ));
}

static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Runs `call` against `driver` on `host`'s clock. `step_timeout_ns` is
/// the device's `timeout`, the bound of every driver call of a step.
pub async fn run<D: Driver>(
    driver: &mut D,
    host: &dyn Host,
    step_timeout_ns: u64,
    call: ActionCall,
) {
    let ticket = Arc::clone(&call.ticket);
    if ticket.cancelled() {
        let _ = call.reply.send(ActionOutcome::failed(
            FailureKind::Cancelled,
            "cancelled before it ran",
        ));
        return;
    }
    ticket.start(crate::wire::utc_now_ns());
    let deadline = host.now().mono_ns.saturating_add(call.timeout_ns);
    let guard = Cx::new(host, deadline);
    let call_host = CallHost {
        inner: host,
        ticket: &ticket,
    };
    let mut cx = Cx::new(&call_host, deadline);
    let outcome = match &call.program {
        ActionProgram::Steps(steps) => {
            let views: Vec<_> = steps.iter().map(DeviceStep::view).collect();
            let pairs: Vec<(&str, Scalar)> =
                call.args.iter().map(|(n, v)| (n.as_str(), *v)).collect();
            let args = ArgList(&pairs);
            let invocation = Call {
                device: 0,
                args: &args,
                step_timeout_ns,
            };
            let mut record = Record {
                fields: Vec::new(),
                ticket: &ticket,
            };
            let ran = guard
                .within(joker_hal::interpreter::run_into(
                    driver,
                    &mut cx,
                    &invocation,
                    &views,
                    &mut record,
                ))
                .await;
            match ran {
                Err(_) => {
                    ActionOutcome::failed(FailureKind::Timeout, "the action exceeded its timeout")
                }
                Ok(Ok(())) => ActionOutcome::Ok(result_json(&record.fields)),
                Ok(Err(f)) => project(f),
            }
        }
        ActionProgram::Native { id, args } => {
            let mut buf = vec![0u8; 4096];
            let mut out = ActionOut::new(&mut buf);
            let native = NativeCall {
                call: CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                action: *id,
                args: Cbor::new(args),
            };
            let ran = guard
                .within(driver.invoke(&mut cx, &native, &mut out))
                .await;
            if let Some((permille, message)) = cx.reported_progress() {
                ticket.set_progress(permille, message);
            }
            match ran {
                Err(_) => {
                    ActionOutcome::failed(FailureKind::Timeout, "the action exceeded its timeout")
                }
                Ok(Err(f)) => project_native(f),
                Ok(Ok(())) => {
                    let record = out.as_cbor();
                    if record.as_bytes().is_empty() {
                        ActionOutcome::Ok(json!({}))
                    } else {
                        match cbor_json(record) {
                            Some(v) => ActionOutcome::Ok(v),
                            None => ActionOutcome::failed(
                                FailureKind::Device,
                                "the driver returned a result that is not a CBOR record",
                            ),
                        }
                    }
                }
            }
        }
    };
    let _ = call.reply.send(outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_kind_has_its_documented_code() {
        assert_eq!(FailureKind::Timeout.api(), ("timeout", "J-E1406"));
        assert_eq!(FailureKind::Device.api(), ("device", "J-E1407"));
        assert_eq!(FailureKind::Invalid.api(), ("invalid", "J-E1408"));
        assert_eq!(FailureKind::Unsupported.api(), ("device", "J-E1408"));
        assert_eq!(FailureKind::NotConnected.api(), ("device", "J-E1415"));
        assert_eq!(FailureKind::Cancelled.api(), ("cancelled", "J-E1409"));
    }

    #[test]
    fn a_quality_failure_is_a_device_failure_naming_the_quality() {
        let outcome = project(Failure::Quality(RawStatus::bad(joker_hal::Reason(12))));
        let ActionOutcome::Failed { kind, detail } = outcome else {
            panic!("expected a failure");
        };
        assert_eq!(kind, FailureKind::Device);
        assert!(detail.contains("bad("), "{detail}");
    }

    #[test]
    fn a_fail_if_is_invalid_with_its_message() {
        assert_eq!(
            project(Failure::FailedCondition("already faulted")),
            ActionOutcome::failed(FailureKind::Invalid, "already faulted")
        );
    }

    #[test]
    fn a_config_error_of_a_step_is_unsupported_not_a_comm_failure() {
        let ActionOutcome::Failed { kind, .. } = project(Failure::Device(
            BatchFailure::ConfigError("raw-access not supported"),
        )) else {
            panic!("expected a failure");
        };
        assert_eq!(kind, FailureKind::Unsupported);
    }

    #[test]
    fn an_uncertain_reading_keeps_its_quality_in_the_result() {
        let fields = [
            (
                "a",
                Reading {
                    value: Scalar::Uint(7),
                    status: RawStatus::OK,
                },
            ),
            (
                "b",
                Reading {
                    value: Scalar::Float(1.5),
                    status: RawStatus::uncertain(joker_hal::Reason(3)),
                },
            ),
        ];
        let v = result_json(&fields);
        assert_eq!(v["a"], json!(7));
        assert_eq!(v["b"]["value"], json!(1.5));
        assert!(
            v["b"]["quality"]
                .as_str()
                .is_some_and(|q| q.starts_with("uncertain")),
            "{v}"
        );
    }

    #[test]
    fn a_cbor_record_reads_as_json() {
        use joker_hal::cbor::Value as C;
        let bytes = C::map([
            ("n", C::Int(-3)),
            ("ok", C::Bool(true)),
            ("list", C::Array(vec![C::text("x")])),
        ])
        .encode();
        let v = cbor_json(Cbor::new(&bytes)).expect("json");
        assert_eq!(v, json!({"n": -3, "ok": true, "list": ["x"]}));
    }
}
