use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "1")]
    soc: Input<f64>,
}

#[joker::task(on = "charge")]
async fn watch(_io: &TaskIo<Probe>, _s: Sample<f64>) -> Result<()> {
    Ok(())
}

fn main() {}
