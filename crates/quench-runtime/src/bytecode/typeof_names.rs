//! The names `typeof` yields and their indices: self-contained, so the stencil build reads
//! the same file (`quench-bjit/stencils/facade.rs`).

/// The names `typeof` yields (ECMA-262 13.5.3.1), indexed as `typeof_name_index` (vm/coercion.rs) classifies
/// values.
pub const TYPEOF_NAMES: [&str; 8] =
    ["undefined", "object", "boolean", "number", "string", "bigint", "symbol", "function"];

/// The index in `TYPEOF_NAMES` of `name`, if some value's `typeof` is `name`: the one owner of
/// the name indices, read by the compiler and, as constants, by every `typeof` row.
pub const fn typeof_name_index(name: &str) -> Option<usize> {
    let wanted = name.as_bytes();
    let mut index = 0;
    while index < TYPEOF_NAMES.len() {
        let candidate = TYPEOF_NAMES[index].as_bytes();
        let mut at = 0;
        while at < wanted.len() && at < candidate.len() && wanted[at] == candidate[at] {
            at += 1;
        }
        if at == wanted.len() && at == candidate.len() {
            return Some(index);
        }
        index += 1;
    }
    None
}
