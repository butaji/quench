//! Which Wasm rows the library holds, as codes (`wasm_codes.rs`): built from the kind table there
//! (`ROW_FAMILIES`), the one definition `build.rs` generates the stencils from and the stitcher
//! looks rows up by.

#[path = "wasm_codes.rs"]
mod wasm_codes;
pub use wasm_codes::*;

/// The routines around a function's rows (`wasm_entry.rs` on x86-64, `wasm_entry_a64.rs` on AArch64): its entry and the leaves
/// with a word, with none, and with a trap's status. `build.rs` requires each to be a built row.
pub const ROUTINES: [&str; 4] = ["entry_wasm", "leave_wasm_word", "leave_wasm_void", "leave_wasm_trap"];

/// A row as codes: kind, operator tag, width, shape.
pub type WasmCode = (u8, u32, u8, u8);

/// The operators a family holds, in order.
fn operators(ops: RowOps) -> Vec<u32> {
    let accesses = 0..ACCESSES.len() as u32;
    match ops {
        RowOps::Span(first, end) => (first..end).collect(),
        RowOps::Except(first, end, skipped) => (first..end).filter(|operator| !skipped.contains(operator)).collect(),
        RowOps::List(list) => list.to_vec(),
        RowOps::Accesses => accesses.collect(),
        RowOps::UnsignedAccesses => accesses.filter(|&index| !ACCESSES[index as usize].2).collect(),
    }
}

/// Every row the table names: each kind's families (`ROW_FAMILIES`), each family's operators at
/// each of its shapes.
pub fn wasm_library() -> Vec<WasmCode> {
    let mut rows = Vec::new();
    for &(kind, families) in ROW_FAMILIES {
        for family in families {
            for operator in operators(family.ops) {
                for &shape in family.shapes {
                    rows.push((kind, operator, family.width, shape));
                }
            }
        }
    }
    rows
}

/// The name of a Wasm row's stencil symbol.
pub fn wasm_name((kind, operator, width, shape): WasmCode) -> String {
    // A row that calls must be named so (`build/checks.rs`).
    let call = if matches!(kind, CALL | CALL_KEEP | CALL_INDIRECT | CALL_VM | MEMORY_GROW | CALLOUT | GC_ALLOCATE) { "call_" } else { "" };
    format!("{call}w_{kind}_{operator}_{width}_{shape}")
}
