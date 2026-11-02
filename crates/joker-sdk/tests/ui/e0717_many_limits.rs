pub struct Pump;
pub struct Many<C, const MIN: usize, const MAX: usize>(C);

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[device]
    pumps: Many<Pump, 8, 2>,
}

fn main() {}
