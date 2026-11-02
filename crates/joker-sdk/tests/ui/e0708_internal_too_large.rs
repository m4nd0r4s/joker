use joker::prelude::*;

#[derive(Record, Clone, Copy, Default)]
pub struct Big {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
}

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[internal]
    big: Internal<Big>,
}

fn main() {}
