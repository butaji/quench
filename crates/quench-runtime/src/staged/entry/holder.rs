//! The holder reads of an IC entry: the own entry value and the value a prototype holder names.

use super::*;

/// The value the own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's, the entry
/// names a prototype holder (or is empty), or the record's values are not plain. The entry
/// stores each of its words complemented against the empty entry, so a holder of zero and a
/// shape equal to the record's shape complemented are the guards.
// Read by the b-jit stencil generator (thread 7) once it derives GetField from this row; the
// tests run it on `Concrete` meanwhile.

/// The value at `slot` of the prototype record `holder`, whose shape the caller proved: a
/// prototype keeps its values in its extras (`Object::mark_as_prototype`), wide and inline or as
/// a run in the arena, so `None` (leaving at `miss`) where it has none, or where an inline value
/// is past the inline ones.
#[inline(always)]
fn holder_value<S: Stage>(stage: &mut S, holder: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(Region::Payload.mutable(), holder, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let pointer = extras_pointer(stage, payload);
    let inline = inline_storage(stage, holder);
    let limit = stage.constant(OBJECT.inline_property_count as u64);
    let below = stage.ult(slot, limit);
    let flat = stage.not(inline);
    let fits = stage.any(flat, below);
    stage.guard(fits, miss)?;
    let run = stage.choose(inline, #[inline(always)] |stage| stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64), #[inline(always)] |stage| {
        let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
        arena_run(stage, start)
    });
    let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
    let address = stage.add(run, offset);
    Some(stage.load_u64(Region::ObjectSlots.mutable(), address, 0))
}

/// The value an own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's. A record whose
/// extras displaced its values (an intrinsic such as `Math`, a prototype) answers from them
/// (`holder_value`). The entry stores each of its words complemented against the empty entry, so
/// a shape equal to the record's shape complemented is the guard.
#[inline(always)]
pub(super) fn own_entry_value<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    let slot = entry_slot(stage, entry);
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.choose_leaving(plain, #[inline(always)] |stage| plain_slot(stage, record, payload, slot, miss),
        #[inline(always)] |stage| holder_value(stage, record, slot, miss))
}

/// The value a prototype-held entry at `entry` names for the object record at `record`
/// (`Vm::ordinary_holder_entry_value`): the receiver's shape is the entry's, its prototype link
/// names an object record of the entry's holder shape, and the value is the holder's at the
/// entry's slot. `None` (leaving at `miss`) for any other receiver, link or holder, so a changed
/// prototype is a miss.
#[inline(always)]
pub(super) fn holder_entry_value<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    let link = super::super::object::proto_link(stage, record);
    let id = super::super::object::proto_object_record_id(stage, link, miss)?;
    let holder = super::super::object::object_record_at(stage, records, id);
    shape_matches(stage, holder, entry, (ENTRY.holder_offset, ENTRY.holder_complement), miss)?;
    let slot = entry_slot(stage, entry);
    holder_value(stage, holder, slot, miss)
}
