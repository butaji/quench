use super::*;
use crate::typed_ir::Type;
use crate::vm::switches::with_float_slots;

/// The word `slot` of `object` holds in its arena run, as stored.
fn word(heap: &Heap, object: Value, slot: usize) -> Value {
    let (start, _, _) = float_slots::arena_run_of(&heap.properties, heap.object(object).unwrap()).unwrap();
    // SAFETY: the run holds the object's slot.
    unsafe { *heap.properties.values_base().add(start + slot) }
}

#[test]
fn a_slot_holds_a_raw_double_while_its_owner_is_number_and_a_value_otherwise() {
    with_float_slots(|| {
        let mut heap = Heap::new();
        heap.track_slot_representations(&[(0, None), (3, None)]);
        let objects: Vec<Value> = (0..3)
            .map(|index| heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(index), Value::integer(7), Value::integer(8)]))
            .collect();
        assert!(heap.properties.float_slots());
        // An int32 owner holds the tagged words.
        assert_eq!(word(&heap, objects[0], 1), Value::integer(7));
        // One double makes its owner Number, and the words of every object of it doubles.
        heap.property_set(objects[1], 0, Value::number(0.5));
        assert_eq!(heap.slot_type(1, 0), Type::NUMBER);
        for object in &objects {
            assert_eq!(word(&heap, *object, 1).to_bits(), 7.0_f64.to_bits());
            assert_eq!(heap.property_get(heap.object(*object).unwrap(), 1), Some(Value::integer(7)));
        }
        assert_eq!(word(&heap, objects[1], 0), Value::number(0.5));
        // A new object of the shape, and a store of an int32, join them.
        let fresh = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1), Value::number(-0.0), Value::integer(3)]);
        heap.property_set(objects[0], 2, Value::integer(-4));
        assert_eq!(word(&heap, fresh, 0).to_bits(), 1.0_f64.to_bits());
        assert_eq!(word(&heap, fresh, 1).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(word(&heap, objects[0], 2).to_bits(), (-4.0_f64).to_bits());
        assert_eq!(heap.property_get(heap.object(objects[0]).unwrap(), 2), Some(Value::integer(-4)));
        assert_eq!(heap.property_get(heap.object(fresh).unwrap(), 1), Some(Value::number(-0.0)));
        assert_eq!(heap.unrepresented_slots(), 0);
        // A value that is no Number leaves the type, and the words are values again.
        heap.property_set(objects[2], 2, Value::NULL);
        assert_eq!(heap.slot_type(1, 0), Type::ANY);
        assert_eq!(word(&heap, objects[0], 1), Value::integer(7));
        assert_eq!(word(&heap, objects[1], 0), Value::number(0.5));
        assert_eq!(word(&heap, fresh, 0), Value::integer(1));
        assert_eq!(heap.unrepresented_slots(), 0);
    });
}

#[test]
fn raw_doubles_are_not_admitted_to_a_native_writer_and_stay_values_through_a_shape_table_remap() {
    with_float_slots(|| {
        let mut heap = Heap::new();
        heap.track_slot_representations(&[(0, None), (3, None)]);
        let object = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::number(0.5), Value::integer(1), Value::integer(2)]);
        assert!(heap.slot_holds_raw_doubles(1, 0));
        assert!(!heap.admit_slot_writes(1, 0, Type::NUMBER), "a raw writer stores tagged words");
        heap.remap_live_object_shapes(&[0, 1], &[(0, None), (3, None)]);
        assert!(heap.slot_holds_raw_doubles(1, 0));
        assert_eq!(word(&heap, object, 1).to_bits(), 1.0_f64.to_bits());
        assert_eq!(heap.property_get(heap.object(object).unwrap(), 1), Some(Value::integer(1)));
        assert_eq!(heap.unrepresented_slots(), 0);
    });
}
