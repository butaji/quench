//! Array record tests: the array record's element and header layout.

use super::*;

use super::super::ArrayElements;

fn values(array: &ArrayObject) -> Vec<Value> {
    array.elements().as_slice().to_vec()
}

/// The backing header the staged reader finds for `array` (`staged::elements::array_header`).
fn staged_header(array: &ArrayObject) -> u64 {
    let mut stage = crate::staged::Concrete::new(std::ptr::null());
    crate::staged::elements::array_header(&mut stage, array as *const ArrayObject as u64)
}

#[test]
fn an_array_without_extras_keeps_its_backing_in_its_payload_word() {
    let elements = ArrayElements::from_slice(&[Value::integer(1), Value::integer(2)]);
    let array = ArrayObject::new(Object::new(Value::NULL), elements.clone());
    assert!(!array.has_extras());
    assert_eq!({ array.object.payload }, elements.as_ptr() as u64);
    assert_eq!(staged_header(&array), elements.as_ptr() as u64);
    assert_eq!(array.properties().into_parts().0 & VALUE_VEC_START_BITS, EMPTY_POSITION);
    assert_eq!(array.plain_storage_start(), None);
    assert_eq!(elements.owner_count(), 2);
    let copy = array.clone();
    assert_eq!(elements.owner_count(), 3);
    assert_eq!(values(&copy), values(&array));
    // The object alone owns no backing.
    let object = Object::clone(&array);
    assert_eq!({ object.payload }, u64::from(EMPTY_POSITION));
    drop(object);
    assert_eq!(elements.owner_count(), 3);
    drop(copy);
    drop(array);
    assert_eq!(elements.owner_count(), 1);
    // The empty backing and a fresh array own nothing.
    let empty = ArrayObject::fresh(Value::NULL, ArrayElements::new());
    assert!(!empty.needs_drop());
    assert_eq!(empty.elements().len(), 0);
}

#[test]
fn extras_take_an_arrays_backing_with_them() {
    let elements = ArrayElements::from_slice(&[Value::integer(3)]);
    let mut array = ArrayObject::fresh(Value::NULL, elements.clone());
    array.watch_as(Default::default());
    assert!(array.has_extras() && array.needs_drop());
    assert_eq!(values(&array), [Value::integer(3)]);
    assert_eq!(staged_header(&array), elements.as_ptr() as u64);
    assert_eq!(elements.owner_count(), 2);
    let copy = array.clone();
    assert!(copy.has_extras());
    assert_eq!(elements.owner_count(), 3);
    array.elements_mut().make_mut().push(Value::integer(4));
    assert_eq!(values(&array), [Value::integer(3), Value::integer(4)]);
    assert_eq!(values(&copy), [Value::integer(3)]);
    assert_eq!(staged_header(&array), array.elements().as_ptr() as u64);
    assert_eq!(elements.owner_count(), 2);
    // A new array of an array's object with extras takes the new backing in them.
    let rebuilt = ArrayObject::new(Object::clone(&copy), ArrayElements::from_slice(&[Value::integer(5)]));
    assert!(rebuilt.has_extras());
    assert_eq!(values(&rebuilt), [Value::integer(5)]);
    assert_eq!(elements.owner_count(), 2);
    drop((array, copy, rebuilt));
    assert_eq!(elements.owner_count(), 1);
}

#[test]
fn array_records_round_trip_through_a_snapshot_with_and_without_extras() {
    for extras in [false, true] {
        let mut array = ArrayObject::fresh(Value::NULL, ArrayElements::from_slice(&[Value::integer(6), Value::NULL]));
        if extras {
            array.mark_indexed_descriptors();
        }
        let mut writer = crate::snapshot::Writer::new();
        array.write_snapshot(&mut writer).unwrap();
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let restored = ArrayObject::read_snapshot(&mut reader).unwrap();
        assert_eq!(restored.has_extras(), extras);
        assert_eq!(restored.has_indexed_descriptors(), extras);
        assert_eq!(values(&restored), [Value::integer(6), Value::NULL]);
        assert_eq!(restored.elements().owner_count(), 1);
        assert_eq!(restored.properties().into_parts().0 & VALUE_VEC_START_BITS, EMPTY_POSITION);
    }
}
