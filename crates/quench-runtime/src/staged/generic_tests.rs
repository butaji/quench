//! The value steps the words decide (`generic`), on the interpreter's stage.

use super::tests::ConcreteFrame;
use super::*;
use crate::heap::Heap;
use crate::Value;

/// Run `operation` over `registers` (the operands first, the destination last).
fn run(operation: Operation<u64, ()>, values: &[Value], heap: Option<&Heap>) -> (Step<u64, Edge>, Vec<u64>) {
    let mut stage = Concrete::new(std::ptr::null());
    let mut registers: Vec<u64> = values.iter().map(|value| value.to_bits()).collect();
    registers.push(0);
    let mut frame = ConcreteFrame { registers, heap };
    let step = step(&mut stage, &mut frame, operation);
    (step, frame.registers)
}

/// The boolean `left == right` (or `===`, negated) the staged equality produces, or `None`
/// where the words leave it to the VM.
fn equality(left: Value, right: Value, strict: bool, negated: bool) -> Option<bool> {
    let operation = Operation::Equality { destination: 2, left: 0, right: 1, strict, negated };
    match run(operation, &[left, right], None) {
        (Step::Value(word), registers) => {
            assert_eq!(registers[2], word);
            Some(Value::from_bits(word) == Value::TRUE)
        }
        (Step::Unresolved, _) => None,
        (Step::Control(_), _) => panic!("equality transfers no control"),
    }
}

#[test]
fn void_defines_undefined_whatever_the_operand() {
    let (step, registers) = run(Operation::Void { destination: 1 }, &[Value::number(3.0)], None);
    assert!(matches!(step, Step::Value(word) if Value::from_bits(word) == Value::UNDEFINED));
    assert_eq!(Value::from_bits(registers[1]), Value::UNDEFINED);
}

#[test]
fn equality_is_decided_for_the_same_word_and_for_two_int32s() {
    let (three, four) = (Value::number(3.0), Value::number(4.0));
    for strict in [false, true] {
        assert_eq!(equality(three, three, strict, false), Some(true));
        assert_eq!(equality(three, four, strict, false), Some(false));
        assert_eq!(equality(three, four, strict, true), Some(true));
        assert_eq!(equality(Value::TRUE, Value::TRUE, strict, true), Some(false));
        assert_eq!(equality(Value::NAN, Value::NAN, strict, false), Some(false));
        assert_eq!(equality(Value::NAN, Value::NAN, strict, true), Some(true));
    }
}

#[test]
fn loose_equality_of_null_and_undefined_is_true_and_strict_is_left_to_the_vm() {
    assert_eq!(equality(Value::NULL, Value::UNDEFINED, false, false), Some(true));
    assert_eq!(equality(Value::UNDEFINED, Value::NULL, false, true), Some(false));
    assert_eq!(equality(Value::NULL, Value::UNDEFINED, true, false), None);
}

#[test]
fn equality_leaves_pairs_the_words_do_not_decide_to_the_vm() {
    // Zero and negative zero are distinct words and equal Numbers; a Number and a boolean
    // convert; a string compares by its cell.
    assert_eq!(equality(Value::number(0.0), Value::number(-0.0), true, false), None);
    assert_eq!(equality(Value::number(1.0), Value::TRUE, false, false), None);
    let mut heap = Heap::new();
    let (a, b) = (heap.alloc_string("x".into()), heap.alloc_string("x".into()));
    let operation = Operation::Equality { destination: 2, left: 0, right: 1, strict: true, negated: false };
    if a != b {
        assert!(matches!(run(operation, &[a, b], Some(&heap)).0, Step::Unresolved));
    }
}

#[test]
fn a_string_is_its_own_property_key_and_other_values_are_left_to_the_vm() {
    let mut heap = Heap::new();
    let text = heap.alloc_string("key".into());
    let operation = Operation::PropertyKey { destination: 1, source: 0 };
    let (step, registers) = run(operation, &[text], Some(&heap));
    if text.direct_string_index().is_some() {
        assert!(matches!(step, Step::Value(word) if word == text.to_bits()));
        assert_eq!(registers[1], text.to_bits());
    } else {
        assert!(matches!(step, Step::Unresolved));
    }
    for value in [Value::number(1.0), Value::UNDEFINED, Value::TRUE] {
        assert!(matches!(run(operation, &[value], None).0, Step::Unresolved), "{value:?}");
    }
}
