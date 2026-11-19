//! The codes of a Wasm row, which `build.rs` generates the stencils from (`wasm_library.rs`),
//! the stencils dispatch on (`wasm_rows.rs`) and the stitcher looks a row up by. An operator is
//! its selector tag (`I64BinaryOperator`, `I64UnaryOperator`): the i32 operators are the first
//! tags of their i64 namesakes.
//!
//! The row kinds are one table, `wasm_row_kinds!` below. A kind's code is its position in that
//! table, and each kind lists the families of rows it holds, so `wasm_library.rs` builds the
//! library from the same table and no code is typed by hand. Operator tags and counts that the
//! runtime defines are asserted equal to the runtime's values in
//! `quench-runtime/src/vm/compiled/wasm_codes_check.rs`, because this crate (compiled alone as the
//! stencil build) cannot see them.
//!
//! Some items are read only by `build.rs` and others only by the stencils or the runtime, so the
//! lint is off for the whole module.

#![allow(dead_code)]

// The operator lists, expanded here and by the runtime from the one file (`staged/wasm_lists.rs`, which
// has no `use` and no item, so the stencil build, the lib and the build script compile it alike).
#[macro_use]
#[path = "../../quench-runtime/src/staged/wasm_lists.rs"]
mod wasm_lists;

/// A SIMD operator's family, the list's column: the row kind its operator runs in.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum SimdFamily {
    Unary, Binary, Splat, Extract, Replace, MoreBinary, MoreCompare, MoreFloat, MoreShift, MoreTest, MoreUnary, MoreWiden, MoreWide, MoreConvert,
}

/// A vector load's family: a scalar access, the whole vector, or a vector load that splats, widens or zeroes.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum LoadFamily {
    Scalar, Plain, Vector,
}

/// How many of `table`'s entries hold `code`.
const fn count_of(table: &[u8], code: u8) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < table.len() {
        if table[index] == code {
            count += 1;
        }
        index += 1;
    }
    count
}

/// The indices of `table`'s entries that hold `code`, in order: the tags of one family.
const fn tags_of<const N: usize>(table: &[u8], code: u8) -> [u32; N] {
    let mut tags = [0u32; N];
    let mut index = 0;
    let mut filled = 0;
    while index < table.len() {
        if table[index] == code {
            tags[filled] = index as u32;
            filled += 1;
        }
        index += 1;
    }
    tags
}

/// One row family: operators at one width, each at every shape.
#[derive(Clone, Copy)]
pub struct RowFamily {
    pub ops: RowOps,
    pub width: u8,
    pub shapes: &'static [u8],
}

/// The operators a family's rows take.
#[derive(Clone, Copy)]
pub enum RowOps {
    /// The operators `first..end`.
    Span(u32, u32),
    /// The operators `first..end` except those listed.
    Except(u32, u32, &'static [u32]),
    /// The operators listed.
    List(&'static [u32]),
    /// Every scalar access (`ACCESSES`), by its index.
    Accesses,
    /// Every scalar access that does not sign-extend (`ACCESSES`), by its index.
    UnsignedAccesses,
}

/// A family of a kind's rows: the operators `ops` at `width`, each at every shape in `shapes`.
pub const fn row_family(ops: RowOps, width: u8, shapes: &'static [u8]) -> RowFamily {
    RowFamily { ops, width, shapes }
}

/// The row kinds, one table. Each entry is a kind's name and its families. The kind's code is its
/// position (counted from 0), so a kind added anywhere renumbers the kinds after it; the names
/// that carry a code (`w_<kind>_<op>_<width>_<shape>`, `call_w_...`) follow, since `build.rs` and
/// the stitcher both read these constants. `ROW_FAMILIES` is the table as data: each kind with its
/// families, in table order.
macro_rules! wasm_row_kinds {
    (@number $n:expr;) => {};
    (@number $n:expr; $kind:ident $(, $rest:ident)*) => {
        pub const $kind: u8 = $n;
        wasm_row_kinds!(@number $n + 1; $($rest),*);
    };
    ($($kind:ident => [$($family:expr),* $(,)?]),* $(,)?) => {
        wasm_row_kinds!(@number 0; $($kind),*);
        /// Every kind with its row families, in table order.
        pub const ROW_FAMILIES: &[(u8, &[RowFamily])] = &[$(($kind, &[$($family),*])),*];
    };
}

/// The value types of an access, the codes `T_*` name.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum AccessType {
    I32, I64, F32, F64,
}
pub const T_I32: u8 = AccessType::I32 as u8;
pub const T_I64: u8 = AccessType::I64 as u8;
pub const T_F32: u8 = AccessType::F32 as u8;
pub const T_F64: u8 = AccessType::F64 as u8;

/// The scalar accesses (`staged::wasm_memory::Access`): type, bytes, sign-extending, in the order
/// `wasm_lists.rs` lists them. Every load is one; a store is one that does not sign-extend.
macro_rules! access_table {
    ($($load:ident: $ty:ident, $bytes:expr, $signed:expr;)*) => {
        pub const ACCESSES: &[(u8, usize, bool)] = &[$((AccessType::$ty as u8, $bytes, $signed),)*];
    };
}
scalar_access_list!(access_table);

/// The width an integer row runs at.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum IntWidth {
    W32, W64,
}
pub const W32: u8 = IntWidth::W32 as u8;
pub const W64: u8 = IntWidth::W64 as u8;

/// The shape of a row's second operand: a register, or an immediate in the row's hole.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum OperandShape {
    Register, Immediate,
}
pub const REGISTER: u8 = OperandShape::Register as u8;
pub const IMMEDIATE: u8 = OperandShape::Immediate as u8;

/// A callout's OP bits: the callout writes a result word to the destination, and that word is an
/// i32 to be zero-extended from its low 32 bits.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum CalloutBit {
    Result = 1,
    Wrap = 2,
}
pub const CALLOUT_RESULT: u32 = CalloutBit::Result as u32;
pub const CALLOUT_WRAP: u32 = CalloutBit::Wrap as u32;
/// The frame offset a callout's absent operand names: the row passes the zero word.
pub const CALLOUT_ABSENT: u32 = u32::MAX;

/// A global's storage operator codes: a 32-bit payload, a 64-bit payload.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum StorageCode {
    Bits32, Bits64,
}
pub const STORAGE_BITS32: u32 = StorageCode::Bits32 as u32;
pub const STORAGE_BITS64: u32 = StorageCode::Bits64 as u32;

/// The float operators, the conversions. `FLOAT_BINARY_COUNT` is `F64BinaryOperator`'s selectors
/// (13: `Add` through `GreaterEqual`); the runtime asserts it.
pub const FLOAT_BINARY_COUNT: u32 = 13;
pub const FLOAT_UNARY_COUNT: u32 = 7;
pub const CONVERT_COUNT: u32 = 33;

/// The rows that once compiled with a constant-pool load: none now, since their float bounds and
/// masks are built from integer immediates (`wasm_rows_convert.rs`). Kept so the library and its
/// tests name one list.
pub const CONVERT_NEEDS_CONSTANTS: [u32; 0] = [];
pub const FLOAT_BINARY_NEEDS_CONSTANTS: [u32; 0] = [];

/// The frame's header words, before the registers. `wasm_entry.rs` writes them and the rows read
/// the memory base and length from them. `FRAME_HEADER_WORDS` is one past the last header word.
#[derive(Clone, Copy)]
#[repr(usize)]
enum FrameWord {
    Result,
    Base,
    Length,
}
pub const FRAME_RESULT_WORD: usize = FrameWord::Result as usize;
pub const FRAME_BASE_WORD: usize = FrameWord::Base as usize;
pub const FRAME_LENGTH_WORD: usize = FrameWord::Length as usize;
pub const FRAME_HEADER_WORDS: u64 = (FrameWord::Length as usize + 1) as u64;

/// The log2 of a Wasm page. The stencil build cannot see `crate::wasm::memory`, so the runtime
/// asserts it equals `WASM_PAGE_BITS` (`wasm_codes_check.rs`).
pub const WASM_PAGE_BITS: u32 = 16;

/// `CallContext`'s byte offsets the `call_indirect` row reads (`wasm_rows_indirect.rs`). The
/// runtime's `offset_of!` values are asserted equal to these in `wasm_codes_check.rs`.
pub const CONTEXT_TABLE_WORDS_AT: usize = 0x48;
pub const CONTEXT_CALL_REFERENCE_AT: usize = 0x58;
pub const CONTEXT_MEMORY_LENGTH_AT: usize = 0x90;
pub const CONTEXT_INSTANCE_AT: usize = 0x100;

/// `TableTarget`'s size and the offsets of its fields (`SIGNATURE`, `ENTRY`, `INSTANCE`, `BODY`),
/// and `TableWords`' (`BASE`, `LENGTH`). Asserted against the runtime's types in
/// `wasm_codes_check.rs`.
/// `TableTarget`'s layout as a row reads it: `#[repr(C)]` with the runtime struct's fields
/// (`heap/cell/wasm_fields.rs`), the signature one word. The runtime asserts each offset and the size.
#[repr(C)]
#[allow(dead_code)]
struct TableTargetLayout {
    signature: u32,
    entry: u32,
    instance: u64,
    body: usize,
}
pub const TARGET_BYTES: usize = core::mem::size_of::<TableTargetLayout>();
pub const TARGET_SIGNATURE_AT: usize = core::mem::offset_of!(TableTargetLayout, signature);
pub const TARGET_ENTRY_AT: usize = core::mem::offset_of!(TableTargetLayout, entry);
pub const TARGET_INSTANCE_AT: usize = core::mem::offset_of!(TableTargetLayout, instance);
pub const TARGET_BODY_AT: usize = core::mem::offset_of!(TableTargetLayout, body);
/// `TableWords`' layout as a row reads it: `#[repr(C)]`, the base then the length.
#[repr(C)]
#[allow(dead_code)]
struct TableWordsLayout {
    base: usize,
    length: u64,
}
pub const TABLE_WORDS_BASE_AT: usize = core::mem::offset_of!(TableWordsLayout, base);
pub const TABLE_WORDS_LENGTH_AT: usize = core::mem::offset_of!(TableWordsLayout, length);
/// `TableTarget::NO_ENTRY`: an element that is no module function's body.
pub const NO_ENTRY: u32 = u32::MAX;

/// The width-independent counts of the integer families: the i32 operators are the first
/// `SHARED_*` tags of the i64 families, which have the wide operators after them. The runtime
/// asserts each count and the comparison bounds.
pub const SHARED_BINARY: u32 = 25;
pub const BINARY_COUNT: u32 = 27;
/// The comparisons, `Equal` through `GreaterEqualUnsigned`.
pub const FIRST_COMPARISON: u32 = 15;
pub const LAST_COMPARISON: u32 = 24;
/// The unary operators: those both widths have, then the i64-only `ExtendSigned32`.
pub const SHARED_UNARY: u32 = 6;
pub const UNARY_COUNT: u32 = 7;

/// The row kinds, in table order. Each family says what the library builds for its kind: the
/// stencil symbol of every row is `w_<kind>_<op>_<width>_<shape>`, or `call_w_...` for a row that
/// calls (`wasm_library.rs`, `wasm_name`).
wasm_row_kinds! {
    // A binary or unary operator at each width, its second operand a register or an immediate.
    // `D = A op B`; a comparison branches through `JUMP_IF`.
    BINARY => [
        row_family(RowOps::Span(0, SHARED_BINARY), W32, &[REGISTER, IMMEDIATE]),
        row_family(RowOps::Span(0, BINARY_COUNT), W64, &[REGISTER, IMMEDIATE]),
    ],
    // `D = op A`.
    UNARY => [
        row_family(RowOps::Span(0, SHARED_UNARY), W32, &[REGISTER]),
        row_family(RowOps::Span(0, UNARY_COUNT), W64, &[REGISTER]),
    ],
    // A conditional branch on a comparison operator.
    JUMP_IF => [
        row_family(RowOps::Span(FIRST_COMPARISON, LAST_COMPARISON + 1), W32, &[REGISTER, IMMEDIATE]),
        row_family(RowOps::Span(FIRST_COMPARISON, LAST_COMPARISON + 1), W64, &[REGISTER, IMMEDIATE]),
    ],
    // `D = A`: a register's word.
    MOVE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = ` the word in the row's hole.
    CONSTANT => [row_family(RowOps::Span(0, 1), W64, &[IMMEDIATE])],
    // Continues at the taken successor.
    GOTO => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = memory[A + B]`, the access the operator code indexes in `ACCESSES`; traps out of bounds.
    LOAD => [row_family(RowOps::Accesses, W64, &[REGISTER])],
    // `memory[A + B] = D`, likewise; only the accesses that do not sign-extend store.
    STORE => [row_family(RowOps::UnsignedAccesses, W64, &[REGISTER])],
    // Calls the `WasmEntry` in the cell at address B with the arguments from register A on and the
    // result to D; a trap's status leaves. A `call_` row.
    CALL => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = A op B` on floats (width `W32` is f32), an `F64BinaryOperator` tag; a comparison leaves
    // an i32 flag.
    FLOAT_BINARY => [
        row_family(RowOps::Except(0, FLOAT_BINARY_COUNT, &FLOAT_BINARY_NEEDS_CONSTANTS), W32, &[REGISTER]),
        row_family(RowOps::Except(0, FLOAT_BINARY_COUNT, &FLOAT_BINARY_NEEDS_CONSTANTS), W64, &[REGISTER]),
    ],
    // `D = op A` on floats, an `F64UnaryOperator` tag.
    FLOAT_UNARY => [
        row_family(RowOps::Span(0, FLOAT_UNARY_COUNT), W32, &[REGISTER]),
        row_family(RowOps::Span(0, FLOAT_UNARY_COUNT), W64, &[REGISTER]),
    ],
    // `D = op A`, a `ScalarConversionOperator` tag; a truncation traps.
    CONVERT => [row_family(RowOps::Except(0, CONVERT_COUNT, &CONVERT_NEEDS_CONSTANTS), W64, &[REGISTER])],
    // `D = (the i32 in hole 3) ? A : B`.
    SELECT => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Traps: `unreachable`.
    TRAP => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = ` the frame memory's size in pages.
    MEMORY_SIZE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Fills `memory[A..A + C]` with the byte in B; traps out of bounds, writing nothing.
    MEMORY_FILL => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Copies `memory[B..B + C]` to `memory[A..]` (overlap allowed); traps out of bounds.
    MEMORY_COPY => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = ` the global bound at environment slot B, its storage the operator code
    // (`STORAGE_BITS32`, `STORAGE_BITS64`); the context's global area holds the cell.
    GLOBAL_GET => [row_family(RowOps::List(&[STORAGE_BITS32, STORAGE_BITS64]), W64, &[REGISTER])],
    // The global bound at environment slot B `= A`, likewise.
    GLOBAL_SET => [row_family(RowOps::List(&[STORAGE_BITS32, STORAGE_BITS64]), W64, &[REGISTER])],
    // Calls through the table of the indirect call's binding: the element index in register B, the
    // result to D, a trap's status leaves. A `call_` row.
    CALL_INDIRECT => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // `D = memory.grow` of the frame's memory by the pages in register C, the size before in pages
    // (-1 on failure). Calls the VM through the context's `memory_grow` slot: a `call_` row.
    MEMORY_GROW => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // A VM callout of a table, segment or `memory.init` operation (`wasm_rows_callout.rs`): OP is
    // `CALLOUT_RESULT | CALLOUT_WRAP` bits. A `call_` row.
    CALLOUT => [row_family(RowOps::Span(0, (CALLOUT_RESULT | CALLOUT_WRAP) + 1), W64, &[REGISTER])],
    // A direct call of a defined function the stitcher did not stitch: the VM runs it through the
    // callout in hole B (`wasm_rows_call_vm.rs`). A `call_` row.
    CALL_VM => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Native roots of a stitched frame (`wasm_rows_roots.rs`): `ROOT_PUSH` holds up to four
    // registers, `ROOT_POP` releases the count its hole names. Neither calls.
    ROOT_PUSH => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    ROOT_POP => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // The allocating GC step's callout (`wasm_rows_callout.rs`, `gc_allocate`). A `call_` row.
    GC_ALLOCATE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Vector rows (`wasm_rows_simd.rs`): each OP is its `SimdOperator` tag.
    SIMD_BINARY => [row_family(RowOps::List(SIMD_BINARY_OPS), W64, &[REGISTER])],
    SIMD_UNARY => [row_family(RowOps::List(SIMD_UNARY_OPS), W64, &[REGISTER])],
    SIMD_SHUFFLE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    SIMD_CONST => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    SIMD_LOAD => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    SIMD_STORE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    SIMD_SPLAT => [row_family(RowOps::List(SIMD_SPLAT_OPS), W64, &[REGISTER])],
    SIMD_EXTRACT => [row_family(RowOps::List(SIMD_EXTRACT_OPS), W64, &[REGISTER])],
    SIMD_REPLACE => [row_family(RowOps::List(SIMD_REPLACE_OPS), W64, &[REGISTER])],
    SIMD_MOVE => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
    // Vector rows beyond `SIMD_*` (`wasm_rows_simd_more.rs`): each OP is its `SimdOperator` tag.
    SIMD_MORE_BINARY => [row_family(RowOps::List(SIMD_MORE_BINARY_OPS), W64, &[REGISTER])],
    SIMD_MORE_COMPARE => [row_family(RowOps::List(SIMD_MORE_COMPARE_OPS), W64, &[REGISTER])],
    SIMD_MORE_FLOAT => [row_family(RowOps::List(SIMD_MORE_FLOAT_OPS), W64, &[REGISTER])],
    SIMD_MORE_SHIFT => [row_family(RowOps::List(SIMD_MORE_SHIFT_OPS), W64, &[REGISTER])],
    SIMD_MORE_TEST => [row_family(RowOps::List(SIMD_MORE_TEST_OPS), W64, &[REGISTER])],
    SIMD_MORE_UNARY => [row_family(RowOps::List(SIMD_MORE_UNARY_OPS), W64, &[REGISTER])],
    SIMD_MORE_WIDEN => [row_family(RowOps::List(SIMD_MORE_WIDEN_OPS), W64, &[REGISTER])],
    SIMD_MORE_WIDE => [row_family(RowOps::List(SIMD_MORE_WIDE_OPS), W64, &[REGISTER])],
    SIMD_MORE_CONVERT => [row_family(RowOps::List(SIMD_MORE_CONVERT_OPS), W64, &[REGISTER])],
    SIMD_MORE_LOAD => [row_family(RowOps::List(SIMD_MORE_LOAD_OPS), W64, &[REGISTER])],
}

/// The SIMD operators, from the list (`wasm_lists.rs`): each family's code, in tag order, and every
/// operator's tag as the enum `SimdTag` (the runtime's `SimdOperator` has the same order). The b-jit
/// names its operators by these constants.
macro_rules! simd_tags {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        /// A SIMD operator's tag: its position in the list.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum SimdTag {
            $($name,)*
        }
        /// Each SIMD operator's family code, by tag.
        pub const SIMD_FAMILY_OF: &[u8] = &[$(SimdFamily::$family as u8,)*];
    };
}
simd_operator_list!(simd_tags);

/// The number of SIMD operators, which the runtime asserts equals `SimdOperator::ALL`.
pub const SIMD_OPERATOR_COUNT: u32 = SIMD_FAMILY_OF.len() as u32;

/// The row kind of an operator in a `SIMD_MORE_*` family, `None` for any other operator.
pub const fn simd_more_kind(tag: u32) -> Option<u8> {
    if (tag as usize) >= SIMD_FAMILY_OF.len() {
        return None;
    }
    let family = SIMD_FAMILY_OF[tag as usize];
    if family == SimdFamily::MoreBinary as u8 {
        Some(SIMD_MORE_BINARY)
    } else if family == SimdFamily::MoreCompare as u8 {
        Some(SIMD_MORE_COMPARE)
    } else if family == SimdFamily::MoreFloat as u8 {
        Some(SIMD_MORE_FLOAT)
    } else if family == SimdFamily::MoreShift as u8 {
        Some(SIMD_MORE_SHIFT)
    } else if family == SimdFamily::MoreTest as u8 {
        Some(SIMD_MORE_TEST)
    } else if family == SimdFamily::MoreUnary as u8 {
        Some(SIMD_MORE_UNARY)
    } else if family == SimdFamily::MoreWiden as u8 {
        Some(SIMD_MORE_WIDEN)
    } else if family == SimdFamily::MoreWide as u8 {
        Some(SIMD_MORE_WIDE)
    } else if family == SimdFamily::MoreConvert as u8 {
        Some(SIMD_MORE_CONVERT)
    } else {
        None
    }
}

/// The `Unary` family's operator tags, in tag order.
pub const SIMD_UNARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Unary as u8) }>(SIMD_FAMILY_OF, SimdFamily::Unary as u8);
/// The `Binary` family's operator tags, in tag order.
pub const SIMD_BINARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Binary as u8) }>(SIMD_FAMILY_OF, SimdFamily::Binary as u8);
/// The `Splat` family's operator tags, in tag order.
pub const SIMD_SPLAT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Splat as u8) }>(SIMD_FAMILY_OF, SimdFamily::Splat as u8);
/// The `Extract` family's operator tags, in tag order.
pub const SIMD_EXTRACT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Extract as u8) }>(SIMD_FAMILY_OF, SimdFamily::Extract as u8);
/// The `Replace` family's operator tags, in tag order.
pub const SIMD_REPLACE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::Replace as u8) }>(SIMD_FAMILY_OF, SimdFamily::Replace as u8);
/// The `MoreBinary` family's operator tags, in tag order.
pub const SIMD_MORE_BINARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreBinary as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreBinary as u8);
/// The `MoreCompare` family's operator tags, in tag order.
pub const SIMD_MORE_COMPARE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreCompare as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreCompare as u8);
/// The `MoreFloat` family's operator tags, in tag order.
pub const SIMD_MORE_FLOAT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreFloat as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreFloat as u8);
/// The `MoreShift` family's operator tags, in tag order.
pub const SIMD_MORE_SHIFT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreShift as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreShift as u8);
/// The `MoreTest` family's operator tags, in tag order.
pub const SIMD_MORE_TEST_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreTest as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreTest as u8);
/// The `MoreUnary` family's operator tags, in tag order.
pub const SIMD_MORE_UNARY_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreUnary as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreUnary as u8);
/// The `MoreWiden` family's operator tags, in tag order.
pub const SIMD_MORE_WIDEN_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreWiden as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreWiden as u8);
/// The `MoreWide` family's operator tags, in tag order.
pub const SIMD_MORE_WIDE_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreWide as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreWide as u8);
/// The `MoreConvert` family's operator tags, in tag order.
pub const SIMD_MORE_CONVERT_OPS: &[u32] = &tags_of::<{ count_of(SIMD_FAMILY_OF, SimdFamily::MoreConvert as u8) }>(SIMD_FAMILY_OF, SimdFamily::MoreConvert as u8);

/// The memory loads, from the list: each load's tag as `MemoryLoadTag`, and each one's family.
macro_rules! memory_load_tags {
    ($($name:ident $family:ident),* $(,)?) => {
        /// A memory load's tag: its position in the list (the runtime's `MemoryLoad` has the same order).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum MemoryLoadTag {
            $($name,)*
        }
        /// Each memory load's family code, by tag.
        pub const MEMORY_LOAD_FAMILY_OF: &[u8] = &[$(LoadFamily::$family as u8,)*];
    };
}
memory_load_list!(memory_load_tags);

/// The vector loads a `SIMD_MORE_LOAD` row computes (the splatting, widening and zeroing loads), by tag.
pub const SIMD_MORE_LOAD_OPS: &[u32] = &tags_of::<{ count_of(MEMORY_LOAD_FAMILY_OF, LoadFamily::Vector as u8) }>(MEMORY_LOAD_FAMILY_OF, LoadFamily::Vector as u8);

pub const SIMD_V128_NOT: u32 = SimdTag::V128Not as u32;
pub const SIMD_V128_AND: u32 = SimdTag::V128And as u32;
pub const SIMD_V128_ANDNOT: u32 = SimdTag::V128AndNot as u32;
pub const SIMD_V128_OR: u32 = SimdTag::V128Or as u32;
pub const SIMD_V128_XOR: u32 = SimdTag::V128Xor as u32;
pub const SIMD_I8X16_SWIZZLE: u32 = SimdTag::I8x16Swizzle as u32;
pub const SIMD_I8X16_SPLAT: u32 = SimdTag::I8x16Splat as u32;
pub const SIMD_I16X8_SPLAT: u32 = SimdTag::I16x8Splat as u32;
pub const SIMD_I32X4_SPLAT: u32 = SimdTag::I32x4Splat as u32;
pub const SIMD_I64X2_SPLAT: u32 = SimdTag::I64x2Splat as u32;
pub const SIMD_F32X4_SPLAT: u32 = SimdTag::F32x4Splat as u32;
pub const SIMD_F64X2_SPLAT: u32 = SimdTag::F64x2Splat as u32;
pub const SIMD_I8X16_EXTRACT_S: u32 = SimdTag::I8x16ExtractLaneS as u32;
pub const SIMD_I8X16_EXTRACT_U: u32 = SimdTag::I8x16ExtractLaneU as u32;
pub const SIMD_I16X8_EXTRACT_S: u32 = SimdTag::I16x8ExtractLaneS as u32;
pub const SIMD_I16X8_EXTRACT_U: u32 = SimdTag::I16x8ExtractLaneU as u32;
pub const SIMD_I32X4_EXTRACT: u32 = SimdTag::I32x4ExtractLane as u32;
pub const SIMD_I64X2_EXTRACT: u32 = SimdTag::I64x2ExtractLane as u32;
pub const SIMD_F32X4_EXTRACT: u32 = SimdTag::F32x4ExtractLane as u32;
pub const SIMD_F64X2_EXTRACT: u32 = SimdTag::F64x2ExtractLane as u32;
pub const SIMD_I8X16_REPLACE: u32 = SimdTag::I8x16ReplaceLane as u32;
pub const SIMD_I16X8_REPLACE: u32 = SimdTag::I16x8ReplaceLane as u32;
pub const SIMD_I32X4_REPLACE: u32 = SimdTag::I32x4ReplaceLane as u32;
pub const SIMD_I64X2_REPLACE: u32 = SimdTag::I64x2ReplaceLane as u32;
pub const SIMD_F32X4_REPLACE: u32 = SimdTag::F32x4ReplaceLane as u32;
pub const SIMD_F64X2_REPLACE: u32 = SimdTag::F64x2ReplaceLane as u32;
pub const SIMD_F32X4_ADD: u32 = SimdTag::F32x4Add as u32;
pub const SIMD_F32X4_SUB: u32 = SimdTag::F32x4Sub as u32;
pub const SIMD_F32X4_MUL: u32 = SimdTag::F32x4Mul as u32;
pub const SIMD_F32X4_DIV: u32 = SimdTag::F32x4Div as u32;
pub const SIMD_F32X4_EQ: u32 = SimdTag::F32x4Eq as u32;
pub const SIMD_F32X4_NE: u32 = SimdTag::F32x4Ne as u32;
pub const SIMD_F32X4_LT: u32 = SimdTag::F32x4Lt as u32;
pub const SIMD_F32X4_GT: u32 = SimdTag::F32x4Gt as u32;
pub const SIMD_F32X4_LE: u32 = SimdTag::F32x4Le as u32;
pub const SIMD_F32X4_GE: u32 = SimdTag::F32x4Ge as u32;
pub const SIMD_F32X4_ABS: u32 = SimdTag::F32x4Abs as u32;
pub const SIMD_F32X4_NEG: u32 = SimdTag::F32x4Neg as u32;
pub const SIMD_F64X2_ADD: u32 = SimdTag::F64x2Add as u32;
pub const SIMD_F64X2_SUB: u32 = SimdTag::F64x2Sub as u32;
pub const SIMD_F64X2_MUL: u32 = SimdTag::F64x2Mul as u32;
pub const SIMD_F64X2_DIV: u32 = SimdTag::F64x2Div as u32;
pub const SIMD_F64X2_EQ: u32 = SimdTag::F64x2Eq as u32;
pub const SIMD_F64X2_NE: u32 = SimdTag::F64x2Ne as u32;
pub const SIMD_F64X2_LT: u32 = SimdTag::F64x2Lt as u32;
pub const SIMD_F64X2_GT: u32 = SimdTag::F64x2Gt as u32;
pub const SIMD_F64X2_LE: u32 = SimdTag::F64x2Le as u32;
pub const SIMD_F64X2_GE: u32 = SimdTag::F64x2Ge as u32;
pub const SIMD_F64X2_ABS: u32 = SimdTag::F64x2Abs as u32;
pub const SIMD_F64X2_NEG: u32 = SimdTag::F64x2Neg as u32;
pub const SIMD_I8X16_EQ: u32 = SimdTag::I8x16Eq as u32;
pub const SIMD_I8X16_NE: u32 = SimdTag::I8x16Ne as u32;
pub const SIMD_I8X16_LT_S: u32 = SimdTag::I8x16LtS as u32;
pub const SIMD_I8X16_GT_S: u32 = SimdTag::I8x16GtS as u32;
pub const SIMD_I8X16_LE_S: u32 = SimdTag::I8x16LeS as u32;
pub const SIMD_I8X16_GE_S: u32 = SimdTag::I8x16GeS as u32;
pub const SIMD_I8X16_LT_U: u32 = SimdTag::I8x16LtU as u32;
pub const SIMD_I8X16_GT_U: u32 = SimdTag::I8x16GtU as u32;
pub const SIMD_I8X16_LE_U: u32 = SimdTag::I8x16LeU as u32;
pub const SIMD_I8X16_GE_U: u32 = SimdTag::I8x16GeU as u32;
pub const SIMD_I8X16_ADD: u32 = SimdTag::I8x16Add as u32;
pub const SIMD_I8X16_SUB: u32 = SimdTag::I8x16Sub as u32;
pub const SIMD_I8X16_ABS: u32 = SimdTag::I8x16Abs as u32;
pub const SIMD_I8X16_NEG: u32 = SimdTag::I8x16Neg as u32;
pub const SIMD_I16X8_EQ: u32 = SimdTag::I16x8Eq as u32;
pub const SIMD_I16X8_NE: u32 = SimdTag::I16x8Ne as u32;
pub const SIMD_I16X8_ADD: u32 = SimdTag::I16x8Add as u32;
pub const SIMD_I16X8_SUB: u32 = SimdTag::I16x8Sub as u32;
pub const SIMD_I16X8_MUL: u32 = SimdTag::I16x8Mul as u32;
pub const SIMD_I32X4_EQ: u32 = SimdTag::I32x4Eq as u32;
pub const SIMD_I32X4_NE: u32 = SimdTag::I32x4Ne as u32;
pub const SIMD_I32X4_ADD: u32 = SimdTag::I32x4Add as u32;
pub const SIMD_I32X4_SUB: u32 = SimdTag::I32x4Sub as u32;
pub const SIMD_I32X4_MUL: u32 = SimdTag::I32x4Mul as u32;
pub const SIMD_I64X2_ADD: u32 = SimdTag::I64x2Add as u32;
pub const SIMD_I64X2_SUB: u32 = SimdTag::I64x2Sub as u32;
pub const SIMD_I64X2_MUL: u32 = SimdTag::I64x2Mul as u32;
pub const SIMD_I8X16_MIN_S: u32 = SimdTag::I8x16MinS as u32;
pub const SIMD_I8X16_MIN_U: u32 = SimdTag::I8x16MinU as u32;
pub const SIMD_I8X16_MAX_S: u32 = SimdTag::I8x16MaxS as u32;
pub const SIMD_I8X16_MAX_U: u32 = SimdTag::I8x16MaxU as u32;
pub const SIMD_I16X8_MIN_S: u32 = SimdTag::I16x8MinS as u32;
pub const SIMD_I16X8_MIN_U: u32 = SimdTag::I16x8MinU as u32;
pub const SIMD_I16X8_MAX_S: u32 = SimdTag::I16x8MaxS as u32;
pub const SIMD_I16X8_MAX_U: u32 = SimdTag::I16x8MaxU as u32;
pub const SIMD_I32X4_MIN_S: u32 = SimdTag::I32x4MinS as u32;
pub const SIMD_I32X4_MIN_U: u32 = SimdTag::I32x4MinU as u32;
pub const SIMD_I32X4_MAX_S: u32 = SimdTag::I32x4MaxS as u32;
pub const SIMD_I32X4_MAX_U: u32 = SimdTag::I32x4MaxU as u32;
pub const SIMD_I8X16_ADD_SAT_S: u32 = SimdTag::I8x16AddSatS as u32;
pub const SIMD_I8X16_ADD_SAT_U: u32 = SimdTag::I8x16AddSatU as u32;
pub const SIMD_I8X16_SUB_SAT_S: u32 = SimdTag::I8x16SubSatS as u32;
pub const SIMD_I8X16_SUB_SAT_U: u32 = SimdTag::I8x16SubSatU as u32;
pub const SIMD_I16X8_ADD_SAT_S: u32 = SimdTag::I16x8AddSatS as u32;
pub const SIMD_I16X8_ADD_SAT_U: u32 = SimdTag::I16x8AddSatU as u32;
pub const SIMD_I16X8_SUB_SAT_S: u32 = SimdTag::I16x8SubSatS as u32;
pub const SIMD_I16X8_SUB_SAT_U: u32 = SimdTag::I16x8SubSatU as u32;
pub const SIMD_I8X16_AVGR_U: u32 = SimdTag::I8x16AvgrU as u32;
pub const SIMD_I16X8_AVGR_U: u32 = SimdTag::I16x8AvgrU as u32;
pub const SIMD_I16X8_Q15_MULR_SAT_S: u32 = SimdTag::I16x8Q15MulrSatS as u32;
pub const SIMD_I16X8_RELAXED_Q15MULR_S: u32 = SimdTag::I16x8RelaxedQ15mulrS as u32;
pub const SIMD_I8X16_RELAXED_SWIZZLE: u32 = SimdTag::I8x16RelaxedSwizzle as u32;
pub const SIMD_I16X8_LT_S: u32 = SimdTag::I16x8LtS as u32;
pub const SIMD_I16X8_GT_S: u32 = SimdTag::I16x8GtS as u32;
pub const SIMD_I16X8_LE_S: u32 = SimdTag::I16x8LeS as u32;
pub const SIMD_I16X8_GE_S: u32 = SimdTag::I16x8GeS as u32;
pub const SIMD_I16X8_LT_U: u32 = SimdTag::I16x8LtU as u32;
pub const SIMD_I16X8_GT_U: u32 = SimdTag::I16x8GtU as u32;
pub const SIMD_I16X8_LE_U: u32 = SimdTag::I16x8LeU as u32;
pub const SIMD_I16X8_GE_U: u32 = SimdTag::I16x8GeU as u32;
pub const SIMD_I32X4_LT_S: u32 = SimdTag::I32x4LtS as u32;
pub const SIMD_I32X4_GT_S: u32 = SimdTag::I32x4GtS as u32;
pub const SIMD_I32X4_LE_S: u32 = SimdTag::I32x4LeS as u32;
pub const SIMD_I32X4_GE_S: u32 = SimdTag::I32x4GeS as u32;
pub const SIMD_I32X4_LT_U: u32 = SimdTag::I32x4LtU as u32;
pub const SIMD_I32X4_GT_U: u32 = SimdTag::I32x4GtU as u32;
pub const SIMD_I32X4_LE_U: u32 = SimdTag::I32x4LeU as u32;
pub const SIMD_I32X4_GE_U: u32 = SimdTag::I32x4GeU as u32;
pub const SIMD_I64X2_EQ: u32 = SimdTag::I64x2Eq as u32;
pub const SIMD_I64X2_NE: u32 = SimdTag::I64x2Ne as u32;
pub const SIMD_I64X2_LT_S: u32 = SimdTag::I64x2LtS as u32;
pub const SIMD_I64X2_GT_S: u32 = SimdTag::I64x2GtS as u32;
pub const SIMD_I64X2_LE_S: u32 = SimdTag::I64x2LeS as u32;
pub const SIMD_I64X2_GE_S: u32 = SimdTag::I64x2GeS as u32;
pub const SIMD_F32X4_MIN: u32 = SimdTag::F32x4Min as u32;
pub const SIMD_F32X4_MAX: u32 = SimdTag::F32x4Max as u32;
pub const SIMD_F32X4_PMIN: u32 = SimdTag::F32x4PMin as u32;
pub const SIMD_F32X4_PMAX: u32 = SimdTag::F32x4PMax as u32;
pub const SIMD_F32X4_RELAXED_MIN: u32 = SimdTag::F32x4RelaxedMin as u32;
pub const SIMD_F32X4_RELAXED_MAX: u32 = SimdTag::F32x4RelaxedMax as u32;
pub const SIMD_F64X2_MIN: u32 = SimdTag::F64x2Min as u32;
pub const SIMD_F64X2_MAX: u32 = SimdTag::F64x2Max as u32;
pub const SIMD_F64X2_PMIN: u32 = SimdTag::F64x2PMin as u32;
pub const SIMD_F64X2_PMAX: u32 = SimdTag::F64x2PMax as u32;
pub const SIMD_F64X2_RELAXED_MIN: u32 = SimdTag::F64x2RelaxedMin as u32;
pub const SIMD_F64X2_RELAXED_MAX: u32 = SimdTag::F64x2RelaxedMax as u32;
pub const SIMD_I8X16_SHL: u32 = SimdTag::I8x16Shl as u32;
pub const SIMD_I8X16_SHR_S: u32 = SimdTag::I8x16ShrS as u32;
pub const SIMD_I8X16_SHR_U: u32 = SimdTag::I8x16ShrU as u32;
pub const SIMD_I16X8_SHL: u32 = SimdTag::I16x8Shl as u32;
pub const SIMD_I16X8_SHR_S: u32 = SimdTag::I16x8ShrS as u32;
pub const SIMD_I16X8_SHR_U: u32 = SimdTag::I16x8ShrU as u32;
pub const SIMD_I32X4_SHL: u32 = SimdTag::I32x4Shl as u32;
pub const SIMD_I32X4_SHR_S: u32 = SimdTag::I32x4ShrS as u32;
pub const SIMD_I32X4_SHR_U: u32 = SimdTag::I32x4ShrU as u32;
pub const SIMD_I64X2_SHL: u32 = SimdTag::I64x2Shl as u32;
pub const SIMD_I64X2_SHR_S: u32 = SimdTag::I64x2ShrS as u32;
pub const SIMD_I64X2_SHR_U: u32 = SimdTag::I64x2ShrU as u32;
pub const SIMD_V128_ANY_TRUE: u32 = SimdTag::V128AnyTrue as u32;
pub const SIMD_I8X16_ALL_TRUE: u32 = SimdTag::I8x16AllTrue as u32;
pub const SIMD_I8X16_BITMASK: u32 = SimdTag::I8x16Bitmask as u32;
pub const SIMD_I16X8_ALL_TRUE: u32 = SimdTag::I16x8AllTrue as u32;
pub const SIMD_I16X8_BITMASK: u32 = SimdTag::I16x8Bitmask as u32;
pub const SIMD_I32X4_ALL_TRUE: u32 = SimdTag::I32x4AllTrue as u32;
pub const SIMD_I32X4_BITMASK: u32 = SimdTag::I32x4Bitmask as u32;
pub const SIMD_I64X2_ALL_TRUE: u32 = SimdTag::I64x2AllTrue as u32;
pub const SIMD_I64X2_BITMASK: u32 = SimdTag::I64x2Bitmask as u32;
pub const SIMD_I8X16_POPCNT: u32 = SimdTag::I8x16Popcnt as u32;
pub const SIMD_I16X8_ABS: u32 = SimdTag::I16x8Abs as u32;
pub const SIMD_I16X8_NEG: u32 = SimdTag::I16x8Neg as u32;
pub const SIMD_I32X4_ABS: u32 = SimdTag::I32x4Abs as u32;
pub const SIMD_I32X4_NEG: u32 = SimdTag::I32x4Neg as u32;
pub const SIMD_I64X2_ABS: u32 = SimdTag::I64x2Abs as u32;
pub const SIMD_I64X2_NEG: u32 = SimdTag::I64x2Neg as u32;
pub const SIMD_F32X4_SQRT: u32 = SimdTag::F32x4Sqrt as u32;
pub const SIMD_F32X4_CEIL: u32 = SimdTag::F32x4Ceil as u32;
pub const SIMD_F32X4_FLOOR: u32 = SimdTag::F32x4Floor as u32;
pub const SIMD_F32X4_TRUNC: u32 = SimdTag::F32x4Trunc as u32;
pub const SIMD_F32X4_NEAREST: u32 = SimdTag::F32x4Nearest as u32;
pub const SIMD_F64X2_SQRT: u32 = SimdTag::F64x2Sqrt as u32;
pub const SIMD_F64X2_CEIL: u32 = SimdTag::F64x2Ceil as u32;
pub const SIMD_F64X2_FLOOR: u32 = SimdTag::F64x2Floor as u32;
pub const SIMD_F64X2_TRUNC: u32 = SimdTag::F64x2Trunc as u32;
pub const SIMD_F64X2_NEAREST: u32 = SimdTag::F64x2Nearest as u32;
pub const SIMD_I16X8_EXTEND_LOW_I8X16_S: u32 = SimdTag::I16x8ExtendLowI8x16S as u32;
pub const SIMD_I16X8_EXTEND_LOW_I8X16_U: u32 = SimdTag::I16x8ExtendLowI8x16U as u32;
pub const SIMD_I16X8_EXTEND_HIGH_I8X16_S: u32 = SimdTag::I16x8ExtendHighI8x16S as u32;
pub const SIMD_I16X8_EXTEND_HIGH_I8X16_U: u32 = SimdTag::I16x8ExtendHighI8x16U as u32;
pub const SIMD_I32X4_EXTEND_LOW_I16X8_S: u32 = SimdTag::I32x4ExtendLowI16x8S as u32;
pub const SIMD_I32X4_EXTEND_LOW_I16X8_U: u32 = SimdTag::I32x4ExtendLowI16x8U as u32;
pub const SIMD_I32X4_EXTEND_HIGH_I16X8_S: u32 = SimdTag::I32x4ExtendHighI16x8S as u32;
pub const SIMD_I32X4_EXTEND_HIGH_I16X8_U: u32 = SimdTag::I32x4ExtendHighI16x8U as u32;
pub const SIMD_I64X2_EXTEND_LOW_I32X4_S: u32 = SimdTag::I64x2ExtendLowI32x4S as u32;
pub const SIMD_I64X2_EXTEND_LOW_I32X4_U: u32 = SimdTag::I64x2ExtendLowI32x4U as u32;
pub const SIMD_I64X2_EXTEND_HIGH_I32X4_S: u32 = SimdTag::I64x2ExtendHighI32x4S as u32;
pub const SIMD_I64X2_EXTEND_HIGH_I32X4_U: u32 = SimdTag::I64x2ExtendHighI32x4U as u32;
pub const SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S: u32 = SimdTag::I16x8ExtAddPairwiseI8x16S as u32;
pub const SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U: u32 = SimdTag::I16x8ExtAddPairwiseI8x16U as u32;
pub const SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S: u32 = SimdTag::I32x4ExtAddPairwiseI16x8S as u32;
pub const SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U: u32 = SimdTag::I32x4ExtAddPairwiseI16x8U as u32;
pub const SIMD_I8X16_NARROW_I16X8_S: u32 = SimdTag::I8x16NarrowI16x8S as u32;
pub const SIMD_I8X16_NARROW_I16X8_U: u32 = SimdTag::I8x16NarrowI16x8U as u32;
pub const SIMD_I16X8_NARROW_I32X4_S: u32 = SimdTag::I16x8NarrowI32x4S as u32;
pub const SIMD_I16X8_NARROW_I32X4_U: u32 = SimdTag::I16x8NarrowI32x4U as u32;
pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_S: u32 = SimdTag::I16x8ExtMulLowI8x16S as u32;
pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_U: u32 = SimdTag::I16x8ExtMulLowI8x16U as u32;
pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_S: u32 = SimdTag::I16x8ExtMulHighI8x16S as u32;
pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_U: u32 = SimdTag::I16x8ExtMulHighI8x16U as u32;
pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_S: u32 = SimdTag::I32x4ExtMulLowI16x8S as u32;
pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_U: u32 = SimdTag::I32x4ExtMulLowI16x8U as u32;
pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_S: u32 = SimdTag::I32x4ExtMulHighI16x8S as u32;
pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_U: u32 = SimdTag::I32x4ExtMulHighI16x8U as u32;
pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_S: u32 = SimdTag::I64x2ExtMulLowI32x4S as u32;
pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_U: u32 = SimdTag::I64x2ExtMulLowI32x4U as u32;
pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_S: u32 = SimdTag::I64x2ExtMulHighI32x4S as u32;
pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_U: u32 = SimdTag::I64x2ExtMulHighI32x4U as u32;
pub const SIMD_I32X4_DOT_I16X8_S: u32 = SimdTag::I32x4DotI16x8S as u32;
pub const SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S: u32 = SimdTag::I16x8RelaxedDotI8x16I7x16S as u32;
pub const SIMD_I32X4_TRUNC_SAT_F32X4_S: u32 = SimdTag::I32x4TruncSatF32x4S as u32;
pub const SIMD_I32X4_TRUNC_SAT_F32X4_U: u32 = SimdTag::I32x4TruncSatF32x4U as u32;
pub const SIMD_F32X4_CONVERT_I32X4_S: u32 = SimdTag::F32x4ConvertI32x4S as u32;
pub const SIMD_F32X4_CONVERT_I32X4_U: u32 = SimdTag::F32x4ConvertI32x4U as u32;
pub const SIMD_I32X4_TRUNC_SAT_F64X2_SZERO: u32 = SimdTag::I32x4TruncSatF64x2SZero as u32;
pub const SIMD_I32X4_TRUNC_SAT_F64X2_UZERO: u32 = SimdTag::I32x4TruncSatF64x2UZero as u32;
pub const SIMD_F64X2_CONVERT_LOW_I32X4_S: u32 = SimdTag::F64x2ConvertLowI32x4S as u32;
pub const SIMD_F64X2_CONVERT_LOW_I32X4_U: u32 = SimdTag::F64x2ConvertLowI32x4U as u32;
pub const SIMD_F32X4_DEMOTE_F64X2_ZERO: u32 = SimdTag::F32x4DemoteF64x2Zero as u32;
pub const SIMD_F64X2_PROMOTE_LOW_F32X4: u32 = SimdTag::F64x2PromoteLowF32x4 as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_S: u32 = SimdTag::I32x4RelaxedTruncF32x4S as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_U: u32 = SimdTag::I32x4RelaxedTruncF32x4U as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO: u32 = SimdTag::I32x4RelaxedTruncF64x2SZero as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO: u32 = SimdTag::I32x4RelaxedTruncF64x2UZero as u32;
pub const SIMD_V128_LOAD8X8_S: u32 = MemoryLoadTag::V128Load8x8S as u32;
pub const SIMD_V128_LOAD8X8_U: u32 = MemoryLoadTag::V128Load8x8U as u32;
pub const SIMD_V128_LOAD16X4_S: u32 = MemoryLoadTag::V128Load16x4S as u32;
pub const SIMD_V128_LOAD16X4_U: u32 = MemoryLoadTag::V128Load16x4U as u32;
pub const SIMD_V128_LOAD32X2_S: u32 = MemoryLoadTag::V128Load32x2S as u32;
pub const SIMD_V128_LOAD32X2_U: u32 = MemoryLoadTag::V128Load32x2U as u32;
pub const SIMD_V128_LOAD8_SPLAT: u32 = MemoryLoadTag::V128Load8Splat as u32;
pub const SIMD_V128_LOAD16_SPLAT: u32 = MemoryLoadTag::V128Load16Splat as u32;
pub const SIMD_V128_LOAD32_SPLAT: u32 = MemoryLoadTag::V128Load32Splat as u32;
pub const SIMD_V128_LOAD64_SPLAT: u32 = MemoryLoadTag::V128Load64Splat as u32;
pub const SIMD_V128_LOAD32_ZERO: u32 = MemoryLoadTag::V128Load32Zero as u32;
pub const SIMD_V128_LOAD64_ZERO: u32 = MemoryLoadTag::V128Load64Zero as u32;
