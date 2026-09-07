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
    assert_eq!(selected.result.register, 1);
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
    assert!(graph
        .select(Instruction::ret(2), &BTreeSet::new())
        .is_none());
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
fn block_value_graph_enforces_fixed_capacity_and_versions_definitions() {
    let mut graph = BlockValueGraph::new();
    for index in 0..MAX_BLOCK_VALUES {
        let register = u16::try_from(index + 1).unwrap();
        assert!(graph.push(Instruction::load_local(register, register), |_| None));
    }
    assert_eq!(graph.len(), MAX_BLOCK_VALUES);
    assert!(!graph.push(Instruction::load_local(20, 20), |_| None));

    let mut duplicate = BlockValueGraph::new();
    assert!(duplicate.push(Instruction::load_local(2, 20), |_| None));
    assert!(duplicate.push(Instruction::load_local(2, 21), |_| None));
    assert_eq!(duplicate.current_value(2).unwrap().version, 1);
}

#[test]
fn block_values_version_redefinitions_without_retargeting_aliases() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(graph.push(Instruction::move_(3, 2), |_| None));
    assert!(graph.push(Instruction::load_local(2, 21), |_| None));
    let old = ValueId {
        register: 2,
        version: 0,
    };
    let alias = graph
        .value(ValueId {
            register: 3,
            version: 0,
        })
        .unwrap();
    assert_eq!(alias.definition, ValueDefinition::Alias(old));
    assert_eq!(
        graph.current_value(2).unwrap(),
        ValueId {
            register: 2,
            version: 1
        }
    );
}

#[test]
fn local_value_numbering_is_deterministic_for_sources_and_binary_nodes() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_const(2, 0), |_| Some(3.0_f64.to_bits())));
    assert!(graph.push(Instruction::load_const(3, 1), |_| Some(3.0_f64.to_bits())));
    assert!(graph.push(Instruction::add(4, 2, 3), |_| None));
    assert!(graph.push(Instruction::add(5, 2, 3), |_| None));
    assert_eq!(
        graph
            .value(ValueId {
                register: 3,
                version: 0
            })
            .unwrap()
            .definition,
        ValueDefinition::Alias(ValueId {
            register: 2,
            version: 0
        })
    );
    assert_eq!(
        graph
            .value(ValueId {
                register: 5,
                version: 0
            })
            .unwrap()
            .definition,
        ValueDefinition::Alias(ValueId {
            register: 4,
            version: 0
        })
    );
}

#[test]
fn proven_numeric_nodes_fold_exact_tree_and_omit_dead_nodes() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_const(2, 0), |_| Some(
        (-0.0_f64).to_bits()
    )));
    assert!(graph.push(Instruction::load_const(9, 1), |_| Some(99.0_f64.to_bits())));
    assert!(graph.push(Instruction::load_const(3, 2), |_| Some(
        (-0.0_f64).to_bits()
    )));
    assert!(graph.push(Instruction::add(4, 2, 3), |_| None));
    assert_eq!(
        graph.marked_len(&[4]),
        2,
        "dead node is omitted and equal constants share one numbered value"
    );
    let selected = graph
        .select(Instruction::add(6, 4, 2), &BTreeSet::new())
        .unwrap();
    let LocalNumericInputs::Folded { bits } = selected.inputs else {
        panic!("constant tree must fold")
    };
    assert_eq!(bits, (-0.0_f64 + -0.0 + -0.0).to_bits());
    assert_eq!(selected.span, 5, "residual span remains authoritative");
}

#[test]
fn value_graph_selects_ordered_nonconstant_add_tree() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(0, 6), |_| None));
    assert!(graph.push(Instruction::load_local(1, 7), |_| None));
    assert!(graph.push(Instruction::add(2, 0, 1), |_| None));
    assert!(graph.push(Instruction::load_local(3, 8), |_| None));
    let selected = graph
        .select(Instruction::add(4, 2, 3), &BTreeSet::new())
        .expect("ordered add tree");
    assert_eq!(selected.span, 5);
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::AddChain {
            sources: [
                NumericSource::Local(6),
                NumericSource::Local(7),
                NumericSource::Local(8),
            ],
            bindings: F64x3Bindings {
                inputs: [0, 1, 3],
                output: 4,
            },
        }
    );
}

#[test]
fn value_graph_selects_bounded_ordered_repeated_add() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(0, 6), |_| None));
    assert!(graph.push(Instruction::load_local(1, 7), |_| None));
    assert!(graph.push(Instruction::add(2, 0, 1), |_| None));
    assert!(graph.push(Instruction::add(3, 2, 1), |_| None));
    let selected = graph
        .select(Instruction::add(4, 3, 1), &BTreeSet::new())
        .expect("ordered repeated add");
    assert_eq!(selected.span, 5);
    assert_eq!(
        selected.inputs,
        LocalNumericInputs::RepeatedAdd {
            sources: [NumericSource::Local(6), NumericSource::Local(7)],
            repetitions: 3,
        }
    );
}

#[test]
fn value_graph_rejects_live_or_reassociated_add_tree() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(0, 6), |_| None));
    assert!(graph.push(Instruction::load_local(1, 7), |_| None));
    assert!(graph.push(Instruction::add(2, 0, 1), |_| None));
    assert!(graph.push(Instruction::load_local(3, 8), |_| None));
    assert!(graph
        .select(Instruction::add(4, 2, 3), &BTreeSet::from([2]))
        .is_none());
    let subtract = Instruction {
        opcode: Opcode::Sub,
        flags: 0,
        a: 4,
        b: 2,
        c: 3,
    };
    assert!(graph.select(subtract, &BTreeSet::new()).is_none());
}

#[test]
fn value_slice_rejects_coercion_effects_control_and_computed_live_outs() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_const(2, 0), |_| Some(1.0_f64.to_bits())));
    assert!(graph.push(Instruction::load_const(3, 1), |_| Some(2.0_f64.to_bits())));
    let coercive = Instruction::binary_operator(4, crate::ops::BinaryOp::Add, 2, 3);
    assert!(!graph.push(coercive, |_| None));
    assert!(!graph.push(Instruction::jump(0), |_| None));
    assert!(!graph.push(Instruction::load_local_checked(4, 0), |_| None));
    assert!(graph.push(Instruction::add(4, 2, 3), |_| None));
    assert!(graph
        .select(Instruction::add(5, 4, 2), &BTreeSet::from([4]))
        .is_none());
}

#[test]
fn block_value_graph_selects_local_property_through_aliases() {
    let mut graph = BlockValueGraph::new();
    assert!(graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(graph.push(Instruction::move_(3, 2), |_| None));
    assert!(graph.push(Instruction::load_local(4, 21), |_| None));
    let get = Instruction {
        opcode: Opcode::GetN,
        flags: 0,
        a: 5,
        b: 3,
        c: 0,
    };
    let selected = graph.select_property(get, &BTreeSet::new()).unwrap();
    assert_eq!(selected.receiver_slot, 20);
    assert_eq!(selected.result.register, 5);
    assert_eq!(selected.span, 4);
    assert_eq!(selected.discarded.iter().flatten().count(), 3);
}

#[test]
fn block_value_graph_rejects_unsafe_property_inputs() {
    let get = Instruction {
        opcode: Opcode::GetN,
        flags: 0,
        a: 5,
        b: 2,
        c: 0,
    };
    let mut local_graph = BlockValueGraph::new();
    assert!(local_graph.push(Instruction::load_local(2, 20), |_| None));
    assert!(local_graph
        .select_property(get, &BTreeSet::from([2]))
        .is_none());
    assert!(local_graph
        .select_property(Instruction { flags: 1, ..get }, &BTreeSet::new())
        .is_none());

    let mut constant_graph = BlockValueGraph::new();
    assert!(constant_graph.push(Instruction::load_const(2, 7), |_| Some(1.0_f64.to_bits())));
    assert!(constant_graph
        .select_property(get, &BTreeSet::new())
        .is_none());
}
