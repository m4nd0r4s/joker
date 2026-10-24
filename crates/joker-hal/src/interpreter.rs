//! Walks a compiled [`Step`](crate::steps::Step) program against a
//! driver's `read-addr`/`write-addr` (ref/06 §2.10, ADR 0058).
//!
//! Every value a step reads is a sample with a quality, and the step
//! honours it (ADR 0060, document 16 §3.2): a `Bad` read ends the program
//! with [`Failure::Quality`] and its raw value is never compared or
//! stored; an `Uncertain` read ends a `fail_if` the same way, is not a
//! match for `until`, and enters the result of `read` with its flag.
//!
//! Comparisons (`until`, `fail_if`) and `read`'s `into` only support the
//! scalar raw kinds (`bool`, `int`, `uint`, `float`); `Regs`/`Bytes`/`Text`
//! are out of scope here. `StepValue::Arg` (a `"$name"` substitution) is
//! not resolved yet: a step that needs it fails with
//! [`Failure::Unsupported`]. Both are open follow-ups, not a silent
//! limitation of the step language itself.

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
/// result record, field by field (ref/03 §2.2.12 `into`).
pub trait Results<'a> {
    /// Stores `reading` under the result field `into`.
    fn record(&mut self, into: &'a str, reading: Reading);
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
    /// A borrowed raw kind (`Regs`/`Bytes`/`Text`) or a `"$name"`
    /// argument: not resolved by this interpreter yet.
    Unsupported,
}

struct Capture(Option<(RawStatus, Option<Scalar>)>);

impl SampleSink for Capture {
    fn accept(&mut self, sample: &RawSample<'_>) -> Result<(), Full> {
        self.0 = Some((sample.status, Scalar::of(sample.value)));
        Ok(())
    }
}

async fn read_scalar<D: Driver>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    device: DeviceIx,
    addr: crate::Cbor<'_>,
    type_name: &str,
    expect: RawKind,
) -> Result<Reading, Failure<'static>> {
    let mut capture = Capture(None);
    let req = AddrRead {
        device,
        addr,
        expect,
        width: 1,
        type_name,
    };
    driver
        .read_addr(cx, &req, &mut RawSink::new(&mut capture))
        .await
        .map_err(Failure::Device)?;
    let (status, value) = capture.0.ok_or(Failure::Unsupported)?;
    if status.code == QualityCode::Bad {
        return Err(Failure::Quality(status));
    }
    let value = value.ok_or(Failure::Unsupported)?;
    Ok(Reading { value, status })
}

/// Runs `program` against `driver`'s `device`, in order, stopping at the
/// first failure, for a program whose `read` results nobody needs (ADR
/// 0058 §4). An invoked action uses [`run_into`], so its result record
/// is filled.
pub async fn run<'a, D: Driver>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    device: DeviceIx,
    program: &'a [Step<'a>],
) -> Result<(), Failure<'a>> {
    run_into(driver, cx, device, program, &mut NoResults).await
}

/// Runs `program` against `driver`'s `device`, in order, stopping at the
/// first failure; every `read` step stores its value and quality in
/// `results` under its `into`.
pub async fn run_into<'a, D: Driver, R: Results<'a>>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    device: DeviceIx,
    program: &'a [Step<'a>],
    results: &mut R,
) -> Result<(), Failure<'a>> {
    for step in program {
        match step {
            Step::Write {
                addr,
                type_name,
                value,
            } => {
                let StepValue::Literal(value) = value else {
                    return Err(Failure::Unsupported);
                };
                let req = AddrWrite {
                    device,
                    addr: *addr,
                    value: *value,
                    type_name,
                };
                driver.write_addr(cx, &req).await.map_err(Failure::Device)?;
            }
            Step::Read {
                addr,
                type_name,
                into,
            } => {
                let reading =
                    read_scalar(driver, cx, device, *addr, type_name, RawKind::Float).await?;
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
                let target = Scalar::of(*equals).ok_or(Failure::Unsupported)?;
                let deadline = cx.now().mono_ns + timeout.as_nanos();
                loop {
                    let got =
                        read_scalar(driver, cx, device, *addr, type_name, target.kind()).await?;
                    if got.status.code == QualityCode::Good && got.value == target {
                        break;
                    }
                    let now = cx.now().mono_ns;
                    if now >= deadline {
                        return Err(Failure::TimedOut);
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
                let target = Scalar::of(*equals).ok_or(Failure::Unsupported)?;
                let got = read_scalar(driver, cx, device, *addr, type_name, target.kind()).await?;
                if got.status.code != QualityCode::Good {
                    return Err(Failure::Quality(got.status));
                }
                if got.value == target {
                    return Err(Failure::FailedCondition(message));
                }
            }
        }
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
            _cx: &mut Cx<'_>,
            _req: &AddrRead<'_>,
            out: &mut RawSink<'_>,
        ) -> Result<(), BatchFailure> {
            self.reads += 1;
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
        let result = host
            .run(run_into(driver, &mut cx, 0u16, program, &mut results))
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
}
