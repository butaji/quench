//! The proved field records of a region (`region_state`): a register receiver's own data field
//! whose packed integer a read or store reaches with none of the lane's per-access guards.
//!
//! The entry proves them once per entry of the loop (`field_holds`): the site's field cache
//! names an own entry, the receiver is a direct object of that exact shape whose values are
//! plain inline narrow words (a packed pair), the slot holds an integer, and a store is one the
//! store gate admits for any integer (no slot representation observes it, `store_unlogged`, and
//! no method cache names the field). The body admits no allocation, call or other store, so
//! nothing the entry proved can change before the loop's next entry: each access is the
//! integer row (`staged::integer_field`) at the receiver's record. A store the narrow form
//! cannot hold leaves the lane at its record, the general store runs, and the loop's next entry
//! proves the facts again.

use super::*;
use super::int_region::carried;
use crate::heap::Object;
use crate::staged::integer_field::{read_integer, write_integer};
use crate::staged::object::{inline_storage, object_record_at, own_layout};
use crate::staged::{read_slot, Concrete, Form, PackedAccess, Slots};
use crate::typed_ir::counted_loop::FieldAccess;

/// Whether `object` keeps its values as plain inline narrow words and holds an integer at `slot`.
fn packed_integer(object: &Object, values: *const Value, slot: u16) -> bool {
    if usize::from(slot) >= INLINE_PROPERTY_COUNT {
        return false;
    }
    let mut stage = Concrete::new(values);
    let record = object as *const Object as u64;
    let inline = inline_storage(&mut stage, record);
    let Some(layout) = own_layout(&mut stage, record, inline, Form::Either, usize::from(slot) + 1, PackedAccess::Reload, ()) else {
        return false;
    };
    matches!(layout.slots, Slots::Packed(_)) && Value::from_bits(read_slot(&mut stage, layout, slot)).as_int().is_some()
}

/// Whether the facts the proved record of field `access` relies on hold now, and if so gives
/// `record` the site's entry as its hint (`set_field_hint`), whose slot the proved record reads.
///
/// # Safety
/// `record` is the access's record in the running lane's view, and `cx` is its context.
pub(super) unsafe fn field_holds<H: Host>(cx: *mut LaneContext<H>, r: Registers, record: Ip, access: &FieldAccess) -> bool {
    let vm = unsafe { vm(cx) };
    let cache = vm.field_cache_entry(access.site);
    let Some(id) = r.get(access.receiver).direct_object_index() else { return false };
    if cache.holder != super::super::NO_FIELD_HOLDER || cache.receiver == super::super::NO_FIELD_RECEIVER {
        return false;
    }
    let owned = vm.shapes.get(cache.receiver as usize).is_some_and(|shape| usize::from(cache.slot) < shape.storage_len());
    let object = vm.heap.object_record_by_id(id);
    let stores = access.store.is_none_or(|atom| !vm.heap.tracks_slot_representations() && !vm.method_caches_may_name(atom));
    if !owned || !stores || object.exact_shape() != cache.receiver || !packed_integer(object, vm.heap.arena_values_base(), cache.slot) {
        return false;
    }
    let Some(base) = vm.heap.object_record_base() else { return false };
    // SAFETY: the context is the running lane's; the base is the record range of its heap.
    unsafe { (*cx).binding.object_base = base };
    set_field_hint(record, cache);
    true
}

/// The record address of the object the receiver register `register` holds.
#[inline(always)]
fn receiver_record<H: Host>(cx: *mut LaneContext<H>, r: Registers, register: u16) -> u64 {
    // SAFETY: the entry proved the register holds a direct object and bound the record range.
    let records = unsafe { (*cx).binding.object_base } as u64;
    object_record_at(&mut Concrete::new(std::ptr::null()), records, u64::from(r.payload(register)))
}

script_handler! {
    /// `GetField` of a proved region's packed integer field: the integer row at the receiver's
    /// record, written as the destination's payload. The slot is the record's hint.
    pub(super) fn lane_get_integer_field<>(cx, r, ip, i, view) {
        let record = receiver_record(cx, r, i.b);
        let word = read_integer(&mut Concrete::new(std::ptr::null()), record, i.call_link.get());
        r.set_payload(i.a, word as u32);
        next!(cx, r, following(ip), carried(view))
    }
}

script_handler! {
    /// `SetField` of a register's int32 value to a proved region's packed integer field: the
    /// integer row, which leaves at the record where the narrow form cannot hold the value.
    pub(super) fn lane_set_integer_field<>(cx, r, ip, i, view) {
        let record = receiver_record(cx, r, i.b);
        let word = u64::from(r.payload(i.a));
        if write_integer(&mut Concrete::new(std::ptr::null()), record, i.call_link.get(), word, ()).is_none() {
            return exit(ip);
        }
        next!(cx, r, following(ip), carried(view))
    }
}
