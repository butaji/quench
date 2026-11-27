use super::*;
use crate::heap::{ArrayElements, ArrayObject, Heap, Object};
use crate::Value;

/// A heap with a plain object of two properties (shapes 1 and 2) and the snapshot array of its two
/// keys; the object's exact shape is the first answer.
fn listed() -> (Heap, Value, Value, u32) {
    let mut heap = Heap::new();
    for shape in 1..=2 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    let object = heap.alloc_object(Object::new(Value::NULL));
    heap.property_add(object, Value::integer(1), 1);
    heap.property_add(object, Value::integer(2), 2);
    let shape = heap.object(object).expect("an object").exact_shape();
    let snapshot = ArrayElements::from_slice(&[Value::integer(10), Value::integer(20)]);
    let keys = heap.alloc_array(ArrayObject::fresh(Value::NULL, snapshot));
    (heap, object, keys, shape)
}

fn state(heap: &mut Heap, (object, keys): (Value, Value), memo: [u64; 2]) -> Value {
    let elements = [object, keys, Value::integer(0), Value::from_bits(memo[0]), Value::from_bits(memo[1]), Value::UNDEFINED];
    heap.alloc_array(ArrayObject::fresh(Value::NULL, ArrayElements::from_slice(&elements)))
}

fn step(heap: &Heap, state: Value, epoch: u64) -> Option<Value> {
    concrete_for_in_next(heap, state, epoch)
}

fn cursor(heap: &Heap, state: Value) -> Value {
    heap.array(state).expect("a state").elements()[FOR_IN_CURSOR]
}

/// A state whose memo is the object's shape and the current epoch yields its keys in order and moves
/// its cursor; at the end of the snapshot the row leaves for the full step, which jumps.
#[test]
fn a_state_whose_memo_holds_steps_through_its_keys() {
    let (mut heap, object, keys, shape) = listed();
    let epoch = 7;
    let state = state(&mut heap, (object, keys), [u64::from(shape), epoch]);
    assert_eq!(step(&heap, state, epoch), Some(Value::integer(10)));
    assert_eq!(cursor(&heap, state), Value::integer(1));
    assert_eq!(step(&heap, state, epoch), Some(Value::integer(20)));
    assert_eq!(step(&heap, state, epoch), None, "the snapshot is exhausted");
    assert_eq!(cursor(&heap, state), Value::integer(2), "a leave moves nothing");
}

/// A state whose memo is another epoch or shape, or none, or whose object has changed shape, leaves
/// before its cursor moves.
#[test]
fn a_state_whose_memo_does_not_hold_leaves_before_writing() {
    let (mut heap, object, keys, shape) = listed();
    let epoch = 7;
    let held = (object, keys);
    let stale = state(&mut heap, held, [u64::from(shape), epoch]);
    assert_eq!(step(&heap, stale, epoch + 1), None, "a collection passed");
    let unmemoed = state(&mut heap, held, [FOR_IN_NO_MEMO; 2]);
    assert_eq!(step(&heap, unmemoed, epoch), None, "the keys came from the general walk");
    let other_shape = state(&mut heap, held, [u64::from(shape) + 1, epoch]);
    assert_eq!(step(&heap, other_shape, epoch), None, "the object is not of the listed shape");
    heap.object_record_mut(object).expect("an object").mark_as_prototype();
    let prototype = state(&mut heap, held, [u64::from(shape), epoch]);
    assert_eq!(step(&heap, prototype, epoch), None, "an object with extras keeps keys outside its shape");
    for state in [stale, unmemoed, other_shape, prototype] {
        assert_eq!(cursor(&heap, state), Value::integer(0));
    }
}
