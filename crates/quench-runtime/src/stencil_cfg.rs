//! Control-flow facts shared by stencil admission and verification.
//!
//! Canonical residual instructions remain the authority. This module derives
//! successors, region-entry legality, and register liveness without owning a
//! second control-flow representation.

use crate::machine::BaselineEntry;
use std::collections::BTreeSet;

#[path = "stencil_cfg_liveness.rs"]
mod liveness;
#[cfg(test)]
use liveness::bounded_register_liveness;
use liveness::{live_inputs, register_liveness};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionEdge {
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RegionBlock {
    pub start: usize,
    pub end: usize,
}

/// A conservative integer induction update discovered from a verified
/// backedge.  This is analysis data only: it does not authorize a native loop
/// by itself.  The destination and source must both be loop-carried `IncI`
/// registers so later emitters can materialize the recurrence without
/// inventing a value-flow edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InductionCandidate {
    pub register: u16,
    pub source: u16,
    pub update_pc: usize,
    pub decrement: bool,
}

/// Verified region shape. Blocks and edges are derived storage, not a semantic
/// limit; `try_reserve` failure rejects optional admission to canonical code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegionControlPlan {
    start: usize,
    end: usize,
    blocks: Vec<usize>,
    edges: Vec<RegionEdge>,
    /// All verified edges in lookup order. Transfer validation and resident
    /// backedge checks ask about one concrete pair on every hot iteration;
    /// keeping this derived index avoids scanning all CFG edges (or
    /// materializing the diagnostic `internal_backedges` view) in that path.
    edge_lookup: Vec<RegionEdge>,
    /// Registers required by successors outside this region.  This is a
    /// derived exit contract, not a second liveness authority; it is cached
    /// with the immutable CFG plan so native exits do not reconstruct it.
    external_live_out: Vec<u16>,
}

impl RegionControlPlan {
    pub(crate) fn linear(start: usize, len: usize) -> Option<Self> {
        let end = start.checked_add(len)?;
        if len == 0 {
            return None;
        }
        let mut plan = empty_region_control(start, end);
        push_block(&mut plan, start)?;
        Some(plan)
    }

    /// Construct a physical layout from already-verified relative
    /// CFG edges. Canonical source admission must validate semantics first;
    /// this value only describes how selected fragments are stitched.
    pub(crate) fn from_relative_edges(len: usize, edges: &[(usize, usize)]) -> Option<Self> {
        (len > 0).then_some(())?;
        let mut plan = empty_region_control(0, len);
        push_block(&mut plan, 0)?;
        for &(from, to) in edges {
            (from < len && to < len).then_some(())?;
            let edge = RegionEdge { from, to };
            (!plan.edges().contains(&edge)).then_some(())?;
            push_edge(&mut plan, edge)?;
            push_block(&mut plan, to)?;
        }
        sort_edge_lookup(&mut plan);
        Some(plan)
    }

    pub(crate) fn blocks(&self) -> &[usize] {
        &self.blocks
    }

    pub(crate) fn edges(&self) -> &[RegionEdge] {
        &self.edges
    }

    pub(crate) fn external_live_out(&self) -> &[u16] {
        &self.external_live_out
    }

    /// Return sorted basic-block spans derived from the verified block-entry
    /// set. The region end is the exclusive sentinel; no source-shaped window
    /// is reconstructed by consumers.
    pub(crate) fn block_ranges(&self) -> Option<Vec<RegionBlock>> {
        let mut starts = self.blocks.clone();
        starts.sort_unstable();
        starts.dedup();
        starts.try_reserve(1).ok()?;
        starts.push(self.end);
        let mut ranges = Vec::new();
        ranges.try_reserve(starts.len().saturating_sub(1)).ok()?;
        for pair in starts.windows(2) {
            let &[start, end] = pair else {
                continue;
            };
            if start < end {
                ranges.push(RegionBlock { start, end });
            }
        }
        Some(ranges)
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

    /// Return control transfers that target an earlier (or current) PC.
    /// This includes backward exits to an enclosing region; callers deriving
    /// loop state must use [`Self::internal_backedges`] instead.
    pub(crate) fn backedges(&self) -> Vec<RegionEdge> {
        self.edges()
            .iter()
            .copied()
            .filter(|edge| edge.to <= edge.from)
            .collect()
    }

    /// Return only backedges whose target remains inside this region. A
    /// backward edge may also be a legal external exit (for example a
    /// handoff to an enclosing loop); that edge is control-flow data but not
    /// loop-carried state for this plan.
    pub(crate) fn internal_backedges(&self) -> Vec<RegionEdge> {
        self.backedges()
            .into_iter()
            .filter(|edge| (self.start..self.end).contains(&edge.to))
            .collect()
    }

    /// Test one resident transfer without materializing the derived
    /// backedge list. The execution loop calls this on every iteration, so
    /// keep the CFG fact as a borrowed predicate and leave allocation to the
    /// diagnostic/list-producing views above.
    #[inline]
    pub(crate) fn has_internal_backedge(&self, from: usize, to: usize) -> bool {
        self.edge_lookup
            .binary_search_by_key(&(from, to), |edge| (edge.from, edge.to))
            .is_ok_and(|_| (self.start..self.end).contains(&to) && to <= from)
    }

    /// Return blocks reached from more than one distinct predecessor. Duplicate
    /// edges from a single conditional instruction are not a join.
    pub(crate) fn join_blocks(&self) -> Vec<usize> {
        self.blocks()
            .iter()
            .copied()
            .filter(|block| {
                let mut first = None;
                for edge in self.edges().iter().filter(|edge| edge.to == *block) {
                    match first {
                        None => first = Some(edge.from),
                        Some(predecessor) if predecessor != edge.from => return true,
                        Some(_) => {}
                    }
                }
                false
            })
            .collect()
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

    pub(crate) fn matches_operations(&self, operations: &[crate::ir::Opcode]) -> bool {
        self.span_len() == operations.len()
            && operations.iter().enumerate().all(|(offset, opcode)| {
                self.operation_edges_match(self.start + offset, opcode.control_flow())
            })
            && self.blocks_match_edges()
    }

    pub(crate) fn permits_operation_transfer(
        &self,
        operations: &[crate::ir::Opcode],
        from_offset: usize,
        to_offset: usize,
    ) -> bool {
        let Some(to) = self.start.checked_add(to_offset) else {
            return false;
        };
        self.permits_transfer(operations, from_offset, to)
    }

    pub(crate) fn permits_transfer(
        &self,
        operations: &[crate::ir::Opcode],
        from_offset: usize,
        to: usize,
    ) -> bool {
        let Some(opcode) = operations.get(from_offset) else {
            return false;
        };
        let Some(from) = self.start.checked_add(from_offset) else {
            return false;
        };
        match opcode.control_flow() {
            crate::facts::ControlFlow::Next => to == from.saturating_add(1),
            crate::facts::ControlFlow::Branch | crate::facts::ControlFlow::Jump => self
                .edge_lookup
                .binary_search_by_key(&(from, to), |edge| (edge.from, edge.to))
                .is_ok(),
            crate::facts::ControlFlow::Return
            | crate::facts::ControlFlow::Throw
            | crate::facts::ControlFlow::Loop => false,
        }
    }

    fn operation_edges_match(&self, pc: usize, control: crate::facts::ControlFlow) -> bool {
        let mut edge_count = 0usize;
        let mut has_fallthrough = false;
        for edge in self.edges().iter().filter(|edge| edge.from == pc) {
            edge_count += 1;
            has_fallthrough |= edge.to == pc.saturating_add(1);
        }
        match control {
            crate::facts::ControlFlow::Next
            | crate::facts::ControlFlow::Return
            | crate::facts::ControlFlow::Throw => edge_count == 0,
            crate::facts::ControlFlow::Branch => edge_count == 2 && has_fallthrough,
            crate::facts::ControlFlow::Jump => edge_count == 1,
            crate::facts::ControlFlow::Loop => edge_count == 0,
        }
    }

    fn blocks_match_edges(&self) -> bool {
        self.blocks().first() == Some(&self.start)
            && self.blocks()[1..].iter().all(|block| {
                (self.start..self.end).contains(block)
                    && self.edges().iter().any(|edge| edge.to == *block)
            })
            && self.edges().iter().all(|edge| {
                !(self.start..self.end).contains(&edge.to) || self.blocks().contains(&edge.to)
            })
    }
}

/// Immutable control-flow facts derived from canonical residual code.
///
/// Admission consumers share this value so liveness and predecessor edges are
/// computed once per baseline plan rather than rediscovered per candidate.
#[derive(Debug)]
pub(crate) struct ControlFlowFacts {
    live_in: Vec<BTreeSet<u16>>,
    live_out: Vec<BTreeSet<u16>>,
    predecessors: Vec<Vec<usize>>,
    malformed_edges: BTreeSet<usize>,
    successors: Vec<Successors>,
    /// Exclusive endpoints for straight-line admission scans. `usize::MAX`
    /// marks a path that encounters malformed control data before its first
    /// explicit edge.
    straight_line_ends: Vec<usize>,
    /// Exclusive endpoints for generic value/control Bridge prefixes. These
    /// are derived once from canonical operation effects so admission does
    /// not rescan the remaining function from every candidate PC.
    pure_control_prefix_ends: Vec<usize>,
}

/// Successors of one canonical instruction. Two slots are a semantic shape
/// invariant (fall-through plus one branch target), not a region-size budget.
#[derive(Clone, Copy, Debug)]
struct Successors {
    pcs: [usize; 2],
    len: u8,
    control_len: u8,
    explicit_control: bool,
    malformed: bool,
}

impl Successors {
    const fn none() -> Self {
        Self {
            pcs: [0; 2],
            len: 0,
            control_len: 0,
            explicit_control: false,
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
        let straight_line_ends = straight_line_scan_ends(&successors);
        let pure_control_prefix_ends = pure_control_prefix_ends(entries);
        Self {
            live_in: live_inputs(entries, operand_windows, &live_out),
            live_out,
            predecessors: predecessor_pcs(entries.len(), &successors),
            malformed_edges: malformed_edges(&successors),
            successors,
            straight_line_ends,
            pure_control_prefix_ends,
        }
    }

    pub(crate) fn live_out(&self) -> &[BTreeSet<u16>] {
        &self.live_out
    }

    pub(crate) fn live_in(&self) -> &[BTreeSet<u16>] {
        &self.live_in
    }

    pub(crate) fn live_in_at(&self, pc: usize) -> Option<&BTreeSet<u16>> {
        self.live_in.get(pc)
    }

    /// Return the canonical predecessor PCs for a block boundary.  Consumers
    /// use this derived view to build join moves; they must not reconstruct a
    /// second predecessor graph from source-shaped windows.
    pub(crate) fn predecessors_at(&self, pc: usize) -> Option<&[usize]> {
        self.predecessors.get(pc).map(Vec::as_slice)
    }

    /// Derive registers carried across each backedge. A register is carried
    /// only when it is live at the backedge target and redefined on the loop
    /// path before the transfer; values merely live through the loop are not
    /// classified as induction state.
    pub(crate) fn loop_carried_registers(
        &self,
        entries: &[BaselineEntry],
        plan: &RegionControlPlan,
    ) -> Vec<u16> {
        self.loop_carried_set(entries, plan).into_iter().collect()
    }

    fn loop_carried_set(
        &self,
        entries: &[BaselineEntry],
        plan: &RegionControlPlan,
    ) -> BTreeSet<u16> {
        let mut carried = BTreeSet::new();
        for edge in plan.internal_backedges() {
            let Some(live) = self.live_in_at(edge.to) else {
                continue;
            };
            for register in live {
                if entries.get(edge.to..=edge.from).is_some_and(|path| {
                    path.iter().any(|entry| {
                        entry.instruction.register_flow().definition == Some(*register)
                    })
                }) {
                    carried.insert(*register);
                }
            }
        }
        carried
    }

    /// Derive conservative integer induction updates for a region's verified
    /// backedges.  Only the canonical `IncI` recurrence is classified here;
    /// opaque updates and arithmetic constants remain ordinary loop state for
    /// later effect/type analysis.  Candidates are deduplicated so multiple
    /// backedges cannot create competing representations of one update.
    pub(crate) fn induction_candidates(
        &self,
        entries: &[BaselineEntry],
        plan: &RegionControlPlan,
    ) -> Vec<InductionCandidate> {
        let carried = self.loop_carried_set(entries, plan);
        let mut candidates = Vec::new();
        for edge in plan.internal_backedges() {
            let Some(path) = entries.get(edge.to..=edge.from) else {
                continue;
            };
            for (offset, entry) in path.iter().enumerate() {
                let instruction = entry.instruction;
                if instruction.opcode != crate::ir::Opcode::IncI
                    || !carried.contains(&instruction.a)
                    || !carried.contains(&instruction.b)
                {
                    continue;
                }
                let Some(update_pc) = edge.to.checked_add(offset) else {
                    continue;
                };
                let candidate = InductionCandidate {
                    register: instruction.a,
                    source: instruction.b,
                    update_pc,
                    decrement: instruction.flags & 1 != 0,
                };
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates.sort_by_key(|candidate| (candidate.update_pc, candidate.register));
        candidates
    }

    /// Derive the canonical register set that must be materialized when a
    /// region exits. Every successor outside the region contributes the
    /// liveness at its target; backedges and other interior transfers do not.
    /// This keeps multi-exit regions honest instead of checking only the
    /// contiguous end PC.
    pub(crate) fn region_live_out(&self, plan: &RegionControlPlan) -> BTreeSet<u16> {
        if !plan.external_live_out.is_empty() {
            return plan.external_live_out.iter().copied().collect();
        }
        // Plans built from projected physical edges (rather than this CFG)
        // have no liveness payload. Retain the derived fallback for those
        // images; source-admitted plans take the cached path above.
        let mut live_out = BTreeSet::new();
        let start = plan.start();
        let end = plan.end();
        for pc in start..end {
            let Some(successors) = self.successors.get(pc) else {
                continue;
            };
            for target in successors
                .iter()
                .filter(|target| !(start..end).contains(target))
            {
                if let Some(live) = self.live_in_at(target) {
                    live_out.extend(live.iter().copied());
                }
            }
        }
        live_out
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

    /// Build a region plan whose reachable paths may terminate at more than
    /// one `Return`/`Throw`.  Generated fixed-shape rows use `region_plan`
    /// because their terminal boundary is part of the physical contract;
    /// the generic bridge uses this derived CFG view for arbitrary structured
    /// blocks and preserves every canonical completion edge.
    pub(crate) fn region_plan_with_terminal_exits(
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
                entry_matches_region_with_terminal_exits(
                    &entries[start + offset],
                    *opcode,
                    start,
                    end,
                    entries.len(),
                )
            }))
        .then_some(())?;
        self.region_control(start, end)
    }

    pub(crate) fn region_control(&self, start: usize, end: usize) -> Option<RegionControlPlan> {
        (start < end && self.region_entry_is_legal(start, end)).then_some(())?;
        region_control_from_cfg(&self.successors, &self.live_in, start, end)
    }

    /// Return the exclusive end of the straight-line scan beginning at
    /// `start`, stopping immediately after the first explicit control edge.
    /// Admission consumers use this derived CFG boundary instead of a
    /// source-shaped instruction-count window. A malformed edge rejects the
    /// scan; callers then retain the canonical handler path.
    pub(crate) fn straight_line_scan_end(&self, start: usize) -> Option<usize> {
        let end = *self.straight_line_ends.get(start)?;
        (end != usize::MAX).then_some(end)
    }

    /// Return the exclusive endpoint of the contiguous value/control prefix
    /// beginning at `start`. Heap reads/writes, observable helpers and
    /// structured-loop gateways are explicit transition boundaries owned by
    /// later execution-tier work.
    pub(crate) fn pure_control_prefix_end(&self, start: usize) -> Option<usize> {
        self.pure_control_prefix_ends.get(start).copied()
    }

    pub(crate) fn has_backedge_at(&self, pc: usize) -> bool {
        self.successors
            .get(pc)
            .is_some_and(|successors| successors.control_iter().any(|target| target <= pc))
    }

    /// Return the canonical target of a backedge source.  This is the CFG
    /// authority used by OSR handoff after a baseline plan is published; a
    /// malformed target or a forward-only branch does not produce a target.
    pub(crate) fn backedge_target_at(&self, pc: usize) -> Option<usize> {
        let successors = self.successors.get(pc)?;
        successors
            .control_iter()
            .find(|target| *target <= pc && *target < self.successors.len())
    }
}

fn straight_line_scan_ends(successors: &[Successors]) -> Vec<usize> {
    let mut ends = vec![successors.len(); successors.len()];
    for start in (0..successors.len()).rev() {
        let successors_at = successors[start];
        ends[start] = if successors_at.malformed {
            usize::MAX
        } else if explicit_control_edge(&successors_at) {
            start.saturating_add(1)
        } else if start + 1 < successors.len() {
            ends[start + 1]
        } else {
            successors.len()
        };
    }
    ends
}

fn pure_control_prefix_ends(entries: &[BaselineEntry]) -> Vec<usize> {
    let mut ends = vec![entries.len(); entries.len()];
    let mut boundary = entries.len();
    for (pc, entry) in entries.iter().enumerate().rev() {
        let opcode = entry.instruction.opcode;
        if !opcode.spec().generic_bridge_safe() {
            boundary = pc;
        }
        ends[pc] = boundary;
    }
    ends
}

fn region_control_from_cfg(
    successors: &[Successors],
    live_in: &[BTreeSet<u16>],
    start: usize,
    end: usize,
) -> Option<RegionControlPlan> {
    let mut plan = empty_region_control(start, end);
    push_block(&mut plan, start)?;
    for pc in start..end {
        let edges = successors.get(pc)?;
        if explicit_control_edge(edges) {
            push_control_edges(&mut plan, pc, edges, start, end)?;
        }
    }
    derive_external_live_out(&mut plan, successors, live_in)?;
    sort_edge_lookup(&mut plan);
    Some(plan)
}

fn empty_region_control(start: usize, end: usize) -> RegionControlPlan {
    RegionControlPlan {
        start,
        end,
        blocks: Vec::new(),
        edges: Vec::new(),
        edge_lookup: Vec::new(),
        external_live_out: Vec::new(),
    }
}

#[inline]
fn sort_edge_lookup(plan: &mut RegionControlPlan) {
    plan.edge_lookup
        .sort_unstable_by_key(|edge| (edge.from, edge.to));
}

fn derive_external_live_out(
    plan: &mut RegionControlPlan,
    successors: &[Successors],
    live_in: &[BTreeSet<u16>],
) -> Option<()> {
    let mut registers = BTreeSet::new();
    for pc in plan.start..plan.end {
        let successors = successors.get(pc)?;
        for target in successors
            .iter()
            .filter(|target| !(plan.start..plan.end).contains(target))
        {
            // The exclusive code-end sentinel has no liveness row; reaching
            // it is a valid empty-live exit rather than an admission failure.
            if let Some(live) = live_in.get(target) {
                registers.extend(live.iter().copied());
            }
        }
    }
    plan.external_live_out.try_reserve(registers.len()).ok()?;
    plan.external_live_out.extend(registers);
    Some(())
}

fn explicit_control_edge(edges: &Successors) -> bool {
    edges.explicit_control
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
    plan.blocks.try_reserve(1).ok()?;
    plan.blocks.push(block);
    Some(())
}

fn push_edge(plan: &mut RegionControlPlan, edge: RegionEdge) -> Option<()> {
    plan.edges.try_reserve(1).ok()?;
    plan.edge_lookup.try_reserve(1).ok()?;
    plan.edge_lookup.push(edge);
    plan.edges.push(edge);
    Some(())
}

fn entry_matches_region(
    entry: &BaselineEntry,
    expected: crate::ir::Opcode,
    start: usize,
    end: usize,
    pc: usize,
) -> bool {
    expected.operands_match_physical_contract_with_flags(
        entry.instruction.opcode,
        entry.instruction.flags,
        [
            entry.instruction.a,
            entry.instruction.b,
            entry.instruction.c,
        ],
    ) && control_stays_in_region(expected.control_operands(entry.instruction), start, end, pc)
}

fn entry_matches_region_with_terminal_exits(
    entry: &BaselineEntry,
    expected: crate::ir::Opcode,
    start: usize,
    end: usize,
    code_end: usize,
) -> bool {
    expected.operands_match_physical_contract_with_flags(
        entry.instruction.opcode,
        entry.instruction.flags,
        [
            entry.instruction.a,
            entry.instruction.b,
            entry.instruction.c,
        ],
    ) && control_stays_in_region_with_terminal_exits(
        expected.control_operands(entry.instruction),
        code_end,
    )
}

fn control_stays_in_region(
    control: crate::ir::ControlOperands,
    start: usize,
    end: usize,
    pc: usize,
) -> bool {
    match control {
        crate::ir::ControlOperands::Return { .. } | crate::ir::ControlOperands::Throw { .. } => {
            pc + 1 == end
        }
        crate::ir::ControlOperands::Branch { target, .. }
        | crate::ir::ControlOperands::Jump { target } => {
            (start..=end).contains(&usize::from(target))
        }
        // A structured loop is itself a complete canonical gateway. It has
        // no bytecode successor; admit it only as a one-operation region so
        // larger regions cannot skip its owned continuation state.
        crate::ir::ControlOperands::Loop { .. } => start == pc && pc + 1 == end,
        crate::ir::ControlOperands::Next => true,
    }
}

fn control_stays_in_region_with_terminal_exits(
    control: crate::ir::ControlOperands,
    code_end: usize,
) -> bool {
    match control {
        crate::ir::ControlOperands::Return { .. } | crate::ir::ControlOperands::Throw { .. } => {
            true
        }
        crate::ir::ControlOperands::Branch { target, .. }
        | crate::ir::ControlOperands::Jump { target } => {
            // A generic region may hand a valid edge to any other canonical
            // PC, including a helper boundary or an enclosing loop. The CFG
            // fact rejects malformed targets; the region's exclusive end
            // remains the local sentinel for exits at its boundary.
            usize::from(target) <= code_end
        }
        // Structured loop state belongs to its canonical gateway.  A generic
        // region may not swallow that gateway and then continue with bytes
        // after it; the one-operation path remains handled by `region_plan`.
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
        crate::ir::ControlOperands::Return { .. }
        | crate::ir::ControlOperands::Throw { .. }
        | crate::ir::ControlOperands::Loop { .. } => {
            return Successors {
                explicit_control: true,
                ..Successors::none()
            };
        }
        crate::ir::ControlOperands::Next => return Successors::none(),
    };
    Successors {
        pcs,
        len: 1,
        control_len: 1,
        explicit_control: !matches!(entry.control, crate::ir::ControlOperands::Next),
        // The exclusive code-end sentinel is a valid normal exit. Targets
        // beyond it remain malformed and retain canonical fallback.
        malformed: pcs[0] > entries.len(),
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
        explicit_control: true,
        // A branch may target the exclusive code-end sentinel to complete the
        // current fragment normally. Only a target beyond that sentinel is
        // malformed.
        malformed: target > len,
    }
}

#[cfg(test)]
#[path = "stencil_cfg_tests.rs"]
mod tests;
