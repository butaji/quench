//! The property arena: its runs, slot binding and slot addresses.

use super::*;

/// The address of the first arena value of the run starting at `start`.
#[inline(always)]
pub fn arena_run<S: Stage>(stage: &mut S, start: S::Word) -> S::Word {
    let start = stage.and_imm(start, u64::from(OBJECT.storage_start_mask));
    let offset = stage.mul_imm(start, OBJECT.property_value_size as u64);
    let base = stage.table(Table::PropertyValues);
    stage.add(base, offset)
}

/// The arena run a field slot binds: the address of value `slot` of the run of `length` values at
/// `values`, and how many values remain from it (none where the slot lies outside the run). An
/// address formed outside the run grants no permission to dereference it.
#[inline(always)]
pub fn bind_arena_slot<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, slot: S::Word) -> (S::Word, S::Word) {
    let fits = stage.ult(slot, length);
    let difference = stage.sub(length, slot);
    let none = stage.constant(0);
    let remaining = stage.select(fits, difference, none);
    let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
    (stage.add(values, offset), remaining)
}

/// The address of value `position` of the run of `length` values at `values`; `None` (leaving at
/// `miss`) at or past its end.
#[inline(always)]
pub fn arena_slot_address<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, position: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let within = stage.ult(position, length);
    stage.guard(within, miss)?;
    let offset = stage.mul_imm(position, OBJECT.property_value_size as u64);
    Some(stage.add(values, offset))
}
