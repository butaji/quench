//! The codes of a Wasm row, which `build.rs` generates the stencils from (`wasm_library.rs`),
//! the stencils dispatch on (`wasm_rows.rs`) and the stitcher looks a row up by. An operator is
//! its selector tag (`I64BinaryOperator`, `I64UnaryOperator`): the i32 operators are the first
//! tags of their i64 namesakes.

/// A row's kind.
pub const BINARY: u8 = 0;
pub const UNARY: u8 = 1;
/// A conditional branch on a comparison operator.
pub const JUMP_IF: u8 = 2;

/// The width an integer row runs at.
pub const W32: u8 = 0;
pub const W64: u8 = 1;

/// The shape of a row's second operand: a register, or an immediate in the row's hole.
pub const REGISTER: u8 = 0;
pub const IMMEDIATE: u8 = 1;

/// The binary operators: those both widths have, then the i64-only wide products.
pub const SHARED_BINARY: u32 = 25;
pub const BINARY_COUNT: u32 = 27;
/// The comparisons, `Equal` through `GreaterEqualUnsigned`.
pub const FIRST_COMPARISON: u32 = 15;
pub const LAST_COMPARISON: u32 = 24;
/// The unary operators: those both widths have, then the i64-only `ExtendSigned32`.
pub const SHARED_UNARY: u32 = 6;
pub const UNARY_COUNT: u32 = 7;
