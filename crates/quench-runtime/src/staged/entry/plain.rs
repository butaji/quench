//! Plain own-value reads of an IC entry: the shape check, the own entry and the plain slot.

use super::*;

/// Guards that the shape of the object record at `record` is the one the entry at `entry`
/// stores in its word at `offset`, complemented by `complement`: the stored word and the
/// record's shape complemented differ by zero.
#[inline(always)]
pub(super) fn shape_matches<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, (offset, complement): (usize, u32), miss: S::Miss) -> Option<()> {
    let exact = super::super::object::exact_shape(stage, record);
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, offset as i32);
    let complement = stage.constant(u64::from(complement));
    let wanted = stage.sub(complement, exact);
    let difference = stage.sub(stored, wanted);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// Whether the entry at `entry` is an own entry: its holder word is the stored own holder.
#[inline(always)]
pub(super) fn own_entry<S: Stage>(stage: &mut S, entry: S::Word) -> S::Cond {
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.holder_offset as i32);
    stage.eq_imm(stored, u64::from(ENTRY.holder_complement ^ ENTRY.own_holder))
}

/// The slot the entry at `entry` names.
#[inline(always)]
pub fn entry_slot<S: Stage>(stage: &mut S, entry: S::Word) -> S::Word {
    let slot = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.slot_offset as i32);
    stage.and_imm(slot, u64::from(u16::MAX))
}

/// The value at `slot` of the object record at `record`, whose shape the caller proved: `None`
/// (leaving at `miss`) where its values are not plain (extras displaced them). Packed values
/// decode from the record, flat ones read from the arena run.
#[inline(always)]
pub(in super::super) fn plain_value<S: Stage>(stage: &mut S, record: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    plain_slot(stage, record, payload, slot, miss)
}

/// The value at `slot` of the record at `record` whose payload word `payload` holds plain
/// storage (`plain_payload`): packed values decode from the record, flat ones read from the arena
/// run; `None` (leaving at `miss`) for a narrow run (`object::flat_run`).
#[inline(always)]
pub(super) fn plain_slot<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let inline = inline_storage(stage, record);
    stage.choose_leaving(inline, #[inline(always)] |stage| {
        let half = stage.mul_imm(slot, std::mem::size_of::<u32>() as u64);
        let address = stage.add(record, half);
        let narrow = stage.load_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32);
        Some(decode_narrow(stage, narrow))
    }, #[inline(always)] |stage| {
        let run = super::super::object::flat_run(stage, payload, miss)?;
        let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
        let address = stage.add(run, offset);
        Some(stage.load_u64(Region::ObjectSlots.mutable(), address, 0))
    })
}
