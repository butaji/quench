//! The engine's one builder of webs: definitions joined where one value of a register meets
//! another, as register allocators build them. Three analyses used to build their own (the Wasm
//! raw-register webs, the b-jit region's renaming and the u-jit loop region's value identities),
//! each with its own union-find and its own reaching-definitions fixpoint; they now share these.
//!
//! - `DefSets` is the union-find over definition nodes.
//! - `solve` runs forward reaching definitions over rows of a flat region whose states are bit
//!   rows (`bitset::BitRows`): each row kills and generates its own bits, and control goes to a
//!   row or leaves to a PC outside, whose incoming states it keeps as well.

use crate::bitset::BitRows;

/// Union-find over definition nodes, numbered from 0.
#[derive(Clone, Debug, Default)]
pub struct DefSets {
    parent: Vec<u32>,
}

impl DefSets {
    /// `count` nodes, each its own set.
    pub fn new(count: usize) -> Self {
        Self { parent: (0..count as u32).collect() }
    }

    /// A new node, its own set.
    pub(crate) fn add(&mut self) -> usize {
        let node = self.parent.len();
        self.parent.push(node as u32);
        node
    }

    /// The nodes there are.
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    /// The root of `node`'s set, without shortening its path.
    pub fn find(&self, mut node: usize) -> usize {
        while self.parent[node] as usize != node {
            node = self.parent[node] as usize;
        }
        node
    }

    /// The root of `node`'s set, halving its path on the way.
    pub fn root(&mut self, mut node: usize) -> usize {
        while self.parent[node] as usize != node {
            let grand = self.parent[self.parent[node] as usize];
            self.parent[node] = grand;
            node = grand as usize;
        }
        node
    }

    /// Joins the sets of `keep` and `other`; `keep`'s root stays the root. The roots joined
    /// (kept, absorbed), or `None` where they were one set already.
    pub fn union(&mut self, keep: usize, other: usize) -> Option<(usize, usize)> {
        let (keep, other) = (self.root(keep), self.root(other));
        (keep != other).then(|| {
            self.parent[other] = keep as u32;
            (keep, other)
        })
    }

    /// Joins every node of `nodes` into the first one's set; the first, if any.
    pub fn join_all(&mut self, nodes: impl IntoIterator<Item = usize>) -> Option<usize> {
        let mut nodes = nodes.into_iter();
        let first = nodes.next()?;
        for other in nodes {
            self.union(first, other);
        }
        Some(first)
    }
}

/// Where control goes after a row: a row of the region, or a PC outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Row(usize),
    Leave(u32),
}

/// The definitions reaching each row, and each PC outside the region control leaves to.
pub struct Reaching {
    pub(crate) rows: BitRows,
    /// The PCs outside the region control leaves to, ascending.
    pub leaf_pcs: Vec<u32>,
    /// Per leaf PC (by its index in `leaf_pcs`), the definitions reaching it.
    pub leaves: BitRows,
    /// Per row, whether control from the first row reaches it.
    pub reached: Vec<bool>,
}

impl Reaching {
    /// The definitions reaching row `row`.
    pub fn row(&self, row: usize) -> &[u64] {
        self.rows.row(row)
    }
}

/// Whether `bit` is set in `words`.
pub fn has(words: &[u64], bit: usize) -> bool {
    crate::bitset::words_contain(words, bit)
}

/// Sets `bit` in `words`.
pub fn set(words: &mut [u64], bit: usize) {
    words[bit / crate::bitset::WORD_BITS] |= 1 << (bit % crate::bitset::WORD_BITS);
}

/// The set bits of `words`, ascending.
pub fn members(words: &[u64]) -> impl Iterator<Item = usize> + '_ {
    words.iter().enumerate().flat_map(|(index, word)| crate::bitset::word_members(index, *word))
}

/// Forward reaching definitions over `count` rows of states `bits` wide. Row 0 is entered with
/// `seed`; `transfer` turns a row's incoming state into its outgoing one (its kills and
/// definitions), and `successors` names where control goes after a row. Where `reseed` holds, a
/// row control never reaches is taken as entered with `seed` (its definitions must exist though
/// they never run), and so are the rows it reaches; `reached` stays false for them.
pub fn solve(
    count: usize,
    bits: usize,
    seed: &[u64],
    transfer: &dyn Fn(usize, &mut [u64]),
    successors: &dyn Fn(usize, &mut dyn FnMut(Next)),
    reseed: bool,
) -> Reaching {
    let (leaf_pcs, edges, leaves_of) = control(count, successors);
    let mut flow = Reaching { rows: BitRows::new(count, bits), leaves: BitRows::new(leaf_pcs.len(), bits), leaf_pcs, reached: Vec::new() };
    if count == 0 {
        return flow;
    }
    let (seeds, reached) = entries(&edges, seed, reseed);
    flow.reached = reached;
    let states = crate::dataflow::solve(&mut Reach { edges, seeds, step: transfer });
    for (row, state) in states.iter().enumerate() {
        let Some(state) = state else { continue };
        flow.rows.join(row, state);
        let mut out = state.clone();
        transfer(row, &mut out);
        for &leaf in &leaves_of[row] {
            flow.leaves.join(leaf, &out);
        }
    }
    flow
}

/// The PCs control leaves the rows to, ascending and distinct; each row's edges to rows; and each row's leaves, as
/// indices into those PCs.
fn control(count: usize, successors: &dyn Fn(usize, &mut dyn FnMut(Next))) -> (Vec<u32>, Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut leaf_pcs = Vec::new();
    for row in 0..count {
        successors(row, &mut |next| if let Next::Leave(pc) = next { leaf_pcs.push(pc) });
    }
    leaf_pcs.sort_unstable();
    leaf_pcs.dedup();
    let (mut edges, mut leaves_of) = (vec![Vec::new(); count], vec![Vec::new(); count]);
    for row in 0..count {
        successors(row, &mut |next| match next {
            Next::Row(next) => edges[row].push(next),
            Next::Leave(pc) => leaves_of[row].push(leaf_pcs.binary_search(&pc).expect("a collected leave PC")),
        });
    }
    (leaf_pcs, edges, leaves_of)
}

/// The seed of each row the fixpoint starts from, and the rows row 0 reaches. Row 0 is entered with `seed`; where
/// `reseed` holds, the rows no earlier entry reaches are entered with it, in row order.
fn entries(edges: &[Vec<usize>], seed: &[u64], reseed: bool) -> (Vec<Option<Vec<u64>>>, Vec<bool>) {
    let mut seeds: Vec<Option<Vec<u64>>> = vec![None; edges.len()];
    seeds[0] = Some(seed.to_vec());
    let mut visited = vec![false; edges.len()];
    mark(edges, 0, &mut visited);
    let reached = visited.clone();
    for row in 0..edges.len() {
        if reseed && !visited[row] {
            seeds[row] = Some(seed.to_vec());
            mark(edges, row, &mut visited);
        }
    }
    (seeds, reached)
}

/// Marks `start` and every row control reaches from it in `marked`.
fn mark(edges: &[Vec<usize>], start: usize, marked: &mut [bool]) {
    let mut pending = vec![start];
    while let Some(row) = pending.pop() {
        if !marked[row] {
            marked[row] = true;
            pending.extend(edges[row].iter().copied());
        }
    }
}

/// Reaching definitions as a forward `Dataflow` problem over the rows: a row's state is its bit row, its edges are
/// the rows control reaches from it (a leave is no edge), and its seed is its entry row where it has one.
struct Reach<'a> {
    edges: Vec<Vec<usize>>,
    seeds: Vec<Option<Vec<u64>>>,
    /// A row's kills and definitions, applied to its incoming state.
    step: &'a dyn Fn(usize, &mut [u64]),
}

impl crate::dataflow::Dataflow for Reach<'_> {
    type State = Vec<u64>;

    fn blocks(&self) -> usize {
        self.edges.len()
    }

    fn edges(&self, block: usize) -> &[usize] {
        &self.edges[block]
    }

    fn entry_state(&self, block: usize) -> Option<Vec<u64>> {
        self.seeds[block].clone()
    }

    fn join(&self, into: &mut Vec<u64>, incoming: &Vec<u64>) -> bool {
        let mut grew = false;
        for (word, incoming) in into.iter_mut().zip(incoming) {
            let next = *word | incoming;
            grew |= next != *word;
            *word = next;
        }
        grew
    }

    fn transfer(&mut self, block: usize, state: &Vec<u64>) -> Vec<u64> {
        let mut out = state.clone();
        (self.step)(block, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_keeps_the_first_root() {
        let mut sets = DefSets::new(4);
        assert_eq!(sets.union(2, 3), Some((2, 3)));
        assert_eq!(sets.union(1, 3), Some((1, 2)));
        assert_eq!(sets.find(3), 1);
        assert_eq!(sets.union(3, 2), None);
        assert_eq!(sets.join_all([0, 1]), Some(0));
        assert_eq!(sets.root(3), 0);
    }

    /// Two definitions of one register, at rows 1 and 2 of a loop 0 -> 1 -> 2 -> 0 that leaves
    /// from row 2 to PC 10; the entry is bit 0.
    #[test]
    fn a_loop_reaches_its_header_with_both_definitions() {
        let successors = |row: usize, next: &mut dyn FnMut(Next)| match row {
            0 => next(Next::Row(1)),
            1 => next(Next::Row(2)),
            _ => { next(Next::Row(0)); next(Next::Leave(10)); }
        };
        let transfer = |row: usize, state: &mut [u64]| if row > 0 { state[0] = 1 << row };
        let flow = solve(4, 3, &[1], &transfer, &successors, false);
        assert_eq!(flow.row(0), &[0b101]);
        assert_eq!(flow.row(1), &[0b101]);
        assert_eq!(flow.row(2), &[0b010]);
        assert_eq!((flow.leaf_pcs.as_slice(), flow.leaves.row(0)), (&[10][..], &[0b100][..]));
        assert_eq!(flow.reached, [true, true, true, false]);
        let reseeded = solve(4, 3, &[1], &transfer, &successors, true);
        assert_eq!(reseeded.row(3), &[1]);
        assert!(!reseeded.reached[3]);
        assert_eq!(members(reseeded.row(0)).collect::<Vec<_>>(), [0, 2]);
    }
}
