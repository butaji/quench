//! The one fixpoint driver of the fact analyses: a worklist over basic blocks. A block's output
//! is joined into the input of each block it flows into, and a block runs again only when its
//! input grew. Forward analyses name their successors as `edges`, backward ones their
//! predecessors.

/// A bitset of `usize` indices, the analyses' state: one bit per value or slot, iterated in
/// index order.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct BitSet { words: Vec<u64> }

impl BitSet {
    pub fn new() -> Self { Self::default() }
    pub fn with_capacity(count: usize) -> Self { Self { words: vec![0; count.div_ceil(u64::BITS as usize)] } }
    fn word(&mut self, index: usize) -> &mut u64 {
        let word = index / u64::BITS as usize;
        if word >= self.words.len() { self.words.resize(word + 1, 0); }
        &mut self.words[word]
    }
    pub fn insert(&mut self, index: usize) -> bool { let bit = 1 << (index % u64::BITS as usize); let word = self.word(index); let was = *word & bit != 0; *word |= bit; !was }
    pub fn remove(&mut self, index: usize) -> bool { let bit = 1 << (index % u64::BITS as usize); let was = self.words.get(index / u64::BITS as usize).is_some_and(|word| word & bit != 0); if let Some(word) = self.words.get_mut(index / u64::BITS as usize) { *word &= !bit; } was }
    pub fn contains(&self, index: usize) -> bool { self.words.get(index / u64::BITS as usize).is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0) }
    pub fn clear(&mut self) { self.words.fill(0); }
    pub fn len(&self) -> usize { self.words.iter().map(|word| word.count_ones() as usize).sum() }
    pub fn is_empty(&self) -> bool { self.words.iter().all(|word| *word == 0) }
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        let (mut word, mut bits): (usize, u64) = (0, 0);
        std::iter::from_fn(move || loop {
            if bits != 0 { let bit = bits.trailing_zeros() as usize; bits &= bits - 1; return Some(word * u64::BITS as usize + bit); }
            bits = *self.words.get(word)?; word += 1;
        })
    }
}

/// The worklist as a bitset: one bit per block, so a block waits in it at most once and the
/// lowest waiting block runs first.
struct WorkSet { words: Vec<u64> }

impl WorkSet {
    fn with_capacity(count: usize) -> Self { Self { words: vec![0; count.div_ceil(u64::BITS as usize)] } }
    fn insert(&mut self, block: usize) { self.words[block / u64::BITS as usize] |= 1 << (block % u64::BITS as usize); }
    fn remove(&mut self, block: usize) { self.words[block / u64::BITS as usize] &= !(1 << (block % u64::BITS as usize)); }
    fn next(&self) -> Option<usize> {
        self.words.iter().enumerate().find(|(_, word)| **word != 0)
            .map(|(index, word)| index * u64::BITS as usize + word.trailing_zeros() as usize)
    }
}
/// A dataflow problem over `blocks()` basic blocks.
pub trait Dataflow {
    /// The state on entry to a block, or the output of one.
    type State: Clone + PartialEq;
    /// The number of blocks.
    fn blocks(&self) -> usize;
    /// The blocks a block's output flows into: its successors (forward) or predecessors (backward).
    /// `solve` reads a block's edges right after its `transfer`, so an analysis whose control
    /// depends on the state may name the edges its last `transfer` of the block took.
    fn edges(&self, block: usize) -> &[usize];
    /// The input a block starts with before any output reaches it, or `None` where nothing
    /// seeds it (it runs only once an output reaches it).
    fn entry_state(&self, block: usize) -> Option<Self::State>;
    /// Joins `incoming` into `into`; true when `into` changed.
    fn join(&self, into: &mut Self::State, incoming: &Self::State) -> bool;
    /// The output of `block` for the input `state`. May record per-instruction states.
    fn transfer(&mut self, block: usize, state: &Self::State) -> Self::State;
    /// The state `block` sends to its edge `target`, given its output `out`. Analyses that refine
    /// a state per edge (a branch's taken side) override this; the default passes `out` as is.
    fn transfer_edge(&self, _block: usize, _target: usize, out: &Self::State) -> Self::State {
        out.clone()
    }
    /// The state `block` sends along its `index`th edge, read right after its `transfer`. An
    /// analysis whose block reaches one target by several edges in different states (exception
    /// edges out of the middle of a block, recorded by `transfer`) overrides this; the default is
    /// `transfer_edge` to the edge's target.
    fn transfer_edge_at(&self, block: usize, index: usize, out: &Self::State) -> Self::State {
        self.transfer_edge(block, self.edges(block)[index], out)
    }
    /// The input of `block` when the first output reaches it, from that output `incoming`. An
    /// analysis that adjusts a block's input whenever it joins (a contract it keeps, a widening it
    /// counts) adjusts the first one here; the default takes `incoming` as is.
    fn admit(&self, _block: usize, incoming: Self::State) -> Self::State {
        incoming
    }
}

/// Solves `analysis` to its fixpoint and returns each block's input state, `None` where no
/// seed or output reaches the block. Blocks run lowest index first; the queue is a bitset, so
/// a block waits in it at most once.
pub fn solve<D: Dataflow>(analysis: &mut D) -> Vec<Option<D::State>> {
    let count = analysis.blocks();
    let mut states: Vec<Option<D::State>> = (0..count).map(|block| analysis.entry_state(block)).collect();
    let mut work = WorkSet::with_capacity(count);
    for (block, state) in states.iter().enumerate() {
        if state.is_some() { work.insert(block); }
    }
    while let Some(block) = work.next() {
        work.remove(block);
        let Some(state) = states[block].as_ref() else { continue };
        let out = analysis.transfer(block, state);
        for index in 0..analysis.edges(block).len() {
            let target = analysis.edges(block)[index];
            let edge = analysis.transfer_edge_at(block, index, &out);
            let changed = match &mut states[target] {
                Some(known) => analysis.join(known, &edge),
                slot => {
                    *slot = Some(analysis.admit(target, edge));
                    true
                }
            };
            if changed { work.insert(target); }
        }
    }
    states
}
