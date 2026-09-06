//! Bounded control-flow facts shared by stencil admission and verification.
//!
//! Canonical residual instructions remain the authority. This module derives
//! successors, region-entry legality, and register liveness without owning a
//! second control-flow representation.

use crate::machine::BaselineEntry;
use std::collections::BTreeSet;

pub(crate) const MAX_REGION_BLOCKS: usize = 8;
pub(crate) const MAX_REGION_EDGES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionEdge {
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionControlPlan {
    start: usize,
    end: usize,
    blocks: [usize; MAX_REGION_BLOCKS],
    block_len: u8,
    edges: [RegionEdge; MAX_REGION_EDGES],
    edge_len: u8,
}

impl RegionControlPlan {
    pub(crate) fn linear(start: usize, len: usize) -> Option<Self> {
        let end = start.checked_add(len)?;
        (len > 0).then(|| {
            let mut plan = empty_region_control(start, end);
            plan.blocks[0] = start;
            plan.block_len = 1;
            plan
        })
    }

    pub(crate) fn blocks(&self) -> &[usize] {
        &self.blocks[..usize::from(self.block_len)]
    }

    pub(crate) fn edges(&self) -> &[RegionEdge] {
        &self.edges[..usize::from(self.edge_len)]
    }

    pub(crate) const fn start(&self) -> usize {
        self.start
    }

    pub(crate) const fn end(&self) -> usize {
        self.end
    }

    pub(crate) const fn span_len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub(crate) fn is_linear(&self) -> bool {
        self.blocks() == [self.start] && self.edges().is_empty()
    }

    pub(crate) fn has_backedge(&self) -> bool {
        self.edges().iter().any(|edge| edge.to <= edge.from)
    }

    pub(crate) fn terminal_conditional_exits(&self) -> Option<(usize, usize)> {
        let branch = self.end.checked_sub(1)?;
        let mut exits = self.edges().iter().filter(|edge| edge.from == branch);
        let first = exits.next()?.to;
        let second = exits.next()?.to;
        (exits.next().is_none() && self.edges().len() == 2).then_some(())?;
        if first == self.end && second == self.end {
            Some((self.end, self.end))
        } else if first == self.end {
            Some((second, first))
        } else if second == self.end {
            Some((first, second))
        } else {
            None
        }
    }
}

/// Immutable, bounded control-flow facts derived from canonical residual code.
///
/// Admission consumers share this value so liveness and predecessor edges are
/// computed once per baseline plan rather than rediscovered per candidate.
pub(crate) struct ControlFlowFacts {
    live_in: Vec<BTreeSet<u16>>,
    live_out: Vec<BTreeSet<u16>>,
    predecessors: Vec<Vec<usize>>,
    malformed_edges: BTreeSet<usize>,
    successors: Vec<Successors>,
}

#[derive(Clone, Copy)]
struct Successors {
    pcs: [usize; 2],
    len: u8,
    control_len: u8,
    malformed: bool,
}

impl Successors {
    const fn none() -> Self {
        Self {
            pcs: [0; 2],
            len: 0,
            control_len: 0,
            malformed: false,
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.pcs[..usize::from(self.len)].iter().copied()
    }

    fn control_iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.pcs[..usize::from(self.control_len)].iter().copied()
    }
}

impl ControlFlowFacts {
    pub(crate) fn new(entries: &[BaselineEntry], operand_windows: &[Option<&[u16]>]) -> Self {
        let successors = successor_table(entries);
        let live_out = register_liveness(entries, operand_windows, &successors);
        Self {
            live_in: live_inputs(entries, operand_windows, &live_out),
            live_out,
            predecessors: predecessor_pcs(entries.len(), &successors),
            malformed_edges: malformed_edges(&successors),
            successors,
        }
    }

    pub(crate) fn live_out(&self) -> &[BTreeSet<u16>] {
        &self.live_out
    }

    pub(crate) fn live_in_at(&self, pc: usize) -> Option<&BTreeSet<u16>> {
        self.live_in.get(pc)
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
        self.region_plan(entries, start, operations).is_some()
    }

    pub(crate) fn region_plan(
        &self,
        entries: &[BaselineEntry],
        start: usize,
        operations: &[crate::ir::Opcode],
    ) -> Option<RegionControlPlan> {
        let Some(end) = start.checked_add(operations.len()) else {
            return None;
        };
        (operations.len() > 0
            && end <= entries.len()
            && operations.iter().enumerate().all(|(offset, opcode)| {
                entry_matches_region(
                    &entries[start + offset],
                    *opcode,
                    start,
                    end,
                    start + offset,
                )
            }))
        .then_some(())?;
        self.region_control(start, end)
    }

    pub(crate) fn region_control(&self, start: usize, end: usize) -> Option<RegionControlPlan> {
        (start < end && self.region_entry_is_legal(start, end)).then_some(())?;
        bounded_region_control(&self.successors, start, end)
    }
}

fn bounded_region_control(
    successors: &[Successors],
    start: usize,
    end: usize,
) -> Option<RegionControlPlan> {
    let mut plan = empty_region_control(start, end);
    push_block(&mut plan, start)?;
    for pc in start..end {
        let edges = successors.get(pc)?;
        if explicit_control_edge(edges, pc) {
            push_control_edges(&mut plan, pc, edges, start, end)?;
        }
    }
    Some(plan)
}

fn empty_region_control(start: usize, end: usize) -> RegionControlPlan {
    const EMPTY_EDGE: RegionEdge = RegionEdge { from: 0, to: 0 };
    RegionControlPlan {
        start,
        end,
        blocks: [0; MAX_REGION_BLOCKS],
        block_len: 0,
        edges: [EMPTY_EDGE; MAX_REGION_EDGES],
        edge_len: 0,
    }
}

fn explicit_control_edge(edges: &Successors, pc: usize) -> bool {
    edges.control_len > 1 || (edges.control_len == 1 && edges.pcs[0] != pc.saturating_add(1))
}

fn push_control_edges(
    plan: &mut RegionControlPlan,
    pc: usize,
    edges: &Successors,
    start: usize,
    end: usize,
) -> Option<()> {
    for target in edges.control_iter() {
        push_edge(
            plan,
            RegionEdge {
                from: pc,
                to: target,
            },
        )?;
        if (start..end).contains(&target) {
            push_block(plan, target)?;
        }
    }
    Some(())
}

fn push_block(plan: &mut RegionControlPlan, block: usize) -> Option<()> {
    if plan.blocks().contains(&block) {
        return Some(());
    }
    let slot = plan.blocks.get_mut(usize::from(plan.block_len))?;
    *slot = block;
    plan.block_len += 1;
    Some(())
}

fn push_edge(plan: &mut RegionControlPlan, edge: RegionEdge) -> Option<()> {
    let slot = plan.edges.get_mut(usize::from(plan.edge_len))?;
    *slot = edge;
    plan.edge_len += 1;
    Some(())
}

fn live_inputs(
    entries: &[BaselineEntry],
    windows: &[Option<&[u16]>],
    live_out: &[BTreeSet<u16>],
) -> Vec<BTreeSet<u16>> {
    let conservative = all_register_uses(entries, windows);
    entries
        .iter()
        .enumerate()
        .map(|(pc, entry)| {
            live_input(
                &live_out[pc],
                entry.instruction.register_flow(),
                windows.get(pc).copied().flatten(),
                &conservative,
            )
        })
        .collect()
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
        control_len: 1,
        malformed: pcs[0] >= entries.len(),
    }
}

fn successor_table(entries: &[BaselineEntry]) -> Vec<Successors> {
    (0..entries.len())
        .map(|pc| successors(entries, pc))
        .collect()
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
    let has_fallthrough = fallthrough < len;
    Successors {
        pcs: [target, fallthrough],
        len: if has_fallthrough && target != fallthrough {
            2
        } else {
            1
        },
        control_len: if has_fallthrough { 2 } else { 1 },
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
        let input = live_input(
            &output,
            flow,
            windows.get(pc).copied().flatten(),
            conservative,
        );
        changed |= live_out[pc] != output || live_in[pc] != input;
        live_out[pc] = output;
        live_in[pc] = input;
    }
    changed
}

fn successor_input_union(successors: &Successors, live_in: &[BTreeSet<u16>]) -> BTreeSet<u16> {
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
#[path = "stencil_cfg_tests.rs"]
mod tests;
