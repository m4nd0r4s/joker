use joker::prelude::*;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "1")]
    soc: Input<f64>,
}

#[joker::task(every = "1m", on = "soc")]
async fn watch(_io: &TaskIo<Probe>) -> Result<()> {
    Ok(())
}

fn main() {}
