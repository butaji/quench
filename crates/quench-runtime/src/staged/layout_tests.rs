//! The object layouts and narrow encodings on the concrete stage, against the interpreter's own
//! object model.

use super::*;
use super::object::decode_narrow;
use crate::heap::{Cell, Heap, Object};
use crate::value::NarrowLayout;
use crate::value_vec::INLINE_PROPERTY_COUNT;
use crate::Value;

/// The storage forms an exact-shape object can take, by how the interpreter's object API
/// (independent of `staged`) describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Observed {
    Packed,
    Arena,
    DisplacedInline,
    DisplacedArena,
}

fn observed(object: &Object) -> Observed {
    match (object.inline_properties().is_some(), object.plain_property_storage().is_some()) {
        (true, true) => Observed::Packed,
        (false, true) => Observed::Arena,
        (true, false) => Observed::DisplacedInline,
        (false, false) => Observed::DisplacedArena,
    }
}

/// Objects of every storage form, each with its values: narrow integers, narrow references,
/// a double (no narrow form), and enough values to spill past the inline ones.
fn objects_of_every_form(heap: &mut Heap) -> Vec<(Value, Vec<Value>)> {
    let wide = INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide {
        heap.register_property_shape(shape as u32, shape, None);
    }
    let reference = heap.alloc(Cell::Error("reference".into()));
    let value_sets: Vec<Vec<Value>> = vec![
        vec![Value::number(7.0), Value::number(-3.0)],
        vec![reference, Value::TRUE],
        vec![Value::number(0.5), Value::NULL],
        (0..wide).map(|index| Value::number(index as f64)).collect(),
    ];
    let mut objects = Vec::new();
    for prototype in [false, true] {
        for values in &value_sets {
            let object = heap.alloc_object(Object::new(Value::NULL));
            for (slot, value) in values.iter().enumerate() {
                heap.property_add(object, *value, slot as u32 + 1);
            }
            if prototype {
                heap.object_record_mut(object).unwrap().mark_as_prototype();
            }
            objects.push((object, values.clone()));
        }
    }
    objects
}

#[test]
fn the_interpreter_reading_of_own_layout_matches_the_object_model_in_every_form() {
    let mut heap = Heap::new();
    let objects = objects_of_every_form(&mut heap);
    let mut forms = std::collections::HashSet::new();
    for (object, values) in objects {
        let data = heap.object(object).unwrap().record_only().unwrap();
        forms.insert(observed(data));
        for (slot, expected) in values.iter().enumerate() {
            // SAFETY: the object's exact shape holds every slot it was given.
            let reference = unsafe { heap.property_get_unchecked(data, slot) };
            assert_eq!(reference, *expected, "{:?} slot {slot}", observed(data));
            // SAFETY: as above; the heap's arena holds the object's run.
            let staged = unsafe { concrete_own_read(data, Concrete::over(&heap), slot) };
            assert_eq!(staged, *expected, "{:?} slot {slot}", observed(data));
        }
    }
    for form in [Observed::Packed, Observed::Arena, Observed::DisplacedInline, Observed::DisplacedArena] {
        assert!(forms.contains(&form), "the corpus covers {form:?}: {forms:?}");
    }
}

#[test]
fn a_narrow_decode_inverts_every_narrow_encoding() {
    let mut heap = Heap::new();
    let reference = heap.alloc(Cell::Error("reference".into()));
    let limit = 1_i32 << (NarrowLayout::INTEGER_BITS - 1);
    let values = [Value::number(0.0), Value::number(f64::from(limit - 1)), Value::number(f64::from(-limit)),
        Value::UNDEFINED, Value::NULL, Value::TRUE, Value::FALSE, Value::DELETED, reference];
    let mut stage = Concrete::pure();
    for value in values {
        let narrow = value.to_narrow().unwrap_or_else(|| panic!("{value:?} has a narrow form"));
        let decoded = decode_narrow(&mut stage, u64::from(narrow));
        assert_eq!(Value::from_bits(decoded), Value::from_narrow(narrow), "{value:?}");
        assert_eq!(Value::from_bits(decoded), value);
    }
}

/// The interpreter's reading of a prototype link (`ProtoLink::object_granule`) names the same granule
/// the staged rule reads from it, at and around every range boundary.
#[test]
fn a_prototype_link_names_the_object_record_the_staged_rule_names() {
    use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
    let edges = [0, OBJECT.proto_record_base, OBJECT.proto_record_end, OBJECT.proto_mask];
    for link in edges.iter().flat_map(|edge| [edge.wrapping_sub(1), *edge, edge + 1]).filter(|link| *link <= OBJECT.proto_mask) {
        let interpreter = crate::heap::ProtoLink::from_bits(link).object_granule();
        if let Some(granule) = interpreter {
            let staged = super::object::proto_record_id(&mut Concrete::pure(), u64::from(link));
            assert_eq!(u64::from(granule), staged, "link {link:#x}");
        }
    }
}
