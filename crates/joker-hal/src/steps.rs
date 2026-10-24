//! The host-side compiled form of a profile's step program (ref/03
//! §2.2.12, ADR 0058): `write`, `read`, `wait`, `until`, `fail_if`.
//!
//! This only models the *static* shape of a program: the aggregate
//! default timeout ref/01 §5.8.2 falls back to when the manifest does
//! not give one, and the data [`crate::interpreter`] walks (ADR 0058).

use joker_units::Duration;

use crate::Cbor;
use crate::RawValue;

/// A literal, or a named argument substituted at invocation (`"$name"`,
/// ref/03 §2.2.12).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepValue<'a> {
    /// A literal value.
    Literal(RawValue<'a>),
    /// `"$name"`: the value of that argument.
    Arg(&'a str),
}

/// One step of a compiled program. Addresses and values are already
/// validated against the driver's `address` schema (`J0821` happens when
/// the program is built, not here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step<'a> {
    /// Writes a raw value, or a value in the unit of a signal (scaled by
    /// the core before this point).
    Write {
        /// Where.
        addr: Cbor<'a>,
        /// The driver's `type` for this address.
        type_name: &'a str,
        /// What to write.
        value: StepValue<'a>,
    },
    /// Reads and stores the result under `into`.
    Read {
        /// Where.
        addr: Cbor<'a>,
        /// The driver's `type` for this address.
        type_name: &'a str,
        /// The result field this fills.
        into: &'a str,
    },
    /// Waits before the next step.
    Wait(Duration),
    /// Reads until the value matches `equals`, or fails with `timeout`
    /// once `timeout` elapses.
    Until {
        /// Where.
        addr: Cbor<'a>,
        /// The driver's `type` for this address.
        type_name: &'a str,
        /// What it waits to see.
        equals: RawValue<'a>,
        /// How long it waits.
        timeout: Duration,
    },
    /// Ends the action with `message` if the value matches `equals`.
    FailIf {
        /// Where.
        addr: Cbor<'a>,
        /// The driver's `type` for this address.
        type_name: &'a str,
        /// What ends the action.
        equals: RawValue<'a>,
        /// Why.
        message: &'a str,
    },
}

/// The action's default `timeout` when the manifest gives none (ref/03
/// §2.2.12): the sum of every `wait` and `until` budget, plus one second
/// per step.
pub fn default_timeout(steps: &[Step<'_>]) -> Duration {
    let mut nanos: u64 = 0;
    for step in steps {
        nanos += Duration::from_secs(1).as_nanos();
        nanos += match step {
            Step::Wait(d) => d.as_nanos(),
            Step::Until { timeout, .. } => timeout.as_nanos(),
            Step::Write { .. } | Step::Read { .. } | Step::FailIf { .. } => 0,
        };
    }
    Duration::from_nanos(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(addr: Cbor<'_>) -> Step<'_> {
        Step::Write {
            addr,
            type_name: "u16",
            value: StepValue::Literal(RawValue::Uint(1)),
        }
    }

    #[test]
    fn an_empty_program_has_a_zero_default_timeout() {
        assert_eq!(default_timeout(&[]), Duration::ZERO);
    }

    #[test]
    fn each_step_adds_one_second() {
        let addr = Cbor::new(&[]);
        let steps = [write(addr), write(addr), write(addr)];
        assert_eq!(default_timeout(&steps), Duration::from_secs(3));
    }

    #[test]
    fn waits_and_until_budgets_add_to_the_one_second_per_step() {
        let addr = Cbor::new(&[]);
        let steps = [
            write(addr),
            Step::Wait(Duration::from_millis(500)),
            Step::Until {
                addr,
                type_name: "u16",
                equals: RawValue::Uint(1),
                timeout: Duration::from_secs(2),
            },
        ];
        assert_eq!(
            default_timeout(&steps),
            Duration::from_millis(3_000 + 500 + 2_000)
        );
    }
}
