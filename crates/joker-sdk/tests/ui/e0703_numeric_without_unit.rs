#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input]
    temp: joker::Input<f64>,
}

fn main() {}
