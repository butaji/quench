#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

#[test]
fn an_allocation_is_a_staged_operation_with_a_host_boundary() {
    // The interpreter stage has no allocator of its own: an allocation is the host callout
    // boundary, so the frame declines it and the lane's Emit stage provides the semantics.
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0; 3], heap: None };
    for operation in [
        Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () },
    ] {
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Unresolved));
    }
}

/// The window, closure, template and concatenation rows are one staged operation over the
/// frame's allocator: a frame with none declines each, writing nothing, and one with a heap
/// defines the destination with the word it made.
#[test]
fn an_allocation_row_defines_its_destination_through_the_frame_or_declines() {
    use crate::bytecode::{RegisterList, RegisterWindow};
    let window = RegisterWindow { base: 1, count: 2 };
    let rows = [
        (Allocation::ObjectLiteral { site: 4, window }, 2),
        (Allocation::Concat { window }, 2),
        (Allocation::Closure { function: 1 }, 0),
        (Allocation::Template { template: 0 }, 0),
        (Allocation::Empty, 0),
        (Allocation::Array { length: 3, numbers: false }, 0),
        (Allocation::RegExp { site: 0, callee: None }, 0),
        (Allocation::RegExp { site: 0, callee: Some(1) }, 1),
        (Allocation::ConstArray { constant: 0, count: 2 }, 0),
        (Allocation::ObjectPair { site: 1, first: 1, second: 2 }, 2),
        (Allocation::ArrayOf { elements: RegisterList::collect(2, |index| 2 - index as u16).expect("two fit") }, 2),
    ];
    let heap = Heap::new();
    let mut stage = Concrete::pure();
    for (allocation, made) in rows {
        let operation = Operation::Allocate { destination: 0, allocation, proof: () };
        let mut bare = ConcreteFrame { registers: vec![0; 3], heap: None };
        assert!(matches!(step(&mut stage, &mut bare, operation), Step::Unresolved), "{allocation:?}");
        assert_eq!(bare.registers[0], 0, "a declined row writes nothing");
        let mut held = ConcreteFrame { registers: vec![0; 3], heap: Some(&heap) };
        let Step::Value(word) = step(&mut stage, &mut held, operation) else { panic!("{allocation:?} allocates") };
        assert_eq!(Value::from_bits(word), Value::integer(made), "{allocation:?}");
        assert_eq!(held.registers[0], word, "the row defines its destination");
    }
}

/// Only a closure writes the frame (its promotion), only concatenation may run guest code,
/// and every allocation row may allocate: the rows a specializer keeps facts across.
#[test]
fn allocation_rows_declare_their_effects() {
    use crate::bytecode::RegisterWindow;
    let window = RegisterWindow { base: 0, count: 1 };
    let row = |allocation| Operation::<u64, ()>::Allocate { destination: 0, allocation, proof: () }.effects();
    let (literal, closure) = (row(Allocation::ObjectLiteral { site: 0, window }), row(Allocation::Closure { function: 0 }));
    assert!(literal.may_alloc && !literal.may_call_js && !literal.writes.contains(Region::Frame));
    let elements = row(Allocation::ArrayOf { elements: crate::bytecode::RegisterList::collect(1, |_| 0).expect("one fits") });
    assert!(elements.may_alloc && !elements.may_call_js && !elements.writes.contains(Region::Frame), "its elements are stored fresh, with no barrier");
    assert!(closure.may_alloc && closure.writes.contains(Region::Frame));
    assert!(row(Allocation::Concat { window }).may_call_js, "ToString of an object runs guest code");
}
