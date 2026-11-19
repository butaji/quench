//! The codes of a Wasm row, which `build.rs` generates the stencils from (`wasm_library.rs`),
//! the stencils dispatch on (`wasm_rows.rs`) and the stitcher looks a row up by. An operator is
//! its selector tag (`I64BinaryOperator`, `I64UnaryOperator`): the i32 operators are the first
//! tags of their i64 namesakes.

/// A row's kind.
pub const BINARY: u8 = 0;
pub const UNARY: u8 = 1;
/// A conditional branch on a comparison operator.
pub const JUMP_IF: u8 = 2;
/// `D = A`: a register's word.
pub const MOVE: u8 = 3;
/// `D = ` the word in the row's hole.
pub const CONSTANT: u8 = 4;
/// Continues at the taken successor.
pub const GOTO: u8 = 5;
/// `D = memory[A + B]`, the access the operator code indexes in `ACCESSES`; traps out of bounds.
pub const LOAD: u8 = 6;
/// `memory[A + B] = D`, likewise.
pub const STORE: u8 = 7;
/// Calls the `WasmEntry` in the cell at address B with the arguments from register A on and
/// the result to D; a trap's status leaves. Its row is a `call_` row (`wasm_name`).
pub const CALL: u8 = 8;
/// `D = A op B` on floats (width `W32` is f32), an `F64BinaryOperator` tag; a comparison
/// leaves an i32 flag.
pub const FLOAT_BINARY: u8 = 9;
/// `D = op A` on floats, an `F64UnaryOperator` tag.
pub const FLOAT_UNARY: u8 = 10;
/// `D = op A`, a `ScalarConversionOperator` tag; a truncation traps.
pub const CONVERT: u8 = 11;
/// `D = (the i32 in hole 3) ? A : B`.
pub const SELECT: u8 = 12;
/// Traps: `unreachable`.
pub const TRAP: u8 = 13;

/// The float operators, the conversions.
pub const FLOAT_BINARY_COUNT: u32 = 14;
pub const FLOAT_UNARY_COUNT: u32 = 7;
pub const CONVERT_COUNT: u32 = 33;

/// The rows LLVM compiles with a constant-pool load (float bounds and masks), which the stencil
/// build cannot relocate yet: the truncations and unsigned conversions, and copysign. The
/// library leaves them out, and a function using one keeps its compiled entry.
pub const CONVERT_NEEDS_CONSTANTS: [u32; 17] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 26];
pub const FLOAT_BINARY_NEEDS_CONSTANTS: [u32; 1] = [6];

/// The value types of an access.
pub const T_I32: u8 = 0;
pub const T_I64: u8 = 1;
pub const T_F32: u8 = 2;
pub const T_F64: u8 = 3;

/// The scalar accesses (`staged::wasm_memory::Access`): type, bytes, sign-extending. Every load
/// is one; a store is one that does not sign-extend.
pub const ACCESSES: [(u8, usize, bool); 14] = [
    (T_I32, 4, false), (T_I32, 1, true), (T_I32, 1, false), (T_I32, 2, true), (T_I32, 2, false),
    (T_I64, 8, false), (T_I64, 1, true), (T_I64, 1, false), (T_I64, 2, true), (T_I64, 2, false),
    (T_I64, 4, true), (T_I64, 4, false), (T_F32, 4, false), (T_F64, 8, false),
];

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
