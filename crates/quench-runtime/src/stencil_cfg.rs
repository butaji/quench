//! Bounded control-flow facts shared by stencil admission and verification.
//!
//! Canonical residual instructions remain the authority. This module derives
//! successors, region-entry legality, and register liveness without owning a
//! second control-flow representation.

use crate::machine::BaselineEntry;
use std::collections::BTreeSet;

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
    let conservative = all_register_uses(entries, operand_windows);
    let mut live_in = vec![BTreeSet::new(); entries.len()];
    let mut live_out = live_in.clone();
    for _ in 0..=entries.len().saturating_mul(2) {
        if !liveness_round(entries, operand_windows, &conservative, &mut live_in, &mut live_out) {
            break;
        }
    }
    live_out
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

pub(crate) fn region_entry_is_legal(
    entries: &[BaselineEntry],
    start: usize,
    end: usize,
) -> bool {
    let Some(interior_start) = start.checked_add(1) else {
        return false;
    };
    (interior_start..end).all(|target| {
        (0..entries.len()).all(|predecessor| {
            !successor_pcs(entries, predecessor).contains(&target)
                || (start..end).contains(&predecessor)
        })
    })
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
    fn region_entry_check_accepts_internal_and_rejects_external_edges() {
        let internal = entries(&[
            crate::ir::Instruction::move_(0, 1),
            crate::ir::Instruction::jump(1),
            crate::ir::Instruction::ret(0),
        ]);
        assert!(region_entry_is_legal(&internal, 0, 2));
        let external = entries(&[
            crate::ir::Instruction::move_(0, 1),
            crate::ir::Instruction::ret(0),
            crate::ir::Instruction::jump(1),
        ]);
        assert!(!region_entry_is_legal(&external, 0, 2));
    }

    #[test]
    fn branch_successors_do_not_duplicate_fallthrough() {
        let entries = entries(&[
            crate::ir::Instruction::jump_if_false(0, 1),
            crate::ir::Instruction::ret(0),
        ]);
        assert_eq!(successor_pcs(&entries, 0), [1]);
    }
}
