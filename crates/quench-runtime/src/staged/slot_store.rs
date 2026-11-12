//! The pieces of an own data field's store that a flat run of arena values shares: the guards
//! that admit the write and the write itself. `store.rs` composes them with the packed arm; a
//! stencil row for a site whose storage the hint proved flat (`Form::Arena`) composes them
//! alone, so its slot operand is the run's byte offset and nothing else.

use super::object::{arena_run, inline_storage, plain_payload};
use super::region::Region;
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// The payload word of the record at `record` (id `record_id`) once a store of `value` may
/// proceed: its exact shape is `shape`, the store gate admits the write and its values are
/// plain (no extras); `None` (leaving at `miss`) otherwise, changing nothing.
#[inline(always)]
pub(crate) fn admitted_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, value: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let exact = stage.load_u32(Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset as i32);
    let shape = stage.site(u64::from(shape));
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    let open = stage.store_gate(record_id, value);
    stage.guard(open, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    Some(payload)
}

/// Stores `value` at `slot` of the flat run the plain `payload` word addresses; the slot is a
/// per-site operand, so its byte offset is a hole.
#[inline(always)]
pub(crate) fn store_flat<S: Stage>(stage: &mut S, payload: S::Word, slot: u16, value: S::Word) {
    let run = arena_run(stage, payload);
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    let address = stage.add_imm(run, offset);
    stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value);
}

/// `own_store` for a record whose values are a flat run (`Form::Arena`): the guards of
/// `admitted_payload`, then the run's slot; `None` (leaving at `miss`) where the record keeps
/// its values inline, changing nothing.
#[inline(always)]
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/ic_rows.rs`), which includes this file
pub(crate) fn own_flat_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, slot: u16,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted_payload(stage, record, record_id, shape, value, miss)?;
    let inline = inline_storage(stage, record);
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    store_flat(stage, payload, slot, value);
    Some(())
}
