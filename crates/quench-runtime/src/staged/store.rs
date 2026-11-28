//! An own data field's store as a staged operation: a record of a proved exact shape takes
//! a value at a slot, whichever of the two plain storage forms it keeps its values in. The
//! interpreter runs it on `Concrete`; a specializer emits it from the same rows.

use super::narrow::{narrow_form, packed_form, storable_form, UPPER_WORD};
use super::object::{inline_storage, HALF_BYTES};
use super::slot_store::{admitted_payload, gate_open, store_fact, store_flat};
use super::Stage;
use super::region::Region;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::{Representation, Type};
use crate::value::ValueLayout;
use crate::{Value, ValueType};

/// Stores `value` at `slot` of the object record at `record` (id `record_id`) where its exact
/// shape is `shape`, its values are plain (packed in its payload word or a run in the arena,
/// no extras), a packed slot can hold the value's narrow form and the store gate admits the
/// write; `None` (leaving at `miss`) otherwise, changing nothing.
#[inline(always)]
pub fn own_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, hinted: bool, slot: u16,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted_payload(stage, record, record_id, shape, hinted, value, miss)?;
    if hinted {
        // The hint is a layout id the record's matched: its form, packed or a flat run, is a
        // fact of the id (`ic::layout_form`), so the store takes its arm with no flag test.
        let packed = super::ic::layout_form(shape) & OBJECT.inline_form != 0;
        return own_store_formed(stage, (record, payload, slot), value, packed, miss);
    }
    let fact = store_fact(stage, value, Type::ANY);
    own_store_with(stage, (record, payload, slot), (value, fact), miss)
}

/// `own_store_with` of a record whose form the caller knows and of a value nothing is proved
/// of: packed (`packed`), its narrow form into its half of the payload word, else into its run.
///
/// The flat word is stored as it is: this store passed `store_gate`, which admits no write while
/// slot representations are tracked (`Heap::store_unlogged`), and a Number-typed slot exists only
/// while they are. The stores that write a Number slot convert: `slot_store::class_word` for the
/// arm stores, `ValueArena::stored_word` for the heap's.
#[inline(always)]
pub fn own_store_formed<S: Stage>(stage: &mut S, (record, payload, slot): (S::Word, S::Word, u16),
    value: S::Word, packed: bool, miss: S::Miss) -> Option<()> {
    if !packed {
        store_flat(stage, payload, slot, (value, Representation::Tagged));
        return Some(());
    }
    let fact = store_fact(stage, value, Type::ANY);
    let narrow = narrow_form(stage, value, fact);
    let narrow = storable_form(stage, narrow, miss)?;
    let half = stage.site(OBJECT.payload_offset as u64 + u64::from(slot) * HALF_BYTES);
    let address = stage.add_imm(record, half);
    stage.store_u32(Region::Payload.mutable(), address, 0, narrow);
    Some(())
}

/// `own_store` for a record whose exact shape the caller proved: the same gate and storage
/// decisions, leaving the shape test to whoever holds its proof (`property_write::in_place`).
/// `fact` is what the frame proved of `value` (`PropertyFrame::value_fact`): it folds the gate
/// and selects the narrow encoder.
#[inline(always)]
pub fn own_store_proved<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, slot: u16,
    value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    // The arm proved the slot's type holds `value` (`fact`): the store observes nothing.
    let value = (value.0, store_fact(stage, value.0, value.1));
    super::slot_store::held_gate_open(stage, record_id, value, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    own_store_with(stage, (record, payload, slot), value, miss)
}

/// `own_store_proved` of a record whose layout the caller proved keeps its values in a plain arena
/// run (`StoreArm::layout`): the gate, then the word into the run, with no form test.
#[inline(always)]
pub fn own_store_flat_proved<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, slot: u16,
    value: (S::Word, Type), miss: S::Miss) -> Option<()> {
    let value = (value.0, store_fact(stage, value.0, value.1));
    super::slot_store::held_gate_open(stage, record_id, value, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    store_flat(stage, payload, slot, (value.0, Representation::Tagged));
    Some(())
}

/// The storage arms of an own store whose guards have passed: a packed slot takes the narrow
/// form of `value`, a flat one any value.
#[inline(always)]
fn own_store_with<S: Stage>(stage: &mut S, (record, payload, slot): (S::Word, S::Word, u16),
    (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    let inline = inline_storage(stage, record);
    // A packed slot needs the value's narrow form; a flat one takes any value.
    let narrow = packed_form(stage, inline, value, fact, miss)?;
    stage.choose(inline, #[inline(always)] |stage| {
        let half = stage.site(OBJECT.payload_offset as u64 + u64::from(slot) * HALF_BYTES);
        let address = stage.add_imm(record, half);
        stage.store_u32(Region::Payload.mutable(), address, 0, narrow);
        stage.constant(0)
    }, #[inline(always)] |stage| {
        store_flat(stage, payload, slot, (value, Representation::Tagged));
        stage.constant(0)
    });
    Some(())
}

/// `value` into the full-word slot at `address` of record `record_id`, which a frame bound
/// before the run (the site's cache slot): behind the barrier's gate, as the own store is.
#[inline(always)]
pub fn bound_full_store<S: Stage>(stage: &mut S, record_id: S::Word, address: S::Word,
    (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    let fact = store_fact(stage, value, fact);
    gate_open(stage, record_id, (value, fact), miss)?;
    stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value);
    Some(())
}

/// `value`'s narrow form into the packed slot at `address` of record `record_id`; a value with
/// no narrow form leaves at `miss`.
#[inline(always)]
pub fn bound_packed_store<S: Stage>(stage: &mut S, record_id: S::Word, address: S::Word,
    (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    let fact = store_fact(stage, value, fact);
    let narrow = narrow_form(stage, value, fact);
    let narrow = storable_form(stage, narrow, miss)?;
    gate_open(stage, record_id, (value, fact), miss)?;
    stage.store_u32(Region::Payload.mutable(), address, 0, narrow);
    Some(())
}

/// Whether `bits` is a value of the type `holds` names: an int32, a direct object, `null` or
/// `undefined`, each where `holds` has it. A slot of that type then keeps its representation
/// under the store (`StoreArm::of`): no value widens it.
#[inline(always)]
pub fn slot_holds<S: Stage>(stage: &mut S, bits: S::Word, holds: Type) -> S::Cond {
    let mut held = stage.ult(bits, bits);
    if Type::INT32.is_subtype(holds) {
        let upper = stage.and_imm(bits, UPPER_WORD);
        let integer = stage.eq_imm(upper, ValueLayout::INT_TAG);
        held = stage.any(held, integer);
    }
    if Type::DIRECT_OBJECT_FAMILY.is_subtype(holds) {
        let object = super::value::is_direct_object(stage, bits);
        held = stage.any(held, object);
    }
    for (class, constant) in [(ValueType::Null, Value::NULL), (ValueType::Undefined, Value::UNDEFINED)] {
        if Type::language(class).is_subtype(holds) {
            let named = stage.eq_imm(bits, constant.to_bits());
            held = stage.any(held, named);
        }
    }
    held
}

#[cfg(test)]
mod tests;

/// Whether `value` can never be called: an immediate, or a direct object, array or string. Every
/// callable (a function, a callable proxy) is another heap family.
#[inline(always)]
pub fn uncallable<S: Stage>(stage: &mut S, value: S::Word) -> S::Cond {
    let heap = super::value::is_heap(stage, value);
    let immediate = stage.not(heap);
    let object = super::value::is_direct_object(stage, value);
    let array = super::value::is_direct_array(stage, value);
    let string = super::value::is_direct_string(stage, value);
    let plain = stage.any(object, array);
    let plain = stage.any(plain, string);
    stage.any(immediate, plain)
}

/// Whether the store of `value` into `slot` of the plain object at `record`, whose exact shape the
/// caller proved, can change no method cache: a cache sees a callable written or overwritten
/// (`Vm::cached_store_is_callable`), so neither `value` nor the slot's word may be one. A record
/// that keeps its values inline is not answered (its slot is a narrow half).
#[inline(always)]
pub fn quiet_store<S: Stage>(stage: &mut S, record: S::Word, slot: u16, value: S::Word) -> S::Cond {
    let fresh = uncallable(stage, value);
    let inline = inline_storage(stage, record);
    let kept = stage.choose(inline, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| {
        let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
        let run = super::object::arena_run(stage, payload);
        let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
        let address = stage.add_imm(run, offset);
        let old = stage.load_u64(Region::ObjectSlots.mutable(), address, 0);
        let quiet = uncallable(stage, old);
        let (one, zero) = (stage.constant(1), stage.constant(0));
        stage.select(quiet, one, zero)
    });
    let kept = stage.ne_imm(kept, 0u32);
    stage.all(fresh, kept)
}

/// The run's admission of a store into `slot` of the proved `record` where a method cache may
/// name its atom (`named`, the test that none does): admitted where none does or the store is
/// quiet (`quiet_store`), else it leaves at `miss`. `None` admits it as is.
#[inline(always)]
pub fn admit_named<S: Stage>(stage: &mut S, named: Option<S::Cond>, (record, slot): (S::Word, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    let Some(named) = named else { return Some(()) };
    let quiet = quiet_store(stage, record, slot, value);
    let admitted = stage.any(named, quiet);
    stage.guard(admitted, miss)
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
