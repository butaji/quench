//! The pieces of an own data field's store that a flat run of arena values shares: the guards
//! that admit the write and the write itself. `store.rs` composes them with the packed arm; a
//! stencil row for a site whose storage the hint proved flat (`Form::Arena`) composes them
//! alone, so its slot operand is the run's byte offset and nothing else.

use super::object::{arena_run, recorded_shape, storage_form};
use super::region::Region;
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// The payload word of the record at `record` (id `record_id`) once a store of `value` may
/// proceed, with whether its values are inline: its exact shape is `shape`, the store gate
/// admits the write and its values are plain (no extras); `None` (leaving at `miss`)
/// otherwise, changing nothing. The auxiliary word that proves the shape tells the storage
/// form by its form bits (`object::storage_form`).
#[inline(always)]
///
/// A `hinted` store's `shape` is a lane record's hint, a whole auxiliary word
/// (`Object::hint_shape`): the word equals it without its extras bit, so one compare also
/// proves the values plain. Any other `shape` is a recorded one, which the word equals
/// without its form bits, and the extras bit is tested apart.
pub(crate) fn admitted_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, hinted: bool,
    value: S::Word, miss: S::Miss) -> Option<(S::Word, S::Cond)> {
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    if hinted {
        let shape = stage.site(u64::from(shape & !OBJECT.extras_form));
        let same = stage.eq_imm(word, shape);
        stage.guard(same, miss)?;
    } else {
        let exact = recorded_shape(stage, word);
        let shape = stage.site(u64::from(shape));
        let same = stage.eq_imm(exact, shape);
        stage.guard(same, miss)?;
        let extras = storage_form(stage, word, OBJECT.extras_form);
        let plain = stage.not(extras);
        stage.guard(plain, miss)?;
    }
    let open = stage.store_gate(record_id, value);
    stage.guard(open, miss)?;
    let inline = storage_form(stage, word, OBJECT.inline_form);
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    Some((payload, inline))
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
    let (payload, inline) = admitted_payload(stage, record, record_id, shape, false, value, miss)?;
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    store_flat(stage, payload, slot, value);
    Some(())
}
