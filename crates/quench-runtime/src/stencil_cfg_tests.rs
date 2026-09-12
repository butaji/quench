use super::*;

fn entries(instructions: &[crate::ir::Instruction]) -> Vec<BaselineEntry> {
    instructions
        .iter()
        .copied()
        .map(|instruction| BaselineEntry {
            instruction,
            control: instruction.opcode.control_operands(instruction),
        })
        .collect()
}

#[test]
fn branch_liveness_unions_both_successors() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 2),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(2),
    ]);
    let successors = successor_table(&entries);
    let live = register_liveness(&entries, &[None, None, None], &successors);
    assert_eq!(live[0], BTreeSet::from([1, 2]));
}

#[test]
fn live_inputs_distinguish_region_exit_from_internal_definition() {
    let entries = entries(&[
        crate::ir::Instruction::move_(2, 1),
        crate::ir::Instruction::ret(2),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None, None]);
    assert_eq!(facts.live_in_at(0), Some(&BTreeSet::from([1])));
    assert_eq!(facts.live_in_at(1), Some(&BTreeSet::from([2])));
}

#[test]
fn liveness_budget_exhaustion_is_conservative() {
    let entries = entries(&[
        crate::ir::Instruction::move_(2, 1),
        crate::ir::Instruction::ret(2),
    ]);
    let successors = successor_table(&entries);
    let live = bounded_register_liveness(&entries, &[None, None], &successors, 0);
    assert_eq!(live, vec![BTreeSet::from([1, 2]); 2]);
}

#[test]
fn region_entry_check_accepts_internal_and_rejects_external_edges() {
    let internal = entries(&[
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::ret(0),
    ]);
    let internal_facts = ControlFlowFacts::new(&internal, &[None, None, None]);
    assert!(internal_facts.region_entry_is_legal(0, 2));
    let external = entries(&[
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::jump(1),
    ]);
    let external_facts = ControlFlowFacts::new(&external, &[None, None, None]);
    assert!(!external_facts.region_entry_is_legal(0, 2));
}

#[test]
fn region_shape_uses_canonical_operands_and_cfg_edges() {
    let valid = entries(&[
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::jump_if_false(0, 2),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&valid, &[None; 3]);
    assert!(facts.region_matches(
        &valid,
        0,
        &[crate::ir::Opcode::Move, crate::ir::Opcode::JumpIfFalse]
    ));

    let mut noncanonical = valid.clone();
    noncanonical[0].instruction.c = 1;
    let facts = ControlFlowFacts::new(&noncanonical, &[None; 3]);
    assert!(!facts.region_matches(
        &noncanonical,
        0,
        &[crate::ir::Opcode::Move, crate::ir::Opcode::JumpIfFalse]
    ));
}

#[test]
fn region_shape_rejects_operation_drift_and_external_entry() {
    let entries = entries(&[
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::jump(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    assert!(!facts.region_matches(
        &entries,
        0,
        &[crate::ir::Opcode::Move, crate::ir::Opcode::Jump]
    ));
    assert!(!facts.region_matches(&entries, 0, &[crate::ir::Opcode::Add]));
}

#[test]
fn structured_loop_is_only_admitted_as_a_single_gateway_operation() {
    let instruction = crate::ir::Instruction {
        opcode: crate::ir::Opcode::ForI,
        flags: 0,
        a: 0,
        b: 0,
        c: 0,
    };
    let entries = entries(&[instruction]);
    let facts = ControlFlowFacts::new(&entries, &[None]);
    assert!(facts.region_matches(&entries, 0, &[crate::ir::Opcode::ForI]));
    let mut wider = entries.clone();
    let ret = crate::ir::Instruction::ret(0);
    wider.push(BaselineEntry {
        instruction: ret,
        control: ret.opcode.control_operands(ret),
    });
    let wider_facts = ControlFlowFacts::new(&wider, &[None, None]);
    assert!(!wider_facts.region_matches(
        &wider,
        0,
        &[crate::ir::Opcode::ForI, crate::ir::Opcode::Return]
    ));
}

#[test]
fn branch_successors_do_not_duplicate_fallthrough() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 1),
        crate::ir::Instruction::ret(0),
    ]);
    assert_eq!(successors(&entries, 0).iter().collect::<Vec<_>>(), [1]);
}

#[test]
fn malformed_edge_rejects_its_region() {
    let entries = entries(&[
        crate::ir::Instruction::jump(99),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None, None]);
    assert!(!facts.region_entry_is_legal(0, 1));
    assert_eq!(facts.backedge_target_at(0), None);
}

#[test]
fn branch_to_code_end_is_a_valid_external_exit() {
    let branch_entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 2),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&branch_entries, &[None, None]);
    assert!(facts.region_entry_is_legal(0, 2));
    let plan = facts
        .region_plan_with_terminal_exits(
            &branch_entries,
            0,
            &[crate::ir::Opcode::JumpIfFalse, crate::ir::Opcode::Return],
        )
        .expect("end sentinel is a valid external edge");
    assert!(plan.edges().contains(&RegionEdge { from: 0, to: 2 }));

    let entries = entries(&[
        crate::ir::Instruction::jump(2),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None, None]);
    assert!(facts.region_entry_is_legal(0, 2));
    assert!(facts
        .region_plan_with_terminal_exits(
            &entries,
            0,
            &[crate::ir::Opcode::Jump, crate::ir::Opcode::Return],
        )
        .expect("jump end sentinel is a valid external edge")
        .edges()
        .contains(&RegionEdge { from: 0, to: 2 }));
}

#[test]
fn region_plan_records_branch_blocks_and_edges() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 2),
        crate::ir::Instruction::jump(3),
        crate::ir::Instruction::move_(1, 2),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(0, 4).expect("derived branch plan");
    assert_eq!(plan.blocks(), [0, 2, 1, 3]);
    assert_eq!(
        plan.block_ranges().expect("derived block ranges"),
        [
            RegionBlock { start: 0, end: 1 },
            RegionBlock { start: 1, end: 2 },
            RegionBlock { start: 2, end: 3 },
            RegionBlock { start: 3, end: 4 },
        ]
    );
    assert_eq!(
        plan.edges(),
        [
            RegionEdge { from: 0, to: 2 },
            RegionEdge { from: 0, to: 1 },
            RegionEdge { from: 1, to: 3 },
        ]
    );
    assert!(!plan.has_backedge());
}

#[test]
fn region_plan_derives_distinct_predecessor_joins() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::jump(3),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(0, 4).expect("joined branch plan");
    assert_eq!(plan.join_blocks(), [3]);
    assert_eq!(facts.predecessors_at(3), Some([0, 1, 2].as_slice()));
}

#[test]
fn region_plan_records_native_backedge_with_a_finite_graph() {
    let entries = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 3),
        crate::ir::Instruction::jump(0),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(0, 3).expect("derived loop plan");
    assert_eq!(plan.start(), 0);
    assert_eq!(plan.end(), 3);
    assert!(plan.has_backedge());
    assert_eq!(plan.backedges(), [RegionEdge { from: 2, to: 0 }]);
    assert_eq!(plan.internal_backedges(), [RegionEdge { from: 2, to: 0 }]);
    assert!(plan.has_internal_backedge(2, 0));
    assert!(!plan.has_internal_backedge(2, 3));
    assert!(facts.has_backedge_at(2));
    assert_eq!(facts.backedge_target_at(2), Some(0));
    assert_eq!(facts.backedge_target_at(1), None);
    assert!(!facts.has_backedge_at(1));
    assert!(plan.matches_operations(&[
        crate::ir::Opcode::Move,
        crate::ir::Opcode::JumpIfFalse,
        crate::ir::Opcode::Jump,
    ]));
    assert!(!plan.matches_operations(&[
        crate::ir::Opcode::Move,
        crate::ir::Opcode::Add,
        crate::ir::Opcode::Jump,
    ]));
}

#[test]
fn region_transfer_lookup_accepts_unsorted_edges_without_scanning_the_graph() {
    let plan = RegionControlPlan::from_relative_edges(5, &[(3, 1), (0, 2), (2, 4)])
        .expect("relative edge plan");
    let operations = [
        crate::ir::Opcode::JumpIfFalse,
        crate::ir::Opcode::Jump,
        crate::ir::Opcode::Jump,
        crate::ir::Opcode::Jump,
    ];
    assert!(plan.permits_transfer(&operations, 0, 2));
    assert!(plan.permits_transfer(&operations, 2, 4));
    assert!(plan.permits_transfer(&operations, 3, 1));
    assert!(!plan.permits_transfer(&operations, 3, 2));
    assert!(plan.has_internal_backedge(3, 1));
    assert!(!plan.has_internal_backedge(3, 4));
}

#[test]
fn region_plan_derives_loop_carried_registers_from_liveness() {
    let entries = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 3),
        crate::ir::Instruction::move_(1, 1),
        crate::ir::Instruction::jump(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(1, 4).expect("loop region");
    assert_eq!(plan.backedges(), [RegionEdge { from: 3, to: 1 }]);
    assert_eq!(facts.loop_carried_registers(&entries, &plan), [1]);
}

#[test]
fn region_plan_derives_conservative_integer_induction_candidates() {
    let loop_entries = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 4),
        crate::ir::Instruction::inc_i(1, 1, false),
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&loop_entries, &[None; 5]);
    let plan = facts.region_control(1, 4).expect("loop region");
    assert_eq!(
        facts.induction_candidates(&loop_entries, &plan),
        [InductionCandidate {
            register: 1,
            source: 1,
            update_pc: 2,
            decrement: false,
        }]
    );

    let decrement = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 4),
        crate::ir::Instruction::inc_i(1, 1, true),
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::ret(1),
    ]);
    let decrement_facts = ControlFlowFacts::new(&decrement, &[None; 5]);
    let decrement_plan = decrement_facts
        .region_control(1, 4)
        .expect("decrement loop region");
    assert_eq!(
        decrement_facts.induction_candidates(&decrement, &decrement_plan),
        [InductionCandidate {
            register: 1,
            source: 1,
            update_pc: 2,
            decrement: true,
        }]
    );
}

#[test]
fn induction_candidates_reject_opaque_updates() {
    let entries = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 4),
        crate::ir::Instruction::move_(1, 1),
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    let plan = facts.region_control(1, 4).expect("loop region");
    assert!(facts.induction_candidates(&entries, &plan).is_empty());
}

#[test]
fn loop_state_ignores_backward_edges_that_exit_the_region() {
    let entries = entries(&[
        crate::ir::Instruction::ret(2),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::move_(2, 0),
        crate::ir::Instruction::inc_i(2, 2, false),
        crate::ir::Instruction::jump(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    let plan = facts
        .region_control(2, 5)
        .expect("region with external exit");
    assert_eq!(plan.backedges(), [RegionEdge { from: 4, to: 0 }]);
    assert!(plan.internal_backedges().is_empty());
    assert!(facts.loop_carried_registers(&entries, &plan).is_empty());
    assert!(facts.induction_candidates(&entries, &plan).is_empty());
}

#[test]
fn region_plan_derives_live_outs_from_all_external_exits() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump(4),
        crate::ir::Instruction::move_(2, 1),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    let plan = facts.region_control(0, 4).expect("multi-exit region");
    assert_eq!(
        plan.edges(),
        [
            RegionEdge { from: 0, to: 3 },
            RegionEdge { from: 0, to: 1 },
            RegionEdge { from: 2, to: 4 },
        ]
    );
    assert_eq!(
        facts.region_live_out(&plan),
        std::collections::BTreeSet::from([1])
    );
    assert_eq!(plan.external_live_out(), &[1]);
}

#[test]
fn region_plan_retains_external_exits_without_admitting_external_entries() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(0, 1).expect("branch with two exits");
    assert_eq!(plan.blocks(), [0]);
    assert_eq!(
        plan.edges(),
        [RegionEdge { from: 0, to: 3 }, RegionEdge { from: 0, to: 1 },]
    );
}

#[test]
fn terminal_exit_plan_allows_valid_external_targets_beyond_region_end() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump(4),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    let operations = [
        crate::ir::Opcode::JumpIfFalse,
        crate::ir::Opcode::Move,
        crate::ir::Opcode::Jump,
    ];
    let plan = facts
        .region_plan_with_terminal_exits(&entries, 0, &operations)
        .expect("valid external CFG targets remain admissible");
    assert_eq!(
        plan.edges(),
        [
            RegionEdge { from: 0, to: 3 },
            RegionEdge { from: 0, to: 1 },
            RegionEdge { from: 2, to: 4 },
        ]
    );
}

#[test]
fn region_plan_retains_coincident_conditional_exits() {
    let entries = entries(&[
        crate::ir::Instruction::jump_if_false(0, 1),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 2]);
    let plan = facts.region_control(0, 1).expect("conditional region");
    assert_eq!(
        plan.edges(),
        [RegionEdge { from: 0, to: 1 }, RegionEdge { from: 0, to: 1 }]
    );
    assert!(plan.join_blocks().is_empty());
    assert_eq!(plan.terminal_conditional_exits(), Some((1, 1)));
}

#[test]
fn region_plan_scales_beyond_previous_block_budget() {
    let mut instructions = (0..12)
        .map(|index| crate::ir::Instruction::jump_if_false(0, (index + 2).min(12) as u16))
        .collect::<Vec<_>>();
    instructions.push(crate::ir::Instruction::ret(0));
    let entries = entries(&instructions);
    let facts = ControlFlowFacts::new(&entries, &vec![None; entries.len()]);
    let plan = facts
        .region_control(0, entries.len())
        .expect("derived CFG storage grows with the region");
    assert!(plan.blocks().len() > 8);
}

#[test]
fn region_plan_scales_beyond_previous_edge_budget() {
    let mut plan = empty_region_control(0, 1);
    for index in 0..32 {
        assert!(push_edge(&mut plan, RegionEdge { from: 0, to: index }).is_some());
    }
    assert_eq!(plan.edges().len(), 32);
}

#[test]
fn straight_line_scan_ends_after_first_explicit_control_edge() {
    let entries = entries(&[
        crate::ir::Instruction::move_(0, 1),
        crate::ir::Instruction::move_(2, 0),
        crate::ir::Instruction::jump_if_false(2, 4),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::ret(2),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    assert_eq!(facts.straight_line_scan_end(0), Some(3));
    assert_eq!(facts.straight_line_scan_end(2), Some(3));
}

#[test]
fn pure_control_prefix_end_is_derived_once_at_effect_boundaries() {
    let entries = entries(&[
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction {
            opcode: crate::ir::Opcode::Call,
            flags: 0,
            a: 0,
            b: 1,
            c: 0,
        },
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 5]);
    assert_eq!(facts.pure_control_prefix_end(0), Some(2));
    assert_eq!(facts.pure_control_prefix_end(1), Some(2));
    assert_eq!(facts.pure_control_prefix_end(2), Some(2));
    assert_eq!(facts.pure_control_prefix_end(3), Some(5));
    assert_eq!(facts.pure_control_prefix_end(5), None);
}

#[test]
fn straight_line_scan_rejects_malformed_successors() {
    let entries = entries(&[
        crate::ir::Instruction::jump(99),
        crate::ir::Instruction::ret(0),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 2]);
    assert_eq!(facts.straight_line_scan_end(0), None);
}

#[test]
fn straight_line_scan_stops_at_terminal_and_explicit_next_jump() {
    let terminal_entries = entries(&[
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::move_(1, 0),
    ]);
    let terminal_facts = ControlFlowFacts::new(&terminal_entries, &[None; 2]);
    assert_eq!(terminal_facts.straight_line_scan_end(0), Some(1));

    let jump_entries = entries(&[
        crate::ir::Instruction::jump(1),
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::ret(1),
    ]);
    let jump_facts = ControlFlowFacts::new(&jump_entries, &[None; 3]);
    assert_eq!(jump_facts.straight_line_scan_end(0), Some(1));
}
