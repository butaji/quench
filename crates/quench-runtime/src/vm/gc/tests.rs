use super::shape_reclaim_threshold_met;
use crate::value::Value;
use crate::vm::{
    DEFAULT_DATA_ATTRIBUTES, DEFAULT_PROPERTY_ATTRIBUTES, DictionaryTrigger,
    PropertyAttributes, Shape, ShapeAttributes, ShapeTransition, property_key::PropertyKey,
};

#[test]
fn shape_compaction_preserves_creation_metadata_without_property_slots() {
    use super::{append_compacted_shape, rebuild_shape, provenance};
    let mut shapes = vec![Shape::root()];
    let mut transitions = crate::vm::shape_transitions::ShapeTransitionIndex::default();
    let named = append_compacted_shape(&mut shapes, &mut transitions, 0,
        ShapeTransition::FunctionName { name: 7 }, 0);
    let reconfigured = append_compacted_shape(&mut shapes, &mut transitions, named,
        ShapeTransition::FunctionNameReconfigured, 0);
    let length_reconfigured = append_compacted_shape(&mut shapes, &mut transitions, reconfigured,
        ShapeTransition::FunctionLengthReconfigured, 0);
    let created = append_compacted_shape(&mut shapes, &mut transitions, length_reconfigured,
        ShapeTransition::Constructed { constructor: Value::heap(17), new_target_is_base: false }, 0);
    let with_field = append_compacted_shape(&mut shapes, &mut transitions, created,
        ShapeTransition::Add { key: PropertyKey::string(9), slot: 0, attributes: DEFAULT_DATA_ATTRIBUTES }, 1);
    let mut rebuilt = vec![Shape::root()];
    let shape = rebuild_shape(&shapes, &[], with_field, &mut rebuilt,
        &mut Vec::new(), &mut crate::vm::shape_transitions::ShapeTransitionIndex::default());
    assert_eq!(provenance::shape_names(&rebuilt, shape), (Some(7), Some((Value::heap(17), false)), true));
    assert!(provenance::shape_function_length_reconfigured(&rebuilt, shape));
    assert_eq!(rebuilt[shape as usize].storage_len(), 1);
    assert_eq!(super::active_shape_attributes(&rebuilt, shape).len(), 1);
}

#[test]
fn shape_compaction_waits_until_reclaim_is_material() {
    assert!(!shape_reclaim_threshold_met(1_000, 25));
    assert!(!shape_reclaim_threshold_met(1_000, 200));
    assert!(!shape_reclaim_threshold_met(1_000, 499));
    assert!(shape_reclaim_threshold_met(1_000, 500));
}

#[test]
fn shape_gc_root_fact_is_inherited_from_symbol_and_accessor_transitions() {
    let root = Shape::root();
    assert!(!root.facts.may_have_gc_roots());
    let string_key = Shape::child(
        Some(0),
        ShapeTransition::Add {
            key: PropertyKey::string(0),
            slot: 0,
            attributes: DEFAULT_DATA_ATTRIBUTES,
        },
        1,
        None,
        root.facts,
    );
    assert!(!string_key.facts.may_have_gc_roots());

    let symbol_key = PropertyKey::symbol(Value::heap(1));
    let symbol_shape = Shape::child(
        Some(0),
        ShapeTransition::Add {
            key: symbol_key,
            slot: 0,
            attributes: DEFAULT_DATA_ATTRIBUTES,
        },
        1,
        None,
        root.facts,
    );
    assert!(symbol_shape.facts.may_have_gc_roots());
    let deleted_symbol_shape = Shape::child(
        Some(1),
        ShapeTransition::Delete {
            key: symbol_key,
            slot: 0,
        },
        1,
        None,
        symbol_shape.facts,
    );
    assert!(deleted_symbol_shape.facts.may_have_gc_roots());

    let accessor_shape = |getter, setter| {
        let attributes = ShapeAttributes::record(
            PropertyAttributes {
                accessor: true,
                getter,
                setter,
                ..DEFAULT_PROPERTY_ATTRIBUTES
            },
            &mut Vec::new(),
        );
        Shape::child(
            Some(1),
            ShapeTransition::Descriptor {
                slot: 0,
                attributes,
            },
            1,
            Some(DictionaryTrigger::PropertyCount),
            string_key.facts,
        )
    };
    // A lone setter lives in the shape's accessor table, which the shape roots.
    let table_accessor = accessor_shape(None, Some(Value::heap(3)));
    assert!(table_accessor.facts.may_have_gc_roots());
    assert!(!table_accessor.facts.slot_getters());
    assert!(table_accessor.facts.accessor_pairs());
    // A lone getter lives in its object's slot, traced with the object.
    let slot_getter = accessor_shape(Some(Value::heap(2)), None);
    assert!(!slot_getter.facts.may_have_gc_roots());
    assert!(slot_getter.facts.slot_getters());
    assert!(!slot_getter.facts.accessor_pairs());
    // A getter and a setter live in the slot as one pair cell, told by its value.
    let slot_pair = accessor_shape(Some(Value::heap(2)), Some(Value::heap(3)));
    assert!(!slot_pair.facts.may_have_gc_roots());
    assert!(!slot_pair.facts.slot_getters());
    assert!(slot_pair.facts.accessor_pairs());
}
