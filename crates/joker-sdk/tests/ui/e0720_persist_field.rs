#[joker::state(persist, version = 2)]
#[derive(Default)]
struct Ctl {
    integral: f64,
    history: Vec<f64>,
}

fn main() {}
