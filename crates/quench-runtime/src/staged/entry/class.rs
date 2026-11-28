//! Class-cell field reads and stores of an IC entry, and the class checks they run.

use super::*;
use super::super::Table;
use crate::staged::field_cache_layout::{SPILL_FROM_SHIFT, SPILL_INDEX_MASK, SPILLED_SLOT};

/// The address of the inline field at index `index` of the live class cell `cell`: its flat run at `fields_offset`.
#[inline(always)]
fn inline_address<S: Stage>(stage: &mut S, cell: S::Word, index: S::Word) -> S::Word {
    let fields = stage.add_imm(cell, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.fields_offset as u64);
    let offset = stage.mul_imm(index, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.field_size as u64);
    stage.add(fields, offset)
}

/// The address of a spilled field (`spilled_slot`) of the live class cell `cell`: the cell's inline slot `spill_from`
/// holds the run's offset in the spill arena (its low 32 bits), so field `i` of the run is at
/// `SpillBase + (offset + i) * field_size`. The spill arena's base is read live (`Table::SpillBase`).
#[inline(always)]
fn spilled_address<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let from_bits = stage.and_imm(slot, u64::from(!SPILLED_SLOT));
    let from = stage.shr_imm(from_bits, u64::from(SPILL_FROM_SHIFT));
    let run_slot = inline_address(stage, cell, from);
    let run = stage.load_u64(Region::ObjectSlots.mutable(), run_slot, 0);
    let offset = stage.and_imm(run, u64::from(u32::MAX));
    let index = stage.and_imm(slot, u64::from(SPILL_INDEX_MASK));
    let position = stage.add(offset, index);
    let base = stage.table(Table::SpillBase);
    let bytes = stage.mul_imm(position, super::super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.field_size as u64);
    stage.add(base, bytes)
}

/// The address of the field the entry's slot word `slot` names in the live class cell `cell`: an inline field's
/// (`cell + fields_offset + slot * field_size`), or a spilled field's (`spilled_address`). A plain field index has no
/// spilled bit, so it takes the inline arm; the non-spilled arm does no extra load.
#[inline(always)]
pub fn class_slot_address<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let spilled_bit = stage.and_imm(slot, u64::from(SPILLED_SLOT));
    let is_spilled = stage.ne_imm(spilled_bit, 0u32);
    stage.choose(is_spilled, #[inline(always)] |stage| spilled_address(stage, cell, slot),
        #[inline(always)] |stage| inline_address(stage, cell, slot))
}

/// `value` into field `slot` of the live class cell `cell`, once its gate admitted the store. The gate
/// leaves no constant mark standing: a constant-marked class has no key (`Heap::live_class_key`).
#[inline(always)]
pub fn class_store<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word, value: S::Word) {
    let address = class_slot_address(stage, cell, slot);
    stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value);
}

/// Guards that the live class cell `cell` holds the class key the own entry `entry` stores in its receiver
/// word (stored complemented: `key = complement - stored`), by `class_key_matches`, the one class guard.
#[inline(always)]
pub fn class_matches<S: Stage>(stage: &mut S, cell: S::Word, entry: S::Word, miss: S::Miss) -> Option<()> {
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.receiver_offset as i32);
    let complement = stage.constant(u64::from(ENTRY.receiver_complement));
    let key = stage.sub(complement, stored);
    class_key_matches(stage, cell, key, miss)
}

/// Guards that the live class cell `cell` holds the class key `key` a row's hole names (`class | CLASS_KEY`):
/// `class_matches` with the key as an operand, where the entry's complemented word is not read. A key of another
/// class, or a shape key, equals no cell's key, so the cell misses.
#[inline(always)]
pub fn class_key_matches<S: Stage>(stage: &mut S, cell: S::Word, key: S::Word, miss: S::Miss) -> Option<()> {
    // `class + CLASS_KEY` in the 64-bit word: every live cell's class is below `LAYOUT_CAPACITY`, so the compare is the whole guard.
    let class = super::super::object::cell_class(stage, cell);
    let held = stage.add_imm(class, u64::from(CLASS_KEY));
    let difference = stage.sub(held, key);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// The live cell of the direct object `object` whose class is the one `key` names (`class_key_matches`), with
/// its granule: the receiver of a class-keyed row. `None` (leaving at `miss`) where the word is no direct
/// object or its cell is of another class.
#[inline(always)]
pub fn class_key_cell<S: Stage>(stage: &mut S, object: S::Word, key: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let direct = super::super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let granule = super::super::value::direct_object_id(stage, object);
    let cell = super::super::object::cell_at_granule(stage, granule);
    class_key_matches(stage, cell, key, miss)?;
    Some((granule, cell))
}

/// The own field at `slot` (a field index) of the class-keyed receiver `object` (`class_key_cell`), as `cell_own_field` reads it.
#[inline(always)]
pub fn class_own_field<S: Stage>(stage: &mut S, object: S::Word, key: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (_, cell) = class_key_cell(stage, object, key, miss)?;
    cell_own_field(stage, cell, slot, miss)
}

/// The held field at `slot` of the class-keyed receiver `object`, whose holder is the live cell at `holder` (its
/// granule): the class fixes the prototype link and a holder whose fields move resets the field caches, as
/// `class_held_value`, so the value is one load after the cell lookup.
#[inline(always)]
pub fn class_held_field<S: Stage>(stage: &mut S, object: S::Word, key: S::Word, slot: S::Word, holder: S::Word, miss: S::Miss) -> Option<S::Word> {
    class_key_cell(stage, object, key, miss)?;
    let holder = super::super::object::cell_at_granule(stage, holder);
    Some(cell_field(stage, holder, slot))
}

/// The field at `slot` (an entry's slot word, inline or spilled) of the live class cell `cell`: one load at
/// `class_slot_address`.
#[inline(always)]
pub fn cell_field<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word) -> S::Word {
    let address = class_slot_address(stage, cell, slot);
    stage.load_u64(Region::ObjectSlots.mutable(), address, 0)
}

/// The value at `slot` of the live class cell `cell` that the own entry `entry` names. The class key
/// guards the cell, so the value is one load: no record, no payload test and no shape guard.
#[inline(always)]
pub fn class_own_value<S: Stage>(stage: &mut S, cell: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    class_matches(stage, cell, entry, miss)?;
    let slot = entry_slot(stage, entry);
    cell_own_field(stage, cell, slot, miss)
}

/// The own field at `slot` of the live class cell `cell`: `cell_field`, which leaves at `miss` where the field holds the
/// hole. A fixed class's absent union field holds it (R4-F): the class names the slot for every instance, so an entry
/// filled from an instance that holds it meets one that does not. The hole leaves for the prototype walk; it is a value
/// state, not a representation test.
#[inline(always)]
pub fn cell_own_field<S: Stage>(stage: &mut S, cell: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let value = cell_field(stage, cell, slot);
    let hole = stage.eq_imm(value, super::super::value_encoding::TAG_DELETED);
    let present = stage.not(hole);
    stage.guard(present, miss)?;
    Some(value)
}

/// The value a class-keyed held entry at `entry` names for the live cell `object` (`direct_class_read`):
/// the class key of the receiver matches (`class_matches`), and the holder is the live cell whose
/// granule the entry stores in `holder_cell`, whose field at `slot` is the value. The class fixes the
/// prototype link, so there is no link guard, and a holder whose fields move resets the field caches
/// (`Vm::drop_moved_holder_entries`), so there is no holder guard.
#[inline(always)]
pub fn class_held_value<S: Stage>(stage: &mut S, object: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let granule = super::super::value::direct_object_id(stage, object);
    let cell = super::super::object::cell_at_granule(stage, granule);
    class_matches(stage, cell, entry, miss)?;
    let holder_granule = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.holder_cell_offset as i32);
    let holder = super::super::object::cell_at_granule(stage, holder_granule);
    let slot = entry_slot(stage, entry);
    Some(cell_field(stage, holder, slot))
}

