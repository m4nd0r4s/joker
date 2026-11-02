#[joker::app(name = "one", version = "0.1.0")]
pub struct One {
    #[output]
    led: joker::Output<bool>,
}

#[joker::app(name = "two", version = "0.1.0")]
pub struct Two {
    #[output]
    led: joker::Output<bool>,
}

fn main() {}
