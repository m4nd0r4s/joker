#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[param(unit = "kW", range = "0..5000", default = 6000.0)]
    limit: joker::Param<f64>,
}

fn main() {}
