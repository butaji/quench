//! `GetIndex` sites that settle on one kind of read past the generic order: a string key
//! naming a property, as `o[k]` in a for-in body reads, by atom (`lane_get_index_named`), and
//! a string's unit (`lane_get_index_string`).
use super::*;

script_handler! {
    /// `lane_get_index_registers` at a site that read a name key (`named_key_read`); any other
    /// read takes the generic order.
    fn lane_get_index_named<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(value) = named_key_read(vm, r.get(i.b), r.get(i.c)) else {
            sibling!(lane_get_index_registers::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// `base[key]` of a string `key` that names no array index (`Vm::plain_string_keyed_get`).
#[inline(always)]
fn named_key_read<H: Host>(vm: &Vm<H>, base: Value, key: Value) -> Option<Value> {
    vm.plain_string_keyed_get(base, key)
}

/// `named_key_read` in the generic order (`lane_index_slot_element`): the site then reads
/// name keys first (`lane_get_index_named`).
pub(super) fn lane_named_key_element<H: Host>(ip: Ip, vm: &Vm<H>, base: Value, key: Value) -> Option<Value> {
    let value = named_key_read(vm, base, key)?;
    requicken::<H>(ip, lane_get_index_named::<H>);
    Some(value)
}

/// A string's unit at an index inside its length (`lane_string_element`): the site then
/// tries it first (`lane_get_index_string`).
pub(super) fn lane_string_index_element<H: Host>(ip: Ip, vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    let value = lane_string_element(vm, base, key)?;
    requicken::<H>(ip, lane_get_index_string::<H>);
    Some(value)
}

script_handler! {
    /// `lane_get_index_registers` at a site that read a string: its unit at an index inside
    /// its length is the whole read; any other read takes the generic order.
    fn lane_get_index_string<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(value) = lane_string_element(vm, r.get(i.b), r.get(i.c)) else {
            sibling!(lane_get_index_registers::<H>, r, ip, view)
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}
