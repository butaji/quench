//! The Wasm row kinds and the codes of their operands, which every context that names a Wasm code
//! reads: the stencil rows dispatch on the kinds, `wasm_library.rs` builds the library from them and
//! from `wasm_families.rs`, and the runtime names them. A kind's code is its position in the one kind
//! table, `row_kind_table!` below: each context expands that table through a callback, so the table
//! is written once and no code is typed by hand. The kinds are numbered here (`kind_codes`);
//! `wasm_families.rs` builds `ROW_FAMILIES` from the same table (`row_families`).
//!
//! Operator tags and counts that the runtime defines are asserted equal to the runtime's values in
//! `quench-runtime/src/vm/compiled/wasm_codes_check.rs`, because this crate (compiled alone as the
//! stencil build) cannot see them.
//!
//! The contexts include only the files they read: the stencil crate (`rows.rs`) includes this file
//! and `wasm_layout.rs`, the build script (`build.rs`) includes this file and `wasm_families.rs`
//! through `wasm_library.rs`, and the lib (`src/lib.rs`) includes all five.

#[macro_use]
#[path = "../../quench-runtime/src/staged/wasm_lists.rs"]
mod wasm_lists;

/// The kind callback: numbers the kinds of the table, counted from 0, so a kind added anywhere
/// renumbers the kinds after it; the names that carry a code follow, since `build.rs` and the
/// stitcher both read these constants. The families of each kind are the other callbacks' business.
macro_rules! kind_codes {
    (@number $n:expr;) => {};
    (@number $n:expr; $kind:ident $(, $rest:ident)*) => {
        pub const $kind: u8 = $n;
        kind_codes!(@number $n + 1; $($rest),*);
    };
    ($($kind:ident => [$($family:expr),* $(,)?]),* $(,)?) => {
        kind_codes!(@number 0; $($kind),*);
    };
}

/// The row kinds and their families, one table: each kind's name and its families, in table order.
/// `$callback` names the macro that expands the table (`kind_codes` here, `row_families` in
/// `wasm_families.rs`). The families are expressions over the operator lists and the families'
/// constants, which only the callbacks that use them resolve.
macro_rules! row_kind_table {
    ($callback:ident) => {
        $callback! {
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
            // `CALL` for a callee that cannot grow the memory: the frame's base and length are kept, not
            // reloaded (`wasm_frame.rs`, `restore_memory`). A `call_` row.
            CALL_KEEP => [row_family(RowOps::Span(0, 1), W64, &[REGISTER])],
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
    };
}
pub(crate) use row_kind_table;
row_kind_table!(kind_codes);

/// The value types of an access, the codes `T_*` name.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum AccessType {
    I32, I64, F32, F64,
}

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

/// The shape of a row's second operand: a register, or an immediate in the row's hole.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum OperandShape {
    Register, Immediate,
}

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

/// A global's storage operator codes: a 32-bit payload, a 64-bit payload.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum StorageCode {
    Bits32, Bits64,
}
pub const STORAGE_BITS32: u32 = StorageCode::Bits32 as u32;

