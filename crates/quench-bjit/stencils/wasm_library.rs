//! Which Wasm rows the library holds, as codes (`wasm_codes.rs`): the one definition `build.rs`
//! generates the stencils from and the stitcher looks rows up by.

#[path = "wasm_codes.rs"]
mod wasm_codes;
pub use wasm_codes::*;

/// A row as codes: kind, operator tag, width, shape.
pub type WasmCode = (u8, u32, u8, u8);

/// Every row: each operator at each width it has, with a register or an immediate second
/// operand where it has one.
pub fn wasm_library() -> Vec<WasmCode> {
    let mut rows = Vec::new();
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
    rows
}

/// The name of a Wasm row's stencil symbol.
pub fn wasm_name((kind, operator, width, shape): WasmCode) -> String {
    format!("w_{kind}_{operator}_{width}_{shape}")
}
