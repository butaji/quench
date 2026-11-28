use super::*;
use crate::staged::stage::Stage;
use crate::heap::{ArrayObject, Heap, Object, ProtoLink};
use crate::code_dependencies::PublishedCode;
use crate::Value;
use std::rc::Rc;

/// An array of `prototype` whose named properties are `values`, in slots from 1 under their shapes. An ordinary
/// object is a class cell with no record (`record_only` is `None`); an array keeps its record and its properties.
fn array_of(heap: &mut Heap, prototype: Value, values: &[Value]) -> Value {
    let array = heap.alloc_array(ArrayObject::new(Object::new(prototype), Vec::<Value>::new()));
    for (slot, value) in values.iter().enumerate() {
        heap.property_add(array, *value, slot as u32 + 1);
    }
    array
}

#[test]
fn a_holder_read_needs_the_shape_the_link_and_the_watch() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 1, None);
    let prototype = heap.alloc_object(Object::new(Value::NULL));
    let object = array_of(&mut heap, prototype, &[Value::integer(1)]);
    let record = heap.object(object).unwrap().record_only().unwrap();
    let guard = record.guard_word();
    let header = crate::heap::layouts::word(guard as u32);
    assert_eq!(guard as u32, record.hint_shape());
    assert_eq!(header as u32 & !crate::staged::ic::FORM_BITS, record.exact_shape());
    assert_eq!((header >> u32::BITS) as u32 & ProtoLink::of(prototype).bits(), ProtoLink::of(prototype).bits());
    let watch = Rc::new(PublishedCode::new(()));
    let hint = Value::integer(42);
    let read = |guard, watch: &Rc<PublishedCode<()>>| unsafe { concrete_holder_read(record, guard, Rc::as_ptr(watch), hint) };
    assert_eq!(read(guard, &watch), Some(hint));
    assert_eq!(read(guard + (1 << u32::BITS), &watch), None, "another shape");
    assert_eq!(read(guard ^ 1, &watch), None, "another prototype");
    watch.retire();
    assert_eq!(read(guard, &watch), None, "a retired watch");
}

#[test]
fn an_own_constant_read_needs_the_record_itself_and_the_watch() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 1, None);
    let first = array_of(&mut heap, Value::NULL, &[Value::integer(1)]);
    let second = array_of(&mut heap, Value::NULL, &[Value::integer(1)]);
    let (one, other) = (heap.object(first).unwrap().record_only().unwrap(), heap.object(second).unwrap().record_only().unwrap());
    assert_eq!(one.guard_word(), other.guard_word(), "one layout");
    let expected = one as *const Object as u64;
    let watch = Rc::new(PublishedCode::new(()));
    let value = Value::integer(42);
    let read = |record: &Object, watch: &Rc<PublishedCode<()>>| unsafe { concrete_own_constant_read(record, expected, Rc::as_ptr(watch), value) };
    assert_eq!(read(one, &watch), Some(value));
    assert_eq!(read(other, &watch), None, "another object of the layout");
    watch.retire();
    assert_eq!(read(one, &watch), None, "a retired watch");
}

/// The row's operands are holes in a fixed order: guard, watch, value.
#[test]
fn the_holder_read_leaves_its_operands_as_holes() {
    let run = |guard, watch, value| {
        let mut stage = crate::staged::trace::Trace::default();
        let record = stage.constant(0);
        holder_read(&mut stage, record, guard, watch, value, ());
        (stage.ops, stage.holes)
    };
    let (first, holes) = run(3, 100, 7);
    let (second, other) = run(4, 200, 8);
    assert_eq!(first, second);
    assert_eq!(holes, [3, 100, 7]);
    assert_eq!(other, [4, 200, 8]);
}
