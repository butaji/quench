use super::*;
use crate::heap::{Heap, Object};
use crate::staged::Concrete;
use crate::Value;

/// The interpreter's reading of `run`: `None` for a miss.
fn concrete(heap: &Heap, ops: &[IcOp], object: Value) -> Option<Value> {
    let mut stage = Concrete::over(heap);
    let record = heap.object(object)?.record_only()? as *const Object as u64;
    let records = heap.object_record_base()? as u64;
    run(&mut stage, ops, Reached::bare(record), Some(records), ()).map(Value::from_bits)
}

/// A prototype's read guards the receiver's and the prototype's shapes; a receiver of
/// another shape, a prototype of another, or a record with no object prototype misses.
#[test]
fn an_op_list_reads_a_prototype_slot_and_misses_past_its_guards() {
    let mut heap = Heap::new();
    for shape in 1..=2_u32 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    let prototype = heap.alloc_object(Object::new(Value::NULL));
    heap.property_add(prototype, Value::number(1.5), 1);
    heap.object_record_mut(prototype).unwrap().mark_as_prototype();
    let object = heap.alloc_object(Object::new(prototype));
    heap.property_add(object, Value::number(7.0), 1);
    heap.property_add(object, Value::number(8.0), 2);
    let (own, holder) = (heap.object(object).unwrap().exact_shape(), heap.object(prototype).unwrap().exact_shape());
    let read = |receiver, holder| [IcOp::GuardShape(receiver), IcOp::Prototype, IcOp::GuardShape(holder), IcOp::LoadPrototypeSlot(0)];
    assert_eq!(concrete(&heap, &read(own, holder), object), Some(Value::number(1.5)));
    let load = IcOp::LoadSlot { slot: 1, form: Form::Either, storage_len: 2, rep: Representation::Tagged };
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(own), load], object), Some(Value::number(8.0)));
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(holder), load], object), None);
    assert_eq!(concrete(&heap, &read(holder, holder), object), None);
    assert_eq!(concrete(&heap, &read(own, own), object), None);
    assert_eq!(concrete(&heap, &read(holder, holder), prototype), None);
}

/// A prototype-selected constant answers for the link of the receiver's prototype and
/// misses for any other.
#[test]
fn a_prototype_constant_is_selected_by_the_receivers_link() {
    let mut heap = Heap::new();
    let prototype = heap.alloc_object(Object::new(Value::NULL));
    heap.object_record_mut(prototype).unwrap().mark_as_prototype();
    let object = heap.alloc_object(Object::new(prototype));
    let (link, shape) = { let record = heap.object(object).unwrap(); (record.proto_link().bits(), record.exact_shape()) };
    let (one, two) = (Value::number(1.5).to_bits(), Value::number(2.5).to_bits());
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(shape), IcOp::PrototypeConstants(&[(link ^ 1, one), (link, two)])], object), Some(Value::number(2.5)));
    assert_eq!(concrete(&heap, &[IcOp::GuardShape(shape), IcOp::PrototypeConstants(&[(link ^ 1, one)])], object), None);
}

/// The interpreter keeps its typed prototype read (`Heap::holder_property_get_flat`),
/// which routing through `run` made slower (deltablue +1.6%); it reads what the staged
/// `LoadPrototypeSlot` reads at every slot of a prototype, of narrow and wide layouts.
#[test]
fn the_interpreters_prototype_read_agrees_with_the_staged_one() {
    let mut heap = Heap::new();
    let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide as u32 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    for len in [1, wide] {
        let prototype = heap.alloc_object(Object::new(Value::NULL));
        for slot in 0..len {
            heap.property_add(prototype, Value::number(slot as f64 + 0.5), slot as u32 + 1);
        }
        heap.object_record_mut(prototype).unwrap().mark_as_prototype();
        let object = heap.alloc_object(Object::new(prototype));
        let holder = heap.object(prototype).unwrap();
        for slot in 0..len {
            // SAFETY: the prototype's exact shape holds `slot` outside dictionary storage.
            let interpreter = unsafe { heap.holder_property_get_flat(holder.record_only().unwrap(), slot) };
            assert_eq!(concrete(&heap, &[IcOp::Prototype, IcOp::GuardShape(holder.exact_shape()), IcOp::LoadPrototypeSlot(slot as u16)], object), Some(interpreter),
                "slot {slot} of {len}");
        }
    }
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
        run(&mut trace, &ops, at, None, ()).expect("a load ends the read");
        trace
    };
    let (first, second) = (read(5, 1), read(6, 0));
    assert_eq!(first.ops, second.ops, "the trace never depends on a site value");
    // Either form reads through one operand, the flat byte offset (`object::site_offset`).
    assert_eq!(first.holes, [5, 8]);
    assert_eq!(second.holes, [6, 0]);
}
