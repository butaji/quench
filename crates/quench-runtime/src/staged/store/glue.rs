#![allow(unused_imports)]
use super::*;
use crate::staged::narrow::{narrow_form, packed_form, storable_form, UPPER_WORD};
use crate::staged::object::{inline_storage, HALF_BYTES};
use crate::staged::slot_store::{admitted_payload, gate_open, store_fact, store_flat};
use crate::staged::Stage;
use crate::staged::region::Region;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::{Representation, Type};
use crate::value::ValueLayout;
use crate::{Value, ValueType};

#[cfg(not(stencil_build))]
/// The interpreter's reading of `own_store`: `value` stored at `slot` of object record
/// `record_id` of `heap` where its exact shape is `shape` (a lane record's hint, the whole
/// auxiliary word, where `hinted`: `admitted_payload`); false changes nothing.
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose shape's slot the hint covers.
#[inline(always)]
pub unsafe fn concrete_own_store(heap: &mut crate::heap::Heap, record_id: u32, shape: u32, hinted: bool, slot: u16,
    value: crate::Value) -> bool {
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) };
    if hinted {
        // A hinted store's layout id (`Object::hint_shape`) names the shape and the form: one
        // compare of the record's own word, then the form the id's entry tells picks the arm,
        // as `own_store` does for a hint, without its second compare.
        // SAFETY: a live record, read before any write.
        if unsafe { (*record).layout_id() } != shape {
            return false;
        }
        let form = crate::staged::ic::layout_form(shape);
        if form == OBJECT.inline_form {
            // Packed: the value's narrow form into its half of the payload word, behind the gate.
            let record = record as u64;
            let mut stage = crate::staged::Concrete::over(heap);
            let fact = store_fact(&stage, value.to_bits(), Type::ANY);
            if gate_open(&mut stage, u64::from(record_id), (value.to_bits(), fact), ()).is_none() {
                return false;
            }
            return own_store_formed(&mut stage, (record, 0, slot), value.to_bits(), true, ()).is_some();
        }
        if form != 0 {
            // Extras or a narrow run: the general store.
            return false;
        }
        // The arena form: the payload is the run's start (`Object::plain_arena_start`).
        // SAFETY: a live record, read before any write.
        let start = unsafe { (*record).proved_arena_start() };
        if !heap.store_unlogged(crate::staged::StagedRecordSpace::Object, record_id, value) {
            return false;
        }
        // SAFETY: the layout matched, so its run holds `slot`; the gate admitted the write.
        unsafe { heap.arena_values_base().cast_mut().add(start + usize::from(slot)).write(value) };
        return true;
    }
    // The common form first, as `own_store` takes it: a plain object's arena run, whose start
    // is the payload itself, after the same store gate. An exact shape equals the record's
    // word in the arena form alone, so one compare proves the shape and the form
    // (`Object::formed_plain_start`). Packed inline values and every miss take the staged
    // store, which changes nothing on a miss.
    // SAFETY: a live record, read before any write.
    if let Some(start) = unsafe { (*record).formed_plain_start(shape) } {
        if !heap.store_unlogged(crate::staged::StagedRecordSpace::Object, record_id, value) {
            return false;
        }
        // SAFETY: the shape matched, so its run holds `slot`; the gate admitted the write.
        unsafe { heap.arena_values_base().cast_mut().add(start + usize::from(slot)).write(value) };
        return true;
    }
    let record = record as u64;
    let mut stage = crate::staged::Concrete::over(heap);
    own_store(&mut stage, record, u64::from(record_id), shape, hinted, slot, value.to_bits(), ()).is_some()
}

#[cfg(not(stencil_build))]
/// A hinted own-field store into a narrow run (`Object::formed_narrow_start`): `value`'s
/// narrow form at `slot` of record `record_id` where its layout id is `shape`, a
/// narrow run, and the store gate admits the write; false changes nothing (a value without a
/// narrow form leaves to the general store, which widens the run).
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose shape's slot the hint covers.
#[inline(always)]
pub unsafe fn concrete_narrow_store(heap: &mut crate::heap::Heap, record_id: u32, shape: u32, slot: u16, value: crate::Value) -> bool {
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) };
    // SAFETY: a live record, read before any write.
    let Some(start) = (unsafe { (*record).formed_narrow_start(shape) }) else {
        return false;
    };
    let Some(narrow) = value.to_narrow() else {
        return false;
    };
    if !heap.store_unlogged(crate::staged::StagedRecordSpace::Object, record_id, value) {
        return false;
    }
    // SAFETY: the shape matched, so the run's halves hold `slot`; the gate admitted the write.
    unsafe { heap.arena_values_base().cast::<u32>().cast_mut().add(2 * start + usize::from(slot)).write(narrow) };
    true
}
