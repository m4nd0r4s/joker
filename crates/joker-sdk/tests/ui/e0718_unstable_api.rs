pub struct Model;

#[joker::app(name = "probe", version = "0.1.0")]
pub struct Probe {
    #[model(inputs = "f32[1,96,4]", outputs = "f32[1,96]")]
    forecast: Model,
}

fn main() {}
