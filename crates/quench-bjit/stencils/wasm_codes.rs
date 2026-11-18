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
/// `D = ` the frame memory's size in pages.
pub const MEMORY_SIZE: u8 = 14;
/// Fills `memory[A..A + C]` with the byte in B; traps out of bounds, writing nothing.
pub const MEMORY_FILL: u8 = 15;
/// Copies `memory[B..B + C]` to `memory[A..]` (overlap allowed); traps out of bounds.
pub const MEMORY_COPY: u8 = 16;
/// `D = ` the global bound at environment slot B, its storage the operator code
/// (`STORAGE_BITS32`, `STORAGE_BITS64`); the context's global area holds the cell.
pub const GLOBAL_GET: u8 = 17;
/// The global bound at environment slot B `= A`, likewise.
pub const GLOBAL_SET: u8 = 18;
/// Calls through the table of the indirect call's binding: the element index in register B,
/// the result to D, a trap's status leaves. Its row is a `call_` row (`wasm_name`).
pub const CALL_INDIRECT: u8 = 19;
/// A VM callout of a table, segment or `memory.init` operation (`wasm_rows_callout.rs`): OP is
/// `CALLOUT_RESULT | CALLOUT_WRAP` bits (`0..=3`). Its row is a `call_` row (`wasm_name`).
pub const CALLOUT: u8 = 31;
/// A callout's OP bits: the callout writes a result word to the destination, and that word is an
/// i32 to be zero-extended from its low 32 bits.
pub const CALLOUT_RESULT: u32 = 1;
pub const CALLOUT_WRAP: u32 = 2;
/// The frame offset a callout's absent operand names: the row passes the zero word.
#[allow(dead_code)] // The rows and the stitcher read it; `build.rs` includes this file too.
pub const CALLOUT_ABSENT: u32 = u32::MAX;
/// `D = memory.grow` of the frame's memory by the pages in register C, the size before in pages
/// (-1 on failure). Calls the VM through the context's `memory_grow` slot, so its row is a `call_`
/// row (`wasm_name`).
pub const MEMORY_GROW: u8 = 30;
/// A direct call of a defined function the stitcher did not stitch: the VM runs it through the
/// callout in hole B (`wasm_rows_call_vm.rs`), the callee's defined index in hole FOURTH. Its row
/// is a `call_` row (`wasm_name`).
pub const CALL_VM: u8 = 32;
/// A global's storage operator codes: a 32-bit payload, a 64-bit payload.
pub const STORAGE_BITS32: u32 = 0;
pub const STORAGE_BITS64: u32 = 1;

/// The float operators, the conversions.
pub const FLOAT_BINARY_COUNT: u32 = 14;
pub const FLOAT_UNARY_COUNT: u32 = 7;
pub const CONVERT_COUNT: u32 = 33;

/// The rows that once compiled with a constant-pool load: none now, since their float bounds and
/// masks are built from integer immediates (`wasm_rows_convert.rs`). Kept so the library and its
/// tests name one list.
pub const CONVERT_NEEDS_CONSTANTS: [u32; 0] = [];
pub const FLOAT_BINARY_NEEDS_CONSTANTS: [u32; 0] = [];

/// The frame's header words before the registers: the result pointer, the memory base and its
/// length (the call row reloads the pins from them).
#[allow(dead_code)] // The stitcher reads it; `build.rs` includes this file too.
pub const FRAME_HEADER_WORDS: u64 = 3;

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

/// Vector rows (`wasm_rows_simd.rs`): the kinds 50..=59; the OP of each is its `SimdOperator` tag.
pub const SIMD_BINARY: u8 = 50;
pub const SIMD_UNARY: u8 = 51;
pub const SIMD_SHUFFLE: u8 = 52;
pub const SIMD_CONST: u8 = 53;
pub const SIMD_LOAD: u8 = 54;
pub const SIMD_STORE: u8 = 55;
pub const SIMD_SPLAT: u8 = 56;
pub const SIMD_EXTRACT: u8 = 57;
pub const SIMD_REPLACE: u8 = 58;
pub const SIMD_MOVE: u8 = 59;

pub const SIMD_V128_NOT: u32 = 0;
pub const SIMD_V128_AND: u32 = 1;
pub const SIMD_V128_ANDNOT: u32 = 2;
pub const SIMD_V128_OR: u32 = 3;
pub const SIMD_V128_XOR: u32 = 4;
pub const SIMD_I8X16_SWIZZLE: u32 = 6;
pub const SIMD_I8X16_SPLAT: u32 = 7;
pub const SIMD_I16X8_SPLAT: u32 = 8;
pub const SIMD_I32X4_SPLAT: u32 = 9;
pub const SIMD_I64X2_SPLAT: u32 = 10;
pub const SIMD_F32X4_SPLAT: u32 = 11;
pub const SIMD_F64X2_SPLAT: u32 = 12;
pub const SIMD_I8X16_EXTRACT_S: u32 = 13;
pub const SIMD_I8X16_EXTRACT_U: u32 = 14;
pub const SIMD_I16X8_EXTRACT_S: u32 = 15;
pub const SIMD_I16X8_EXTRACT_U: u32 = 16;
pub const SIMD_I32X4_EXTRACT: u32 = 17;
pub const SIMD_I64X2_EXTRACT: u32 = 18;
pub const SIMD_F32X4_EXTRACT: u32 = 19;
pub const SIMD_F64X2_EXTRACT: u32 = 20;
pub const SIMD_I8X16_REPLACE: u32 = 21;
pub const SIMD_I16X8_REPLACE: u32 = 22;
pub const SIMD_I32X4_REPLACE: u32 = 23;
pub const SIMD_I64X2_REPLACE: u32 = 24;
pub const SIMD_F32X4_REPLACE: u32 = 25;
pub const SIMD_F64X2_REPLACE: u32 = 26;
pub const SIMD_F32X4_ADD: u32 = 27;
pub const SIMD_F32X4_SUB: u32 = 28;
pub const SIMD_F32X4_MUL: u32 = 29;
pub const SIMD_F32X4_DIV: u32 = 30;
pub const SIMD_F32X4_EQ: u32 = 33;
pub const SIMD_F32X4_NE: u32 = 34;
pub const SIMD_F32X4_LT: u32 = 35;
pub const SIMD_F32X4_GT: u32 = 36;
pub const SIMD_F32X4_LE: u32 = 37;
pub const SIMD_F32X4_GE: u32 = 38;
pub const SIMD_F32X4_ABS: u32 = 41;
pub const SIMD_F32X4_NEG: u32 = 42;
pub const SIMD_F64X2_ADD: u32 = 48;
pub const SIMD_F64X2_SUB: u32 = 49;
pub const SIMD_F64X2_MUL: u32 = 50;
pub const SIMD_F64X2_DIV: u32 = 51;
pub const SIMD_F64X2_EQ: u32 = 54;
pub const SIMD_F64X2_NE: u32 = 55;
pub const SIMD_F64X2_LT: u32 = 56;
pub const SIMD_F64X2_GT: u32 = 57;
pub const SIMD_F64X2_LE: u32 = 58;
pub const SIMD_F64X2_GE: u32 = 59;
pub const SIMD_F64X2_ABS: u32 = 62;
pub const SIMD_F64X2_NEG: u32 = 63;
pub const SIMD_I8X16_EQ: u32 = 69;
pub const SIMD_I8X16_NE: u32 = 70;
pub const SIMD_I8X16_LT_S: u32 = 71;
pub const SIMD_I8X16_GT_S: u32 = 72;
pub const SIMD_I8X16_LE_S: u32 = 73;
pub const SIMD_I8X16_GE_S: u32 = 74;
pub const SIMD_I8X16_LT_U: u32 = 75;
pub const SIMD_I8X16_GT_U: u32 = 76;
pub const SIMD_I8X16_LE_U: u32 = 77;
pub const SIMD_I8X16_GE_U: u32 = 78;
pub const SIMD_I8X16_ADD: u32 = 79;
pub const SIMD_I8X16_SUB: u32 = 80;
pub const SIMD_I8X16_ABS: u32 = 81;
pub const SIMD_I8X16_NEG: u32 = 82;
pub const SIMD_I16X8_EQ: u32 = 99;
pub const SIMD_I16X8_NE: u32 = 100;
pub const SIMD_I16X8_ADD: u32 = 109;
pub const SIMD_I16X8_SUB: u32 = 110;
pub const SIMD_I16X8_MUL: u32 = 111;
pub const SIMD_I32X4_EQ: u32 = 140;
pub const SIMD_I32X4_NE: u32 = 141;
pub const SIMD_I32X4_ADD: u32 = 150;
pub const SIMD_I32X4_SUB: u32 = 151;
pub const SIMD_I32X4_MUL: u32 = 152;
pub const SIMD_I64X2_ADD: u32 = 180;
pub const SIMD_I64X2_SUB: u32 = 181;
pub const SIMD_I64X2_MUL: u32 = 182;

pub const SIMD_UNARY_OPS: &[u32] = &[SIMD_V128_NOT, SIMD_I8X16_ABS, SIMD_I8X16_NEG, SIMD_F32X4_ABS, SIMD_F32X4_NEG, SIMD_F64X2_ABS, SIMD_F64X2_NEG];
pub const SIMD_BINARY_OPS: &[u32] = &[
    SIMD_V128_AND, SIMD_V128_ANDNOT, SIMD_V128_OR, SIMD_V128_XOR, SIMD_I8X16_SWIZZLE,
    SIMD_I8X16_EQ, SIMD_I8X16_NE, SIMD_I8X16_LT_S, SIMD_I8X16_GT_S, SIMD_I8X16_LE_S, SIMD_I8X16_GE_S,
    SIMD_I8X16_LT_U, SIMD_I8X16_GT_U, SIMD_I8X16_LE_U, SIMD_I8X16_GE_U, SIMD_I8X16_ADD, SIMD_I8X16_SUB,
    SIMD_I16X8_EQ, SIMD_I16X8_NE, SIMD_I16X8_ADD, SIMD_I16X8_SUB, SIMD_I16X8_MUL,
    SIMD_I32X4_EQ, SIMD_I32X4_NE, SIMD_I32X4_ADD, SIMD_I32X4_SUB, SIMD_I32X4_MUL,
    SIMD_I64X2_ADD, SIMD_I64X2_SUB, SIMD_I64X2_MUL,
    SIMD_F32X4_ADD, SIMD_F32X4_SUB, SIMD_F32X4_MUL, SIMD_F32X4_DIV, SIMD_F32X4_EQ, SIMD_F32X4_NE,
    SIMD_F32X4_LT, SIMD_F32X4_GT, SIMD_F32X4_LE, SIMD_F32X4_GE,
    SIMD_F64X2_ADD, SIMD_F64X2_SUB, SIMD_F64X2_MUL, SIMD_F64X2_DIV, SIMD_F64X2_EQ, SIMD_F64X2_NE,
    SIMD_F64X2_LT, SIMD_F64X2_GT, SIMD_F64X2_LE, SIMD_F64X2_GE,
];
pub const SIMD_SPLAT_OPS: &[u32] = &[SIMD_I8X16_SPLAT, SIMD_I16X8_SPLAT, SIMD_I32X4_SPLAT, SIMD_I64X2_SPLAT, SIMD_F32X4_SPLAT, SIMD_F64X2_SPLAT];
pub const SIMD_EXTRACT_OPS: &[u32] = &[SIMD_I8X16_EXTRACT_S, SIMD_I8X16_EXTRACT_U, SIMD_I16X8_EXTRACT_S, SIMD_I16X8_EXTRACT_U, SIMD_I32X4_EXTRACT, SIMD_I64X2_EXTRACT, SIMD_F32X4_EXTRACT, SIMD_F64X2_EXTRACT];
pub const SIMD_REPLACE_OPS: &[u32] = &[SIMD_I8X16_REPLACE, SIMD_I16X8_REPLACE, SIMD_I32X4_REPLACE, SIMD_I64X2_REPLACE, SIMD_F32X4_REPLACE, SIMD_F64X2_REPLACE];

/// Kinds 60..=69 (`wasm_rows_simd_more.rs`); each row's OP is its `SimdOperator` tag.
pub const SIMD_MORE_BINARY: u8 = 60;
pub const SIMD_MORE_COMPARE: u8 = 61;
pub const SIMD_MORE_FLOAT: u8 = 62;
pub const SIMD_MORE_SHIFT: u8 = 63;
pub const SIMD_MORE_TEST: u8 = 64;
pub const SIMD_MORE_UNARY: u8 = 65;
pub const SIMD_MORE_WIDEN: u8 = 66;
pub const SIMD_MORE_WIDE: u8 = 67;
pub const SIMD_MORE_CONVERT: u8 = 68;
pub const SIMD_MORE_LOAD: u8 = 69;
pub const SIMD_I8X16_MIN_S: u32 = 88; pub const SIMD_I8X16_MIN_U: u32 = 89;
pub const SIMD_I8X16_MAX_S: u32 = 90; pub const SIMD_I8X16_MAX_U: u32 = 91;
pub const SIMD_I16X8_MIN_S: u32 = 119; pub const SIMD_I16X8_MIN_U: u32 = 120;
pub const SIMD_I16X8_MAX_S: u32 = 121; pub const SIMD_I16X8_MAX_U: u32 = 122;
pub const SIMD_I32X4_MIN_S: u32 = 160; pub const SIMD_I32X4_MIN_U: u32 = 161;
pub const SIMD_I32X4_MAX_S: u32 = 162; pub const SIMD_I32X4_MAX_U: u32 = 163;
pub const SIMD_I8X16_ADD_SAT_S: u32 = 92; pub const SIMD_I8X16_ADD_SAT_U: u32 = 93;
pub const SIMD_I8X16_SUB_SAT_S: u32 = 94; pub const SIMD_I8X16_SUB_SAT_U: u32 = 95;
pub const SIMD_I16X8_ADD_SAT_S: u32 = 123; pub const SIMD_I16X8_ADD_SAT_U: u32 = 124;
pub const SIMD_I16X8_SUB_SAT_S: u32 = 125; pub const SIMD_I16X8_SUB_SAT_U: u32 = 126;
pub const SIMD_I8X16_AVGR_U: u32 = 96; pub const SIMD_I16X8_AVGR_U: u32 = 127;
pub const SIMD_I16X8_Q15_MULR_SAT_S: u32 = 199; pub const SIMD_I16X8_RELAXED_Q15MULR_S: u32 = 220;
pub const SIMD_I8X16_RELAXED_SWIZZLE: u32 = 211; pub const SIMD_I16X8_LT_S: u32 = 101;
pub const SIMD_I16X8_GT_S: u32 = 102; pub const SIMD_I16X8_LE_S: u32 = 103;
pub const SIMD_I16X8_GE_S: u32 = 104; pub const SIMD_I16X8_LT_U: u32 = 105;
pub const SIMD_I16X8_GT_U: u32 = 106; pub const SIMD_I16X8_LE_U: u32 = 107;
pub const SIMD_I16X8_GE_U: u32 = 108; pub const SIMD_I32X4_LT_S: u32 = 142;
pub const SIMD_I32X4_GT_S: u32 = 143; pub const SIMD_I32X4_LE_S: u32 = 144;
pub const SIMD_I32X4_GE_S: u32 = 145; pub const SIMD_I32X4_LT_U: u32 = 146;
pub const SIMD_I32X4_GT_U: u32 = 147; pub const SIMD_I32X4_LE_U: u32 = 148;
pub const SIMD_I32X4_GE_U: u32 = 149; pub const SIMD_I64X2_EQ: u32 = 174;
pub const SIMD_I64X2_NE: u32 = 175; pub const SIMD_I64X2_LT_S: u32 = 176;
pub const SIMD_I64X2_GT_S: u32 = 177; pub const SIMD_I64X2_LE_S: u32 = 178;
pub const SIMD_I64X2_GE_S: u32 = 179; pub const SIMD_F32X4_MIN: u32 = 31;
pub const SIMD_F32X4_MAX: u32 = 32; pub const SIMD_F32X4_PMIN: u32 = 39;
pub const SIMD_F32X4_PMAX: u32 = 40; pub const SIMD_F32X4_RELAXED_MIN: u32 = 216;
pub const SIMD_F32X4_RELAXED_MAX: u32 = 217; pub const SIMD_F64X2_MIN: u32 = 52;
pub const SIMD_F64X2_MAX: u32 = 53; pub const SIMD_F64X2_PMIN: u32 = 60;
pub const SIMD_F64X2_PMAX: u32 = 61; pub const SIMD_F64X2_RELAXED_MIN: u32 = 218;
pub const SIMD_F64X2_RELAXED_MAX: u32 = 219; pub const SIMD_I8X16_SHL: u32 = 85;
pub const SIMD_I8X16_SHR_S: u32 = 86; pub const SIMD_I8X16_SHR_U: u32 = 87;
pub const SIMD_I16X8_SHL: u32 = 116; pub const SIMD_I16X8_SHR_S: u32 = 117;
pub const SIMD_I16X8_SHR_U: u32 = 118; pub const SIMD_I32X4_SHL: u32 = 157;
pub const SIMD_I32X4_SHR_S: u32 = 158; pub const SIMD_I32X4_SHR_U: u32 = 159;
pub const SIMD_I64X2_SHL: u32 = 187; pub const SIMD_I64X2_SHR_S: u32 = 188;
pub const SIMD_I64X2_SHR_U: u32 = 189; pub const SIMD_V128_ANY_TRUE: u32 = 5;
pub const SIMD_I8X16_ALL_TRUE: u32 = 83; pub const SIMD_I8X16_BITMASK: u32 = 84;
pub const SIMD_I16X8_ALL_TRUE: u32 = 114; pub const SIMD_I16X8_BITMASK: u32 = 115;
pub const SIMD_I32X4_ALL_TRUE: u32 = 155; pub const SIMD_I32X4_BITMASK: u32 = 156;
pub const SIMD_I64X2_ALL_TRUE: u32 = 185; pub const SIMD_I64X2_BITMASK: u32 = 186;
pub const SIMD_I8X16_POPCNT: u32 = 198; pub const SIMD_I16X8_ABS: u32 = 112;
pub const SIMD_I16X8_NEG: u32 = 113; pub const SIMD_I32X4_ABS: u32 = 153;
pub const SIMD_I32X4_NEG: u32 = 154; pub const SIMD_I64X2_ABS: u32 = 183;
pub const SIMD_I64X2_NEG: u32 = 184; pub const SIMD_F32X4_SQRT: u32 = 43;
pub const SIMD_F32X4_CEIL: u32 = 44; pub const SIMD_F32X4_FLOOR: u32 = 45;
pub const SIMD_F32X4_TRUNC: u32 = 46; pub const SIMD_F32X4_NEAREST: u32 = 47;
pub const SIMD_F64X2_SQRT: u32 = 64; pub const SIMD_F64X2_CEIL: u32 = 65;
pub const SIMD_F64X2_FLOOR: u32 = 66; pub const SIMD_F64X2_TRUNC: u32 = 67;
pub const SIMD_F64X2_NEAREST: u32 = 68; pub const SIMD_I16X8_EXTEND_LOW_I8X16_S: u32 = 128;
pub const SIMD_I16X8_EXTEND_LOW_I8X16_U: u32 = 130; pub const SIMD_I16X8_EXTEND_HIGH_I8X16_S:
    u32 = 132;
pub const SIMD_I16X8_EXTEND_HIGH_I8X16_U: u32 = 134; pub const SIMD_I32X4_EXTEND_LOW_I16X8_S:
    u32 = 164;
pub const SIMD_I32X4_EXTEND_LOW_I16X8_U: u32 = 166; pub const SIMD_I32X4_EXTEND_HIGH_I16X8_S:
    u32 = 168;
pub const SIMD_I32X4_EXTEND_HIGH_I16X8_U: u32 = 170; pub const SIMD_I64X2_EXTEND_LOW_I32X4_S:
    u32 = 190;
pub const SIMD_I64X2_EXTEND_LOW_I32X4_U: u32 = 192; pub const SIMD_I64X2_EXTEND_HIGH_I32X4_S:
    u32 = 194;
pub const SIMD_I64X2_EXTEND_HIGH_I32X4_U: u32 = 196; pub const
    SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S: u32 = 136;
pub const SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U: u32 = 137; pub const
    SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S: u32 = 172;
pub const SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U: u32 = 173; pub const SIMD_I8X16_NARROW_I16X8_S:
    u32 = 97;
pub const SIMD_I8X16_NARROW_I16X8_U: u32 = 98; pub const SIMD_I16X8_NARROW_I32X4_S: u32 = 138;
pub const SIMD_I16X8_NARROW_I32X4_U: u32 = 139; pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_S: u32 = 129;
pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_U: u32 = 131; pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_S:
    u32 = 133;
pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_U: u32 = 135; pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_S:
    u32 = 165;
pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_U: u32 = 167; pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_S:
    u32 = 169;
pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_U: u32 = 171; pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_S:
    u32 = 191;
pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_U: u32 = 193; pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_S:
    u32 = 195;
pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_U: u32 = 197; pub const SIMD_I32X4_DOT_I16X8_S: u32 = 200;
pub const SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S: u32 = 221; pub const
    SIMD_I32X4_TRUNC_SAT_F32X4_S: u32 = 201;
pub const SIMD_I32X4_TRUNC_SAT_F32X4_U: u32 = 202; pub const SIMD_F32X4_CONVERT_I32X4_S: u32 = 203;
pub const SIMD_F32X4_CONVERT_I32X4_U: u32 = 204; pub const SIMD_I32X4_TRUNC_SAT_F64X2_SZERO: u32
    = 205;
pub const SIMD_I32X4_TRUNC_SAT_F64X2_UZERO: u32 = 206; pub const SIMD_F64X2_CONVERT_LOW_I32X4_S:
    u32 = 207;
pub const SIMD_F64X2_CONVERT_LOW_I32X4_U: u32 = 208; pub const SIMD_F32X4_DEMOTE_F64X2_ZERO: u32
    = 209;
pub const SIMD_F64X2_PROMOTE_LOW_F32X4: u32 = 210; pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_S:
    u32 = 212;
pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_U: u32 = 213; pub const
    SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO: u32 = 214;
pub const SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO: u32 = 215; pub const SIMD_V128_LOAD8X8_S: u32 = 15;
pub const SIMD_V128_LOAD8X8_U: u32 = 16; pub const SIMD_V128_LOAD16X4_S: u32 = 17;
pub const SIMD_V128_LOAD16X4_U: u32 = 18; pub const SIMD_V128_LOAD32X2_S: u32 = 19;
pub const SIMD_V128_LOAD32X2_U: u32 = 20; pub const SIMD_V128_LOAD8_SPLAT: u32 = 21;
pub const SIMD_V128_LOAD16_SPLAT: u32 = 22; pub const SIMD_V128_LOAD32_SPLAT: u32 = 23;
pub const SIMD_V128_LOAD64_SPLAT: u32 = 24; pub const SIMD_V128_LOAD32_ZERO: u32 = 25;
pub const SIMD_V128_LOAD64_ZERO: u32 = 26;
pub const SIMD_MORE_BINARY_OPS: &[u32] = &[SIMD_I8X16_MIN_S, SIMD_I8X16_MIN_U, SIMD_I8X16_MAX_S,
    SIMD_I8X16_MAX_U, SIMD_I16X8_MIN_S, SIMD_I16X8_MIN_U, SIMD_I16X8_MAX_S, SIMD_I16X8_MAX_U,
    SIMD_I32X4_MIN_S, SIMD_I32X4_MIN_U, SIMD_I32X4_MAX_S, SIMD_I32X4_MAX_U, SIMD_I8X16_ADD_SAT_S,
    SIMD_I8X16_ADD_SAT_U, SIMD_I8X16_SUB_SAT_S, SIMD_I8X16_SUB_SAT_U, SIMD_I16X8_ADD_SAT_S,
    SIMD_I16X8_ADD_SAT_U, SIMD_I16X8_SUB_SAT_S, SIMD_I16X8_SUB_SAT_U, SIMD_I8X16_AVGR_U,
    SIMD_I16X8_AVGR_U, SIMD_I16X8_Q15_MULR_SAT_S, SIMD_I16X8_RELAXED_Q15MULR_S,
    SIMD_I8X16_RELAXED_SWIZZLE];
pub const SIMD_MORE_COMPARE_OPS: &[u32] = &[SIMD_I16X8_LT_S, SIMD_I16X8_GT_S, SIMD_I16X8_LE_S,
    SIMD_I16X8_GE_S, SIMD_I16X8_LT_U, SIMD_I16X8_GT_U, SIMD_I16X8_LE_U, SIMD_I16X8_GE_U,
    SIMD_I32X4_LT_S, SIMD_I32X4_GT_S, SIMD_I32X4_LE_S, SIMD_I32X4_GE_S, SIMD_I32X4_LT_U,
    SIMD_I32X4_GT_U, SIMD_I32X4_LE_U, SIMD_I32X4_GE_U, SIMD_I64X2_EQ, SIMD_I64X2_NE,
    SIMD_I64X2_LT_S, SIMD_I64X2_GT_S, SIMD_I64X2_LE_S, SIMD_I64X2_GE_S];
/// The relaxed min/max rows compile to a section-relative constant load the stencil build cannot
/// relocate; the library leaves them out, and a function using one keeps the interpreter.
pub const SIMD_RELAXED_MIN_MAX: core::ops::RangeInclusive<u32> = SIMD_F32X4_RELAXED_MIN..=SIMD_F64X2_RELAXED_MAX;
pub const SIMD_MORE_FLOAT_OPS: &[u32] = &[SIMD_F32X4_MIN, SIMD_F32X4_MAX, SIMD_F32X4_PMIN,
    SIMD_F32X4_PMAX, SIMD_F32X4_RELAXED_MIN, SIMD_F32X4_RELAXED_MAX, SIMD_F64X2_MIN, SIMD_F64X2_MAX,
    SIMD_F64X2_PMIN, SIMD_F64X2_PMAX, SIMD_F64X2_RELAXED_MIN, SIMD_F64X2_RELAXED_MAX];
pub const SIMD_MORE_SHIFT_OPS: &[u32] = &[SIMD_I8X16_SHL, SIMD_I8X16_SHR_S, SIMD_I8X16_SHR_U,
    SIMD_I16X8_SHL, SIMD_I16X8_SHR_S, SIMD_I16X8_SHR_U, SIMD_I32X4_SHL, SIMD_I32X4_SHR_S,
    SIMD_I32X4_SHR_U, SIMD_I64X2_SHL, SIMD_I64X2_SHR_S, SIMD_I64X2_SHR_U];
pub const SIMD_MORE_TEST_OPS: &[u32] = &[SIMD_V128_ANY_TRUE, SIMD_I8X16_ALL_TRUE,
    SIMD_I8X16_BITMASK, SIMD_I16X8_ALL_TRUE, SIMD_I16X8_BITMASK, SIMD_I32X4_ALL_TRUE,
    SIMD_I32X4_BITMASK, SIMD_I64X2_ALL_TRUE, SIMD_I64X2_BITMASK];
pub const SIMD_MORE_UNARY_OPS: &[u32] = &[SIMD_I8X16_POPCNT, SIMD_I16X8_ABS, SIMD_I16X8_NEG,
    SIMD_I32X4_ABS, SIMD_I32X4_NEG, SIMD_I64X2_ABS, SIMD_I64X2_NEG, SIMD_F32X4_SQRT,
    SIMD_F32X4_CEIL, SIMD_F32X4_FLOOR, SIMD_F32X4_TRUNC, SIMD_F32X4_NEAREST, SIMD_F64X2_SQRT,
    SIMD_F64X2_CEIL, SIMD_F64X2_FLOOR, SIMD_F64X2_TRUNC, SIMD_F64X2_NEAREST];
pub const SIMD_MORE_WIDEN_OPS: &[u32] = &[SIMD_I16X8_EXTEND_LOW_I8X16_S,
    SIMD_I16X8_EXTEND_LOW_I8X16_U, SIMD_I16X8_EXTEND_HIGH_I8X16_S, SIMD_I16X8_EXTEND_HIGH_I8X16_U,
    SIMD_I32X4_EXTEND_LOW_I16X8_S, SIMD_I32X4_EXTEND_LOW_I16X8_U, SIMD_I32X4_EXTEND_HIGH_I16X8_S,
    SIMD_I32X4_EXTEND_HIGH_I16X8_U, SIMD_I64X2_EXTEND_LOW_I32X4_S, SIMD_I64X2_EXTEND_LOW_I32X4_U,
    SIMD_I64X2_EXTEND_HIGH_I32X4_S, SIMD_I64X2_EXTEND_HIGH_I32X4_U,
    SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S, SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U,
    SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S, SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U];
pub const SIMD_MORE_WIDE_OPS: &[u32] = &[SIMD_I8X16_NARROW_I16X8_S, SIMD_I8X16_NARROW_I16X8_U,
    SIMD_I16X8_NARROW_I32X4_S, SIMD_I16X8_NARROW_I32X4_U, SIMD_I16X8_EXT_MUL_LOW_I8X16_S,
    SIMD_I16X8_EXT_MUL_LOW_I8X16_U, SIMD_I16X8_EXT_MUL_HIGH_I8X16_S,
    SIMD_I16X8_EXT_MUL_HIGH_I8X16_U, SIMD_I32X4_EXT_MUL_LOW_I16X8_S, SIMD_I32X4_EXT_MUL_LOW_I16X8_U,
    SIMD_I32X4_EXT_MUL_HIGH_I16X8_S, SIMD_I32X4_EXT_MUL_HIGH_I16X8_U,
    SIMD_I64X2_EXT_MUL_LOW_I32X4_S, SIMD_I64X2_EXT_MUL_LOW_I32X4_U, SIMD_I64X2_EXT_MUL_HIGH_I32X4_S,
    SIMD_I64X2_EXT_MUL_HIGH_I32X4_U, SIMD_I32X4_DOT_I16X8_S, SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S];
pub const SIMD_MORE_CONVERT_OPS: &[u32] = &[SIMD_I32X4_TRUNC_SAT_F32X4_S,
    SIMD_I32X4_TRUNC_SAT_F32X4_U, SIMD_F32X4_CONVERT_I32X4_S, SIMD_F32X4_CONVERT_I32X4_U,
    SIMD_I32X4_TRUNC_SAT_F64X2_SZERO, SIMD_I32X4_TRUNC_SAT_F64X2_UZERO,
    SIMD_F64X2_CONVERT_LOW_I32X4_S, SIMD_F64X2_CONVERT_LOW_I32X4_U, SIMD_F32X4_DEMOTE_F64X2_ZERO,
    SIMD_F64X2_PROMOTE_LOW_F32X4, SIMD_I32X4_RELAXED_TRUNC_F32X4_S,
    SIMD_I32X4_RELAXED_TRUNC_F32X4_U, SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO,
    SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO];
pub const SIMD_MORE_LOAD_OPS: &[u32] = &[SIMD_V128_LOAD8X8_S, SIMD_V128_LOAD8X8_U,
    SIMD_V128_LOAD16X4_S, SIMD_V128_LOAD16X4_U, SIMD_V128_LOAD32X2_S, SIMD_V128_LOAD32X2_U,
    SIMD_V128_LOAD8_SPLAT, SIMD_V128_LOAD16_SPLAT, SIMD_V128_LOAD32_SPLAT, SIMD_V128_LOAD64_SPLAT,
    SIMD_V128_LOAD32_ZERO, SIMD_V128_LOAD64_ZERO];
