use super::*;
use crate::Value;
use crate::staged::{Arguments, CallFacts, Callee, Receiver};
use crate::staged::call::concrete_fits;
use crate::staged::test_words::FACTS;
use crate::staged::Concrete;

const RETURNED: u32 = 7;

fn run(operation: CallOperation<u64>) -> Option<u64> {
    match step::<Concrete, ()>(&mut Concrete::pure(), (), operation) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

#[test]
fn a_site_runs_no_row_alone_and_a_callee_must_be_the_bound_one() {
    let site = Invocation { callee: Callee::Register(1), receiver: Receiver::Register(2), arguments: Arguments::Window { first: 3, count: 1 },
        result: 0, returns: false, forwards: false };
    assert_eq!(run(CallOperation::Site(site)), None);
    assert_eq!(run(CallOperation::Callee { held: 9, expected: 9 }), Some(9));
    assert_eq!(run(CallOperation::Callee { held: 8, expected: 9 }), None);
    assert_eq!(run(CallOperation::Receiver { receiver: 3 }), Some(3));
    assert_eq!(run(CallOperation::Receiver { receiver: Value::DELETED.to_bits() }), None, "a declined plan leaves");
}

#[test]
fn admission_is_one_unsigned_compare_where_equal_fits() {
    assert!(concrete_fits(4, 4) && concrete_fits(5, 4) && !concrete_fits(3, 4));
    assert_eq!(run(CallOperation::Room { limit: 10, needed: 10 }), Some(10));
    assert_eq!(run(CallOperation::Room { limit: 9, needed: 10 }), None);
    assert!(!concrete_fits(0, u64::MAX), "an unsigned compare, not a signed one");
}

#[test]
fn a_strict_callee_takes_any_receiver_and_a_sloppy_one_binds_by_the_value() {
    let mut heap = crate::heap::Heap::new();
    let object = heap.alloc_object(crate::heap::Object::new(Value::NULL));
    let text = heap.alloc_string("x".into());
    let global = Value::integer(99).to_bits();
    let bind = |receiver: Value, sloppy: Option<u64>| run(CallOperation::BindThis { receiver: receiver.to_bits(), sloppy });
    assert_eq!(bind(Value::integer(5), None), Some(Value::integer(5).to_bits()));
    assert_eq!(bind(object, Some(global)), Some(object.to_bits()));
    assert_eq!(bind(Value::UNDEFINED, Some(global)), Some(global));
    assert_eq!(bind(Value::NULL, Some(global)), Some(global));
    assert_eq!(bind(Value::integer(5), Some(global)), None, "a primitive is boxed by the host");
    assert_eq!(bind(text, Some(global)), None, "a string is boxed by the host");
}

/// The row's family test is the value's own (`Value::is_direct_object_family`).
#[test]
fn the_object_family_is_the_values_own() {
    let mut heap = crate::heap::Heap::new();
    let object = heap.alloc_object(crate::heap::Object::new(Value::NULL));
    let text = heap.alloc_string("x".into());
    let mut stage = Concrete::pure();
    for value in [object, text, Value::UNDEFINED, Value::NULL, Value::integer(1), Value::number(1.5), Value::TRUE] {
        let ordinary = is_direct_object(&mut stage, value.to_bits());
        assert_eq!(is_object_family(&mut stage, value.to_bits(), ordinary), value.is_direct_object_family(), "{value:?}");
    }
}

#[test]
fn a_window_takes_the_arguments_then_undefined() {
    let source = [10_u64, 20, 30];
    let mut window = [0_u64; 4];
    let fill = WindowFill { first: 1, argc: 2, registers: 4 };
    let result = run(CallOperation::Window { top: window.as_mut_ptr() as u64, source: source.as_ptr() as u64, fill });
    assert_eq!(result, Some(window.as_ptr() as u64));
    assert_eq!(window, [20, 30, Value::UNDEFINED.to_bits(), Value::UNDEFINED.to_bits()]);
}

#[test]
fn a_completion_yields_the_value_or_the_receiver_only_once_returned() {
    let complete = |status, receiver| run(CallOperation::Complete { status, value: 1, receiver, returned: RETURNED });
    assert_eq!(complete(u64::from(RETURNED), None), Some(1));
    assert_eq!(complete(u64::from(RETURNED), Some(2)), Some(2), "a construction yields its receiver");
    assert_eq!(complete(u64::from(RETURNED) + 1, None), None);
    assert_eq!(complete(u64::from(RETURNED) + 1, Some(2)), None);
}

/// A row's words are the tier's: the protocol has no per-site hole but the callee identity,
/// a per-site operand the stage keeps as a constant.
#[test]
fn the_rows_have_no_per_site_hole() {
    let mut trace = crate::staged::trace::Trace::default();
    let words = [trace.constant(0), trace.constant(1), trace.constant(2)];
    guard_callee(&mut trace, words[0], 5, ());
    bind_this(&mut trace, words[0], Some(6), ());
    fill_window(&mut trace, words[0], words[1], WindowFill { first: 0, argc: 1, registers: 2 });
    complete_call(&mut trace, words[0], words[1], Some(words[2]), RETURNED, ());
    assert!(trace.holes.is_empty());
}

#[test]
fn a_return_is_admitted_by_its_register_and_the_tier_completing() {
    let row = crate::staged::Operation::<u64, ()>::Return { source: 9 };
    assert_eq!(row.admitted(FACTS), Some(true));
    assert_eq!(crate::staged::Operation::<u64, ()>::Return { source: 10 }.admitted(FACTS), Some(false));
    assert_eq!(row.admitted(CallFacts { activates: false, ..FACTS }), Some(false));
    assert_eq!(crate::staged::Operation::<u64, ()>::Jump.admitted(FACTS), None);
}
