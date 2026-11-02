//! Task state across a hot swap (ref/04 §4.12), the parameter check of
//! `#[joker::on_params]` (§4.3.4), the `Shared<S>` of an async task, and an
//! async `write` that waits for the commit (§4.6).

use joker::prelude::*;
use joker_test::prelude::*;

#[joker::app(name = "keeper", version = "0.2.0")]
pub struct Keeper {
    #[input(unit = "1")]
    x: Input<f64>,
    #[output(unit = "1")]
    y: Output<f64>,
    #[output(unit = "1")]
    runs: Output<u64>,
    #[output(unit = "1")]
    top: Output<f64>,
    #[output(unit = "1")]
    count: Output<u64>,
    #[param(unit = "1", range = "0..10", default = 1.0)]
    gain: Param<f64>,
}

#[joker::state(persist, version = 2)]
#[derive(Default)]
struct Acc {
    sum: f64,
    ticks: u64,
}

#[joker::state(persist)]
#[derive(Default)]
struct Peak {
    max: f64,
}

#[derive(Default)]
struct Scratch {
    n: u64,
}

#[joker::migrate(Peak)]
fn migrate_peak(prev: StateBlob<'_>, _from: &str) -> Result<Peak> {
    Ok(Peak {
        max: prev.field::<f64>("max").unwrap_or(0.0) + 1000.0,
    })
}

#[joker::cycle(period = "100ms")]
fn acc(io: &mut CycleIo<Keeper>, st: &mut Acc) -> Result<()> {
    st.sum += io.x.get()?;
    st.ticks += 1;
    io.y.set(st.sum * io.gain.get());
    Ok(())
}

#[joker::cycle(period = "100ms")]
fn peak(io: &mut CycleIo<Keeper>, st: &mut Peak) -> Result<()> {
    st.max = st.max.max(io.x.get()?);
    io.top.set(st.max);
    Ok(())
}

#[joker::cycle(period = "100ms")]
fn scratch(io: &mut CycleIo<Keeper>, st: &mut Scratch) -> Result<()> {
    st.n += 1;
    io.count.set(st.n);
    Ok(())
}

#[joker::on_params]
fn check(changes: &ParamChanges) -> Result<()> {
    match changes.get::<f64>("gain") {
        Some(g) if g > 5.0 => Err(Error::app(format_args!("gain {g} is above 5"))),
        _ => Ok(()),
    }
}

#[joker::task(every = "1s")]
async fn count(io: &TaskIo<Keeper>, st: &Shared<u64>) -> Result<()> {
    let n = st.with_mut(|n| {
        *n += 1;
        *n
    });
    io.runs.write(n).await?.require_applied()?;
    Ok(())
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_persisted_state_survives_the_hot_swap_and_the_others_restart() {
    let mut h = Harness::<Keeper>::new().input("x", 1.0);
    h.run_for(ms(1000));
    assert_eq!(h.output("y").last(), Some(10.0));
    h.hot_swap().expect("the swap succeeds");
    h.run_for(ms(100));
    assert_eq!(h.output("y").last(), Some(11.0));
    assert_eq!(h.output("top").last(), Some(1001.0));
    assert_eq!(h.output("count").last(), Some(1u64));
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}

#[test]
fn a_refused_parameter_keeps_its_value() {
    let mut h = Harness::<Keeper>::new().input("x", 1.0);
    h.run_for(ms(200));
    h.set_param("gain", 7.0);
    h.run_for(ms(100));
    assert_eq!(h.output("y").last(), Some(3.0));
    let e = &h.errors()[0];
    assert_eq!(
        (e.task.as_str(), e.code.as_str(), e.message.as_str()),
        ("on_params", "J-E0050", "gain 7 is above 5")
    );
    h.set_param("gain", 2.0);
    h.run_for(ms(100));
    assert_eq!(h.output("y").last(), Some(8.0));
}

#[test]
fn an_async_task_keeps_its_shared_state_and_waits_for_its_writes() {
    let mut h = Harness::<Keeper>::new().input("x", 1.0);
    h.run_for(ms(3500));
    let runs = h.output("runs");
    assert_eq!(runs.at(ms(900)), Some(0u64));
    assert_eq!(runs.at(ms(1100)), Some(1u64));
    assert_eq!(runs.at(ms(3100)), Some(3u64));
    let proposals: Vec<Duration> = runs.proposals().map(|(t, _)| t).collect();
    assert_eq!(proposals, [ms(1100), ms(2100), ms(3100)]);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}
