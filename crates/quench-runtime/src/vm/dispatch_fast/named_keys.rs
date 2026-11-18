//! `base[key]` of a string key that names a property, as `o[k]` in a for-in body reads, by atom.
//! It is one of the reads `row_index_read` tries in its generic order; no site is requickened to a
//! specialized handler, so every `GetIndex` runs the generated `ELEMENT_READ` row.
use super::*;

// merge: theirs requickens a GetIndex site to a name-key-first or string-first handler
// (`lane_get_index_named`, `lane_get_index_string`, `named_key_read`). Ours runs every
// GetIndex through the generated ELEMENT_READ row and sets no requickening, so those
// handlers are not ported. Their results are the same reads, only the per-site order differs.
// merge: `named_key_element` restored (it was referenced by `row_index_read` and by the `use` below
// but no longer defined); it is trunk's `named_key_read`, the plain string-keyed read, with no requicken.

/// `base[key]` of a string `key` that names no array index (`Vm::plain_string_keyed_get`).
#[inline(always)]
pub(super) fn named_key_element<H: Host>(vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    vm.plain_string_keyed_get(base, key)
}
