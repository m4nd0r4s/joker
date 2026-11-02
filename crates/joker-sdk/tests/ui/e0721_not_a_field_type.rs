pub struct Opaque;

#[derive(joker::Record)]
pub struct Plan {
    limit: f64,
    extra: Opaque,
}

fn main() {}
