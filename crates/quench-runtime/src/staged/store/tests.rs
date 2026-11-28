use super::*;

/// The store's holes are its shape, packed half offset and flat offset, in that order,
/// whatever their values: a stencil patches them and runs one code.
#[test]
fn the_store_leaves_its_operands_as_holes_not_trace() {
    let run = |shape: u32, slot: u16| {
        let mut stage = crate::staged::trace::Trace::default();
        let (record, id, value) = (stage.constant(0), stage.constant(1), stage.constant(2));
        own_store(&mut stage, record, id, shape, false, slot, value, ());
        (stage.ops, stage.holes)
    };
    let (first, holes) = run(3, 1);
    let (second, other) = run(40, 2);
    assert_eq!(first, second);
    assert_eq!(holes, [3, 4, 8]);
    assert_eq!(other, [40, 8, 16]);
}

/// The operations of the shared own store of a value the frame proved of type `fact`.
fn trace_of(fact: Type) -> Vec<String> {
    let mut stage = crate::staged::trace::Trace::default();
    let (record, id, value) = (stage.constant(0), stage.constant(1), stage.constant(2));
    own_store_proved(&mut stage, record, id, 1, (value, fact), ());
    stage.ops
}

fn count(ops: &[String], operation: &str) -> usize { ops.iter().filter(|line| line.starts_with(operation)).count() }

/// The binding time of the value's type decides the store's shape: an immediate type
/// folds the barrier away, an int32 takes the integer narrow arm alone, and nothing known
/// keeps the gate and the generic encoder.
#[test]
fn a_static_fact_folds_the_barrier_and_selects_the_integer_narrow_arm() {
    let (generic, int32) = (trace_of(Type::ANY), trace_of(Type::INT32));
    assert_eq!(count(&generic, "store_gate"), 1, "an unknown value asks the gate");
    for immediate in [Type::NUMBER, Type::INT32, Type::language(ValueType::Boolean), Type::NULLISH] {
        assert_eq!(count(&trace_of(immediate), "store_gate"), 0, "{immediate:?} names no heap record");
    }
    assert_eq!(count(&trace_of(Type::language(ValueType::Object)), "store_gate"), 1, "an object needs the barrier");
    assert_eq!(count(&generic, "choose"), count(&int32, "choose") + 1, "an int32 needs no tag test");
    assert!(int32.len() < generic.len());
}

#[test]
fn a_slot_holds_test_names_the_classes_its_type_has() {
    let ops = |holds: Type| {
        let mut stage = crate::staged::trace::Trace::default();
        let bits = stage.constant(0);
        slot_holds(&mut stage, bits, holds);
        stage.ops
    };
    let never = ops(Type::language(ValueType::String));
    assert_eq!(count(&never, "any"), 0, "no class of the type is testable");
    assert_eq!(count(&ops(Type::INT32), "any"), 1);
    let nullable = Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH);
    assert_eq!(count(&ops(nullable), "any"), 3, "an object, null and undefined");
}
