use super::*;

fn entries(instructions: &[crate::ir::Instruction]) -> Vec<BaselineEntry> {
    instructions
        .iter()
        .copied()
        .map(|instruction| BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
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
    let plan = facts.region_control(0, 4).expect("bounded branch plan");
    assert_eq!(plan.blocks(), [0, 2, 1, 3]);
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
fn region_plan_records_native_backedge_without_unbounded_graph() {
    let entries = entries(&[
        crate::ir::Instruction::move_(1, 0),
        crate::ir::Instruction::jump_if_false(1, 3),
        crate::ir::Instruction::jump(0),
        crate::ir::Instruction::ret(1),
    ]);
    let facts = ControlFlowFacts::new(&entries, &[None; 4]);
    let plan = facts.region_control(0, 3).expect("bounded loop plan");
    assert_eq!(plan.start(), 0);
    assert_eq!(plan.end(), 3);
    assert!(plan.has_backedge());
    assert!(plan.edges().contains(&RegionEdge { from: 2, to: 0 }));
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
    assert_eq!(plan.terminal_conditional_exits(), Some((1, 1)));
}

#[test]
fn region_plan_rejects_block_budget_overflow() {
    let mut instructions = (0..8)
        .map(|_| crate::ir::Instruction::jump_if_false(0, 8))
        .collect::<Vec<_>>();
    instructions.push(crate::ir::Instruction::ret(0));
    let entries = entries(&instructions);
    let facts = ControlFlowFacts::new(&entries, &vec![None; entries.len()]);
    assert!(facts.region_control(0, entries.len()).is_none());
}

#[test]
fn region_plan_rejects_edge_budget_overflow() {
    let mut plan = empty_region_control(0, 1);
    for index in 0..MAX_REGION_EDGES {
        assert!(push_edge(&mut plan, RegionEdge { from: 0, to: index }).is_some());
    }
    assert!(push_edge(&mut plan, RegionEdge { from: 0, to: 99 }).is_none());
}
