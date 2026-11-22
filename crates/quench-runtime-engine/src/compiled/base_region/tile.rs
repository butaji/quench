//! Tiling: maximal munch of supernodes (`quench-bjit/stencils/supernodes.rs`) over the rows of a
//! straight-line block. At each row, left to right, the largest supernode whose pattern matches
//! the rows from there (kinds, and the dataflow edge: the first row's result is read by exactly
//! one operand of the next and is dead afterwards) replaces them by one stencil; else the row
//! stands alone.
//!
//! A tile is one `Planned` at its first row's index followed by `skipped` rows (no code) for the
//! rest, so row indices, branch targets and `link`'s tables keep one entry per row. No branch may
//! enter a tile past its first row, its rows are consecutive instructions of the caller (a tile
//! never spans an inlined callee's rows), and a tile's exit is its first row: the stencil leaves
//! with nothing written but the pinned register between its rows (dead there, and shared with no
//! web live at that PC), so the interpreter runs the covered instructions from the first.

use super::function::skipped;
use super::*;
use quench_bjit::HOLES;
use quench_bjit::numeric::supernodes::{self, Key, Node, Operand, Shape, Supernode};
use quench_runtime::bitset::BitSet;
use std::sync::OnceLock;

/// Hole roles of the operands (`numeric.rs`: 0 destination, 1 and 2 operands).
const FIRST_OPERAND_HOLE: usize = quench_bjit::numeric::FIRST as usize;
const SECOND_OPERAND_HOLE: usize = quench_bjit::numeric::SECOND as usize;

struct Candidate {
    shape: Shape,
    supernode: &'static Supernode,
}

/// The library's supernodes with their shapes, the largest first.
fn candidates() -> &'static [Candidate] {
    static LIST: OnceLock<Vec<Candidate>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut list: Vec<Candidate> = supernodes::SUPERNODES.iter().filter_map(|supernode| Some(Candidate { shape: supernodes::parse(supernode.pattern, supernodes::FAMILIES)?, supernode })).collect();
        list.sort_by_key(|candidate| std::cmp::Reverse(candidate.shape.keys.len()));
        list
    })
}

/// What a row is in the supernode vocabulary: none for a row that is a step (a call, a helper, a
/// leave), which no supernode names.
fn key_of(row: &Decoded) -> Option<Key> {
    row.step.is_none().then(|| row.heap.map_or(Key::Number(row.kind as u8), |heap| Key::Heap(heap.code())))
}

/// What the matcher asks of the block around a position.
pub(super) struct Block<'a> {
    /// Indices of the rows some branch targets.
    pub(super) targets: &'a BitSet,
    /// Whether the web a row `k` wrote is dead once row `k` has run, on every way out of it.
    pub(super) dead_after: &'a dyn Fn(usize, Register) -> bool,
}

/// The operand of `consumer` that reads the value `producer` writes; none when neither or both do.
fn fed_operand(producer: &Decoded, consumer: &Decoded) -> Option<Operand> {
    let value = producer.dst?;
    let reads = |source: Option<Source>| source.and_then(register_of) == Some(value);
    match (reads(consumer.a), reads(consumer.b)) {
        (true, false) => Some(Operand::A),
        (false, true) => Some(Operand::B),
        _ => None,
    }
}

/// The operand of the last row that reads the first row's value, when `shape` matches the rows
/// from `at`: the kinds agree, no branch enters past the first row, the first row is no branch
/// and its value is dead after the last.
pub(super) fn match_at(shape: &Shape, rows: &[Decoded], at: usize, facts: &Block) -> Option<Operand> {
    let window = rows.get(at..at + shape.keys.len())?;
    let (producer, consumer) = (window.first()?, window.last()?);
    let keyed = window.iter().zip(&shape.keys).all(|(row, key)| key_of(row) == Some(*key));
    let entered_inside = (at + 1..at + window.len()).any(|row| facts.targets.contains(row));
    let fed = fed_operand(producer, consumer)?;
    let dead = (facts.dead_after)(at + window.len() - 1, producer.dst?);
    (keyed && !entered_inside && producer.target.is_none() && dead).then_some(fed)
}

/// Whether `web` is live at none of `successors`.
fn live_nowhere(web: Register, successors: impl IntoIterator<Item = u32>, live_at: impl Fn(u32) -> Vec<usize>) -> bool {
    successors.into_iter().all(|pc| !live_at(pc).contains(&usize::from(web)))
}

/// Whether the web row `k` wrote is dead after it: live at neither its fall-through nor its branch target.
fn dead_after(layout: &Layout, decoded: &[Decoded], k: usize, web: Register) -> bool {
    let next = layout.span.header + k as u32 + 1;
    live_nowhere(web, std::iter::once(next).chain(decoded[k].target), |pc| layout.webs.live_at(&layout.span, pc))
}

/// The row indices a branch of the region targets.
fn branch_targets(layout: &Layout, decoded: &[Decoded]) -> BitSet {
    let inside = layout.span.header..=layout.span.backedge;
    let mut targets = BitSet::new(decoded.len());
    decoded.iter().filter_map(|row| row.target).filter(|pc| inside.contains(pc)).for_each(|pc| {
        targets.insert_growing((pc - layout.span.header) as usize);
    });
    targets
}

/// The second row's node with its operands exchanged where that puts the fed operand among the
/// supernode's `feeds` (only a commutative row), and whether it was exchanged.
fn oriented(node: Node, fed: Operand, feeds: &[Operand]) -> Option<(Node, bool)> {
    if feeds.contains(&fed) {
        return Some((node, false));
    }
    let Node::Number((kind, dst, a, b, speculate)) = node else { return None };
    let exchanged = Node::Number(numeric::library::canonical((kind, dst, b, a, speculate)));
    (supernodes::commutative(kind) && feeds.contains(&fed.other())).then_some((exchanged, true))
}

/// The tile's holes: each role from the first row where it reads through the role, else from the
/// second (whose operand holes are exchanged with its operands).
fn joined(first: &Planned, second: &Planned, exchanged: bool, first_roles: u8) -> [u64; HOLES] {
    let mut later = second.holes;
    if exchanged {
        later.swap(FIRST_OPERAND_HOLE, SECOND_OPERAND_HOLE);
    }
    std::array::from_fn(|role| if first_roles >> role & 1 == 1 { first.holes[role] } else { later[role] })
}

/// The tile of the two planned rows at row `pc`, or `None` where the library has no stencil for
/// their places (a general row, an operand in a frame slot, a pin shared by the tile) or either is
/// more than a stencil (a chain, a leave, a row with rows before or after it).
fn fuse(pc: u32, first: &Planned, second: &Planned, fed: Operand, supernode: &Supernode) -> Option<Planned> {
    if [first, second].iter().any(|row| !row.pre.is_empty() || !row.post.is_empty() || row.stop.is_some() || !row.owned.is_empty()) {
        return None;
    }
    let (head, tail) = (first.node?, second.node?);
    let (tail, exchanged) = oriented(tail, fed, supernode.feeds)?;
    let stencil = numeric::supernode_stencil([head, tail])?;
    let holes = joined(first, second, exchanged, supernodes::roles(&head));
    let leaves = !first.exits.is_empty() || !second.exits.is_empty();
    Some(Planned { stencil, node: None, holes, branch: second.branch, exits: if leaves { vec![Exit::Spilling(pc)] } else { Vec::new() }, pre: Vec::new(), post: Vec::new(), stop: None, owned: Vec::new(), cold: false, symbolic: NO_SYMBOLS })
}

/// Whether the `len` rows from `at` are consecutive instructions of the caller: the interpreter
/// resuming at the first runs the others.
fn caller_run(layout: &Layout, at: usize, len: usize) -> bool {
    let Some(Origin::Caller(first)) = layout.origins.get(at).copied() else { return false };
    (0..len).all(|k| layout.origins.get(at + k) == Some(&Origin::Caller(first + k as u32)))
}

/// The tile starting at row `at` and the rows it covers.
fn tile_at(layout: &Layout, decoded: &[Decoded], singles: &[Planned], facts: &Block, at: usize) -> Option<(Planned, usize)> {
    candidates().iter().find_map(|candidate| {
        let width = candidate.shape.keys.len();
        let fed = match_at(&candidate.shape, decoded, at, facts).filter(|_| caller_run(layout, at, width))?;
        let tile = fuse(at as u32, singles.get(at)?, singles.get(at + 1)?, fed, candidate.supernode)?;
        Some((tile, width))
    })
}

/// The plan of every row of the program, one entry per row: a tile at its first row, `skipped` rows
/// after it, a single row's plan elsewhere. `singles` are the rows' plans on their own.
pub(super) fn tiled(layout: &Layout, decoded: &[Decoded], mut singles: Vec<Planned>) -> Vec<Planned> {
    let targets = branch_targets(layout, decoded);
    let dead = |k: usize, web: Register| dead_after(layout, decoded, k, web);
    let facts = Block { targets: &targets, dead_after: &dead };
    let mut body = Vec::with_capacity(decoded.len());
    let mut at = 0;
    while at < decoded.len() {
        let (planned, width) = match tile_at(layout, decoded, &singles, &facts, at) {
            Some(tile) => tile,
            None => (std::mem::replace(&mut singles[at], skipped()), 1),
        };
        body.push(planned);
        body.extend((1..width).map(|_| skipped()));
        at += width;
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::padded_holes;
    use numeric::library::{ADD, AND, MUL, PIN, SUB};

    fn register(web: Register) -> Option<Source> {
        Some(Source::Register(web))
    }

    fn shape(pattern: &str) -> Shape {
        supernodes::parse(pattern, supernodes::FAMILIES).expect(pattern)
    }

    /// `d = b * c`, then `e = a + d` with the product in the given operand.
    fn mul_then_add(product_in: Operand) -> Vec<Decoded> {
        let (x, y) = if product_in == Operand::B { (register(3), register(5)) } else { (register(5), register(3)) };
        vec![Decoded::number(Kind::Mul, Some(5), register(1), register(2), None), Decoded::number(Kind::Add, Some(6), x, y, None)]
    }

    const MUL_ADD: &str = "NumericMultiply(*) ; NumericAdd(+) [0>1]";

    fn matched(pattern: &str, rows: &[Decoded], at: usize, targets: &[usize], live_after: &[(usize, Register)]) -> Option<Operand> {
        let mut set = BitSet::new(rows.len());
        targets.iter().for_each(|&row| {
            set.insert_growing(row);
        });
        let targets = set;
        let dead = |row: usize, web: Register| !live_after.contains(&(row, web));
        match_at(&shape(pattern), rows, at, &Block { targets: &targets, dead_after: &dead })
    }

    #[test]
    fn a_multiply_feeding_an_add_matches_and_names_the_fed_operand() {
        assert_eq!(matched(MUL_ADD, &mul_then_add(Operand::B), 0, &[], &[]), Some(Operand::B));
        assert_eq!(matched(MUL_ADD, &mul_then_add(Operand::A), 0, &[], &[]), Some(Operand::A));
    }

    #[test]
    fn the_kinds_and_the_dataflow_edge_must_agree() {
        let mut rows = mul_then_add(Operand::B);
        assert_eq!(matched("NumericAdd(+) ; NumericAdd(+) [0>1]", &rows, 0, &[], &[]), None);
        assert_eq!(matched("NumericMultiply(*) ; Binary(-) [0>1]", &rows, 0, &[], &[]), None);
        rows[1] = Decoded::number(Kind::Add, Some(6), register(3), register(4), None);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[]), None, "the sum does not read the product");
        rows[1] = Decoded::number(Kind::Add, Some(6), register(5), register(5), None);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[]), None, "the product is read twice");
    }

    #[test]
    fn the_first_value_must_be_dead_after_the_last_row() {
        let rows = mul_then_add(Operand::B);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[(1, 5)]), None);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[(1, 6), (0, 5)]), Some(Operand::B), "other webs and other rows do not count");
    }

    #[test]
    fn a_value_live_at_a_successor_is_not_dead() {
        let live = |pc: u32| if pc == 7 { vec![5] } else { vec![6] };
        assert!(live_nowhere(5, [8], live));
        assert!(!live_nowhere(5, [8, 7], live), "live at the branch target");
        assert!(live_nowhere(5, [], live));
    }

    #[test]
    fn no_branch_may_enter_a_tile_past_its_first_row() {
        let rows = mul_then_add(Operand::B);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[1], &[]), None);
        assert_eq!(matched(MUL_ADD, &rows, 0, &[0], &[]), Some(Operand::B), "entering at the first row is the tile's entry");
    }

    #[test]
    fn a_tile_starts_anywhere_in_the_block_and_stays_inside_it() {
        let mut rows = vec![Decoded::number(Kind::Move, Some(9), register(8), None, None)];
        rows.extend(mul_then_add(Operand::B));
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[]), None);
        assert_eq!(matched(MUL_ADD, &rows, 1, &[], &[]), Some(Operand::B));
        assert_eq!(matched(MUL_ADD, &rows, 2, &[], &[]), None, "the window runs past the block");
    }

    #[test]
    fn a_branching_first_row_is_no_tile() {
        let rows = vec![Decoded::number(Kind::Mul, Some(5), register(1), register(2), Some(4)), mul_then_add(Operand::B).remove(1)];
        assert_eq!(matched(MUL_ADD, &rows, 0, &[], &[]), None);
    }

    #[test]
    fn an_element_read_or_an_add_feeds_a_compare_and_branch() {
        let index = Decoded::heap(Heap::Index, Some(5), register(1), register(2));
        let compare = Decoded::number(Kind::Lt, None, register(3), register(5), Some(9));
        assert_eq!(matched("GetIndex ; JumpBinaryFalse(<) [0>1]", &[index, compare], 0, &[], &[]), Some(Operand::B));
        let sum = Decoded::number(Kind::Add, Some(5), register(1), register(2), None);
        let compare = Decoded::number(Kind::Lt, None, register(5), register(3), Some(9));
        assert_eq!(matched("NumericAdd(+) ; JumpBinaryFalse(<) [0>1]", &[sum, compare], 0, &[], &[]), Some(Operand::A));
    }

    #[test]
    fn the_largest_supernode_comes_first() {
        let sizes: Vec<usize> = candidates().iter().map(|candidate| candidate.shape.keys.len()).collect();
        assert_eq!(candidates().len(), supernodes::SUPERNODES.len());
        assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn only_a_commutative_row_exchanges_its_operands_to_meet_the_feeds() {
        let add = Node::Number((ADD, PIN, PIN + 1, PIN + 2, true));
        let sub = Node::Number((SUB, PIN, PIN + 1, PIN + 2, true));
        assert_eq!(oriented(add, Operand::B, &[Operand::B]), Some((add, false)));
        assert_eq!(oriented(add, Operand::A, &[Operand::B]), Some((Node::Number((ADD, PIN, PIN + 2, PIN + 1, true)), true)));
        assert_eq!(oriented(sub, Operand::A, &[Operand::B]), None);
        assert_eq!(oriented(sub, Operand::A, &[Operand::A, Operand::B]), Some((sub, false)));
    }

    fn planned(node: Node, holes: [u64; HOLES], exit: Option<u32>, target: Option<u32>) -> Planned {
        Planned { node: Some(node), ..Planned::single(skipped_stencil(), holes, target, exit) }
    }

    fn skipped_stencil() -> &'static Stencil {
        skipped().stencil
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_tile_is_one_stencil_that_exits_at_its_first_pc_with_the_rows_own_holes() {
        let (between, a, c, d) = (PIN, PIN + 1, PIN + 2, PIN + 3);
        let first = planned(Node::Number((MUL, between, a, numeric::library::IMM, true)), padded_holes([10, 11, 12]), Some(40), None);
        let second = planned(Node::Number((ADD, d, c, between, true)), padded_holes([20, 21, 22]), Some(41), None);
        let supernode = &supernodes::SUPERNODES[0];
        let tile = fuse(40, &first, &second, Operand::B, supernode).expect("a stencil for MUL then ADD over pins");
        assert!(tile.node.is_none() && tile.exits == [Exit::Spilling(40)] && tile.branch.is_none());
        assert_eq!(&tile.holes[..3], &[20, 21, 12], "role 2 is the first row's immediate; the others are the second row's");
    }

    #[test]
    fn the_hole_of_an_exchanged_operand_goes_with_it() {
        let first = planned(Node::Number((MUL, PIN, PIN + 1, PIN + 2, true)), padded_holes([10, 11, 12]), Some(40), None);
        let second = planned(Node::Number((AND, PIN + 3, PIN, numeric::library::IMM, true)), padded_holes([20, 21, 22]), Some(41), None);
        assert_eq!(joined(&first, &second, true, 0)[..3], [20, 22, 21]);
        assert_eq!(joined(&first, &second, false, 1 << 2)[..3], [20, 21, 12]);
    }
}
