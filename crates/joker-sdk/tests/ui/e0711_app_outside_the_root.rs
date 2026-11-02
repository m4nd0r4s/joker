mod inner {
    #[joker::app(name = "probe", version = "0.1.0")]
    pub struct Probe {
        #[output]
        led: joker::Output<bool>,
    }
}

fn main() {}
