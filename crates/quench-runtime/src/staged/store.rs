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
pub(crate) fn own_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, hinted: bool, slot: u16,
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
#[inline(always)]
fn own_store_formed<S: Stage>(stage: &mut S, (record, payload, slot): (S::Word, S::Word, u16),
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
pub(crate) fn own_store_proved<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, slot: u16,
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
pub(crate) fn own_store_flat_proved<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, slot: u16,
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
pub(crate) fn slot_holds<S: Stage>(stage: &mut S, bits: S::Word, holds: Type) -> S::Cond {
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

#[cfg(not(stencil_build))]
/// The interpreter's reading of `own_store`: `value` stored at `slot` of object record
/// `record_id` of `heap` where its exact shape is `shape` (a lane record's hint, the whole
/// auxiliary word, where `hinted`: `admitted_payload`); false changes nothing.
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose shape's slot the hint covers.
#[inline(always)]
pub(crate) unsafe fn concrete_own_store(heap: &mut crate::heap::Heap, record_id: u32, shape: u32, hinted: bool, slot: u16,
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
        let form = super::ic::layout_form(shape);
        if form == OBJECT.inline_form {
            // Packed: the value's narrow form into its half of the payload word, behind the gate.
            let record = record as u64;
            let mut stage = super::Concrete::over(heap);
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
        if !heap.store_unlogged(super::StagedRecordSpace::Object, record_id, value) {
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
        if !heap.store_unlogged(super::StagedRecordSpace::Object, record_id, value) {
            return false;
        }
        // SAFETY: the shape matched, so its run holds `slot`; the gate admitted the write.
        unsafe { heap.arena_values_base().cast_mut().add(start + usize::from(slot)).write(value) };
        return true;
    }
    let record = record as u64;
    let mut stage = super::Concrete::over(heap);
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
pub(crate) unsafe fn concrete_narrow_store(heap: &mut crate::heap::Heap, record_id: u32, shape: u32, slot: u16, value: crate::Value) -> bool {
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) };
    // SAFETY: a live record, read before any write.
    let Some(start) = (unsafe { (*record).formed_narrow_start(shape) }) else {
        return false;
    };
    let Some(narrow) = value.to_narrow() else {
        return false;
    };
    if !heap.store_unlogged(super::StagedRecordSpace::Object, record_id, value) {
        return false;
    }
    // SAFETY: the shape matched, so the run's halves hold `slot`; the gate admitted the write.
    unsafe { heap.arena_values_base().cast::<u32>().cast_mut().add(2 * start + usize::from(slot)).write(narrow) };
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Heap, Object};
    use crate::Value;

    /// An object of `count` int32 properties, in packed inline storage where they fit.
    fn object_of(heap: &mut Heap, count: usize) -> (Value, u32) {
        for shape in 1..=count as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc_object(Object::new(Value::NULL));
        for shape in 1..=count as u32 {
            heap.property_add(object, Value::integer(0), shape);
        }
        let shape = heap.object(object).unwrap().exact_shape();
        (object, shape)
    }

    fn read(heap: &Heap, object: Value, shape: u32, slot: u16) -> Option<Value> {
        // SAFETY: a live record of the shape, whose slot it holds.
        unsafe { crate::staged::concrete_own_field_hit(heap.object(object).unwrap().record(), heap.arena_values_base(), shape, slot) }
    }

    fn store(heap: &mut Heap, object: Value, shape: u32, slot: u16, value: Value) -> bool {
        let id = heap.record_id(object.direct_object_index().unwrap());
        // SAFETY: a live record of the shape, whose slot it holds.
        unsafe { concrete_own_store(heap, id, shape, false, slot, value) }
    }

    #[test]
    fn a_packed_store_takes_integers_references_and_constants_and_leaves_the_rest() {
        let mut heap = Heap::new();
        let (object, shape) = object_of(&mut heap, 2);
        let other = heap.alloc_object(Object::new(Value::NULL));
        for value in [Value::integer(-7), Value::integer(1 << 20), other, Value::NULL, Value::UNDEFINED, Value::TRUE, Value::FALSE] {
            assert!(store(&mut heap, object, shape, 1, value), "{value:?}");
            assert_eq!(read(&heap, object, shape, 1), Some(value));
            assert_eq!(read(&heap, object, shape, 0), Some(Value::integer(0)), "the other half stays");
        }
        let before = read(&heap, object, shape, 1);
        for value in [Value::number(1.5), Value::integer(i32::MAX)] {
            assert!(!store(&mut heap, object, shape, 1, value), "{value:?} has no narrow form");
            assert_eq!(read(&heap, object, shape, 1), before, "a refused store changes nothing");
        }
        assert!(!store(&mut heap, object, shape + 1, 1, Value::integer(1)), "another shape leaves");
    }

    #[test]
    fn a_flat_store_takes_any_value() {
        let mut heap = Heap::new();
        let (object, shape) = object_of(&mut heap, crate::value_vec::INLINE_PROPERTY_COUNT + 2);
        let slot = crate::value_vec::INLINE_PROPERTY_COUNT as u16 + 1;
        for value in [Value::number(1.5), Value::integer(i32::MAX), Value::NULL] {
            assert!(store(&mut heap, object, shape, slot, value), "{value:?}");
            assert_eq!(read(&heap, object, shape, slot), Some(value));
        }
    }

    /// The store's holes are its shape, packed half offset and flat offset, in that order,
    /// whatever their values: a stencil patches them and runs one code.
    #[test]
    fn the_store_leaves_its_operands_as_holes_not_trace() {
        let run = |shape: u32, slot: u16| {
            let mut stage = crate::staged::trace::Trace::default();
            let (record, id, value) = (stage.constant(0), stage.constant(1), stage.constant(2));
            own_store(&mut stage, record, id, shape, false, slot, value, ());
            (stage.ops, stage.holes)
        };
        let (first, holes) = run(3, 1);
        let (second, other) = run(40, 2);
        assert_eq!(first, second);
        assert_eq!(holes, [3, 4, 8]);
        assert_eq!(other, [40, 8, 16]);
    }

    /// The operations of the shared own store of a value the frame proved of type `fact`.
    fn trace_of(fact: Type) -> Vec<String> {
        let mut stage = crate::staged::trace::Trace::default();
        let (record, id, value) = (stage.constant(0), stage.constant(1), stage.constant(2));
        own_store_proved(&mut stage, record, id, 1, (value, fact), ());
        stage.ops
    }

    fn count(ops: &[String], operation: &str) -> usize { ops.iter().filter(|line| line.starts_with(operation)).count() }

    /// The binding time of the value's type decides the store's shape: an immediate type
    /// folds the barrier away, an int32 takes the integer narrow arm alone, and nothing known
    /// keeps the gate and the generic encoder.
    #[test]
    fn a_static_fact_folds_the_barrier_and_selects_the_integer_narrow_arm() {
        let (generic, int32) = (trace_of(Type::ANY), trace_of(Type::INT32));
        assert_eq!(count(&generic, "store_gate"), 1, "an unknown value asks the gate");
        for immediate in [Type::NUMBER, Type::INT32, Type::language(ValueType::Boolean), Type::NULLISH] {
            assert_eq!(count(&trace_of(immediate), "store_gate"), 0, "{immediate:?} names no heap record");
        }
        assert_eq!(count(&trace_of(Type::language(ValueType::Object)), "store_gate"), 1, "an object needs the barrier");
        assert_eq!(count(&generic, "choose"), count(&int32, "choose") + 1, "an int32 needs no tag test");
        assert!(int32.len() < generic.len());
    }

    #[test]
    fn a_slot_holds_test_names_the_classes_its_type_has() {
        let ops = |holds: Type| {
            let mut stage = crate::staged::trace::Trace::default();
            let bits = stage.constant(0);
            slot_holds(&mut stage, bits, holds);
            stage.ops
        };
        let never = ops(Type::language(ValueType::String));
        assert_eq!(count(&never, "any"), 0, "no class of the type is testable");
        assert_eq!(count(&ops(Type::INT32), "any"), 1);
        let nullable = Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH);
        assert_eq!(count(&ops(nullable), "any"), 3, "an object, null and undefined");
    }
}

/// Whether `value` can never be called: an immediate, or a direct object, array or string. Every
/// callable (a function, a callable proxy) is another heap family.
#[inline(always)]
pub(crate) fn uncallable<S: Stage>(stage: &mut S, value: S::Word) -> S::Cond {
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
pub(crate) fn quiet_store<S: Stage>(stage: &mut S, record: S::Word, slot: u16, value: S::Word) -> S::Cond {
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
pub(crate) fn admit_named<S: Stage>(stage: &mut S, named: Option<S::Cond>, (record, slot): (S::Word, u16), value: S::Word, miss: S::Miss) -> Option<()> {
    let Some(named) = named else { return Some(()) };
    let quiet = quiet_store(stage, record, slot, value);
    let admitted = stage.any(named, quiet);
    stage.guard(admitted, miss)
}
