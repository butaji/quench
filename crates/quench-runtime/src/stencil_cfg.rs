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

impl ControlFlowFacts {
    pub(crate) fn new(
        entries: &[BaselineEntry],
        operand_windows: &[Option<&[u16]>],
    ) -> Self {
        Self {
            live_out: register_liveness(entries, operand_windows),
            predecessors: predecessor_pcs(entries),
            malformed_edges: malformed_edges(entries),
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
}

pub(crate) fn successor_pcs(entries: &[BaselineEntry], pc: usize) -> Vec<usize> {
    let Some(entry) = entries.get(pc) else {
        return Vec::new();
    };
    match entry.control {
        crate::ir::ControlOperands::Next if pc + 1 < entries.len() => vec![pc + 1],
        crate::ir::ControlOperands::Branch { target, .. } => {
            branch_successors(entries.len(), pc, usize::from(target))
        }
        crate::ir::ControlOperands::Jump { target } => vec![usize::from(target)],
        _ => Vec::new(),
    }
}

fn predecessor_pcs(entries: &[BaselineEntry]) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); entries.len()];
    for pc in 0..entries.len() {
        for successor in successor_pcs(entries, pc) {
            if let Some(incoming) = predecessors.get_mut(successor) {
                incoming.push(pc);
            }
        }
    }
    predecessors
}

fn malformed_edges(entries: &[BaselineEntry]) -> BTreeSet<usize> {
    (0..entries.len())
        .filter(|pc| {
            successor_pcs(entries, *pc)
                .iter()
                .any(|successor| *successor >= entries.len())
        })
        .collect()
}

fn branch_successors(len: usize, pc: usize, target: usize) -> Vec<usize> {
    let mut successors = vec![target];
    if pc + 1 < len && target != pc + 1 {
        successors.push(pc + 1);
    }
    successors
}

pub(crate) fn register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
) -> Vec<BTreeSet<u16>> {
    bounded_register_liveness(
        entries,
        operand_windows,
        entries.len().saturating_mul(2).saturating_add(1),
    )
}

fn bounded_register_liveness(
    entries: &[BaselineEntry],
    operand_windows: &[Option<&[u16]>],
    round_limit: usize,
) -> Vec<BTreeSet<u16>> {
    let conservative = all_register_uses(entries, operand_windows);
    let mut live_in = vec![BTreeSet::new(); entries.len()];
    let mut live_out = live_in.clone();
    for _ in 0..round_limit {
        if !liveness_round(entries, operand_windows, &conservative, &mut live_in, &mut live_out) {
            return live_out;
        }
    }
    vec![conservative; entries.len()]
}

fn liveness_round(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    conservative: &BTreeSet<u16>,
    live_in: &mut [BTreeSet<u16>],
    live_out: &mut [BTreeSet<u16>],
) -> bool {
    let mut changed = false;
    for pc in (0..entries.len()).rev() {
        let output = successor_input_union(entries, pc, live_in);
        let flow = entries[pc].instruction.register_flow();
        let input = live_input(&output, flow, windows.get(pc).copied().flatten(), conservative);
        changed |= live_out[pc] != output || live_in[pc] != input;
        live_out[pc] = output;
        live_in[pc] = input;
    }
    changed
}

fn successor_input_union(
    entries: &[BaselineEntry],
    pc: usize,
    live_in: &[BTreeSet<u16>],
) -> BTreeSet<u16> {
    let mut output = BTreeSet::new();
    for successor in successor_pcs(entries, pc) {
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
        let live = register_liveness(&entries, &[None, None, None]);
        assert_eq!(live[0], BTreeSet::from([1, 2]));
    }

    #[test]
    fn liveness_budget_exhaustion_is_conservative() {
        let entries = entries(&[
            crate::ir::Instruction::move_(2, 1),
            crate::ir::Instruction::ret(2),
        ]);
        let live = bounded_register_liveness(&entries, &[None, None], 0);
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
    fn branch_successors_do_not_duplicate_fallthrough() {
        let entries = entries(&[
            crate::ir::Instruction::jump_if_false(0, 1),
            crate::ir::Instruction::ret(0),
        ]);
        assert_eq!(successor_pcs(&entries, 0), [1]);
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
