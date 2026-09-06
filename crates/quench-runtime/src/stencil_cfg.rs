//! Bounded control-flow facts shared by stencil admission and verification.
//!
//! Canonical residual instructions remain the authority. This module derives
//! successors, region-entry legality, and register liveness without owning a
//! second control-flow representation.

use crate::machine::BaselineEntry;
use std::collections::BTreeSet;

/// Immutable, bounded control-flow facts derived from canonical residual code.
///
/// Admission consumers share this value so liveness and predecessor edges are
/// computed once per baseline plan rather than rediscovered per candidate.
pub(crate) struct ControlFlowFacts {
    live_out: Vec<BTreeSet<u16>>,
    predecessors: Vec<Vec<usize>>,
    malformed_edges: BTreeSet<usize>,
}

#[derive(Clone, Copy)]
struct Successors {
    pcs: [usize; 2],
    len: u8,
    malformed: bool,
}

impl Successors {
    const fn none() -> Self {
        Self {
            pcs: [0; 2],
            len: 0,
            malformed: false,
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.pcs[..usize::from(self.len)].iter().copied()
    }
}

impl ControlFlowFacts {
    pub(crate) fn new(
        entries: &[BaselineEntry],
        operand_windows: &[Option<&[u16]>],
    ) -> Self {
        let successors = successor_table(entries);
        Self {
            live_out: register_liveness(entries, operand_windows, &successors),
            predecessors: predecessor_pcs(entries.len(), &successors),
            malformed_edges: malformed_edges(&successors),
        }
    }

    pub(crate) fn live_out(&self) -> &[BTreeSet<u16>] {
        &self.live_out
    }

    pub(crate) fn region_entry_is_legal(&self, start: usize, end: usize) -> bool {
        let Some(interior_start) = start.checked_add(1) else {
            return false;
        };
        end <= self.predecessors.len()
            && self.malformed_edges.range(start..end).next().is_none()
            && (interior_start..end).all(|target| {
                self.predecessors[target]
                    .iter()
                    .all(|predecessor| (start..end).contains(predecessor))
            })
    }

    pub(crate) fn region_matches(
        &self,
        entries: &[BaselineEntry],
        start: usize,
        operations: &[crate::ir::Opcode],
    ) -> bool {
        let Some(end) = start.checked_add(operations.len()) else {
            return false;
        };
        !operations.is_empty()
            && end <= entries.len()
            && self.region_entry_is_legal(start, end)
            && operations.iter().enumerate().all(|(offset, opcode)| {
                entry_matches_region(&entries[start + offset], *opcode, start, end, start + offset)
            })
    }
}

fn entry_matches_region(
    entry: &BaselineEntry,
    expected: crate::ir::Opcode,
    start: usize,
    end: usize,
    pc: usize,
) -> bool {
    entry.instruction.opcode == expected
        && expected.operands_are_canonical([
            entry.instruction.a,
            entry.instruction.b,
            entry.instruction.c,
        ])
        && control_stays_in_region(expected.control_operands(entry.instruction), start, end, pc)
}

fn control_stays_in_region(
    control: crate::ir::ControlOperands,
    start: usize,
    end: usize,
    pc: usize,
) -> bool {
    match control {
        crate::ir::ControlOperands::Return { .. } => pc + 1 == end,
        crate::ir::ControlOperands::Branch { target, .. }
        | crate::ir::ControlOperands::Jump { target } => {
            (start..=end).contains(&usize::from(target))
        }
        crate::ir::ControlOperands::Loop { .. } => false,
        crate::ir::ControlOperands::Next => true,
    }
}

fn successors(entries: &[BaselineEntry], pc: usize) -> Successors {
    let Some(entry) = entries.get(pc) else {
        return Successors::none();
    };
    let pcs = match entry.control {
        crate::ir::ControlOperands::Next if pc + 1 < entries.len() => [pc + 1, 0],
        crate::ir::ControlOperands::Branch { target, .. } => {
            return branch_successors(entries.len(), pc, usize::from(target));
        }
        crate::ir::ControlOperands::Jump { target } => [usize::from(target), 0],
        _ => return Successors::none(),
    };
    Successors {
        pcs,
        len: 1,
        malformed: pcs[0] >= entries.len(),
    }
}

fn successor_table(entries: &[BaselineEntry]) -> Vec<Successors> {
    (0..entries.len()).map(|pc| successors(entries, pc)).collect()
}

fn predecessor_pcs(len: usize, successors: &[Successors]) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); len];
    for (pc, edges) in successors.iter().enumerate() {
        for successor in edges.iter() {
            if let Some(incoming) = predecessors.get_mut(successor) {
                incoming.push(pc);
            }
        }
    }
    predecessors
}

fn malformed_edges(successors: &[Successors]) -> BTreeSet<usize> {
    successors
        .iter()
        .enumerate()
        .filter_map(|(pc, edges)| edges.malformed.then_some(pc))
        .collect()
}

fn branch_successors(len: usize, pc: usize, target: usize) -> Successors {
    let fallthrough = pc.saturating_add(1);
    let has_fallthrough = fallthrough < len && target != fallthrough;
    Successors {
        pcs: [target, fallthrough],
        len: if has_fallthrough { 2 } else { 1 },
        malformed: target >= len,
    }
}

fn register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
) -> Vec<BTreeSet<u16>> {
    bounded_register_liveness(
        entries,
        operand_windows,
        successors,
        entries.len().saturating_mul(2).saturating_add(1),
    )
}

fn bounded_register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    successors: &[Successors],
    round_limit: usize,
) -> Vec<BTreeSet<u16>> {
    let conservative = all_register_uses(entries, operand_windows);
    let mut live_in = vec![BTreeSet::new(); entries.len()];
    let mut live_out = live_in.clone();
    for _ in 0..round_limit {
        if !liveness_round(
            entries,
            operand_windows,
            successors,
            &conservative,
            &mut live_in,
            &mut live_out,
        ) {
            return live_out;
        }
    }
    vec![conservative; entries.len()]
}

fn liveness_round(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    successors: &[Successors],
    conservative: &BTreeSet<u16>,
    live_in: &mut [BTreeSet<u16>],
    live_out: &mut [BTreeSet<u16>],
) -> bool {
    let mut changed = false;
    for pc in (0..entries.len()).rev() {
        let output = successor_input_union(&successors[pc], live_in);
        let flow = entries[pc].instruction.register_flow();
        let input = live_input(&output, flow, windows.get(pc).copied().flatten(), conservative);
        changed |= live_out[pc] != output || live_in[pc] != input;
        live_out[pc] = output;
        live_in[pc] = input;
    }
    changed
}

fn successor_input_union(
    successors: &Successors,
    live_in: &[BTreeSet<u16>],
) -> BTreeSet<u16> {
    let mut output = BTreeSet::new();
    for successor in successors.iter() {
        if let Some(input) = live_in.get(successor) {
            output.extend(input.iter().copied());
        }
    }
    output
}

fn all_register_uses(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
) -> BTreeSet<u16> {
    let mut uses = entries
        .iter()
        .flat_map(|entry| entry.instruction.register_flow().uses)
        .flatten()
        .collect::<BTreeSet<_>>();
    uses.extend(
        operand_windows
            .iter()
            .flatten()
            .flat_map(|window| window.iter().copied()),
    );
    uses
}

fn live_input(
    output: &BTreeSet<u16>,
    flow: crate::ir::RegisterFlow,
    window: Option<&[u16]>,
    conservative: &BTreeSet<u16>,
) -> BTreeSet<u16> {
    let mut input = if flow.complete {
        output.clone()
    } else {
        conservative.clone()
    };
    if let Some(definition) = flow.definition {
        input.remove(&definition);
    }
    input.extend(flow.uses.into_iter().flatten());
    input.extend(window.into_iter().flatten().copied());
    input
}

#[cfg(test)]
mod tests {
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
}
