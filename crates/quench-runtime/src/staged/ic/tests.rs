use super::*;
use crate::heap::{ArrayObject, Heap, Object};
use crate::staged::Concrete;
use crate::Value;

/// The interpreter's reading of `run`: `None` for a miss.
fn concrete(heap: &Heap, ops: &[IcOp], object: Value) -> Option<Value> {
    let mut stage = Concrete::over(heap);
    let record = heap.object(object)?.record_only()? as *const Object as u64;
    // The object record table is gone (S7f): no table base reaches the ops.
    run(&mut stage, ops, Reached::bare(record), ()).map(Value::from_bits)
}

/// An array of `prototype` whose named properties are `values`, in slots from 1. An ordinary object is a class
/// cell with no record (`record_only` is `None`), so the record reads take an array, which keeps its record.
fn array_of(heap: &mut Heap, prototype: Value, values: &[Value]) -> Value {
    let array = heap.alloc_array(ArrayObject::new(Object::new(prototype), Vec::<Value>::new()));
    for (slot, value) in values.iter().enumerate() {
        heap.property_add(array, *value, slot as u32 + 1);
    }
    array
}

/// An own-field read guards the receiver's exact shape, and misses on a receiver of another shape.
#[test]
fn an_op_list_reads_an_own_slot_and_misses_past_its_guards() {
    let mut heap = Heap::new();
    for shape in 1..=2_u32 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    let object = array_of(&mut heap, Value::NULL, &[Value::number(7.0), Value::number(8.0)]);
    let single = array_of(&mut heap, Value::NULL, &[Value::number(7.0)]);
    let (own, other) = (heap.object(object).unwrap().exact_shape(), heap.object(single).unwrap().exact_shape());
    assert_ne!(own, other, "two shapes");
    let load = IcOp::LoadSlot { slot: 1, form: Form::Either, storage_len: 2, rep: Representation::Tagged };
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(own), load], object), Some(Value::number(8.0)));
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(other), load], object), None, "another shape");
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(own), load], single), None, "a receiver of another shape");
}

/// A prototype-selected constant answers for the link of the receiver's prototype and
/// misses for any other.
#[test]
fn a_prototype_constant_is_selected_by_the_receivers_link() {
    let mut heap = Heap::new();
    let prototype = heap.alloc_object(Object::new(Value::NULL));
    heap.ensure_cell_extras(prototype);
    heap.object_mut(prototype).unwrap().mark_as_prototype();
    let object = array_of(&mut heap, prototype, &[]);
    let (link, shape) = { let record = heap.object(object).unwrap(); (record.proto_link().bits(), record.exact_shape()) };
    let (one, two) = (Value::number(1.5).to_bits(), Value::number(2.5).to_bits());
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(shape), IcOp::PrototypeConstants(&[(link ^ 1, one), (link, two)])], object), Some(Value::number(2.5)));
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(shape), IcOp::PrototypeConstants(&[(link ^ 1, one)])], object), None);
}

/// A row calls `site` in an order its operand values never change: the same ops over two
/// sites give one trace and differ only in what the holes took.
#[test]
fn site_values_change_holes_not_the_trace() {
    let read = |shape: u32, slot: u16| {
        let mut trace = crate::staged::trace::Trace::default();
        let at = Reached::bare(0);
        let record = trace.constant(0);
        let at = Reached { record, ..at };
        let ops = [IcOp::GuardShape(shape), IcOp::LoadSlot { slot, form: Form::Either, storage_len: usize::from(slot) + 1, rep: Representation::Tagged }];
        run(&mut trace, &ops, at, ()).expect("a load ends the read");
        trace
    };
    let (first, second) = (read(5, 1), read(6, 0));
    assert_eq!(first.ops, second.ops, "the trace never depends on a site value");
    // Either form reads through one operand, the flat byte offset (`object::site_offset`).
    assert_eq!(first.holes, [5, 8]);
    assert_eq!(second.holes, [6, 0]);
}
