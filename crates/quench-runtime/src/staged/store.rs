//! An own data field's store as a staged operation: a record of a proved exact shape takes
//! a value at a slot, whichever of the two plain storage forms it keeps its values in. The
//! interpreter runs it on `Concrete`; a specializer emits it from the same rows.

use super::object::{HALF_BYTES, arena_run, inline_storage, packed_form, plain_payload};
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// Stores `value` at `slot` of the object record at `record` (id `record_id`) where its exact
/// shape is `shape`, its values are plain (packed in its payload word or a run in the arena,
/// no extras), a packed slot can hold the value's narrow form and the store gate admits the
/// write; `None` (leaving at `miss`) otherwise, changing nothing.
#[inline(always)]
pub(crate) fn own_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, slot: u16,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let exact = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    let shape = stage.site(u64::from(shape));
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    let open = stage.store_gate(record_id, value);
    stage.guard(open, miss)?;
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    let inline = inline_storage(stage, record);
    // A packed slot needs the value's narrow form; a flat one takes any value.
    let narrow = packed_form(stage, inline, value, miss)?;
    stage.choose(inline, |stage| {
        let half = stage.site(OBJECT.payload_offset as u64 + u64::from(slot) * HALF_BYTES);
        let address = stage.add_imm(record, half);
        stage.store_u32(address, 0, narrow);
        stage.constant(0)
    }, |stage| {
        let run = arena_run(stage, payload);
        let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
        let address = stage.add_imm(run, offset);
        stage.store_u64(address, 0, value);
        stage.constant(0)
    });
    Some(())
}

/// The interpreter's reading of `own_store`: `value` stored at `slot` of object record
/// `record_id` of `heap` where its exact shape is `shape`; false changes nothing.
///
/// # Safety
/// `record_id` names a live object record of `heap`, whose shape's slot the hint covers.
#[inline(always)]
pub(crate) unsafe fn concrete_own_store(heap: &mut crate::heap::Heap, record_id: u32, shape: u32, slot: u16,
    value: crate::Value) -> bool {
    // SAFETY: the caller's contract; the gate reads the heap before any write.
    let record = unsafe { heap.unlogged_object_record(record_id) } as u64;
    let mut stage = super::Concrete::storing(heap);
    own_store(&mut stage, record, u64::from(record_id), shape, slot, value.to_bits(), ()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object};
    use crate::Value;

    /// An object of `count` int32 properties, in packed inline storage where they fit.
    fn object_of(heap: &mut Heap, count: usize) -> (Value, u32) {
        for shape in 1..=count as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        for shape in 1..=count as u32 {
            heap.property_add(object, Value::integer(0), shape);
        }
        let shape = heap.object(object).unwrap().exact_shape();
        (object, shape)
    }

    fn read(heap: &Heap, object: Value, shape: u32, slot: u16) -> Option<Value> {
        // SAFETY: a live record of the shape, whose slot it holds.
        unsafe { crate::staged::concrete_own_field_hit(heap.object(object).unwrap(), heap.arena_values_base(), shape, slot) }
    }

    fn store(heap: &mut Heap, object: Value, shape: u32, slot: u16, value: Value) -> bool {
        let id = object.direct_object_index().unwrap();
        // SAFETY: a live record of the shape, whose slot it holds.
        unsafe { concrete_own_store(heap, id, shape, slot, value) }
    }

    #[test]
    fn a_packed_store_takes_integers_references_and_constants_and_leaves_the_rest() {
        let mut heap = Heap::new();
        let (object, shape) = object_of(&mut heap, 2);
        let other = heap.alloc(Cell::Object(Object::new(Value::NULL)));
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
            own_store(&mut stage, record, id, shape, slot, value, ());
            (stage.ops, stage.holes)
        };
        let (first, holes) = run(3, 1);
        let (second, other) = run(40, 2);
        assert_eq!(first, second);
        assert_eq!(holes, [3, 4, 8]);
        assert_eq!(other, [40, 8, 16]);
    }
}
