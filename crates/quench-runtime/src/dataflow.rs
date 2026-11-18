//! The one fixpoint driver of the fact analyses: a worklist over basic blocks. A block's output
//! is joined into the input of each block it flows into, and a block runs again only when its
//! input grew. Forward analyses name their successors as `edges`, backward ones their
//! predecessors.

use cranelift_codegen::bitset::CompoundBitSet;

/// A dataflow problem over `blocks()` basic blocks.
pub(crate) trait Dataflow {
    /// The state on entry to a block, or the output of one.
    type State: Clone + PartialEq;
    /// The number of blocks.
    fn blocks(&self) -> usize;
    /// The blocks a block's output flows into: its successors (forward) or predecessors (backward).
    fn edges(&self, block: usize) -> &[usize];
    /// The input a block starts with before any output reaches it, or `None` where nothing
    /// seeds it (it runs only once an output reaches it).
    fn entry_state(&self, block: usize) -> Option<Self::State>;
    /// Joins `incoming` into `into`; true when `into` changed.
    fn join(&self, into: &mut Self::State, incoming: &Self::State) -> bool;
    /// The output of `block` for the input `state`. May record per-instruction states.
    fn transfer(&mut self, block: usize, state: &Self::State) -> Self::State;
}

/// Solves `analysis` to its fixpoint and returns each block's input state, `None` where no
/// seed or output reaches the block. Blocks run lowest index first; the queue is a bitset, so
/// a block waits in it at most once.
pub(crate) fn solve<D: Dataflow>(analysis: &mut D) -> Vec<Option<D::State>> {
    let count = analysis.blocks();
    let mut states: Vec<Option<D::State>> = (0..count).map(|block| analysis.entry_state(block)).collect();
    let mut work = CompoundBitSet::with_capacity(count);
    for (block, state) in states.iter().enumerate() {
        if state.is_some() { work.insert(block); }
    }
    while let Some(block) = work.iter().next() {
        work.remove(block);
        let Some(state) = states[block].as_ref() else { continue };
        let out = analysis.transfer(block, state);
        for &target in analysis.edges(block) {
            let changed = match &mut states[target] {
                Some(known) => analysis.join(known, &out),
                slot => {
                    *slot = Some(out.clone());
                    true
                }
            };
            if changed { work.insert(target); }
        }
    }
    states
}
