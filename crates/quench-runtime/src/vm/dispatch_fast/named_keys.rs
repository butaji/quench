//! `GetIndex` sites that settle on one kind of read past the generic order: a string key
//! naming a property, as `o[k]` in a for-in body reads, by atom (`lane_get_index_named`), and
//! a string's unit (`lane_get_index_string`).
use super::*;

/// The site fact of an element read that met a name key (`named_key_read`): the row's miss
/// sibling (`row_named_read_miss`) reads the name first, then the generic order.
pub(super) const ELEMENT_NAMED_SITE: u8 = own_site(false, OWN_FORMS.len() + 27);

/// The site fact of an element read that met a string's unit (`lane_string_element`): the row's
/// miss sibling (`row_string_read_miss`) reads the unit first, then the generic order.
pub(super) const ELEMENT_STRING_SITE: u8 = own_site(false, OWN_FORMS.len() + 28);

pub(super) const fn element_named_site(site: u8) -> bool { site == ELEMENT_NAMED_SITE }
pub(super) const fn element_string_site(site: u8) -> bool { site == ELEMENT_STRING_SITE }

/// The generated keyed read at a name-key site (`lane_row` of `ELEMENT_READ`, `ELEMENT_NAMED_SITE`).
const fn element_named_read_row<H: Host>() -> Handler { lane_row::<H, ELEMENT_READ, ACC_NONE, ELEMENT_NAMED_SITE> }

/// The generated keyed read at a string-unit site (`lane_row` of `ELEMENT_READ`, `ELEMENT_STRING_SITE`).
const fn element_string_read_row<H: Host>() -> Handler { lane_row::<H, ELEMENT_READ, ACC_NONE, ELEMENT_STRING_SITE> }

script_handler! {
    /// The name-key row's refused read: the name read (`named_key_read`) is the whole read; any
    /// other read takes the generic order (`row_index_read_miss`).
    #[inline(never)]
    pub(super) fn row_named_read_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(value) = named_key_read(vm, r.get(i.b), r.get(i.c)) else {
            sibling!(row_index_read_miss::<H>, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}

/// `base[key]` of a string `key` that names no array index (`Vm::plain_string_keyed_get`).
#[inline(always)]
fn named_key_read<H: Host>(vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    vm.plain_string_keyed_get(base, key)
}

/// `named_key_read` in the generic order (`row_index_read`): the site then reads
/// name keys first (`lane_get_index_named`).
pub(super) fn lane_named_key_element<H: Host>(ip: Ip, vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    let value = named_key_read(vm, base, key)?;
    requicken::<H>(ip, element_named_read_row::<H>());
    Some(value)
}

/// A string's unit at an index inside its length (`lane_string_element`): the site then
/// tries it first (`lane_get_index_string`).
pub(super) fn lane_string_index_element<H: Host>(ip: Ip, vm: &mut Vm<H>, base: Value, key: Value) -> Option<Value> {
    let value = lane_string_element(vm, base, key)?;
    requicken::<H>(ip, element_string_read_row::<H>());
    Some(value)
}

script_handler! {
    /// The string-unit row's refused read: its unit at an index inside its length is the whole
    /// read; any other read takes the generic order (`row_index_read_miss`).
    #[inline(never)]
    pub(super) fn row_string_read_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let Some(value) = lane_string_element(vm, r.get(i.b), r.get(i.c)) else {
            sibling!(row_index_read_miss::<H>, r, ip, Carried::script_in(cx, view))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}
