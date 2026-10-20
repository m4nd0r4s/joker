//! Build probe for the Xtensa family (docs/13 §5, §5.1): it links nothing
//! and only re-exports the core crates, so that building it compiles
//! `joker-units`, `joker-expr` and `joker-core` for `xtensa-esp32-none-elf`
//! with the `esp` toolchain, from the same source as every other target.

#![no_std]

pub use joker_core;
pub use joker_expr;
pub use joker_units;
