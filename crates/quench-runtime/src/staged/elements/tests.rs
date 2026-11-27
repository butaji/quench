use super::*;
use crate::Value;

#[test]
fn a_dense_element_reads_present_values_and_leaves_past_the_backing_or_at_a_hole() {
    let elements = [Value::number(1.5), Value::DELETED, Value::UNDEFINED];
    let read = |index| concrete_dense_element(&elements, index).map(Value::to_bits);
    assert_eq!(read(0), Some(Value::number(1.5).to_bits()));
    assert_eq!(read(1), None, "a hole leaves");
    assert_eq!(read(2), Some(Value::UNDEFINED.to_bits()));
    assert_eq!(read(3), None, "past the backing leaves");
}

#[test]
fn a_packed_view_reads_a_hole_as_the_value_its_consumer_rejects_and_still_leaves_past_the_backing() {
    let elements = [Value::number(1.5), Value::DELETED];
    let view = |packed| DenseView { values: elements.as_ptr() as u64, length: elements.len() as u64, packed, within: false, double: None, uncounted: false };
    let read = |packed, index| read_element(&mut Concrete::new(std::ptr::null()), view(packed), index, ());
    assert_eq!(read(false, 1), None, "an unpacked read leaves at a hole");
    assert_eq!(read(true, 1), Some(Value::DELETED.to_bits()), "a packed read has no hole test");
    assert_eq!(read(true, 0), Some(Value::number(1.5).to_bits()));
    assert_eq!(read(true, 2), None, "past the backing leaves either way");
}

#[test]
fn a_double_view_reads_numbers_canonically_and_replaces_numbers_as_binary64() {
    let mut words = [3.0f64.to_bits(), Value::DELETED.to_bits(), 0.5f64.to_bits()];
    let view = DenseView { values: words.as_mut_ptr() as u64, length: 3, packed: true, within: false,
        double: Some(u64::from(crate::heap::DOUBLE_LEN)), uncounted: false };
    let mut stage = Concrete::new(std::ptr::null());
    assert_eq!(read_element(&mut stage, view, 0, ()), Some(Value::integer(3).to_bits()), "a whole double reads as its int32");
    assert_eq!(read_element(&mut stage, view, 1, ()), None, "a hole leaves even through a packed view");
    assert_eq!(read_element(&mut stage, view, 2, ()), Some(Value::number(0.5).to_bits()));
    assert_eq!(read_element(&mut stage, view, 3, ()), None, "past the backing leaves");
    assert!(replace_present(&mut stage, view, 2, Value::integer(1).to_bits(), ()).is_some(), "an int32 writes its binary64");
    assert_eq!(words[2], 1.0f64.to_bits());
    assert!(replace_present(&mut stage, view, 1, Value::number(2.5).to_bits(), ()).is_none(), "a hole is no present element");
    assert!(replace_present(&mut stage, view, 2, Value::number(2.5).to_bits(), ()).is_some());
    assert_eq!(words[2], 2.5f64.to_bits());
}

/// `append_element` on `Concrete` over `elements`' own backing, with its room.
fn append(elements: &mut crate::heap::UniqueElements, index: usize, value: Value) -> Option<u64> {
    let (length, capacity) = (elements.len() as u64, elements.capacity() as u64);
    let values = elements.as_mut_ptr() as u64;
    append_element(&mut Concrete::new(std::ptr::null()), values, length, capacity, index as u64, value.to_bits(), ())
}

#[test]
fn an_append_writes_the_slot_past_the_length_within_room_and_counts_it() {
    let mut backing = crate::heap::ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
    let elements = backing.make_mut();
    elements.reserve(2);
    let others = |elements: &mut crate::heap::UniqueElements| {
        Concrete::new(std::ptr::null()).load_u32(crate::staged::Region::Elements.mutable(), elements.as_mut_ptr() as u64,
            crate::heap::OTHERS_FROM_VALUES)
    };
    assert_eq!(others(elements), 1);
    assert_eq!(append(elements, 3, Value::integer(7)), None, "past the length leaves");
    assert_eq!(append(elements, 1, Value::integer(7)), None, "below the length leaves");
    assert_eq!(append(elements, 2, Value::integer(7)), Some(3));
    assert_eq!(append(elements, 3, Value::number(2.5)), Some(4));
    assert_eq!(elements.len(), 4);
    assert_eq!(others(elements), 2, "the double counts, the int32 does not");
    let read = |index| concrete_dense_element(elements, index).map(Value::to_bits);
    assert_eq!(read(2), Some(Value::integer(7).to_bits()));
    assert_eq!(read(3), Some(Value::number(2.5).to_bits()));
    while elements.len() < elements.capacity() { elements.push(Value::integer(0)); }
    let full = elements.len();
    assert_eq!(append(elements, full, Value::integer(7)), None, "a full backing leaves");
    crate::staged::Stage::store_u32(&mut Concrete::new(std::ptr::null()), crate::staged::Region::Elements.mutable(),
        elements.as_mut_ptr() as u64,
        crate::heap::OTHERS_FROM_VALUES, u64::from(crate::heap::OTHERS_UNKNOWN));
    elements.reserve(1);
    assert_eq!(append(elements, full, Value::number(0.25)), Some(full as u64 + 1));
    assert_eq!(others(elements), u64::from(crate::heap::OTHERS_UNKNOWN), "an unknown count stays unknown");
}

/// `append_in_room` on `Concrete` over `elements`' own backing, whose header sits
/// `header_values_offset` before its values.
fn append_in_room_of(elements: &mut crate::heap::UniqueElements, value: Value) -> Option<u64> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let values = elements.as_mut_ptr() as u64;
    let header = values - ARRAY.header_values_offset as u64;
    let backing = Backing { record: 0, header, values, length: elements.len() as u64 };
    append_in_room(&mut Concrete::new(std::ptr::null()), backing, value.to_bits(), ())
}

#[test]
fn an_append_in_room_reads_the_capacity_word_and_leaves_where_the_backing_is_full() {
    let mut backing = crate::heap::ArrayElements::from_slice(&[Value::integer(1)]);
    let elements = backing.make_mut();
    elements.reserve(2);
    let room = (elements.capacity() - elements.len()) as u64;
    assert!(room >= 2);
    for grown in 2..2 + room {
        assert_eq!(append_in_room_of(elements, Value::integer(0)), Some(grown));
    }
    assert_eq!(elements.len(), elements.capacity());
    assert_eq!(append_in_room_of(elements, Value::integer(0)), None, "a full backing leaves for the VM to grow it");
    assert_eq!(elements.len(), elements.capacity(), "the leave wrote nothing");
}

#[test]
fn the_interpreter_appends_in_room_and_leaves_references_holes_and_full_backings() {
    use crate::heap::{ArrayElements, ArrayObject, Heap};
    let mut heap = Heap::new();
    let mut backing = ArrayElements::from_slice(&[Value::integer(1)]);
    backing.make_mut().reserve(2);
    let array = heap.alloc_array(ArrayObject::fresh(Value::NULL, backing));
    let records = heap.array_record_base().expect("an array record");
    assert!(concrete_append_in_room(records, array, Value::number(2.5)));
    assert!(!concrete_append_in_room(records, array, array), "a reference passes the write barrier");
    assert!(!concrete_append_in_room(records, array, Value::DELETED), "a hole is no element");
    assert!(!concrete_append_in_room(records, Value::NULL, Value::integer(1)), "no array");
    let mut length = 2;
    while concrete_append_in_room(records, array, Value::integer(0)) {
        length += 1;
        assert!(length < 64, "the room is the capacity word's");
    }
    let elements = heap.array(array).expect("the array").elements();
    assert_eq!((elements.len(), elements.as_slice()[1]), (length, Value::number(2.5)));
}
