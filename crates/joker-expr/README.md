# joker-expr

The expression language of the manifest (docs/02 §10, docs/16 §3.2 and §4, ADR 0034). `no_std`, no allocation.

- [`parse`]: lexer and recursive-descent parser into a fixed-capacity [`ast::Ast`], with spans.
- [`check`]: name resolution against an [`Env`] (the registry), type and unit checking, unit alignment (W to kW, °C to K), selector expansion.
- [`Diagnostic`]: every error carries a span and a registry code (`J0108`, `J0109`, `J0149`, `J0152`–`J0155`, `J0158`, `J0301`–`J0307`), and renders in rustc style.
- [`compile`]: straight-line bytecode for a stack VM, with constants folded, conversions and window capacities fixed for the period of the evaluating group.
- [`eval`]: the VM. Fixed stack, caller-provided state arena (zeroed = initial state), quality propagation with the symmetric short-circuit, `Bad(nan)` for non-finite values, and the window and stateful functions of ADR 0034. It never allocates (the crate has no allocator without the `alloc` feature) and never panics, even on a corrupted state arena.
