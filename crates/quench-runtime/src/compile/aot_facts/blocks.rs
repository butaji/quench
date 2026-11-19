//! The basic blocks of a function: the runs of PCs its register dataflow visits as units.

use crate::bytecode::{ControlEdge, Successors};
use std::ops::Range;

/// A function's basic blocks, each a run of PCs entered at its first and left after its last.
pub(super) struct Blocks {
    /// The PCs of each block.
    pub(super) ranges: Vec<Range<usize>>,
    /// Each block's successor blocks.
    pub(super) targets: Vec<Vec<usize>>,
    /// How control reaches each of `targets`.
    pub(super) kinds: Vec<Vec<ControlEdge>>,
}

impl Blocks {
    /// The blocks of a function of `len` instructions with control flow `successors`. A block
    /// ends at every instruction that is not a single normal successor (a branch, a return, an
    /// instruction a handler covers) and at every jump target.
    pub(super) fn of(successors: &Successors, len: usize) -> Self {
        let leader = leaders(successors, len);
        let starts: Vec<usize> = (0..len).filter(|&pc| leader[pc]).collect();
        let ranges: Vec<Range<usize>> = starts
            .iter()
            .enumerate()
            .map(|(block, &start)| start..starts.get(block + 1).copied().unwrap_or(len))
            .collect();
        let mut block_of = vec![0; len];
        for (block, range) in ranges.iter().enumerate() {
            block_of[range.clone()].fill(block);
        }
        let edges_of = |range: &Range<usize>| -> (Vec<usize>, Vec<ControlEdge>) {
            successors
                .at(range.end - 1)
                .filter(|&(target, _)| target < len)
                .map(|(target, edge)| (block_of[target], edge))
                .unzip()
        };
        let (targets, kinds): (Vec<Vec<usize>>, Vec<Vec<ControlEdge>>) = ranges.iter().map(edges_of).unzip();
        Self { ranges, targets, kinds }
    }
}

/// Whether each PC (and the end, at `len`) starts a block: the entry, every jump target, and the
/// PC after each instruction that does not fall through to the next alone.
fn leaders(successors: &Successors, len: usize) -> Vec<bool> {
    let mut leader = vec![false; len + 1];
    leader[0] = true;
    for pc in 0..len {
        let edges: Vec<_> = successors.at(pc).collect();
        if edges != [(pc + 1, ControlEdge::Normal)] {
            leader[pc + 1] = true;
        }
        for (target, _) in edges {
            if let Some(entry) = leader.get_mut(target) {
                *entry = true;
            }
        }
    }
    leader
}
