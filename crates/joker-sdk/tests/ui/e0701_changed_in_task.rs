use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "kW")]
    grid: Input<f64>,
}

#[joker::task(every = "1m")]
async fn watch(io: &TaskIo<Probe>) -> Result<()> {
    let _ = io.grid.changed();
    Ok(())
}

fn main() {}
