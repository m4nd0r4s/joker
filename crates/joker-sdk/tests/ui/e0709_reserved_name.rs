#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "1")]
    now: joker::Input<f64>,
}

fn main() {}
