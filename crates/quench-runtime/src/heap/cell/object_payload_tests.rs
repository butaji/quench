//! Payload-word tests of the object record.


use super::Object;
use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueVec, ValueArena};

#[test]
fn out_of_line_extras_preserve_inline_values_and_clone_state() {
    let inline: [Value; INLINE_PROPERTY_COUNT] =
        std::array::from_fn(|slot| Value::integer(11 + 2 * slot as i32));
    let mut object =
        Object::with_property_storage(Value::NULL, ValueVec::inline_property_storage(7), inline);
    object.set_arguments_object();
    assert!(object.is_arguments_object());
    assert_eq!(object.shape(), 7);
    assert_eq!(object.inline_properties(), Some(inline));

    let mut cloned = object.clone();
    cloned.set_inline_property(0, Value::integer(17));
    assert_eq!(cloned.inline_properties().unwrap()[0], Value::integer(17));
    assert_eq!(object.inline_properties().unwrap()[0], Value::integer(11));
    assert!(cloned.is_arguments_object());

    object.set_shape(19, &mut ValueArena::default());
    assert_eq!(object.shape(), 19);
    assert!(object.is_arguments_object());
}

#[test]
fn prototype_references_round_trip_through_every_family() {
    for proto in [
        Value::NULL,
        Value::UNDEFINED,
        Value::heap(0),
        Value::heap(super::MAX_HEAP_CELLS as u32 - 1),
        Value::direct_object(0),
        Value::direct_object(super::PROTO_RECORD_WINDOW as u32 - 1),
        Value::direct_array(0),
        Value::direct_array(super::PROTO_RECORD_WINDOW as u32 - 1),
    ] {
        let mut object = Object::new(proto);
        assert_eq!(object.proto(), proto);
        object.set_proto(Value::NULL);
        object.set_proto(proto);
        assert_eq!(object.proto(), proto);
    }
}

#[test]
fn out_of_line_extras_keep_arena_storage_mode_and_integrity() {
    let mut properties = ValueVec::new();
    properties.set_extensible(false);
    let mut object =
        Object::with_property_storage(Value::NULL, properties, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
    object.set_module_namespace();
    assert!(object.is_module_namespace());
    assert!(!object.properties().has_inline_property_storage());
    assert!(!object.is_extensible());
}
