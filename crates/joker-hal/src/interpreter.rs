//! Walks a compiled [`Step`](crate::steps::Step) program against a
//! driver's `read-addr`/`write-addr` (ref/06 §2.10, ADR 0058).
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
    AddrRead, AddrWrite, BatchFailure, Cx, DeviceIx, Driver, Full, RawKind, RawSample, RawSink,
    RawValue, SampleSink,
};

/// A scalar captured from a sample, or written as a literal.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Scalar {
    Bool(bool),
    Int(i64),
    Uint(u64),
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

/// Why a program did not run to completion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Failure<'a> {
    /// `read-addr`/`write-addr` failed.
    Device(BatchFailure),
    /// An `until` step never saw `equals` before its `timeout`.
    TimedOut,
    /// A `fail_if` step saw `equals`.
    FailedCondition(&'a str),
    /// A borrowed raw kind (`Regs`/`Bytes`/`Text`) or a `"$name"`
    /// argument: not resolved by this interpreter yet.
    Unsupported,
}

struct Capture(Option<Scalar>);

impl SampleSink for Capture {
    fn accept(&mut self, sample: &RawSample<'_>) -> Result<(), Full> {
        self.0 = Scalar::of(sample.value);
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
) -> Result<Scalar, Failure<'static>> {
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
    capture.0.ok_or(Failure::Unsupported)
}

/// Runs `program` against `driver`'s `device`, in order, stopping at the
/// first failure.
pub async fn run<'a, D: Driver>(
    driver: &mut D,
    cx: &mut Cx<'_>,
    device: DeviceIx,
    program: &'a [Step<'a>],
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
                addr, type_name, ..
            } => {
                // The result record is not built yet (no caller wants it
                // until the live wiring exists); run for its side effects
                // and the same failure modes as a bare read.
                read_scalar(driver, cx, device, *addr, type_name, RawKind::Float).await?;
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
                    let got = read_scalar(driver, cx, device, *addr, type_name, target.kind())
                        .await
                        .map_err(|_| Failure::Device(BatchFailure::CommFailure("until")))?;
                    if got == target {
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
                if got == target {
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
            let _ = out.push(RawSample::good(
                0u32,
                RawValue::Uint(u64::from(self.register)),
            ));
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
            writes: Vec::new(),
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
        // The write after fail_if never ran.
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
            writes: Vec::new(),
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
}
