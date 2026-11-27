use super::*;
use crate::Value;
use crate::heap::{ArrayElements, NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};

fn stage() -> Concrete { Concrete::pure() }

#[test]
fn length_is_an_int32_word_up_to_int32_and_leaves_past_it() {
    let length = |length: u64| super::super::value::length_value(&mut stage(), length, ());
    assert_eq!(length(0), Some(Value::integer(0).to_bits()));
    assert_eq!(length(i32::MAX as u64), Some(Value::integer(i32::MAX).to_bits()));
    assert_eq!(length(i32::MAX as u64 + 1), None);
}

/// The backing of `elements` with room for `room` more values, and a record of a plain
/// array whose meta word is `meta`, as `writable_backing` reads one.
fn backing_of(elements: &mut ArrayElements, room: usize, meta: u32, record: &mut [u64]) -> (Backing<u64>, u64) {
    let unique = elements.make_mut();
    unique.reserve(room);
    let capacity = unique.capacity() as u64;
    let header = elements.as_ptr() as u64;
    let record_address = record.as_mut_ptr() as u64;
    let mut stage = stage();
    // Without extras, the object's payload word is the backing's header pointer.
    stage.store_u64(Region::Payload.mutable(), record_address, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32, header);
    let layout = crate::heap::layouts::intern(u64::from(meta) << u32::BITS);
    stage.store_u32(Region::ShapeWord.mutable(), record_address, (ARRAY.object_offset + OBJECT.layout_offset) as i32, u64::from(layout));
    let values = header + ARRAY.header_values_offset as u64;
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    (Backing { record: record_address, header, values, length }, capacity)
}

fn record() -> Vec<u64> { vec![0; ARRAY.record_size.div_ceil(size_of::<u64>())] }

#[test]
fn push_appends_within_room_and_answers_the_new_length() {
    let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
    let (mut record, mut s) = (record(), stage());
    let (backing, capacity) = backing_of(&mut elements, 2, 0, &mut record);
    let pushed = array_push_one(&mut s, backing, capacity, Value::integer(9).to_bits(), ());
    assert_eq!(pushed, Some(Value::integer(3).to_bits()));
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[2].to_bits(), Value::integer(9).to_bits());
}

#[test]
fn push_on_a_frozen_or_full_backing_leaves_for_the_generic_path() {
    let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
    let (mut record, mut s) = (record(), stage());
    let (frozen, capacity) = backing_of(&mut elements, 2, ARRAY.frozen_mask, &mut record);
    assert_eq!(array_push_one(&mut s, frozen, capacity, Value::integer(9).to_bits(), ()), None, "frozen");
    assert_eq!(elements.len(), 1, "nothing was written");
    let (open, _) = backing_of(&mut elements, 0, 0, &mut record);
    let full = elements.len() as u64;
    assert_eq!(array_push_one(&mut s, open, full, Value::integer(9).to_bits(), ()), None, "no room");
    assert_eq!(elements.len(), 1);
}

#[test]
fn pop_answers_undefined_when_empty_the_last_value_and_leaves_at_a_hole() {
    let mut s = stage();
    let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5), Value::DELETED]);
    let mut record = record();
    let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
    assert_eq!(array_pop(&mut s, backing, ()), None, "a hole leaves");
    assert_eq!(elements.len(), 3);
    let mut elements = ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
    let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
    assert_eq!(array_pop(&mut s, backing, ()), Some(Value::number(0.5).to_bits()));
    assert_eq!(elements.len(), 1);
    let others = s.load_u32(Region::Elements.mutable(), backing.values, crate::heap::OTHERS_FROM_VALUES);
    assert_eq!(others, 0, "the double left the count of others");
    let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
    assert_eq!(array_pop(&mut s, backing, ()), Some(Value::integer(1).to_bits()));
    let (backing, _) = backing_of(&mut elements, 0, 0, &mut record);
    assert_eq!(array_pop(&mut s, backing, ()), Some(Value::UNDEFINED.to_bits()), "empty");
    assert_eq!(elements.len(), 0);
}

#[test]
fn pop_on_a_frozen_backing_leaves_it_alone() {
    let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
    let mut record = record();
    let (backing, _) = backing_of(&mut elements, 0, ARRAY.frozen_mask, &mut record);
    assert_eq!(array_pop(&mut stage(), backing, ()), None);
    assert_eq!(elements.len(), 1);
}

#[test]
fn a_store_of_a_reference_is_left_to_the_logging_heap() {
    let mut s = stage();
    for value in [Value::integer(1), Value::number(0.5), Value::TRUE, Value::UNDEFINED, Value::NULL] {
        assert_eq!(unreferenced(&mut s, value.to_bits(), ()), Some(()), "{:#x}", value.to_bits());
    }
    for value in [Value::direct_object(1), Value::direct_array(1), Value::direct_string(1)] {
        assert_eq!(unreferenced(&mut s, value.to_bits(), ()), None, "{:#x}", value.to_bits());
    }
}

#[test]
fn push_on_a_non_extensible_backing_leaves_for_the_generic_path() {
    let mut elements = ArrayElements::from_slice(&[Value::integer(1)]);
    let mut record = record();
    let (backing, capacity) = backing_of(&mut elements, 2, crate::value_vec::VALUE_VEC_INTEGRITY_BITS & !ARRAY.frozen_mask, &mut record);
    assert_eq!(array_push_one(&mut stage(), backing, capacity, Value::integer(9).to_bits(), ()), None);
    assert_eq!(array_pop(&mut stage(), backing, ()), None);
    assert_eq!(elements.len(), 1, "nothing was written");
}

#[test]
fn is_integral_takes_integers_and_leaves_fractions_nan_and_infinities() {
    for (value, expected) in [
        (Value::integer(0), true), (Value::integer(-7), true), (Value::number(4.0), true),
        (Value::number(-0.0), true), (Value::number(9007199254740992.0), true),
        (Value::number(1.5), false), (Value::number(f64::NAN), false),
        (Value::number(f64::INFINITY), false), (Value::number(f64::NEG_INFINITY), false),
        (Value::TRUE, false), (Value::UNDEFINED, false), (Value::NULL, false),
    ] {
        assert_eq!(concrete_is_integral(value), expected, "{:#x}", value.to_bits());
    }
}
