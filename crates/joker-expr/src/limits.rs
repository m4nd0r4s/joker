/// The longest expression source, in bytes (ref/01 §11.3, `J0149`).
pub const MAX_SOURCE_LEN: usize = 4096;
/// The most syntax tree nodes in one expression.
pub const MAX_NODES: usize = 256;
/// The most call arguments in one expression, over all calls.
pub const MAX_ARGS: usize = 128;
/// The most arguments of a single call.
pub const MAX_CALL_ARGS: usize = 16;
/// The deepest nesting of sub-expressions.
pub const MAX_DEPTH: usize = 48;
/// The most signals a selector may expand to inside one expression.
pub const MAX_ELEMENTS: usize = 64;
