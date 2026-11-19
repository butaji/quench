//! Which Wasm rows the library holds, as codes (`wasm_codes.rs`): the one definition `build.rs`
//! generates the stencils from and the stitcher looks rows up by.

#[path = "wasm_codes.rs"]
mod wasm_codes;
pub use wasm_codes::*;

/// The routines around a function's rows (`wasm_entry.rs` on x86-64, `wasm_entry_a64.rs` on AArch64): its entry and the leaves
/// with a word, with none, and with a trap's status.
#[allow(dead_code)] // `build.rs` includes this file and builds the routines by name, not from this list.
pub const ROUTINES: [&str; 4] = ["entry_wasm", "leave_wasm_word", "leave_wasm_void", "leave_wasm_trap"];

/// A row as codes: kind, operator tag, width, shape.
pub type WasmCode = (u8, u32, u8, u8);

/// Every row: each operator at each width it has, with a register or an immediate second
/// operand where it has one, and the rows that compute nothing: moves, constants and jumps.
pub fn wasm_library() -> Vec<WasmCode> {
    let mut rows = vec![(MOVE, 0, W64, REGISTER), (CONSTANT, 0, W64, IMMEDIATE), (GOTO, 0, W64, REGISTER), (CALL, 0, W64, REGISTER), (SELECT, 0, W64, REGISTER), (TRAP, 0, W64, REGISTER), (MEMORY_SIZE, 0, W64, REGISTER), (MEMORY_FILL, 0, W64, REGISTER), (MEMORY_COPY, 0, W64, REGISTER), (CALL_INDIRECT, 0, W64, REGISTER), (MEMORY_GROW, 0, W64, REGISTER), (CALL_VM, 0, W64, REGISTER)];
    for storage in [STORAGE_BITS32, STORAGE_BITS64] {
        rows.push((GLOBAL_GET, storage, W64, REGISTER));
        rows.push((GLOBAL_SET, storage, W64, REGISTER));
    }
    for float in [W32, W64] {
        for operator in (0..FLOAT_BINARY_COUNT).filter(|operator| !FLOAT_BINARY_NEEDS_CONSTANTS.contains(operator)) {
            rows.push((FLOAT_BINARY, operator, float, REGISTER));
        }
        for operator in 0..FLOAT_UNARY_COUNT {
            rows.push((FLOAT_UNARY, operator, float, REGISTER));
        }
    }
    for operator in (0..CONVERT_COUNT).filter(|operator| !CONVERT_NEEDS_CONSTANTS.contains(operator)) {
        rows.push((CONVERT, operator, W64, REGISTER));
    }
    for width in [W32, W64] {
        let operators = |shared, all| if width == W32 { shared } else { all };
        for operator in 0..operators(SHARED_BINARY, BINARY_COUNT) {
            for shape in [REGISTER, IMMEDIATE] {
                rows.push((BINARY, operator, width, shape));
            }
        }
        for operator in 0..operators(SHARED_UNARY, UNARY_COUNT) {
            rows.push((UNARY, operator, width, REGISTER));
        }
        for operator in FIRST_COMPARISON..=LAST_COMPARISON {
            for shape in [REGISTER, IMMEDIATE] {
                rows.push((JUMP_IF, operator, width, shape));
            }
        }
    }
    for (index, (_, _, signed)) in ACCESSES.iter().enumerate() {
        rows.push((LOAD, index as u32, W64, REGISTER));
        if !signed {
            rows.push((STORE, index as u32, W64, REGISTER));
        }
    }
    for (kind, operators) in [
        (SIMD_BINARY, SIMD_BINARY_OPS),
        (SIMD_UNARY, SIMD_UNARY_OPS),
        (SIMD_SPLAT, SIMD_SPLAT_OPS),
        (SIMD_EXTRACT, SIMD_EXTRACT_OPS),
        (SIMD_REPLACE, SIMD_REPLACE_OPS),
    ] {
        for &operator in operators {
            rows.push((kind, operator, W64, REGISTER));
        }
    }
    for kind in [SIMD_SHUFFLE, SIMD_CONST, SIMD_LOAD, SIMD_STORE, SIMD_MOVE] {
        rows.push((kind, 0, W64, REGISTER));
    }
    for operator in 0..=(CALLOUT_RESULT | CALLOUT_WRAP) {
        rows.push((CALLOUT, operator, W64, REGISTER));
    }
    rows.push((ROOT_PUSH, 0, W64, REGISTER));
    rows.push((ROOT_POP, 0, W64, REGISTER));
    rows.push((GC_ALLOCATE, 0, W64, REGISTER));
    for (kind, operators) in [
        (SIMD_MORE_BINARY, SIMD_MORE_BINARY_OPS),
        (SIMD_MORE_COMPARE, SIMD_MORE_COMPARE_OPS),
        (SIMD_MORE_FLOAT, SIMD_MORE_FLOAT_OPS),
        (SIMD_MORE_SHIFT, SIMD_MORE_SHIFT_OPS),
        (SIMD_MORE_TEST, SIMD_MORE_TEST_OPS),
        (SIMD_MORE_UNARY, SIMD_MORE_UNARY_OPS),
        (SIMD_MORE_WIDEN, SIMD_MORE_WIDEN_OPS),
        (SIMD_MORE_WIDE, SIMD_MORE_WIDE_OPS),
        (SIMD_MORE_CONVERT, SIMD_MORE_CONVERT_OPS),
        (SIMD_MORE_LOAD, SIMD_MORE_LOAD_OPS),
    ] {
        for &operator in operators {
            rows.push((kind, operator, W64, REGISTER));
        }
    }
    rows
}

/// The name of a Wasm row's stencil symbol.
pub fn wasm_name((kind, operator, width, shape): WasmCode) -> String {
    // A row that calls must be named so (`build/checks.rs`).
    let call = if matches!(kind, CALL | CALL_INDIRECT | CALL_VM | MEMORY_GROW | CALLOUT | GC_ALLOCATE) { "call_" } else { "" };
    format!("{call}w_{kind}_{operator}_{width}_{shape}")
}
