#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use joker_manifest::MemoryFileSystem;
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;

pub const JOKER_TOML: &str = r#"
[project]
name  = "bancada"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[cycles.main]
period = "20ms"

[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led" }
params = { half_period = "500 ms" }
cycles = { pisca = "main" }
"#;

pub const SIM_TOML: &str = r#"
extends = ["joker.toml"]

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
"#;

pub fn fs(files: &[(&str, &str)]) -> MemoryFileSystem {
    let mut fs = MemoryFileSystem::default();
    fs.dir("apps/luz");
    for (p, t) in files {
        fs.file(*p, *t);
    }
    fs
}

pub fn bench() -> Loaded {
    let fs = fs(&[("joker.toml", JOKER_TOML), ("joker.sim.toml", SIM_TOML)]);
    match load(&fs, Path::new("joker.sim.toml"), &NativeTasks) {
        Ok(l) => l,
        Err(e) => panic!("{e}"),
    }
}
