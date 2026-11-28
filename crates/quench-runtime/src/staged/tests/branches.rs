#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The edge a `JumpIf` takes on the concrete stage for `value`.
fn jump_if_edge(value: Value, taken: bool, heap: &Heap) -> Edge {
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: Some(heap) };
    match step(&mut stage, &mut frame, Operation::JumpIf { register: 0, taken }) {
        Step::Control(edge) => edge,
        Step::Unresolved => panic!("{value:?} needs its cell, which the frame bound"),
        Step::Value(_) => panic!("a jump produces no value"),
    }
}

#[test]
fn step_jumps_on_the_javascript_truthiness_of_every_value_kind() {
    let mut heap = Heap::new();
    let empty = heap.alloc_string("".into());
    let text = heap.alloc_string("x".into());
    let cases = [
        (Value::number(1.0), Edge::Target),
        (Value::number(-3.5), Edge::Target),
        (Value::number(0.0), Edge::Following),
        (Value::number(-0.0), Edge::Following),
        (Value::number(f64::NAN), Edge::Following),
        (Value::TRUE, Edge::Target),
        (Value::FALSE, Edge::Following),
        (Value::NULL, Edge::Following),
        (Value::UNDEFINED, Edge::Following),
        (empty, Edge::Following),
        (text, Edge::Target),
    ];
    for (value, truthy_edge) in cases {
        assert_eq!(jump_if_edge(value, true, &heap), truthy_edge, "JumpTrue {value:?}");
        let falsy_edge = match truthy_edge {
            Edge::Target => Edge::Following,
            Edge::Following => Edge::Target,
            Edge::Entry(_) => unreachable!("a conditional jump takes no table entry"),
            Edge::Leave => unreachable!("a conditional jump leaves no run"),
            Edge::Transfer => unreachable!("a conditional jump transfers no control"),
        };
        assert_eq!(jump_if_edge(value, false, &heap), falsy_edge, "JumpFalse {value:?}");
    }
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::Jump), Step::Control(Edge::Target)));
}

/// The staged ToBoolean decides every value the interpreter's `Value::truthiness` decides
/// from its word alone, the same way, and leaves exactly the heap values whose cell decides.
#[test]
fn staged_truthiness_agrees_with_the_interpreter() {
    let mut heap = Heap::new();
    let object = heap.alloc_object(Object::new(Value::NULL));
    let text = heap.alloc(Cell::Error("text".into()));
    let numbers = [0.0, -0.0, 1.0, -2.5, f64::NAN, f64::INFINITY, 1e300].map(Value::number);
    let values = [Value::TRUE, Value::FALSE, Value::NULL, Value::UNDEFINED, Value::DELETED,
        Value::integer(0), Value::integer(7), Value::integer(-1), object, text];
    for value in numbers.into_iter().chain(values) {
        let (undecided, truthy) = crate::staged::value::tagged_truthiness(&mut Concrete::pure(), value.to_bits());
        let interpreter = value.truthiness().filter(|_| value.direct_object_index().is_some() || !value.is_heap()
            || value.direct_array_index().is_some());
        assert_eq!((!undecided).then_some(truthy), interpreter, "{value:?}");
    }
}

