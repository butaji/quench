//! The supernode library as data: short dataflow trees of existing rows that one stencil
//! computes, the value between them never touching memory (`supernode_rows.rs`).
//!
//! A supernode is named by the miner's own pattern text (`quench-stencil-mine`,
//! `Pattern::render`: opcode names with the binary operator symbol, then the producer>consumer
//! links, e.g. `NumericMultiply(*) ; NumericAdd(+) [0>1]`), so the miner's `recommended` entries
//! and this list are one format. The miner's pattern does not say which operand of the consumer
//! reads the producer nor which operand places to build; those are the two fields added here
//! (`feeds`, `places`), which the miner's output must also carry to be copied in verbatim.
//!
//! Selection rule. A supernode belongs in `SUPERNODES` when its exact pattern text is in the
//! miner's recommended set (greedy by marginal dispatches saved over the corpus, the sites of an
//! earlier pick no longer counting) and its variant count (`variants`, which the miner's
//! `PlaceTable::variants` estimates) fits the budgets below. The evidence of each entry is its
//! row in the miner's JSON (`pattern`, `static_sites`, `dynamic`, `dispatches_saved`); an entry
//! whose row is absent or ranked below the budget line is removed, never kept for its own sake.
//!
//! Variants. Only int32 speculating rows over pinned registers (and an immediate) are built;
//! the place sets are the data below. A variant exists only where the stitcher can use it:
//! the value between the rows is a pinned register that no other place of the tile names (the
//! register allocator never gives a pin to two webs live together), and the rows read disjoint
//! hole roles (`roles`). A tile with no variant is planned as single rows.

#![allow(dead_code)]

#[path = "library.rs"]
mod library;
pub use library::{Code, HeapCode};
use library::*;
use std::collections::BTreeSet;

/// Rows a supernode fuses (a chain: each row feeds the next).
pub const NODES: usize = 2;
/// Most supernodes the library holds, and most stencils they instantiate together: what the
/// offline stencil build can carry (the library without them is ~26k rows).
pub const MAX_SUPERNODES: usize = 8;
pub const MAX_VARIANTS: usize = 2500;

/// The operand slots of a row, and the hole role each is read through: the destination is role
/// 0, the first operand 1, the second 2 (`numeric.rs`, `heap_rows.rs`).
const ROLES: usize = 3;
const LABEL_SEPARATOR: &str = " ; ";
const LINKS_OPEN: &str = " [";
const LINKS_CLOSE: char = ']';
const LINK_SEPARATOR: char = ',';
const LINK_ARROW: char = '>';

/// Op names the library knows, and the rows they decode to (`base_region/decode.rs`).
const VALUE_CLASSES: [&str; 3] = ["Binary", "NumericAdd", "NumericMultiply"];
const BRANCH_CLASS: &str = "JumpBinaryFalse";
const INDEX_CLASS: &str = "GetIndex";

/// JavaScript operator symbols and the kind that computes them.
const OPERATOR_KINDS: [(&str, u8); 18] = [
    ("+", ADD), ("-", SUB), ("*", MUL), ("/", DIV), ("&", AND), ("|", OR), ("^", XOR),
    ("<<", SHL), (">>", SHR), (">>>", USHR), ("<", LT), ("<=", LE), (">", GT), (">=", GE),
    ("==", EQ), ("===", EQ), ("!=", NE), ("!==", NE),
];

/// The kinds whose two operands may be exchanged without changing the result.
pub const fn commutative(kind: u8) -> bool {
    matches!(kind, ADD | MUL | AND | OR | XOR | EQ | NE)
}

/// What a pattern node is: a Number row of a kind, or a heap row of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Number(u8),
    Heap(u8),
}

/// A row as codes: the library's `Code` or `HeapCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Node {
    Number(Code),
    Heap(HeapCode),
}

/// The operand of the consuming row that reads the producing row's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Operand {
    A,
    B,
}

impl Operand {
    pub const fn other(self) -> Self {
        match self {
            Operand::A => Operand::B,
            Operand::B => Operand::A,
        }
    }
}

/// The places each operand of a row may take (library codes), and for a heap row whether its
/// array is the entry record's.
pub struct NodePlaces {
    pub dst: &'static [u8],
    pub a: &'static [u8],
    pub b: &'static [u8],
    pub entry: &'static [bool],
}

pub struct Supernode {
    /// The miner's pattern text.
    pub pattern: &'static str,
    /// Which operand of the second row the first row's result may feed. A commutative second
    /// row listing only `B` also serves a tile that feeds `A` (the stitcher exchanges them).
    pub feeds: &'static [Operand],
    /// Per row. The first row's destination is the value between the rows; the second row's
    /// place set for the fed operand is not used.
    pub places: [NodePlaces; NODES],
    /// The row of the miner's JSON this entry stands for.
    pub evidence: &'static str,
}

const PINS: [u8; INT_PINS as usize] = [PIN, PIN + 1, PIN + 2, LAST_INT_PIN];
const PINS_OR_IMM: [u8; INT_PINS as usize + 1] = [PIN, PIN + 1, PIN + 2, LAST_INT_PIN, IMM];
const IGNORED: [u8; 1] = [MEM];
const NO_ENTRY: [bool; 1] = [false];
const BOTH_ENTRIES: [bool; 2] = [false, true];

/// An int32 arithmetic or bitwise row over pins, its second operand maybe an immediate.
const VALUE_FIRST: NodePlaces = NodePlaces { dst: &PINS, a: &PINS, b: &PINS_OR_IMM, entry: &NO_ENTRY };
/// A dense element read: the array's place is the library's, the index a pin.
const INDEX_FIRST: NodePlaces = NodePlaces { dst: &PINS, a: &IGNORED, b: &PINS, entry: &BOTH_ENTRIES };
const VALUE_SECOND: NodePlaces = NodePlaces { dst: &PINS, a: &PINS, b: &PINS, entry: &NO_ENTRY };
/// A compare-and-branch writes nothing.
const BRANCH_SECOND: NodePlaces = NodePlaces { dst: &IGNORED, a: &PINS, b: &PINS, entry: &NO_ENTRY };

const FEED_B: [Operand; 1] = [Operand::B];
const FEED_EITHER: [Operand; 2] = [Operand::A, Operand::B];

pub static SUPERNODES: [Supernode; 7] = [
    Supernode { pattern: "NumericMultiply(*) ; NumericAdd(+) [0>1]", feeds: &FEED_B, places: [VALUE_FIRST, VALUE_SECOND], evidence: "a + b * c: miner row for this pattern" },
    Supernode { pattern: "NumericAdd(+) ; NumericAdd(+) [0>1]", feeds: &FEED_B, places: [VALUE_FIRST, VALUE_SECOND], evidence: "a + b + c: miner row for this pattern" },
    Supernode { pattern: "NumericMultiply(*) ; Binary(-) [0>1]", feeds: &FEED_B, places: [VALUE_FIRST, VALUE_SECOND], evidence: "a - b * c: miner row for this pattern" },
    Supernode { pattern: "Binary(>>) ; Binary(&) [0>1]", feeds: &FEED_B, places: [VALUE_FIRST, VALUE_SECOND], evidence: "(a >> b) & c: miner row for this pattern" },
    Supernode { pattern: "NumericAdd(+) ; JumpBinaryFalse(<) [0>1]", feeds: &FEED_EITHER, places: [VALUE_FIRST, BRANCH_SECOND], evidence: "a + b < c branch: miner row for this pattern" },
    Supernode { pattern: "GetIndex ; NumericAdd(+) [0>1]", feeds: &FEED_B, places: [INDEX_FIRST, VALUE_SECOND], evidence: "a + array[i]: miner row for this pattern" },
    Supernode { pattern: "GetIndex ; JumpBinaryFalse(<) [0>1]", feeds: &FEED_EITHER, places: [INDEX_FIRST, BRANCH_SECOND], evidence: "array[i] < c branch: miner row for this pattern" },
];

/// A pattern as the stitcher matches it: the key of each node and the producer>consumer links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub keys: Vec<Key>,
    pub links: Vec<(usize, usize)>,
}

fn kind_of(operator: &str) -> Option<u8> {
    OPERATOR_KINDS.iter().find(|(symbol, _)| *symbol == operator).map(|&(_, kind)| kind)
}

/// The key of one rendered label: `Name` or `Name(operator)`.
fn key_of_label(label: &str) -> Option<Key> {
    let (name, operator) = match label.split_once('(') {
        Some((name, rest)) => (name, Some(rest.strip_suffix(')')?)),
        None => (label, None),
    };
    match (name, operator) {
        (INDEX_CLASS, None) => Some(Key::Heap(INDEX_ROW)),
        (BRANCH_CLASS, Some(operator)) => kind_of(operator).filter(|&kind| is_compare(kind)).map(Key::Number),
        (name, Some(operator)) if VALUE_CLASSES.contains(&name) => kind_of(operator).filter(|&kind| !is_compare(kind)).map(Key::Number),
        _ => None,
    }
}

fn link_of(text: &str) -> Option<(usize, usize)> {
    let (producer, consumer) = text.split_once(LINK_ARROW)?;
    Some((producer.parse().ok()?, consumer.parse().ok()?))
}

/// The shape of a miner pattern text; `None` for a pattern the library cannot build (an op or
/// operator it has no row for, or not a chain of `NODES` rows).
pub fn parse(pattern: &str) -> Option<Shape> {
    let (labels, links) = pattern.rsplit_once(LINKS_OPEN)?;
    let links = links.strip_suffix(LINKS_CLOSE)?;
    let keys = labels.split(LABEL_SEPARATOR).map(key_of_label).collect::<Option<Vec<_>>>()?;
    let links = links.split(LINK_SEPARATOR).filter(|text| !text.is_empty()).map(link_of).collect::<Option<Vec<_>>>()?;
    let chain = links.iter().enumerate().all(|(at, link)| *link == (at, at + 1));
    (chain && keys.len() == NODES && links.len() + 1 == NODES).then_some(Shape { keys, links })
}

/// The hole roles a row reads through, as a bit mask: a place below `PIN` is a frame slot, an
/// immediate or a constant, which its hole supplies.
pub fn roles(node: &Node) -> u8 {
    let (used, places) = match *node {
        Node::Number((kind, dst, a, b, _)) => (read_roles(kind), [dst, a, b]),
        Node::Heap((_, _, dst, a, b)) => ([true; ROLES], [dst, a, b]),
    };
    (0..ROLES).filter(|&role| used[role] && places[role] < PIN).fold(0, |mask, role| mask | 1 << role)
}

/// Which of (destination, first, second) a Number kind reads or writes.
fn read_roles(kind: u8) -> [bool; ROLES] {
    match kind {
        _ if is_compare(kind) => [false, true, true],
        _ if is_copy(kind) => [true, true, false],
        _ => [true; ROLES],
    }
}

/// Every combination of one place from each set.
fn product(sets: &[&[u8]]) -> Vec<Vec<u8>> {
    sets.iter().fold(vec![Vec::new()], |rows, set| rows.iter().flat_map(|row| set.iter().map(move |&place| [row.as_slice(), &[place]].concat())).collect())
}

/// The two rows of one choice (value between, first operands, the second row's other operand
/// and destination), or `None` where the choice has no use: the value between the rows shares a
/// place with another operand, or the rows read the same hole role.
fn rows_of(shape: &Shape, feed: Operand, entry: bool, choice: &[u8]) -> Option<[Node; NODES]> {
    let &[between, a, b, other, dst] = choice else { return None };
    let (Key::Number(kind), false) = (*shape.keys.last()?, [a, b, other, dst].contains(&between)) else { return None };
    let first = match *shape.keys.first()? {
        Key::Number(kind) => Node::Number(canonical((kind, between, a, b, true))),
        Key::Heap(heap) => Node::Heap(heap_canonical((heap, entry, between, a, b))),
    };
    let (x, y) = if feed == Operand::A { (between, other) } else { (other, between) };
    let second = Node::Number(canonical((kind, dst, x, y, true)));
    (roles(&first) & roles(&second) == 0).then_some([first, second])
}

/// Every row pair a supernode instantiates.
pub fn variants(supernode: &Supernode) -> BTreeSet<[Node; NODES]> {
    let shape = parse(supernode.pattern).expect("a supernode pattern in the miner's format");
    let [first, second] = &supernode.places;
    let mut found = BTreeSet::new();
    for &feed in supernode.feeds {
        let other = if feed == Operand::A { second.b } else { second.a };
        for &entry in first.entry {
            for choice in product(&[first.dst, first.a, first.b, other, second.dst]) {
                found.extend(rows_of(&shape, feed, entry, &choice));
            }
        }
    }
    found
}

/// Every supernode stencil the library holds, once.
pub fn instances() -> Vec<[Node; NODES]> {
    SUPERNODES.iter().flat_map(variants).collect::<BTreeSet<_>>().into_iter().collect()
}

fn node_name(node: &Node) -> String {
    match *node {
        Node::Number(code) => library::name(code),
        Node::Heap(code) => heap_name(code),
    }
}

/// The name of a supernode's stencil symbol.
pub fn name(nodes: &[Node; NODES]) -> String {
    format!("s__{}", nodes.iter().map(node_name).collect::<Vec<_>>().join("__"))
}

/// The arguments of `supernode_row!` (`rows.rs`): the first row's family, kind, entry, places
/// and speculation, then the second row's.
pub fn macro_args(nodes: &[Node; NODES]) -> String {
    let first = match nodes[0] {
        Node::Number((kind, dst, a, b, spec)) => format!("{FIRST_NUMBER}, {kind}, false, {dst}, {a}, {b}, {spec}"),
        Node::Heap((heap, entry, dst, a, b)) => format!("{FIRST_HEAP}, {heap}, {entry}, {dst}, {a}, {b}, false"),
    };
    match nodes[1] {
        Node::Number((kind, dst, a, b, spec)) => format!("{first}, {kind}, {dst}, {a}, {b}, {spec}"),
        Node::Heap(_) => unreachable!("the second row of a supernode is a Number row"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checked_in_list_parses_and_fits_its_budgets() {
        assert!(SUPERNODES.len() <= MAX_SUPERNODES);
        for supernode in &SUPERNODES {
            let shape = parse(supernode.pattern).expect(supernode.pattern);
            assert!(matches!(shape.keys[1], Key::Number(_)), "{}", supernode.pattern);
            assert!(!supernode.feeds.is_empty() && !supernode.evidence.is_empty());
        }
        assert!(instances().len() <= MAX_VARIANTS, "{}", instances().len());
        assert_eq!(PINS.len(), INT_PINS as usize);
    }

    #[test]
    fn miner_patterns_parse_to_keys_and_links() {
        let shape = parse("Binary(>>) ; Binary(&) [0>1]").unwrap();
        assert_eq!(shape, Shape { keys: vec![Key::Number(SHR), Key::Number(AND)], links: vec![(0, 1)] });
        assert_eq!(parse("GetIndex ; NumericAdd(+) [0>1]").unwrap().keys[0], Key::Heap(INDEX_ROW));
        assert_eq!(parse("Binary(<<) ; JumpBinaryFalse(>=) [0>1]").unwrap().keys[1], Key::Number(GE));
    }

    #[test]
    fn patterns_the_library_cannot_build_are_refused() {
        for text in [
            "Binary(&) ; Binary(<) [0>1]",
            "JumpBinaryFalse(+) ; Binary(&) [0>1]",
            "Move ; NumericAdd(+) [0>1]",
            "NumericAdd(+) ; NumericAdd(+) []",
            "NumericAdd(+) ; NumericAdd(+) [1>0]",
            "NumericAdd(+) ; NumericAdd(+) ; NumericAdd(+) [0>1,1>2]",
            "NumericAdd(+)",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_variant_keeps_the_value_between_rows_in_its_own_pin_and_the_hole_roles_apart() {
        let mut names = BTreeSet::new();
        for nodes in instances() {
            let (Node::Number((_, dst, a0, b0, _)) | Node::Heap((_, _, dst, a0, b0))) = nodes[0];
            assert!((PIN..=LAST_INT_PIN).contains(&dst), "{nodes:?}");
            assert_eq!(roles(&nodes[0]) & roles(&nodes[1]), 0, "{nodes:?}");
            let Node::Number((_, second_dst, a, b, _)) = nodes[1] else { panic!("{nodes:?}") };
            assert!(![a0, b0, second_dst].contains(&dst), "{nodes:?}");
            assert_eq!([a, b].iter().filter(|place| **place == dst).count(), 1, "{nodes:?}");
            assert!(names.insert(name(&nodes)), "{nodes:?}");
        }
    }

    #[test]
    fn roles_name_the_hole_users() {
        assert_eq!(roles(&Node::Number((ADD, PIN, PIN + 1, IMM, true))), 1 << 2);
        assert_eq!(roles(&Node::Number((LT, MEM, PIN, PIN + 1, true))), 0);
        assert_eq!(roles(&Node::Heap((INDEX_ROW, false, PIN, MEM, PIN))), 1 << 1);
    }
}
