//! Const string equality: its own file so the b-jit crate, which cannot depend on the runtime,
//! mounts the same definition (`quench-bjit/stencils/wasm_families.rs`).

/// Whether `left` and `right` spell the same text, usable in constant evaluation (where
/// `str == str` is not). The one home for const string equality of the runtime and its hosts.
pub const fn same_text(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}
