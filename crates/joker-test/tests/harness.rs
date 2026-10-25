//! The harness drives a native app over the real core: inputs from series,
//! the envelope, the arbiter's level and lease, quality errors, and the
//! same trace in every run (docs/13 task 19, ref/04 §6).

use joker::prelude::*;
use joker_test::prelude::*;

/// Doubles its input into a power setpoint.
#[joker::app(name = "doubler", version = "0.1.0")]
pub struct Doubler {
    /// Measured power.
    #[input(unit = "kW")]
    power: Input<f64>,

    /// Setpoint.
    #[output(unit = "kW")]
    setpoint: Output<f64>,

    /// Gain.
    #[param(unit = "1", range = "0..10", default = 2.0)]
    gain: Param<f64>,
}

#[joker::cycle(period = "100ms")]
fn control(io: &mut CycleIo<Doubler>) -> Result<()> {
    let p = io.power.get()?;
    io.setpoint.set(p * io.gain.get());
    Ok(())
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn the_output_follows_the_input_through_the_envelope() {
    let mut h = Harness::<Doubler>::new()
        .input("power", Series::ramp(0.0, 100.0, ms(1000)))
        .envelope("setpoint", Envelope::new().range(0.0, 150.0));
    h.run_for(ms(2000));
    let out = h.output("setpoint");
    assert_eq!(out.at(ms(500)), Some(100.0));
    assert_eq!(out.last(), Some(150.0));
    assert!(out.all(|v| v <= 150.0));
    let clamped: Vec<Duration> = out
        .outcomes()
        .filter(|(_, o)| {
            matches!(
                o,
                WriteOutcome::Clamped {
                    by: ClampKind::Range,
                    ..
                }
            )
        })
        .map(|(t, _)| t)
        .collect();
    assert_eq!(clamped.first(), Some(&ms(800)));
    assert!(out.levels().all(|(_, l)| l.map(|l| l.get()) == Some(12)));
    assert!(h.errors().is_empty());
}

#[test]
fn a_bad_input_is_an_error_and_the_lease_runs_out_to_safe() {
    let mut h = Harness::<Doubler>::new()
        .input(
            "power",
            Series::constant(10.0).with_quality_at(ms(1000), Quality::bad(CommFailure)),
        )
        .safe("setpoint", 0.0)
        .param("gain", 3.0);
    h.run_for(ms(2000));
    let out = h.output("setpoint");
    assert_eq!(out.at(ms(900)), Some(30.0));
    assert!(out.between(ms(1000), ms(2000)).no_new_writes());
    assert_eq!(out.at(ms(1100)), Some(30.0));
    assert_eq!(out.last(), Some(0.0));
    assert_eq!(out.levels().last().map(|(_, l)| l.is_none()), Some(true));
    let first = &h.errors()[0];
    assert_eq!((first.at, first.code.as_str()), (ms(1000), "J-E0102"));
    assert_eq!(first.task, "control");
}

#[test]
fn the_same_test_gives_the_same_trace() {
    let run = || {
        let mut h = Harness::<Doubler>::new()
            .input("power", Series::sine(50.0, 20.0, ms(700)))
            .envelope("setpoint", Envelope::new().rate(100.0));
        h.run_for(ms(5000));
        h.output("setpoint")
    };
    let first = run();
    assert_eq!(first.values().count(), 50);
    assert_eq!(first, run());
}

#[test]
fn parameters_change_between_ticks() {
    let mut h = Harness::<Doubler>::new().input("power", 1.0);
    h.run_for(ms(300));
    assert_eq!(h.output("setpoint").last(), Some(2.0));
    h.set_param("gain", 5.0);
    h.run_for(ms(100));
    assert_eq!(h.output("setpoint").last(), Some(5.0));
    assert_eq!(h.now(), ms(400));
}
