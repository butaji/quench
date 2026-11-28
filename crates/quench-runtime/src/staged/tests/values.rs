#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

#[test]
fn step_moves_and_defines_constants_on_the_concrete_stage() {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0; 4], heap: None };
    let moved = Value::number(41.0);
    frame.registers[1] = moved.to_bits();
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::Move { destination: 2, source: 1 })
        else { panic!("a move produces a value") };
    assert_eq!(Value::from_bits(word), moved);
    assert_eq!(Value::from_bits(frame.registers[2]), moved);
    let constant = Value::number(7.0);
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::LoadConst {
        destination: 3, constant: Constant::untyped(constant) }) else { panic!("a constant produces a value") };
    assert_eq!(Value::from_bits(word), constant);
    assert_eq!(Value::from_bits(frame.registers[3]), constant);
}

/// Logical negation is ToBoolean then the canonical boolean: one operation for the
/// interpreter and every tier.
#[test]
fn staged_logical_not_is_the_javascript_truthiness_negation() {
    let mut stage = Concrete::pure();
    for (value, expected) in [
        (Value::TRUE, false), (Value::FALSE, true), (Value::number(0.0), true),
        (Value::number(-0.0), true), (Value::number(f64::NAN), true),
        (Value::number(3.0), false), (Value::NULL, true), (Value::UNDEFINED, true),
    ] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
        let Step::Value(word) = step(&mut stage, &mut frame,
            Operation::LogicalNot { destination: 1, source: 0 }) else {
            panic!("logical not produces a value")
        };
        assert_eq!(Value::from_bits(word), Value::boolean(expected), "{value:?}");
        assert_eq!(Value::from_bits(frame.registers[1]), Value::boolean(expected));
    }
}

/// The `typeof` name index the row defines for `value`, or `None` where it leaves for the
/// value's cell.
fn typeof_index_of(value: Value) -> Option<u64> {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
    match step(&mut stage, &mut frame, Operation::TypeofIndex { destination: 1, source: 0 }) {
        Step::Value(word) => {
            assert_eq!(frame.registers[1], word, "{value:?}");
            Some(word)
        }
        Step::Unresolved => None,
        Step::Control(_) => panic!("typeof transfers no control"),
    }
}

/// `typeof` of every value its own word decides names what the interpreter names, and a heap
/// value (whose cell decides, the IsHTMLDDA object included) is left to the host.
#[test]
fn staged_typeof_index_names_every_immediate_and_leaves_heap_values() {
    let name = |text: &str| crate::bytecode::typeof_name_index(text).map(|index| index as u64);
    let mut heap = Heap::new();
    let object = heap.alloc_object(Object::new(Value::NULL));
    let text = heap.alloc_string("x".into());
    let cases = [
        (Value::number(0.0), "number"), (Value::number(-0.0), "number"), (Value::number(1.5), "number"),
        (Value::number(f64::NAN), "number"), (Value::number(f64::INFINITY), "number"),
        (Value::integer(0), "number"), (Value::integer(i32::MIN), "number"), (Value::integer(7), "number"),
        (Value::TRUE, "boolean"), (Value::FALSE, "boolean"), (Value::NULL, "object"),
        (Value::UNDEFINED, "undefined"), (Value::DELETED, "undefined"),
    ];
    for (value, expected) in cases {
        assert_eq!(typeof_index_of(value), name(expected), "{value:?}");
    }
    assert_eq!(typeof_index_of(object), None);
    assert_eq!(typeof_index_of(text), None);
}

/// `~x` of an int32 is `-1 - x`: no overflow, every int32 stays an int32.
#[test]
fn staged_bitwise_not_of_an_int32_is_its_complement() {
    let mut stage = Concrete::pure();
    for value in [0, 1, -1, 7, i32::MAX, i32::MIN] {
        let mut frame = ConcreteFrame { registers: vec![Value::integer(value).to_bits(), 0], heap: None };
        let operation = Operation::Number(NumberOperation::BitwiseNot { destination: 1, source: Operand::register(0) });
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Value(_)));
        assert_eq!(frame.registers[1] as u32 as i32, !value, "~{value}");
    }
}

#[test]
fn staged_require_object_coercible_rejects_only_null_and_undefined() {
    let coercible = |value: Value| {
        let mut stage = Concrete::pure();
        let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::RequireObjectCoercible { register: 0 }) {
            Step::Value(word) => { assert_eq!(word, value.to_bits()); true }
            Step::Unresolved => false,
            Step::Control(_) => panic!("a coercibility test transfers no control"),
        }
    };
    assert!(!coercible(Value::NULL));
    assert!(!coercible(Value::UNDEFINED));
    for value in [Value::TRUE, Value::FALSE, Value::number(0.0), Value::number(f64::NAN), Value::number(-0.0)] {
        assert!(coercible(value), "{value:?}");
    }
}

#[test]
fn staged_to_numeric_copies_numbers_and_leaves_for_the_rest() {
    let convert = |value: Value| {
        let mut stage = Concrete::pure();
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), Value::UNDEFINED.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::Number(NumberOperation::ToNumeric { destination: 1, source: 0 })) {
            Step::Value(_) => Some(Value::from_bits(frame.registers[1])),
            Step::Unresolved => { assert_eq!(frame.registers[1], Value::UNDEFINED.to_bits()); None }
            Step::Control(_) => panic!("a conversion transfers no control"),
        }
    };
    for value in [Value::number(1.5), Value::number(-0.0), Value::number(f64::NAN), Value::number(7.0)] {
        assert_eq!(convert(value).map(Value::to_bits), Some(value.to_bits()), "{value:?}");
    }
    for value in [Value::NULL, Value::UNDEFINED, Value::TRUE] {
        assert_eq!(convert(value), None, "{value:?}");
    }
}

#[test]
fn staged_load_this_leaves_where_the_activation_holds_no_this() {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![Value::NULL.to_bits()], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::LoadThis { destination: 0 }), Step::Unresolved));
    assert_eq!(frame.registers[0], Value::NULL.to_bits(), "a load that left writes nothing");
}


/// The fast arms of `RequireIteratorResult` and `ToPropertyKey` answer from the register's word
/// alone: an object or array record passes, a direct string is its own key, and every other
/// value leaves for the full operation, writing nothing.
#[test]
fn the_key_and_iterator_fast_arms_decide_from_the_word_alone() {
    let mut heap = Heap::new();
    let mut stage = Concrete::pure();
    let object = heap.alloc_object(Object::new(Value::NULL));
    let string = heap.alloc_string(crate::vm::wtf16::JsString::from_str("key"));
    for (value, passes) in [(object, true), (string, false), (Value::UNDEFINED, false), (Value::integer(1), false), (Value::number(0.5), false)] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: None };
        let step = step(&mut stage, &mut frame, Operation::RequireIteratorResult { register: 0 });
        assert_eq!(matches!(step, Step::Value(_)), passes, "{value:?}");
    }
    for (value, copies) in [(string, true), (object, false), (Value::integer(7), false), (Value::UNDEFINED, false)] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), Value::NULL.to_bits()], heap: None };
        let step = step(&mut stage, &mut frame, Operation::PropertyKey { destination: 1, source: 0 });
        assert_eq!(matches!(step, Step::Value(_)), copies, "{value:?}");
        let expected = if copies { value } else { Value::NULL };
        assert_eq!(Value::from_bits(frame.registers[1]), expected, "{value:?}");
    }
}
