//! Walks a compiled [`Step`](crate::steps::Step) program against a
//! driver's `read-addr`/`write-addr` (ref/06 §2.10, ADR 0058, ADR 0066).
//!
//! Every value a step reads is a sample with a quality, and the step
//! honours it (ADR 0060, document 16 §3.2): a `Bad` read ends the program
//! with [`Failure::Quality`] and its raw value is never compared or
//! stored; an `Uncertain` read ends a `fail_if` the same way, is not a
//! match for `until`, and enters the result of `read` with its flag.
//!
//! A step's `type` decides the raw kind it reads and the kind a literal
//! is written as; comparisons are numeric across `int`, `uint` and
//! `float`. `"$name"` takes the argument's value from [`Args`]; one the
//! invocation does not carry is [`Failure::Unsupported`] (ADR 0060 §7).
//! The interpreter checks for cancellation before each step (ADR 0066
//! §9) and bounds every driver call by the step timeout (ref/06 §2.14);
//! the program as a whole is bounded by its caller.

use joker_units::Duration;

use crate::steps::{Step, StepValue};
use crate::{
    AddrRead, AddrWrite, BatchFailure, Cx, DeviceIx, Driver, Full, QualityCode, RawKind, RawSample,
    RawSink, RawStatus, RawValue, SampleSink,
};

/// A scalar captured from a sample, or written as a literal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scalar {
    /// A `bool`.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    Uint(u64),
    /// A float.
    Float(f64),
}

impl Scalar {
    fn of(value: RawValue<'_>) -> Option<Self> {
        match value {
            RawValue::Bool(b) => Some(Self::Bool(b)),
            RawValue::Int(i) => Some(Self::Int(i)),
            RawValue::Uint(u) => Some(Self::Uint(u)),
            RawValue::Float(f) => Some(Self::Float(f)),
            RawValue::None | RawValue::Regs(_) | RawValue::Bytes(_) | RawValue::Text(_) => None,
        }
    }

    fn kind(self) -> RawKind {
        match self {
            Self::Bool(_) => RawKind::Bool,
            Self::Int(_) => RawKind::Int,
            Self::Uint(_) => RawKind::Uint,
            Self::Float(_) => RawKind::Float,
        }
    }

    /// Equality by value: `int`, `uint` and `float` compare as numbers,
    /// a `bool` only with a `bool`.
    pub fn matches(self, other: Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Bool(_), _) | (_, Self::Bool(_)) => false,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Uint(a), Self::Uint(b)) => a == b,
            (Self::Int(a), Self::Uint(b)) | (Self::Uint(b), Self::Int(a)) => {
                u64::try_from(a).is_ok_and(|a| a == b)
            }
            (Self::Float(a), b) | (b, Self::Float(a)) => b.as_f64() == Some(a),
        }
    }

    fn as_f64(self) -> Option<f64> {
        match self {
            Self::Bool(_) => None,
            Self::Int(i) => Some(i as f64),
            Self::Uint(u) => Some(u as f64),
            Self::Float(f) => Some(f),
        }
    }

    /// This value as `kind`, when it fits exactly.
    fn coerce(self, kind: RawKind) -> Option<Self> {
        match (self, kind) {
            (s, k) if s.kind() == k => Some(s),
            (Self::Int(i), RawKind::Uint) => u64::try_from(i).ok().map(Self::Uint),
            (Self::Uint(u), RawKind::Int) => i64::try_from(u).ok().map(Self::Int),
            (Self::Int(i), RawKind::Float) => Some(Self::Float(i as f64)),
            (Self::Uint(u), RawKind::Float) => Some(Self::Float(u as f64)),
            (Self::Float(f), RawKind::Int)
                if (-9.0e15..9.0e15).contains(&f) && (f as i64) as f64 == f =>
            {
                Some(Self::Int(f as i64))
            }
            (Self::Float(f), RawKind::Uint)
                if (0.0..9.0e15).contains(&f) && (f as u64) as f64 == f =>
            {
                Some(Self::Uint(f as u64))
            }
            _ => None,
        }
    }

    fn raw(self) -> RawValue<'static> {
        match self {
            Self::Bool(b) => RawValue::Bool(b),
            Self::Int(i) => RawValue::Int(i),
            Self::Uint(u) => RawValue::Uint(u),
            Self::Float(f) => RawValue::Float(f),
        }
    }
}

/// The raw kind a step's `type` reads and writes (ref/03 §2.2.7):
/// `bool`, the signed and unsigned integers, and the floats. `None` for
/// an empty or other `type`, when the literal's own kind decides.
pub fn kind_of_type(type_name: &str) -> Option<RawKind> {
    match type_name {
        "bool" | "bit" | "coil" => Some(RawKind::Bool),
        "i8" | "i16" | "i32" | "i64" => Some(RawKind::Int),
        "u8" | "u16" | "u32" | "u64" => Some(RawKind::Uint),
        "f32" | "f64" => Some(RawKind::Float),
        _ => None,
    }
}

/// One value a `read` step stored, with the quality the equipment
/// reported for it (`Good` or `Uncertain`; a `Bad` read never gets here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// The value.
    pub value: Scalar,
    /// Its quality.
    pub status: RawStatus,
}

/// Where a program's `read` steps store their values: the action's
/// result record, field by field (ref/03 §2.2.12 `into`), and where its
/// progress goes.
pub trait Results<'a> {
    /// Stores `reading` under the result field `into`.
    fn record(&mut self, into: &'a str, reading: Reading);

    /// `done` of `total` steps have completed (ADR 0066 §10).
    fn progress(&mut self, done: usize, total: usize) {
        let _ = (done, total);
    }
}

/// Discards every `read` result: for the programs that produce none
/// (`[connect]`, `[relinquish]`, `[config] apply` and the fallback, ADR
/// 0058 §4).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoResults;

impl<'a> Results<'a> for NoResults {
    fn record(&mut self, _into: &'a str, _reading: Reading) {}
}

#[cfg(feature = "alloc")]
impl<'a> Results<'a> for alloc::vec::Vec<(&'a str, Reading)> {
    fn record(&mut self, into: &'a str, reading: Reading) {
        self.push((into, reading));
    }
}

/// The arguments of one invocation, by name (ref/03 §2.2.12 `"$name"`).
pub trait Args: crate::cx::MaybeSync {
    /// The value of the argument `name`, already converted to its
    /// declared type (ADR 0066 §5).
    fn arg(&self, name: &str) -> Option<Scalar>;
}

/// An invocation without arguments.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoArgs;

impl Args for NoArgs {
    fn arg(&self, _name: &str) -> Option<Scalar> {
        None
    }
}

/// Arguments as a list of `(name, value)` pairs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArgList<'x>(pub &'x [(&'x str, Scalar)]);

impl Args for ArgList<'_> {
    fn arg(&self, name: &str) -> Option<Scalar> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

/// What one invocation of a program brings besides its steps.
#[derive(Clone, Copy)]
pub struct Call<'x> {
    /// The device the steps address.
    pub device: DeviceIx,
    /// Its arguments.
    pub args: &'x dyn Args,
    /// The bound of each `read-addr`/`write-addr`, from the start of the
    /// call: the device's `timeout` (ref/06 §2.14).
    pub step_timeout_ns: u64,
}

impl core::fmt::Debug for Call<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Call")
            .field("device", &self.device)
            .field("step_timeout_ns", &self.step_timeout_ns)
            .finish_non_exhaustive()
    }
}

/// Why a program did not run to completion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Failure<'a> {
    /// `read-addr`/`write-addr` failed.
    Device(BatchFailure),
    /// An `until` step never saw `equals` with `Good` quality before its
    /// `timeout`.
    TimedOut,
    /// A `fail_if` step saw `equals`.
    FailedCondition(&'a str),
    /// A step read a sample it cannot trust (ADR 0060): `Bad` in any
    /// step, or `Uncertain` in `fail_if`. Projected onto
    /// `device(error)`, naming the reason.
    Quality(RawStatus),
    /// A borrowed raw kind (`Regs`/`Bytes`/`Text`), a literal that does
    /// not fit the step's `type`, or a `"$name"` the invocation does not
    /// carry.
    Unsupported,
    /// The core cancelled the invocation before the next step (ADR 0066
    /// §9).
    Cancelled,
}

struct Capture(Option<(RawStatus, Option<Scalar>)>);

impl SampleSink for Capture {
    fn accept(&mut self, sample: &RawSample<'_>) -> Result<(), Full> {
        self.0 = Some((sample.status, Scalar::of(sample.value)));
        Ok(())
    }
}

fn step_cx<'h>(cx: &Cx<'h>, call: &Call<'_>) -> Cx<'h> {
    cx.narrowed(cx.now().mono_ns.saturating_add(call.step_timeout_ns))
}

async fn read_scalar<D: Driver>(
    driver: &mut D,
    cx: &Cx<'_>,
    call: &Call<'_>,
    addr: crate::Cbor<'_>,
    type_name: &str,
    expect: RawKind,
) -> Result<Reading, Failure<'static>> {
    let mut capture = Capture(None);
    let req = AddrRead {
        device: call.device,
        addr,
        expect,
        width: 1,
        type_name,
    };
    let mut scx = step_cx(cx, call);
    driver
        .read_addr(&mut scx, &req, &mut RawSink::new(&mut capture))
        .await
        .map_err(Failure::Device)?;
    let (status, value) = capture.0.ok_or(Failure::Unsupported)?;
    if status.code == QualityCode::Bad {
        return Err(Failure::Quality(status));
    }
    let value = value.ok_or(Failure::Unsupported)?;
    Ok(Reading { value, status })
}

fn target(equals: RawValue<'_>, type_name: &str) -> Result<(Scalar, RawKind), Failure<'static>> {
    let literal = Scalar::of(equals).ok_or(Failure::Unsupported)?;
    let kind = kind_of_type(type_name).unwrap_or(literal.kind());
    let target = literal.coerce(kind).ok_or(Failure::Unsupported)?;
    Ok((target, kind))
}

/// Runs `program` against `driver`'s `device`, in order, stopping at the
/// first failure, for a program whose `read` results nobody needs and
/// that takes no arguments (ADR 0058 §4). An invoked action uses
/// [`run_into`], so its result record is filled.
pub async fn run<'a, D: Driver>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    device: DeviceIx,
    program: &'a [Step<'a>],
) -> Result<(), Failure<'a>> {
    let call = Call {
        device,
        args: &NoArgs,
        step_timeout_ns: cx.deadline().saturating_sub(cx.now().mono_ns),
    };
    run_into(driver, cx, &call, program, &mut NoResults).await
}

/// Runs `program` against `driver`, in order, stopping at the first
/// failure; every `read` step stores its value and quality in `results`
/// under its `into`, and `results` hears of every completed step.
pub async fn run_into<'a, D: Driver, R: Results<'a>>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    call: &Call<'_>,
    program: &'a [Step<'a>],
    results: &mut R,
) -> Result<(), Failure<'a>> {
    for (done, step) in program.iter().enumerate() {
        if cx.cancelled() {
            return Err(Failure::Cancelled);
        }
        match step {
            Step::Write {
                addr,
                type_name,
                value,
            } => {
                let given = match value {
                    StepValue::Literal(v) => *v,
                    StepValue::Arg(name) => call.args.arg(name).ok_or(Failure::Unsupported)?.raw(),
                };
                let value = match (Scalar::of(given), kind_of_type(type_name)) {
                    (Some(s), Some(kind)) => s.coerce(kind).ok_or(Failure::Unsupported)?.raw(),
                    _ => given,
                };
                let req = AddrWrite {
                    device: call.device,
                    addr: *addr,
                    value,
                    type_name,
                };
                let mut scx = step_cx(cx, call);
                driver
                    .write_addr(&mut scx, &req)
                    .await
                    .map_err(Failure::Device)?;
            }
            Step::Read {
                addr,
                type_name,
                into,
            } => {
                let expect = kind_of_type(type_name).unwrap_or(RawKind::Float);
                let reading = read_scalar(driver, cx, call, *addr, type_name, expect).await?;
                results.record(into, reading);
            }
            Step::Wait(d) => {
                let now = cx.now().mono_ns;
                cx.sleep_until(now + d.as_nanos()).await;
            }
            Step::Until {
                addr,
                type_name,
                equals,
                timeout,
            } => {
                let (target, kind) = target(*equals, type_name)?;
                let deadline = cx.now().mono_ns + timeout.as_nanos();
                loop {
                    let got = read_scalar(driver, cx, call, *addr, type_name, kind).await?;
                    if got.status.code == QualityCode::Good && got.value.matches(target) {
                        break;
                    }
                    let now = cx.now().mono_ns;
                    if now >= deadline {
                        return Err(Failure::TimedOut);
                    }
                    if cx.cancelled() {
                        return Err(Failure::Cancelled);
                    }
                    let poll_ns = Duration::from_millis(20).as_nanos();
                    cx.sleep_until((now + poll_ns).min(deadline)).await;
                }
            }
            Step::FailIf {
                addr,
                type_name,
                equals,
                message,
            } => {
                let (target, kind) = target(*equals, type_name)?;
                let got = read_scalar(driver, cx, call, *addr, type_name, kind).await?;
                if got.status.code != QualityCode::Good {
                    return Err(Failure::Quality(got.status));
                }
                if got.value.matches(target) {
                    return Err(Failure::FailedCondition(message));
                }
            }
        }
        results.progress(done + 1, program.len());
    }
    Ok(())
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::testing::VirtualHost;
    use crate::{ConnectOut, DriverError, Link, OpenArgs, ReadBatch};

    #[derive(Debug, Default)]
    struct Fake {
        register: u16,
        writes: Vec<u16>,
        status: Option<RawStatus>,
        valueless: bool,
        text: bool,
        reads: usize,
        deadlines: Vec<u64>,
    }

    impl Driver for Fake {
        fn open(_args: &OpenArgs<'_>, _link: Link) -> Result<Self, DriverError> {
            Ok(Self::default())
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
            cx: &mut Cx<'_>,
            _req: &AddrRead<'_>,
            out: &mut RawSink<'_>,
        ) -> Result<(), BatchFailure> {
            self.reads += 1;
            self.deadlines.push(cx.deadline());
            let value = if self.valueless {
                RawValue::None
            } else if self.text {
                RawValue::Text("on")
            } else {
                RawValue::Uint(u64::from(self.register))
            };
            let mut sample = RawSample::good(0u32, value);
            if let Some(status) = self.status {
                sample.status = status;
            }
            let _ = out.push(sample);
            Ok(())
        }

        async fn write_addr(
            &mut self,
            _cx: &mut Cx<'_>,
            req: &AddrWrite<'_>,
        ) -> Result<(), BatchFailure> {
            if let RawValue::Uint(v) = req.value {
                self.register = v as u16;
                self.writes.push(self.register);
            }
            Ok(())
        }
    }

    fn write_step(value: u64) -> Step<'static> {
        Step::Write {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            value: StepValue::Literal(RawValue::Uint(value)),
        }
    }

    #[test]
    fn a_write_step_reaches_the_driver() {
        let host = VirtualHost::new(0);
        let host = alloc::sync::Arc::new(host);
        let mut cx = Cx::new(&*host, host.mono() + 1_000_000_000);
        let mut driver = Fake::default();
        host.run(run(&mut driver, &mut cx, 0u16, &[write_step(7)]))
            .unwrap()
            .unwrap();
        assert_eq!(driver.writes, [7]);
    }

    #[test]
    fn fail_if_stops_the_program_with_its_message() {
        let host = VirtualHost::new(0);
        let host = alloc::sync::Arc::new(host);
        let mut cx = Cx::new(&*host, host.mono() + 1_000_000_000);
        let mut driver = Fake {
            register: 9,
            ..Fake::default()
        };
        let program = [
            Step::FailIf {
                addr: crate::Cbor::new(&[]),
                type_name: "u16",
                equals: RawValue::Uint(9),
                message: "already faulted",
            },
            write_step(1),
        ];
        let result = host.run(run(&mut driver, &mut cx, 0u16, &program)).unwrap();
        assert_eq!(result, Err(Failure::FailedCondition("already faulted")));
        assert!(driver.writes.is_empty());
    }

    #[test]
    fn until_times_out_when_the_value_never_matches() {
        let host = VirtualHost::new(0);
        let host = alloc::sync::Arc::new(host);
        let mut cx = Cx::new(&*host, host.mono() + 10_000_000_000);
        let mut driver = Fake::default();
        let program = [Step::Until {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            equals: RawValue::Uint(42),
            timeout: Duration::from_millis(100),
        }];
        let result = host.run(run(&mut driver, &mut cx, 0u16, &program)).unwrap();
        assert_eq!(result, Err(Failure::TimedOut));
    }

    #[test]
    fn until_succeeds_once_a_write_makes_the_value_match() {
        let host = VirtualHost::new(0);
        let host = alloc::sync::Arc::new(host);
        let mut cx = Cx::new(&*host, host.mono() + 10_000_000_000);
        let mut driver = Fake {
            register: 42,
            ..Fake::default()
        };
        let program = [Step::Until {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            equals: RawValue::Uint(42),
            timeout: Duration::from_millis(500),
        }];
        host.run(run(&mut driver, &mut cx, 0u16, &program))
            .unwrap()
            .unwrap();
    }

    const BAD: RawStatus = RawStatus::bad(crate::Reason(12));
    const UNCERTAIN: RawStatus = RawStatus::uncertain(crate::Reason(3));

    fn until(equals: u64, timeout_ms: u64) -> Step<'static> {
        Step::Until {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            equals: RawValue::Uint(equals),
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    fn fail_if(equals: u64) -> Step<'static> {
        Step::FailIf {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            equals: RawValue::Uint(equals),
            message: "already faulted",
        }
    }

    fn read(into: &'static str) -> Step<'static> {
        Step::Read {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            into,
        }
    }

    #[allow(
        clippy::unwrap_used,
        reason = "a test helper; a host failure fails the test"
    )]
    fn run_program(
        driver: &mut Fake,
        program: &[Step<'static>],
    ) -> (Result<(), Failure<'static>>, Vec<(&'static str, Reading)>) {
        let host = alloc::sync::Arc::new(VirtualHost::new(0));
        let mut cx = Cx::new(&*host, host.mono() + 10_000_000_000);
        let program: &'static [Step<'static>] = alloc::boxed::Box::leak(program.into());
        let mut results = Vec::new();
        let call = Call {
            device: 0,
            args: &NoArgs,
            step_timeout_ns: 1_000_000_000,
        };
        let result = host
            .run(run_into(driver, &mut cx, &call, program, &mut results))
            .unwrap();
        (result, results)
    }

    #[test]
    fn a_bad_read_never_satisfies_until_even_when_its_raw_value_matches() {
        let mut driver = Fake {
            register: 42,
            status: Some(BAD),
            ..Fake::default()
        };
        let (result, _) = run_program(&mut driver, &[until(42, 100)]);
        assert_eq!(result, Err(Failure::Quality(BAD)));
    }

    #[test]
    fn a_bad_read_in_fail_if_ends_the_action_with_its_quality_whatever_the_value() {
        for register in [9, 1] {
            let mut driver = Fake {
                register,
                status: Some(BAD),
                ..Fake::default()
            };
            let (result, _) = run_program(&mut driver, &[fail_if(9), write_step(1)]);
            assert_eq!(result, Err(Failure::Quality(BAD)));
            assert!(driver.writes.is_empty());
        }
    }

    #[test]
    fn a_bad_read_without_a_value_is_a_quality_failure_not_unsupported() {
        let mut driver = Fake {
            status: Some(BAD),
            valueless: true,
            ..Fake::default()
        };
        let (result, _) = run_program(&mut driver, &[until(42, 100)]);
        assert_eq!(result, Err(Failure::Quality(BAD)));
    }

    #[test]
    fn an_uncertain_match_does_not_satisfy_until() {
        let mut driver = Fake {
            register: 42,
            status: Some(UNCERTAIN),
            ..Fake::default()
        };
        let (result, _) = run_program(&mut driver, &[until(42, 100)]);
        assert_eq!(result, Err(Failure::TimedOut));
        assert!(driver.reads > 1);
    }

    #[test]
    fn an_uncertain_read_in_fail_if_ends_the_action_with_its_quality() {
        let mut driver = Fake {
            register: 1,
            status: Some(UNCERTAIN),
            ..Fake::default()
        };
        let (result, _) = run_program(&mut driver, &[fail_if(9), write_step(1)]);
        assert_eq!(result, Err(Failure::Quality(UNCERTAIN)));
        assert!(driver.writes.is_empty());
    }

    #[test]
    fn a_read_step_stores_its_value_and_quality_in_the_results() {
        let mut driver = Fake {
            register: 7,
            ..Fake::default()
        };
        let (result, results) = run_program(&mut driver, &[read("first")]);
        assert_eq!(result, Ok(()));
        driver.status = Some(UNCERTAIN);
        let (result, more) = run_program(&mut driver, &[read("second")]);
        assert_eq!(result, Ok(()));
        assert_eq!(
            results,
            [(
                "first",
                Reading {
                    value: Scalar::Uint(7),
                    status: RawStatus::OK
                }
            )]
        );
        assert_eq!(
            more,
            [(
                "second",
                Reading {
                    value: Scalar::Uint(7),
                    status: UNCERTAIN
                }
            )]
        );
    }

    #[test]
    fn a_bad_read_step_ends_the_action_and_stores_nothing() {
        let mut driver = Fake {
            register: 7,
            status: Some(BAD),
            ..Fake::default()
        };
        let (result, results) = run_program(&mut driver, &[read("first")]);
        assert_eq!(result, Err(Failure::Quality(BAD)));
        assert!(results.is_empty());
    }

    #[test]
    fn an_unsupported_value_in_until_stays_unsupported() {
        let mut driver = Fake {
            text: true,
            ..Fake::default()
        };
        let (result, _) = run_program(&mut driver, &[until(42, 100)]);
        assert_eq!(result, Err(Failure::Unsupported));
    }

    #[allow(
        clippy::unwrap_used,
        reason = "a test helper; a host failure fails the test"
    )]
    fn run_with(
        host: &VirtualHost,
        driver: &mut Fake,
        program: &[Step<'static>],
        args: &[(&str, Scalar)],
    ) -> (Result<(), Failure<'static>>, Progress) {
        let mut cx = Cx::new(host, host.mono() + 10_000_000_000);
        let program: &'static [Step<'static>] = alloc::boxed::Box::leak(program.into());
        let args = ArgList(args);
        let call = Call {
            device: 0,
            args: &args,
            step_timeout_ns: 1_000_000_000,
        };
        let mut progress = Progress::default();
        let result = host
            .run(run_into(driver, &mut cx, &call, program, &mut progress))
            .unwrap();
        (result, progress)
    }

    #[derive(Debug, Default)]
    struct Progress(Vec<(usize, usize)>);

    impl<'a> Results<'a> for Progress {
        fn record(&mut self, _into: &'a str, _reading: Reading) {}

        fn progress(&mut self, done: usize, total: usize) {
            self.0.push((done, total));
        }
    }

    #[test]
    fn an_argument_is_written_where_the_step_names_it() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let program = [Step::Write {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            value: StepValue::Arg("level"),
        }];
        let (result, _) = run_with(&host, &mut driver, &program, &[("level", Scalar::Uint(12))]);
        assert_eq!(result, Ok(()));
        assert_eq!(driver.writes, [12]);
    }

    #[test]
    fn a_missing_argument_is_unsupported_and_writes_nothing() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let program = [Step::Write {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            value: StepValue::Arg("level"),
        }];
        let (result, _) = run_with(&host, &mut driver, &program, &[]);
        assert_eq!(result, Err(Failure::Unsupported));
        assert!(driver.writes.is_empty());
    }

    #[test]
    fn a_signed_literal_is_written_as_the_unsigned_type_of_its_step() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let program = [Step::Write {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            value: StepValue::Literal(RawValue::Int(5)),
        }];
        let (result, _) = run_with(&host, &mut driver, &program, &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(driver.writes, [5]);
    }

    #[test]
    fn a_negative_literal_does_not_fit_an_unsigned_step() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let program = [Step::Write {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            value: StepValue::Literal(RawValue::Int(-1)),
        }];
        let (result, _) = run_with(&host, &mut driver, &program, &[]);
        assert_eq!(result, Err(Failure::Unsupported));
    }

    #[test]
    fn until_matches_a_signed_literal_against_an_unsigned_register() {
        let host = VirtualHost::new(0);
        let mut driver = Fake {
            register: 3,
            ..Fake::default()
        };
        let program = [Step::Until {
            addr: crate::Cbor::new(&[]),
            type_name: "u16",
            equals: RawValue::Int(3),
            timeout: Duration::from_millis(100),
        }];
        let (result, _) = run_with(&host, &mut driver, &program, &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(driver.reads, 1);
    }

    #[test]
    fn a_cancelled_invocation_stops_before_its_next_step() {
        let host = VirtualHost::new(0);
        host.set_cancelled(true);
        let mut driver = Fake::default();
        let (result, progress) = run_with(&host, &mut driver, &[write_step(1)], &[]);
        assert_eq!(result, Err(Failure::Cancelled));
        assert!(driver.writes.is_empty());
        assert!(progress.0.is_empty());
    }

    #[test]
    fn every_completed_step_reports_progress() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let (result, progress) = run_with(
            &host,
            &mut driver,
            &[
                write_step(1),
                Step::Wait(Duration::from_millis(5)),
                write_step(0),
            ],
            &[],
        );
        assert_eq!(result, Ok(()));
        assert_eq!(progress.0, [(1, 3), (2, 3), (3, 3)]);
    }

    #[test]
    fn each_driver_call_is_bounded_by_the_step_timeout_within_the_program_deadline() {
        let host = VirtualHost::new(0);
        let mut driver = Fake::default();
        let (result, _) = run_with(&host, &mut driver, &[read("first")], &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(driver.deadlines, [1_000_000_000]);
    }

    #[test]
    fn scalars_compare_by_value_across_numeric_kinds() {
        assert!(Scalar::Int(1).matches(Scalar::Uint(1)));
        assert!(Scalar::Uint(2).matches(Scalar::Float(2.0)));
        assert!(!Scalar::Int(-1).matches(Scalar::Uint(u64::MAX)));
        assert!(!Scalar::Bool(true).matches(Scalar::Uint(1)));
        assert!(Scalar::Bool(false).matches(Scalar::Bool(false)));
    }
}
