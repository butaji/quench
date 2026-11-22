//! The pieces of an own data field's store that a flat run of arena values shares: the guards
//! that admit the write and the write itself. `store.rs` composes them with the packed arm; a
//! stencil row for a site whose storage the hint proved flat (`Form::Arena`) composes them
//! alone, so its slot operand is the run's byte offset and nothing else.

use super::narrow::packed_form;
use super::object::{arena_run, flat_run, inline_storage, recorded_shape, storage_form, Form, HALF_BYTES};
use super::region::Region;
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::{Representation, Type};

/// The auxiliary word of the record at `record` once its exact shape is `shape` and its values
/// are plain (no extras, no narrow run); `None` (leaving at `miss`) otherwise. The word tells
/// the storage form by its form bits (`object::storage_form`).
///
/// A `hinted` store's `shape` is a lane record's hint, a layout id (`Object::hint_shape`):
/// one compare of the record's own word proves the shape and the form, which the id names, so
/// the values are plain with no test, and the word is the id. Any other `shape` is a recorded
/// one, which the word equals without its form bits, and the extras bit is tested apart.
#[inline(always)]
fn admitted_word<S: Stage>(stage: &mut S, record: S::Word, shape: u32, hinted: bool, miss: S::Miss) -> Option<S::Word> {
    if hinted {
        let id = super::ic::layout_id(stage, record);
        let same = { let shape = stage.site(u64::from(shape)); stage.eq_imm(id, shape) };
        stage.guard(same, miss)?;
        // A hint of extras or a narrow run admits no store here: the form is the id's fact,
        // read once the id matched (a cleared hint names no layout).
        if super::ic::layout_form(shape) & (OBJECT.extras_form | OBJECT.narrow_form) != 0 {
            let never = stage.constant(0);
            let refused = stage.ne_imm(never, 0u32);
            stage.guard(refused, miss)?;
        }
        return Some(id);
    }
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let exact = recorded_shape(stage, word);
    let shape = stage.site(u64::from(shape));
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    // Neither extras nor a narrow run: the payload is a packed word or a wide run's start.
    let displaced = storage_form(stage, word, OBJECT.extras_form | OBJECT.narrow_form);
    let plain = stage.not(displaced);
    stage.guard(plain, miss)?;
    Some(word)
}

/// Whether a store admits in place: `specialized` is the dispatch's word that the run stores specialized (a run
/// that does not leaves), and the atom at the next site hole stays numbered. That no method cache names the atom
/// is a Stable fact the code holds (`held_atoms`): naming it retires the code, and a run already inside learns
/// that from the invalidation word its rows test after a call, so no bitset is read here.
#[allow(dead_code)] // the stencil rows' admission; the interpreter's stores hold no per-store test
#[inline(always)]
pub(crate) fn store_admitted<S: Stage>(stage: &mut S, specialized: S::Word, miss: S::Miss) -> Option<()> {
    let on = stage.ne_imm(specialized, 0u32);
    stage.guard(on, miss)?;
    let _atom = stage.site(0);
    Some(())
}

/// The payload word of the record at `record` (id `record_id`) once a store of `value` may
/// proceed: its exact shape is `shape` (`admitted_word`), the store gate admits the write and
/// its values are plain; `None` (leaving at `miss`) otherwise, changing nothing.
#[inline(always)]
pub(crate) fn admitted_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, hinted: bool,
    value: S::Word, miss: S::Miss) -> Option<S::Word> {
    admitted_word(stage, record, shape, hinted, miss)?;
    gated_payload(stage, record, record_id, (value, Type::ANY), miss)
}

/// Continues where a store of `value`, which the frame proved of type `fact`, into record
/// `record_id` needs no barrier log. An immediate type (`Type::IMMEDIATE`) names no heap
/// record, so it asks only whether old records keep counts (`Stage::immediate_store_open`), under
/// which overwriting a reference still logs; any other value asks the stage's gate.
#[inline(always)]
pub(crate) fn gate_open<S: Stage>(stage: &mut S, record_id: S::Word, (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    if fact.is_subtype(Type::IMMEDIATE) {
        let open = stage.immediate_store_open();
        return stage.guard(open, miss);
    }
    let open = stage.store_gate(super::StagedRecordSpace::Object, record_id, value);
    stage.guard(open, miss)
}

/// `gate_open` of a store whose value the slot's type holds (`Stage::held_store_gate`): the
/// barrier's log alone, no slot observation.
#[inline(always)]
pub(crate) fn held_gate_open<S: Stage>(stage: &mut S, record_id: S::Word, (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    if fact.is_subtype(Type::IMMEDIATE) {
        let open = stage.immediate_store_open();
        return stage.guard(open, miss);
    }
    let open = stage.held_store_gate(super::StagedRecordSpace::Object, record_id, value);
    stage.guard(open, miss)
}

/// The payload word of the record at `record` once the gate admits `value` (of `fact`) and its
/// values are plain (no extras); the exact shape is the caller's proof (`property_write::in_place`).
#[inline(always)]
pub(crate) fn gated_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, value: (S::Word, Type),
    miss: S::Miss) -> Option<S::Word> {
    gate_open(stage, record_id, value, miss)?;
    Some(stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32))
}

/// Stores `value`, a word in `rep`'s encoding, at `slot` of the flat run the plain `payload` word
/// addresses; the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
pub(crate) fn store_flat<S: Stage>(stage: &mut S, payload: S::Word, slot: u16, (value, rep): (S::Word, Representation)) {
    let run = arena_run(stage, payload);
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    let address = stage.add_imm(run, offset);
    store_word(stage, address, value, rep);
}

/// `own_store` for a record whose values are a flat run (`Form::Arena`): the guards of
/// `admitted_payload` (the gate passes a `fact` proved immediate), then the run's slot; `None` (leaving at `miss`) where the record keeps
/// its values inline, changing nothing.
#[inline(always)]
pub(crate) fn own_flat_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, slot: u16,
    (value, rep, fact): (S::Word, Representation, Type), miss: S::Miss) -> Option<()> {
    let word = admitted_word(stage, record, shape, false, miss)?;
    let payload = gated_payload(stage, record, record_id, (value, fact), miss)?;
    let inline = storage_form(stage, word, OBJECT.inline_form);
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    store_flat(stage, payload, slot, (value, rep));
    Some(())
}

/// Proves the record at `record` of the exact shape `shape` with plain wide flat values in one compare:
/// its auxiliary word is the shape with no storage-form bit set (`recorded_shape`); `None`, leaving at
/// `miss`, otherwise. What the access of a receiver known to be a direct object runs (`base_region::proofs`).
#[inline(always)]
pub(crate) fn own_flat_known<S: Stage>(stage: &mut S, record: S::Word, shape: u32, miss: S::Miss) -> Option<()> {
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let shape = stage.site(u64::from(shape));
    let same = stage.eq_imm(word, shape);
    stage.guard(same, miss)
}

/// `value` written at flat `slot` of the record at `record`, which `own_flat_known` proved of the slot's
/// shape with plain flat values; the value is one the slot's observed type holds, so the store gate does not apply.
#[inline(always)]
pub(crate) fn proved_flat_store<S: Stage>(stage: &mut S, record: S::Word, slot: u16, (value, rep): (S::Word, Representation)) {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    store_flat(stage, payload, slot, (value, rep));
}

/// `value`, a word in `rep`'s encoding, written at `slot`, a word (the shapes' storage length or a cache
/// entry's slot), of the plain object at `record` whose `payload` word the caller read and proved plain,
/// with its values in `form`:
/// a half of the payload word for a packed object, which needs the value's narrow form, or the arena
/// run's slot. A form the site proved is guarded, not decoded; `None` (leaving at `miss`, nothing
/// written) where the object is in the other form, a packed slot cannot hold the value, or the
/// run is narrow (`object::narrow_run`), whose halves take no wide value.
#[inline(always)]
pub(crate) fn store_slot<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, slot: S::Word, form: Form, (value, rep): (S::Word, Representation),
    miss: S::Miss) -> Option<()> {
    debug_assert!(rep != Representation::Float64 || form == Form::Arena, "a packed slot holds a narrow value, never a raw double");
    let inline = inline_storage(stage, record);
    match form {
        Form::Packed => stage.guard(inline, miss)?,
        Form::Arena => {
            let flat = stage.not(inline);
            stage.guard(flat, miss)?;
        }
        Form::Either => {}
    }
    // A packed slot needs the value's narrow form; a flat one takes any value.
    let narrow = if form == Form::Arena { stage.constant(0) } else { packed_form(stage, inline, value, Type::ANY, miss)? };
    stage.choose_leaving(inline, #[inline(always)] |stage| {
        let half = stage.mul_imm(slot, HALF_BYTES);
        let address = stage.add(record, half);
        stage.store_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32, narrow);
        Some(stage.constant(0))
    }, #[inline(always)] move |stage| {
        let run = flat_run(stage, payload, miss)?;
        let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
        let address = stage.add(run, offset);
        store_word(stage, address, value, rep);
        Some(stage.constant(0))
    })?;
    Some(())
}

/// `value`, a word in `rep`'s encoding, written at the arena slot `address`.
#[inline(always)]
pub(crate) fn store_word<S: Stage>(stage: &mut S, address: S::Word, value: S::Word, rep: Representation) {
    match rep {
        Representation::Float64 => stage.store_f64(Region::ObjectSlots.mutable(), address, 0, value),
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value),
    }
}
