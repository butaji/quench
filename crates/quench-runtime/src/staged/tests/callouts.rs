#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

#[test]
fn the_element_store_helper_is_the_host_callout_boundary() {
    // The interpreter stage has no helper table: an element-store helper is the host callout
    // boundary, so the frame declines it and the lane's Emit stage provides the call.
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0; 3], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::IndexSetHelper { base: 0, key: 1, value: 2, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn a_global_name_read_is_the_host_callout_boundary() {
    // The interpreter reads globals through its own loop; the lane's helper call is the
    // callout boundary, so the frame declines it.
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::LoadName { destination: 0, atom: 0, site: 0, receiver: None, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn the_field_store_helper_is_the_host_callout_boundary() {
    // The host's cached field-store hit is a callout: the interpreter stage declines it.
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0; 2], heap: None };
    let step = step(&mut stage, &mut frame,
        Operation::FieldStoreMiss { object: 0, atom: 0, site: 0, value: 1, proof: () });
    assert!(matches!(step, Step::Unresolved));
}

#[test]
fn every_host_callout_operation_has_a_concrete_reading() {
    // P5's criterion: each host/VM callout is a staged operation the interpreter stage either
    // declines (leaving it to the host) or runs (the safepoint), never an unknown opcode.
    let mut stage = Concrete::pure();
    let mut frame = ConcreteFrame { registers: vec![0; 4], heap: None };
    let declines = [
        Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () },
        Operation::Allocate { destination: 0, allocation: Allocation::Closure { function: 0 }, proof: () },
        Operation::IndexSetHelper { base: 0, key: 1, value: 2, proof: () },
        Operation::LoadName { destination: 0, atom: 0, site: 0, receiver: None, proof: () },
        Operation::FieldStoreMiss { object: 0, atom: 0, site: 0, value: 1, proof: () },
    ];
    for operation in declines {
        assert!(matches!(step(&mut stage, &mut frame, operation), Step::Unresolved));
    }
    assert!(matches!(step(&mut stage, &mut frame, Operation::Safepoint { proof: () }), Step::Value(_)));
}
