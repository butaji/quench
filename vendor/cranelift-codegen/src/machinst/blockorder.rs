//! Computation of basic block order in emitted code.
//!
//! This module handles the translation from CLIF BBs to VCode BBs.
//!
//! The basic idea is that we compute a sequence of "lowered blocks" that
//! correspond to one or more blocks in the graph: (CLIF CFG) `union` (implicit
//! block on *every* edge). Conceptually, the lowering pipeline wants to insert
//! moves for phi-nodes on every block-to-block transfer; these blocks always
//! conceptually exist, but may be merged with an "original" CLIF block (and
//! hence not actually exist; this is equivalent to inserting the blocks only on
//! critical edges).
//!
//! In other words, starting from a CFG like this (where each "CLIF block" and
//! "(edge N->M)" is a separate basic block):
//!
//! ```plain
//!
//!              CLIF block 0
//!               /           \
//!       (edge 0->1)         (edge 0->2)
//!              |                |
//!       CLIF block 1         CLIF block 2
//!              \                /
//!           (edge 1->3)   (edge 2->3)
//!                   \      /
//!                 CLIF block 3
//! ```
//!
//! We can produce a CFG of lowered blocks like so:
//!
//! ```plain
//!            +--------------+
//!            | CLIF block 0 |
//!            +--------------+
//!               /           \
//!     +--------------+     +--------------+
//!     | (edge 0->1)  |     | (edge 0->2)  |
//!     | CLIF block 1 |     | CLIF block 2 |
//!     | (edge 1->3)  |     | (edge 2->3)  |
//!     +--------------+     +--------------+
//!                \           /
//!                 \         /
//!                +------------+
//!                |CLIF block 3|
//!                +------------+
//! ```
//!
//! Each `LoweredBlock` names just an original CLIF block, or just an edge block.
//!
//! To compute this lowering, we do a DFS over the CLIF-plus-edge-block graph
//! (never actually materialized, just defined by a "successors" function), and
//! compute the reverse postorder.
//!
//! This algorithm isn't perfect w.r.t. generated code quality: we don't, for
//! example, consider any information about whether edge blocks will actually
//! have content, because this computation happens as part of lowering *before*
//! regalloc, and regalloc may or may not insert moves/spills/reloads on any
//! particular edge. But it works relatively well and is conceptually simple.
//! Furthermore, the [MachBuffer] machine-code sink performs final peephole-like
//! branch editing that in practice elides empty blocks and simplifies some of
//! the other redundancies that this scheme produces.

use crate::dominator_tree::DominatorTree;
use crate::bitset::CompoundBitSet;
use crate::entity::{EntitySet, SecondaryMap};
use crate::flowgraph::ControlFlowGraph;
use crate::inst_predicates::visit_block_succs;
use crate::ir::{Block, Function, Inst, Opcode};
use crate::loop_analysis::{Loop, LoopAnalysis};
use crate::{FxHashMap, FxHashSet};
use crate::{machinst::*, trace};

/// Keep small function schedules inline; larger CFGs spill without a size limit.
const INLINE_EMISSION_BLOCKS: usize = 16;

/// Mapping from CLIF BBs to VCode BBs.
#[derive(Debug)]
pub struct BlockLoweringOrder {
    /// Lowered blocks, in BlockIndex order. Each block is some combination of
    /// (i) a CLIF block, and (ii) inserted crit-edge blocks before or after;
    /// see [LoweredBlock] for details.
    lowered_order: Vec<LoweredBlock>,
    /// BlockIndex values for successors for all lowered blocks, indexing `lowered_order`.
    lowered_succ_indices: Vec<BlockIndex>,
    /// Ranges in `lowered_succ_indices` giving the successor lists for each lowered
    /// block. Indexed by lowering-order index (`BlockIndex`).
    lowered_succ_ranges: Vec<(Option<Inst>, core::ops::Range<usize>)>,
    /// BlockIndex for each original Block.
    blockindex_by_block: SecondaryMap<Block, BlockIndex>,
    /// Cold blocks. These blocks are not reordered in the
    /// `lowered_order` above; the lowered order must respect RPO
    /// (uses after defs) in order for lowering to be
    /// correct. Instead, this set is used to provide `is_cold()`,
    /// which is used by VCode emission to sink the blocks at the last
    /// moment (when we actually emit bytes into the MachBuffer).
    cold_blocks: FxHashSet<BlockIndex>,
    /// Lowered blocks that are indirect branch targets.
    indirect_branch_targets: FxHashSet<BlockIndex>,
    /// Physical layout may rotate loops after lowering and register allocation.
    emission_order: SmallVec<[BlockIndex; INLINE_EMISSION_BLOCKS]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoweredBlock {
    /// Block in original CLIF.
    Orig {
        /// Original CLIF block.
        block: Block,
    },

    /// Critical edge between two CLIF blocks.
    CriticalEdge {
        /// The predecessor block.
        pred: Block,

        /// The successor block.
        succ: Block,

        /// The index of this branch in the successor edges from `pred`, following the same
        /// indexing order as `inst_predicates::visit_block_succs`. This is used to distinguish
        /// multiple edges between the same CLIF blocks.
        succ_idx: u32,
    },
}

impl LoweredBlock {
    /// Unwrap an `Orig` block.
    pub fn orig_block(&self) -> Option<Block> {
        match self {
            &LoweredBlock::Orig { block } => Some(block),
            &LoweredBlock::CriticalEdge { .. } => None,
        }
    }

    /// The associated in-edge predecessor, if this is a critical edge.
    #[cfg(test)]
    pub fn in_edge(&self) -> Option<Block> {
        match self {
            &LoweredBlock::CriticalEdge { pred, .. } => Some(pred),
            &LoweredBlock::Orig { .. } => None,
        }
    }

    /// The associated out-edge successor, if this is a critical edge.
    #[cfg(test)]
    pub fn out_edge(&self) -> Option<Block> {
        match self {
            &LoweredBlock::CriticalEdge { succ, .. } => Some(succ),
            &LoweredBlock::Orig { .. } => None,
        }
    }
}

impl BlockLoweringOrder {
    /// Compute and return a lowered block order for `f`.
    pub fn new(
        f: &Function,
        domtree: &DominatorTree,
        ctrl_plane: &mut ControlPlane,
    ) -> BlockLoweringOrder {
        trace!("BlockLoweringOrder: function body {:?}", f);

        // Step 1: compute the in-edge and out-edge count of every block.
        let mut block_in_count = SecondaryMap::with_default(0);
        let mut block_out_count = SecondaryMap::with_default(0);

        // Block successors are stored as `LoweredBlocks` to simplify the construction of
        // `lowered_succs` in the final result. Initially, all entries are `Orig` values, and are
        // updated to be `CriticalEdge` when those cases are identified in step 2 below.
        let mut block_succs: SmallVec<[LoweredBlock; 128]> = SmallVec::new();
        let mut block_succ_range = SecondaryMap::with_default(0..0);

        let mut indirect_branch_target_clif_blocks = FxHashSet::default();

        for block in f.layout.blocks() {
            let start = block_succs.len();
            visit_block_succs(f, block, |_, succ, from_table| {
                block_out_count[block] += 1;
                block_in_count[succ] += 1;
                block_succs.push(LoweredBlock::Orig { block: succ });

                if from_table {
                    indirect_branch_target_clif_blocks.insert(succ);
                }
            });

            // Ensure that blocks terminated by br_table instructions
            // with an empty jump table are still treated like
            // conditional blocks from the point of view of critical
            // edge splitting. Also do the same for TryCall and
            // TryCallIndirect: we cannot have edge moves before the
            // branch, even if they have empty handler tables and thus
            // would otherwise have only one successor.
            if let Some(inst) = f.layout.last_inst(block) {
                match f.dfg.insts[inst].opcode() {
                    Opcode::BrTable | Opcode::TryCall | Opcode::TryCallIndirect => {
                        block_out_count[block] = block_out_count[block].max(2);
                    }
                    _ => {}
                }
            }

            let end = block_succs.len();
            block_succ_range[block] = start..end;
        }

        // Step 2: walk the postorder from the domtree in reverse to produce our desired node
        // lowering order, identifying critical edges to split along the way.

        let mut lowered_order = Vec::new();
        let mut blockindex_by_block = SecondaryMap::with_default(BlockIndex::invalid());
        for &block in domtree.cfg_rpo() {
            let idx = BlockIndex::new(lowered_order.len());
            lowered_order.push(LoweredBlock::Orig { block });
            blockindex_by_block[block] = idx;

            if block_out_count[block] > 1 {
                let range = block_succ_range[block].clone();

                // If chaos-mode is enabled in the control plane, iterate over
                // the successors in an arbitrary order, which should have no
                // impact on correctness. The order of the blocks is generally
                // relevant: Uses must be seen before defs for dead-code
                // elimination.
                let succs = ctrl_plane.shuffled(block_succs[range].iter_mut().enumerate());

                for (succ_ix, lb) in succs {
                    let succ = lb.orig_block().unwrap();
                    if block_in_count[succ] > 1 {
                        // Mutate the successor to be a critical edge, as `block` has multiple
                        // edges leaving it, and `succ` has multiple edges entering it.
                        *lb = LoweredBlock::CriticalEdge {
                            pred: block,
                            succ,
                            succ_idx: succ_ix as u32,
                        };
                        lowered_order.push(*lb);
                    }
                }
            }
        }

        let lb_to_bindex = FxHashMap::from_iter(
            lowered_order
                .iter()
                .enumerate()
                .map(|(i, &lb)| (lb, BlockIndex::new(i))),
        );

        // Step 3: build the successor tables given the lowering order. We can't perform this step
        // during the creation of `lowering_order`, as we need `lb_to_bindex` to be fully populated
        // first.
        let mut lowered_succ_indices = Vec::new();
        let mut cold_blocks = FxHashSet::default();
        let mut indirect_branch_targets = FxHashSet::default();
        let lowered_succ_ranges =
            Vec::from_iter(lowered_order.iter().enumerate().map(|(ix, lb)| {
                let bindex = BlockIndex::new(ix);
                let start = lowered_succ_indices.len();
                let opt_inst = match lb {
                    // Block successors are pulled directly over, as they'll have been mutated when
                    // determining the block order already.
                    &LoweredBlock::Orig { block } => {
                        let range = block_succ_range[block].clone();
                        lowered_succ_indices
                            .extend(block_succs[range].iter().map(|lb| lb_to_bindex[lb]));

                        if f.is_effectively_cold(block) {
                            cold_blocks.insert(bindex);
                        }

                        if indirect_branch_target_clif_blocks.contains(&block) {
                            indirect_branch_targets.insert(bindex);
                        }

                        let last = f.layout.last_inst(block).unwrap();
                        let opcode = f.dfg.insts[last].opcode();

                        assert!(opcode.is_terminator());

                        opcode.is_branch().then_some(last)
                    }

                    // Critical edges won't have successor information in block_succ_range, but
                    // they only have a single known successor to record anyway.
                    &LoweredBlock::CriticalEdge { succ, .. } => {
                        let succ_index = lb_to_bindex[&LoweredBlock::Orig { block: succ }];
                        lowered_succ_indices.push(succ_index);

                        // Edges inherit indirect branch and cold block metadata from their
                        // successor.

                        if f.is_effectively_cold(succ) {
                            cold_blocks.insert(bindex);
                        }

                        if indirect_branch_target_clif_blocks.contains(&succ) {
                            indirect_branch_targets.insert(bindex);
                        }

                        None
                    }
                };
                let end = lowered_succ_indices.len();
                (opt_inst, start..end)
            }));

        let mut result = BlockLoweringOrder {
            lowered_order,
            lowered_succ_indices,
            lowered_succ_ranges,
            blockindex_by_block,
            cold_blocks,
            indirect_branch_targets,
            emission_order: SmallVec::new(),
        };
        result.emission_order = result.plan_emission(f, domtree);

        trace!("BlockLoweringOrder: {:#?}", result);
        result
    }

    /// Rotate simple natural-loop cycles so a conditional closes the cycle.
    /// This removes a taken unconditional branch on the repeating path. RPO
    /// remains authoritative for lowering; only final label placement changes.
    fn plan_emission(&self, f: &Function, domtree: &DominatorTree)
        -> SmallVec<[BlockIndex; INLINE_EMISSION_BLOCKS]>
    {
        let cfg = ControlFlowGraph::with_function(f);
        let mut loops = LoopAnalysis::new();
        loops.compute(f, &cfg, domtree);
        let membership: Vec<_> = self.lowered_order.iter().map(|block| match *block {
            LoweredBlock::Orig { block } => loops.innermost_loop(block),
            LoweredBlock::CriticalEdge { pred, succ, .. } => loops.innermost_loop(pred)
                .filter(|lp| loops.is_in_loop(succ, *lp)),
        }).collect();
        let mut sizes = SecondaryMap::<Loop, usize>::new();
        let mut tops = SecondaryMap::<Loop, Option<BlockIndex>>::new();
        let mut exits = SecondaryMap::<Loop, Option<BlockIndex>>::new();
        let mut blocked = EntitySet::<Loop>::new();
        let mut next = vec![None; self.lowered_order.len()];
        for (index, lp) in membership.iter().copied().enumerate() {
            let Some(lp) = lp else { continue; };
            sizes[lp] += 1;
            let block = BlockIndex::new(index);
            if index == 0 || self.is_cold(block) { blocked.insert(lp); }
            let (terminator, successors) = self.succ_indices(block);
            let mut internal = successors.iter().copied()
                .filter(|succ| membership[succ.index()] == Some(lp));
            next[index] = internal.next();
            if next[index].is_none() || internal.next().is_some() { blocked.insert(lp); }
            if successors.len() == 2 && terminator.is_some_and(|inst|
                f.dfg.insts[inst].opcode() == Opcode::Brif) {
                if let Some(exit) = successors.iter().copied().find(|succ|
                    membership[succ.index()] != Some(lp) && !self.is_cold(*succ)) {
                    tops[lp] = next[index];
                    exits[lp] = Some(exit);
                }
            }
        }
        for lp in loops.loops() {
            let Some(top) = tops[lp] else { continue; };
            let mut current = Some(top);
            // The walk must visit every member exactly once before closing.
            // Its bound is the actual CFG size, not a compilation budget.
            for position in 0..sizes[lp] {
                current = current.and_then(|block| next[block.index()]);
                if current.is_none() || (current == Some(top) && position + 1 < sizes[lp]) {
                    current = None;
                    break;
                }
            }
            if blocked.contains(lp) || current != Some(top) {
                tops[lp] = None;
            }
        }
        let mut emitted = CompoundBitSet::<usize>::with_capacity(self.lowered_order.len());
        let mut order = SmallVec::new();
        for (index, lp) in membership.iter().copied().enumerate() {
            let block = BlockIndex::new(index);
            if self.is_cold(block) || emitted.contains(index) { continue; }
            if let Some(lp) = lp && let Some(top) = tops[lp] {
                let exit = exits[lp].expect("conditional cycle exit");
                if !emitted.contains(exit.index()) {
                    let mut current = top;
                    for _ in 0..sizes[lp] {
                        order.push(current);
                        emitted.insert(current.index());
                        current = next[current.index()].expect("verified loop cycle");
                    }
                    // The final conditional's other edge now falls through;
                    // MachBuffer can invert it and erase the hot-path jump.
                    order.push(exit);
                    emitted.insert(exit.index());
                    if let Some(owner) = membership[exit.index()] { tops[owner] = None; }
                    continue;
                }
                tops[lp] = None;
            }
            order.push(block);
            emitted.insert(index);
        }
        order.extend((0..self.lowered_order.len()).map(BlockIndex::new)
            .filter(|block| self.is_cold(*block)));
        order
    }

    /// Final machine-code placement, independent of the lowering RPO.
    pub fn emission_order(&self) -> &[BlockIndex] {
        &self.emission_order
    }

    /// Get the lowered order of blocks.
    pub fn lowered_order(&self) -> &[LoweredBlock] {
        &self.lowered_order[..]
    }

    /// Get the BlockIndex, if any, for a given Block.
    ///
    /// The result will be `None` if the given Block is unreachable
    /// (and thus does not appear in the lowered order).
    pub fn lowered_index_for_block(&self, block: Block) -> Option<BlockIndex> {
        let idx = self.blockindex_by_block[block];
        if idx.is_valid() { Some(idx) } else { None }
    }

    /// Get the successor indices for a lowered block.
    pub fn succ_indices(&self, block: BlockIndex) -> (Option<Inst>, &[BlockIndex]) {
        let (opt_inst, range) = &self.lowered_succ_ranges[block.index()];
        (*opt_inst, &self.lowered_succ_indices[range.clone()])
    }

    /// Determine whether the given lowered-block index is cold.
    pub fn is_cold(&self, block: BlockIndex) -> bool {
        self.cold_blocks.contains(&block)
    }

    /// Determine whether the given lowered block index is an indirect branch
    /// target.
    pub fn is_indirect_branch_target(&self, block: BlockIndex) -> bool {
        self.indirect_branch_targets.contains(&block)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cursor::{Cursor, FuncCursor};
    use crate::flowgraph::ControlFlowGraph;
    use crate::ir::UserFuncName;
    use crate::ir::types::*;
    use crate::ir::{AbiParam, InstBuilder, Signature};
    use crate::isa::CallConv;

    fn build_test_func(n_blocks: usize, edges: &[(usize, usize)]) -> BlockLoweringOrder {
        assert!(n_blocks > 0);

        let name = UserFuncName::testcase("test0");
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(I32));
        let mut func = Function::with_name_signature(name, sig);
        let blocks = (0..n_blocks)
            .map(|i| {
                let bb = func.dfg.make_block();
                assert!(bb.as_u32() == i as u32);
                bb
            })
            .collect::<Vec<_>>();

        let arg0 = func.dfg.append_block_param(blocks[0], I32);

        let mut pos = FuncCursor::new(&mut func);

        let mut edge = 0;
        for i in 0..n_blocks {
            pos.insert_block(blocks[i]);
            let mut succs = vec![];
            while edge < edges.len() && edges[edge].0 == i {
                succs.push(edges[edge].1);
                edge += 1;
            }
            if succs.len() == 0 {
                pos.ins().return_(&[arg0]);
            } else if succs.len() == 1 {
                pos.ins().jump(blocks[succs[0]], &[]);
            } else if succs.len() == 2 {
                pos.ins()
                    .brif(arg0, blocks[succs[0]], &[], blocks[succs[1]], &[]);
            } else {
                panic!("Too many successors");
            }
        }

        let mut cfg = ControlFlowGraph::new();
        cfg.compute(&func);
        let dom_tree = DominatorTree::with_function(&func, &cfg);

        let order = BlockLoweringOrder::new(&func, &dom_tree, &mut Default::default());
        let mut emitted: Vec<_> = order.emission_order().iter().map(|block| block.index()).collect();
        emitted.sort_unstable();
        assert_eq!(emitted, (0..order.lowered_order.len()).collect::<Vec<_>>(),
            "every lowered block must be emitted exactly once");
        assert_eq!(order.emission_order()[0], BlockIndex::new(0), "entry remains first");
        order
    }

    #[test]
    fn simple_loop_emission_closes_at_a_conditional() {
        let order = build_test_func(6, &[(0, 1), (1, 2), (1, 5), (2, 3), (2, 4), (3, 1)]);
        let source: Vec<_> = order.emission_order().iter().filter_map(|index|
            order.lowered_order[index.index()].orig_block()).map(|block| block.as_u32()).collect();
        assert_eq!(source, [0, 3, 1, 2, 4, 5], "conditional closes the cycle and exits by fallthrough");
    }

    #[test]
    fn emission_preserves_entry_nested_branching_and_irreducible_cfgs() {
        build_test_func(3, &[(0, 1), (1, 0), (1, 2)]);
        build_test_func(6, &[(0, 1), (1, 2), (1, 3), (2, 4), (3, 4), (4, 1), (4, 5)]);
        build_test_func(6, &[(0, 1), (1, 2), (1, 5), (2, 3), (2, 4), (3, 2), (4, 1)]);
        build_test_func(4, &[(0, 1), (0, 2), (1, 2), (1, 3), (2, 1), (2, 3)]);
        build_test_func(4, &[(0, 1), (1, 2), (1, 3), (2, 1), (2, 3)]);
    }

    #[test]
    fn test_blockorder_diamond() {
        let order = build_test_func(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);

        // This test case doesn't need to introduce any critical edges, as all regalloc allocations
        // can sit on either the entry or exit of blocks 1 and 2.
        assert_eq!(order.lowered_order.len(), 4);
    }

    #[test]
    fn test_blockorder_critedge() {
        //            0
        //          /   \
        //         1     2
        //        /  \     \
        //       3    4    |
        //       |\  _|____|
        //       | \/ |
        //       | /\ |
        //       5    6
        //
        // (3 -> 5, and 3 -> 6 are critical edges and must be split)
        //
        let order = build_test_func(
            7,
            &[
                (0, 1),
                (0, 2),
                (1, 3),
                (1, 4),
                (2, 5),
                (3, 5),
                (3, 6),
                (4, 6),
            ],
        );

        assert_eq!(order.lowered_order.len(), 9);
        println!("ordered = {:?}", order.lowered_order);

        // block 0
        assert_eq!(order.lowered_order[0].orig_block().unwrap().as_u32(), 0);
        assert!(order.lowered_order[0].in_edge().is_none());
        assert!(order.lowered_order[0].out_edge().is_none());

        // block 2
        assert_eq!(order.lowered_order[1].orig_block().unwrap().as_u32(), 2);
        assert!(order.lowered_order[1].in_edge().is_none());
        assert!(order.lowered_order[1].out_edge().is_none());

        // block 1
        assert_eq!(order.lowered_order[2].orig_block().unwrap().as_u32(), 1);
        assert!(order.lowered_order[2].in_edge().is_none());
        assert!(order.lowered_order[2].out_edge().is_none());

        // block 4
        assert_eq!(order.lowered_order[3].orig_block().unwrap().as_u32(), 4);
        assert!(order.lowered_order[3].in_edge().is_none());
        assert!(order.lowered_order[3].out_edge().is_none());

        // block 3
        assert_eq!(order.lowered_order[4].orig_block().unwrap().as_u32(), 3);
        assert!(order.lowered_order[4].in_edge().is_none());
        assert!(order.lowered_order[4].out_edge().is_none());

        // critical edge 3 -> 5
        assert!(order.lowered_order[5].orig_block().is_none());
        assert_eq!(order.lowered_order[5].in_edge().unwrap().as_u32(), 3);
        assert_eq!(order.lowered_order[5].out_edge().unwrap().as_u32(), 5);

        // critical edge 3 -> 6
        assert!(order.lowered_order[6].orig_block().is_none());
        assert_eq!(order.lowered_order[6].in_edge().unwrap().as_u32(), 3);
        assert_eq!(order.lowered_order[6].out_edge().unwrap().as_u32(), 6);

        // block 6
        assert_eq!(order.lowered_order[7].orig_block().unwrap().as_u32(), 6);
        assert!(order.lowered_order[7].in_edge().is_none());
        assert!(order.lowered_order[7].out_edge().is_none());

        // block 5
        assert_eq!(order.lowered_order[8].orig_block().unwrap().as_u32(), 5);
        assert!(order.lowered_order[8].in_edge().is_none());
        assert!(order.lowered_order[8].out_edge().is_none());
    }
}
