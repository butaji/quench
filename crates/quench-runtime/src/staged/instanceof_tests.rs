//! `instanceof` by the words of a value's prototype chain (`object::instanceof_by_words`), over a real heap.
use super::{Concrete, object};
use crate::heap::{Heap, Object, ProtoLink};
use crate::value::ValueLayout;
use crate::Value;

/// The staged answer for `value` and a constructor whose `prototype` is `prototype`, or `None` where the words leave it.
fn words(heap: &Heap, value: Value, prototype: Value) -> Option<bool> {
    let records = heap.object_record_base().expect("object records are reserved") as u64;
    let mut stage = Concrete::new(std::ptr::null());
    object::instanceof_by_words(&mut stage, records, value.to_bits(), u64::from(ProtoLink::of(prototype).bits()), ())
}

/// A direct object's chain is walked to the prototype or `null`, a primitive is no instance, and any other
/// cell, or no table to walk in, is the VM's.
#[test]
fn a_chain_of_object_records_is_walked_and_a_primitive_is_no_instance() {
    let mut heap = Heap::new();
    let root = heap.alloc_object(Object::new(Value::NULL));
    let middle = heap.alloc_object(Object::new(root));
    let leaf = heap.alloc_object(Object::new(middle));
    let other = heap.alloc_object(Object::new(Value::NULL));
    for (value, prototype, holds) in [(leaf, middle, true), (leaf, root, true), (middle, middle, false), (other, root, false),
        (leaf, other, false), (Value::integer(1), root, false), (Value::UNDEFINED, root, false), (Value::TRUE, root, false)] {
        assert_eq!(words(&heap, value, prototype), Some(holds), "{value:?} {prototype:?}");
    }
    let cell = Value::from_bits(ValueLayout::HEAP_TAG | 1);
    assert_eq!(words(&heap, cell, root), None, "a cell the walk does not follow is the VM's");
    let mut stage = Concrete::new(std::ptr::null());
    assert_eq!(object::instanceof_by_words(&mut stage, 0, leaf.to_bits(), u64::from(ProtoLink::of(root).bits()), ()), None, "no record table");
}
