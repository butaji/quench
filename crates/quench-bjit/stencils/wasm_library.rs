//! Which Wasm rows the library holds, as codes (`wasm_codes.rs`): the one definition `build.rs`
//! generates the stencils from and the stitcher looks rows up by.

#[path = "wasm_codes.rs"]
mod wasm_codes;
pub use wasm_codes::*;

/// A row as codes: kind, operator tag, width, shape.
pub type WasmCode = (u8, u32, u8, u8);

/// Every row: each operator at each width it has, with a register or an immediate second
/// operand where it has one, and the rows that compute nothing: moves, constants and jumps.
pub fn wasm_library() -> Vec<WasmCode> {
    let mut rows = vec![(MOVE, 0, W64, REGISTER), (CONSTANT, 0, W64, IMMEDIATE), (GOTO, 0, W64, REGISTER), (CALL, 0, W64, REGISTER), (SELECT, 0, W64, REGISTER), (TRAP, 0, W64, REGISTER)];
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
    rows
}

/// The name of a Wasm row's stencil symbol.
pub fn wasm_name((kind, operator, width, shape): WasmCode) -> String {
    // A row that calls must be named so (`build/checks.rs`).
    let call = if kind == CALL { "call_" } else { "" };
    format!("{call}w_{kind}_{operator}_{width}_{shape}")
}
