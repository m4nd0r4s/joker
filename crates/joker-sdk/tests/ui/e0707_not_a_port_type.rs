pub struct Mode;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[input]
    mode: joker::Input<Mode>,
}

fn main() {}
