//! Every compilation error of the SDK (ref/04 §4.16) that the compiler
//! emits has a case in `tests/ui`, with the code, the location on the
//! offending field or argument, and the help line, as the user sees them.
//! `TRYBUILD=overwrite cargo test -p joker-sdk --test ui` rewrites the
//! expected `.stderr` files after a deliberate change of a message.
//!
//! Not here: `J-E0715` (`#[joker::implements]`, which needs the class
//! handles), `J-E0716` (emitted by `jk build` from `joker:meta`, not by the
//! compiler) and `J0720` (the `native-micro` backend).

#[test]
fn sdk_compile_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
