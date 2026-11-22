//! `base[key]` of a string key that names a property, as `o[k]` in a for-in body reads, by atom.
//! It is one of the reads `lane_index_slot_element` tries in its generic order; no site is
//! requickened to a specialized handler, so every `GetIndex` runs the generated `ELEMENT_READ` row.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// `base[key]` of a string `key` that names no array index (`Vm::plain_string_keyed_get`).
#[inline(always)]
pub(crate) fn named_key_element(vm: &mut Vm, base: Value, key: Value) -> Option<Value> {
    vm.plain_string_keyed_get(base, key)
}
