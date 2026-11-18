//! Which Wasm rows the library holds, as codes (`wasm_codes.rs`): the one definition `build.rs`
//! generates the stencils from and the stitcher looks rows up by.

#[path = "wasm_codes.rs"]
mod wasm_codes;
pub use wasm_codes::*;

/// The routines around a function's rows (`wasm_entry.rs`, x86-64): its entry and the leaves
/// with a word, with none, and with a trap's status.
#[allow(dead_code)] // `build.rs` includes this file and builds the routines by name, not from this list.
pub const ROUTINES: [&str; 4] = ["entry_wasm", "leave_wasm_word", "leave_wasm_void", "leave_wasm_trap"];

/// A row as codes: kind, operator tag, width, shape.
pub type WasmCode = (u8, u32, u8, u8);

/// Every row: each operator at each width it has, with a register or an immediate second
/// operand where it has one, and the rows that compute nothing: moves, constants and jumps.
pub fn wasm_library() -> Vec<WasmCode> {
    let mut rows = vec![(MOVE, 0, W64, REGISTER), (CONSTANT, 0, W64, IMMEDIATE), (GOTO, 0, W64, REGISTER), (CALL, 0, W64, REGISTER)];
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
    rows
}

/// The name of a Wasm row's stencil symbol.
pub fn wasm_name((kind, operator, width, shape): WasmCode) -> String {
    // A row that calls must be named so (`build/checks.rs`).
    let call = if kind == CALL { "call_" } else { "" };
    format!("{call}w_{kind}_{operator}_{width}_{shape}")
}
