//! Straight-line n-grams within a basic block, keyed by opcode and dataflow shape.
use crate::model::{Mined, Shape, Site, MAX_NGRAM, MIN_NGRAM, pattern_of};
use crate::weights::Weights;
use quench_runtime::FlowInstruction;

/// A block's instructions and where they sit.
pub struct BlockView<'a> {
    pub function: u32,
    pub start: usize,
    pub instructions: &'a [FlowInstruction],
    pub links_in: &'a [Vec<usize>],
}

impl BlockView<'_> {
    /// Records the pattern of `nodes` (ascending block indices) as one site of `shape`; the
    /// site executes as often as its rarest instruction.
    pub fn record(&self, shape: Shape, nodes: &[usize], weights: &Weights, mined: &mut Mined) {
        let pattern = pattern_of(self.instructions, self.links_in, nodes);
        let weight = nodes
            .iter()
            .map(|&node| weights.instruction(self.instructions[node].opcode))
            .fold(f64::INFINITY, f64::min);
        let pcs = nodes.iter().map(|&node| (self.start + node) as u32).collect();
        mined.entry((shape, pattern)).or_default().push(Site { function: self.function, pcs, weight });
    }
}

pub fn mine(block: &BlockView, weights: &Weights, mined: &mut Mined) {
    for n in MIN_NGRAM..=MAX_NGRAM {
        for first in 0..(block.instructions.len() + 1).saturating_sub(n) {
            let nodes: Vec<usize> = (first..first + n).collect();
            block.record(Shape::Sequence, &nodes, weights, mined);
        }
    }
}
