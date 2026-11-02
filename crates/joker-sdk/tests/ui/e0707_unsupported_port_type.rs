#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input(unit = "1")]
    count: joker::Input<i32>,
}

fn main() {}
