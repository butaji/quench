//! Dataflow trees of bounded depth rooted at each instruction: operands defined in the same
//! block and used once, so the whole tree can compute without materializing its interior.
use crate::dataflow::Flow;
use crate::model::{MAX_TREE_DEPTH, MAX_TREE_NODES, MIN_TREE_NODES, Mined, Shape};
use crate::ngram::BlockView;
use crate::weights::Weights;

fn grow(flow: &Flow, node: usize, depth: usize, nodes: &mut Vec<usize>) {
    nodes.push(node);
    if depth == MAX_TREE_DEPTH {
        return;
    }
    for &producer in &flow.links_in[node] {
        if flow.single_use.contains(producer) {
            grow(flow, producer, depth + 1, nodes);
        }
    }
}

pub fn mine(block: &BlockView, flow: &Flow, weights: &Weights, mined: &mut Mined) {
    for root in 0..block.instructions.len() {
        let mut nodes = Vec::new();
        grow(flow, root, 1, &mut nodes);
        if (MIN_TREE_NODES..=MAX_TREE_NODES).contains(&nodes.len()) {
            nodes.sort_unstable();
            block.record(Shape::Tree, &nodes, weights, mined);
        }
    }
}
