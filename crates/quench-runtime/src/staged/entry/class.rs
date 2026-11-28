//! Class-cell field reads and stores of an IC entry, and the class checks they run.

use super::*;

/// The address of field `slot` of the live class cell `cell`: its flat run at `fields_offset`, the address
/// `class_own_value` reads.
#[inline(always)]
fn class_field_address<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let fields = stage.add_imm(cell, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.fields_offset as u64);
    let offset = stage.mul_imm(slot, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.field_size as u64);
    stage.add(fields, offset)
}

/// Field `slot` of the live class cell `cell`, the old value a gated store observes.
#[inline(always)]
pub(super) fn class_field<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let address = class_field_address(stage, cell, slot);
    stage.load_u64(Region::ObjectSlots.mutable(), address, 0)
}

/// `value` into field `slot` of the live class cell `cell`, once its gate admitted the store. The gate
/// leaves no constant mark standing: a constant-marked class has no key (`Heap::live_class_key`).
#[inline(always)]
pub(in super::super) fn class_store<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word, value: S::Word) {
    let address = class_field_address(stage, cell, slot);
    stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value);
}

/// Guards that the live class cell `cell` holds the class key the own entry `entry` stores in its receiver
/// word, complemented as `shape_matches` reads it. The live test stays: `DEPARTED_CLASS` and `NO_RECEIVER`
/// are one word, so without it an empty entry would match a departed cell.
#[inline(always)]
pub(super) fn class_matches<S: Stage>(stage: &mut S, cell: S::Word, entry: S::Word, miss: S::Miss) -> Option<()> {
    // `class + CLASS_KEY` in the 64-bit word: a live class (below `LAYOUT_CAPACITY`) gives its key
    // `class | CLASS_KEY`; `DEPARTED_CLASS` (u32::MAX) gives a word past u32, which equals no
    // stored entry, the empty one included. So the one compare is the class guard: no live test.
    let class = super::super::object::cell_class(stage, cell);
    let key = stage.add_imm(class, u64::from(CLASS_KEY));
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.receiver_offset as i32);
    let complement = stage.constant(u64::from(ENTRY.receiver_complement));
    let wanted = stage.sub(complement, key);
    let difference = stage.sub(stored, wanted);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// The field at `slot` of the live class cell `cell`: its fields are one flat run at `fields_offset`,
/// so the value is one load.
#[inline(always)]
pub(crate) fn cell_field<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let fields = stage.add_imm(cell, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.fields_offset as u64);
    let offset = stage.mul_imm(slot, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.field_size as u64);
    let address = stage.add(fields, offset);
    stage.load_u64(Region::ObjectSlots.mutable(), address, 0)
}

/// The value at `slot` of the live class cell `cell` that the own entry `entry` names. The class key
/// guards the cell, so the value is one load: no record, no payload test and no shape guard.
#[inline(always)]
pub(super) fn class_own_value<S: Stage>(stage: &mut S, cell: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    class_matches(stage, cell, entry, miss)?;
    let slot = entry_slot(stage, entry);
    Some(cell_field(stage, cell, slot))
}

/// The value a class-keyed held entry at `entry` names for the live cell `object` (`direct_read_in`):
/// the class key of the receiver matches (`class_matches`), and the holder is the live cell whose
/// granule the entry stores in `holder_cell`, whose field at `slot` is the value. The class fixes the
/// prototype link, so there is no link guard, and a departed holder resets the field caches
/// (`Vm::drop_departed_holder_entries`), so there is no holder guard.
#[inline(always)]
pub(super) fn class_held_value<S: Stage>(stage: &mut S, object: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let granule = super::super::value::direct_object_id(stage, object);
    let cell = super::super::object::cell_at_granule(stage, granule);
    class_matches(stage, cell, entry, miss)?;
    let holder_granule = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.holder_cell_offset as i32);
    let holder = super::super::object::cell_at_granule(stage, holder_granule);
    let slot = entry_slot(stage, entry);
    Some(cell_field(stage, holder, slot))
}

/// Whether the entry at `entry` names a class key. The receiver word is stored complemented, so the
/// key's `CLASS_KEY` bit is clear in the stored word. It is a fact of the entry, never a test of the
/// object: a class key's read goes to the class cell, a shape key's to the record (`object_record_of`).
#[inline(always)]
pub(super) fn entry_keys_class<S: Stage>(stage: &mut S, entry: S::Word) -> S::Cond {
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.receiver_offset as i32);
    let bit = stage.and_imm(stored, u64::from(CLASS_KEY));
    stage.eq_imm(bit, 0u32)
}
