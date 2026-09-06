use super::*;

fn add(dst: Register, lhs: Register, rhs: Register) -> Instruction {
    Instruction::add(dst, lhs, rhs)
}

fn local(output: Register, slot: u16) -> NumericProducer {
    NumericProducer {
        output,
        definition: NumericDefinition::Source(NumericSource::Local(slot)),
    }
}

fn constant(output: Register, value: f64) -> NumericProducer {
    NumericProducer {
        output,
        definition: NumericDefinition::Source(NumericSource::Constant(value.to_bits())),
    }
}

fn alias(output: Register, input: Register) -> NumericProducer {
    NumericProducer {
        output,
        definition: NumericDefinition::Alias(input),
    }
}

#[test]
fn add_chain_selection_derives_fixed_bindings() {
    let selected = select_add_chain(add(3, 1, 2), add(5, 3, 4), &BTreeSet::new()).unwrap();
    assert_eq!(selected.bindings.inputs, [1, 2, 4]);
    assert_eq!(selected.bindings.output, 5);
    assert!(selected.cost.profitable());
}

#[test]
fn add_chain_selection_rejects_live_intermediate_and_alias() {
    let live = BTreeSet::from([3]);
    assert!(select_add_chain(add(3, 1, 2), add(5, 3, 4), &live).is_none());
    assert!(select_add_chain(add(3, 1, 2), add(5, 3, 3), &BTreeSet::new()).is_none());
}

#[test]
fn add_chain_selection_rejects_noncanonical_operations() {
    let mut guarded = add(3, 1, 2);
    guarded.flags = 1;
    assert!(select_add_chain(guarded, add(5, 3, 4), &BTreeSet::new()).is_none());
    assert!(select_add_chain(
        Instruction::binary_operator(3, crate::ops::BinaryOp::Subtract, 1, 2,),
        add(5, 3, 4),
        &BTreeSet::new(),
    )
    .is_none());
}

#[test]
fn local_binary_selection_forwards_slots_and_removes_materialization() {
    let selected = select_local_binary(
        &[local(4, 9), local(7, 3)],
        Instruction::add(1, 7, 4),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Sources([NumericSource::Local(3), NumericSource::Local(9)])
    );
    assert_eq!(selected.output, 1);
    assert_eq!(selected.span, 3);
    assert_eq!(&selected.discarded[..3], &[Some(4), Some(7), None]);
    assert!(selected.cost.profitable());
}

#[test]
fn local_binary_selection_rejects_live_or_unrelated_loads() {
    let loads = [local(4, 9), local(7, 3)];
    assert!(select_local_binary(&loads, Instruction::add(1, 7, 4), &BTreeSet::from([4])).is_none());
    assert!(select_local_binary(&loads, Instruction::add(1, 7, 8), &BTreeSet::new()).is_none());
}

#[test]
fn local_binary_selection_numbers_repeated_slot_once() {
    let selected = select_local_binary(
        &[local(4, 9), local(7, 9)],
        Instruction::add(1, 4, 7),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Sources([NumericSource::Local(9), NumericSource::Local(9)])
    );
    assert_eq!(&selected.discarded[..3], &[Some(4), Some(7), None]);
}

#[test]
fn local_binary_selection_propagates_constant_and_preserves_order() {
    let selected = select_local_binary(
        &[constant(4, 2.5), local(7, 3)],
        Instruction::binary_operator(1, crate::ops::BinaryOp::Subtract, 4, 7),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Sources([
            NumericSource::Constant(2.5_f64.to_bits()),
            NumericSource::Local(3),
        ])
    );
}

#[test]
fn local_binary_selection_folds_constant_only_work() {
    let producers = [constant(4, 2.5), constant(7, 1.5)];
    let selected =
        select_local_binary(&producers, Instruction::add(1, 4, 7), &BTreeSet::new()).unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Folded {
            bits: 4.0_f64.to_bits()
        }
    );
    assert_eq!(selected.span, 3);
    assert_eq!(&selected.discarded[..3], &[Some(4), Some(7), None]);
    assert!(selected.cost.profitable());
    assert!(
        select_local_binary(&producers, Instruction::add(1, 4, 7), &BTreeSet::from([4]),).is_none()
    );
}

#[test]
fn constant_folding_preserves_ieee_edge_results() {
    let cases = [
        (crate::ops::BinaryOp::Add, -0.0, -0.0, (-0.0_f64).to_bits()),
        (
            crate::ops::BinaryOp::Divide,
            1.0,
            -0.0,
            f64::NEG_INFINITY.to_bits(),
        ),
    ];
    for (operator, lhs, rhs, expected) in cases {
        let operation = Instruction::binary_operator(1, operator, 4, 7);
        let selected = select_local_binary(
            &[constant(4, lhs), constant(7, rhs)],
            operation,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected.inputs,
            LocalNumericInputs::Folded { bits: expected }
        );
    }
    let nan = select_local_binary(
        &[constant(4, f64::INFINITY), constant(7, f64::INFINITY)],
        Instruction::binary_operator(1, crate::ops::BinaryOp::Subtract, 4, 7),
        &BTreeSet::new(),
    )
    .unwrap();
    let LocalNumericInputs::Folded { bits } = nan.inputs else {
        panic!("constant operation must fold")
    };
    assert!(f64::from_bits(bits).is_nan());
}

#[test]
fn producer_graph_never_treats_add_const_pool_id_as_register() {
    let selected = select_local_binary(
        &[local(4, 9), alias(7, 4)],
        Instruction::add_const(1, 7, 4),
        &BTreeSet::new(),
    );
    assert!(selected.is_none());
}

#[test]
fn local_binary_selection_forwards_alias_and_clears_dead_chain() {
    let selected = select_local_binary(
        &[local(4, 9), alias(6, 4), local(7, 3)],
        Instruction::add(1, 6, 7),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Sources([NumericSource::Local(9), NumericSource::Local(3)])
    );
    assert_eq!(selected.span, 4);
    assert_eq!(&selected.discarded[..4], &[Some(4), Some(6), Some(7), None]);
}

#[test]
fn local_binary_selection_rejects_live_alias_and_cycles() {
    let producers = [local(4, 9), alias(6, 4), local(7, 3)];
    assert!(
        select_local_binary(&producers, Instruction::add(1, 6, 7), &BTreeSet::from([6]),).is_none()
    );
    assert!(select_local_binary(
        &[alias(4, 6), alias(6, 4)],
        Instruction::add(1, 4, 6),
        &BTreeSet::new(),
    )
    .is_none());
}

#[test]
fn local_constant_selection_preserves_bits_and_operand_order() {
    let bits = (-0.0_f64).to_bits();
    let selected = select_source_add_const(
        local(4, 9),
        Instruction::add_const(1, 4, 7),
        bits,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::SlotConstant { slot: 9, bits }
    );
    assert_eq!(selected.span, 2);
    assert_eq!(&selected.discarded[..3], &[Some(4), None, None]);
}

#[test]
fn local_constant_selection_rejects_left_or_live_source() {
    let producer = local(4, 9);
    assert!(select_source_add_const(
        producer,
        Instruction::add_const_left(1, 4, 7),
        1.0_f64.to_bits(),
        &BTreeSet::new(),
    )
    .is_none());
    assert!(select_source_add_const(
        producer,
        Instruction::add_const(1, 4, 7),
        1.0_f64.to_bits(),
        &BTreeSet::from([4]),
    )
    .is_none());
}

#[test]
fn source_add_const_folds_constant_producer_and_preserves_order() {
    let selected = select_source_add_const(
        constant(4, -0.0),
        Instruction::add_const_left(1, 4, 7),
        0.0_f64.to_bits(),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Folded {
            bits: 0.0_f64.to_bits()
        }
    );
}

#[test]
fn block_value_graph_selects_bounded_dead_pure_producers() {
    let mut graph = BlockValueGraph::new();
    for (dst, slot) in [(2, 20), (3, 21), (4, 22), (5, 23), (6, 24)] {
        assert!(graph.push(Instruction::load_local(dst, slot), |_| None));
    }
    let selected = graph
        .select(Instruction::add(9, 3, 6), &BTreeSet::new())
        .unwrap();
    assert_eq!(selected.span, 6);
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::Sources([NumericSource::Local(21), NumericSource::Local(24)])
    );
    assert_eq!(selected.discarded.iter().flatten().count(), 5);
}

#[test]
fn block_value_graph_rejects_effects_mismatched_roles_and_live_values() {
    let mut graph = BlockValueGraph::new();
    assert!(!graph.push(Instruction::load_local_checked(2, 20), |_| None));
    assert!(!graph.push(Instruction::jump(4), |_| None));
    assert!(!graph.push(Instruction::load_const(2, 7), |_| None));
    assert!(graph.push(Instruction::load_local(3, 20), |_| None));
    assert!(graph.push(Instruction::move_(5, 3), |_| None));
    assert!(graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(graph.push(Instruction::load_local(4, 21), |_| None));
    assert!(graph
        .select(Instruction::add(9, 2, 4), &BTreeSet::from([2]))
        .is_none());
}

#[test]
fn block_value_graph_enforces_fixed_capacity_and_unique_definitions() {
    let mut graph = BlockValueGraph::new();
    for index in 0..MAX_BLOCK_VALUES {
        let register = u16::try_from(index + 1).unwrap();
        assert!(graph.push(Instruction::load_local(register, register), |_| None));
    }
    assert_eq!(graph.len(), MAX_BLOCK_VALUES);
    assert!(!graph.push(Instruction::load_local(20, 20), |_| None));

    let mut duplicate = BlockValueGraph::new();
    assert!(duplicate.push(Instruction::load_local(2, 20), |_| None));
    assert!(!duplicate.push(Instruction::load_local(2, 21), |_| None));
}
