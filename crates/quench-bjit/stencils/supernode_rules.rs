//! The supernode rules: what a supernode pattern means, which rows it instantiates and which
//! stencil symbols they are named by. Std-free apart from `Vec`/`BTreeSet`, and included by
//! path both by `build.rs` (`build/supernodes.rs` generates the list from it) and by
//! `supernodes.rs` (the library the planner reads). The list itself is data: `supernodes.txt`.
//! The short dataflow trees of existing rows that one stencil computes, the value between them
//! never touching memory (`supernode_rows.rs`).
//!
//! A supernode is named by the miner's own pattern text (`quench-stencil-mine`,
//! `Pattern::render`: opcode names with the binary operator symbol, then the producer>consumer
//! links, e.g. `NumericMultiply(*) ; NumericAdd(+) [0>1]`), so the miner's `recommended` entries
//! and this list are one format. The miner's pattern does not say which operand of the consumer
//! reads the producer nor which operand places to build; those are the two fields added here
//! (`feeds`, `places`), which the miner's output must also carry to be copied in verbatim.
//!
//! Selection rule. A supernode belongs in the list (`supernodes.txt`) when its exact pattern text
//! is in the miner's recommended set (greedy by marginal dispatches saved over the corpus, the
//! sites of an earlier pick no longer counting) and its variant count (`variants`, which the
//! miner's `PlaceTable::variants` estimates) fits the budgets below. The evidence of each entry is
//! its row in the miner's JSON (`pattern`, `static_sites`, `dynamic`, `dispatches_saved`); an entry
//! whose row is absent or ranked below the budget line is removed, never kept for its own sake.
//! Each label's opcode name is resolved against the runtime's opcode table by `build/supernodes.rs`,
//! which also checks every line of the list.
//!
//! Variants. Only int32 speculating rows over pinned registers (and an immediate) are built;
//! the place sets are the data below. A variant exists only where the stitcher can use it:
//! the value between the rows is a pinned register that no other place of the tile names (the
//! register allocator never gives a pin to two webs live together), and the rows read disjoint
//! hole roles (`roles`). A tile with no variant is planned as single rows.


/// The library mounted beside the rules: the build script's own, the lib's `numeric::library` (through `supernodes.rs`).
use super::library;
pub use library::{Code, HeapCode};
use library::*;
use std::collections::BTreeSet;

/// The runtime's Number operator tables (`vm/operations/number_rows.rs`): each relation, arithmetic and bitwise row names
/// the operator symbols that denote it, which `operator_kinds` reads, so the symbols and the kinds have one definition.
#[allow(unused_imports, dead_code)] // The build reads the operator tables alone.
#[path = "../../quench-runtime/src/vm/operations/number_rows.rs"]
mod number_rows;
use number_rows::{number_arithmetic_rows, number_bitwise_rows, number_relation_rows, NumberArithmetic, NumberBitwise, NumberRelation};

/// Rows a supernode fuses (a chain: each row feeds the next).
pub const NODES: usize = 2;
/// Most supernodes the library holds, and most stencils they instantiate together: what the
/// offline stencil build can carry (the library without them is ~26k rows).
pub const MAX_SUPERNODES: usize = 8;
pub const MAX_VARIANTS: usize = 2500;

/// The operand slots of a row, and the hole role each is read through: the destination is role
/// 0, the first operand 1, the second 2 (`numeric.rs`, `heap_rows.rs`).
const ROLES: usize = 3;
/// The hole roles as bits of a role mask: the destination, the first operand and the second.
const DESTINATION_ROLE: u8 = 1 << 0;
const FIRST_ROLE: u8 = 1 << 1;
const SECOND_ROLE: u8 = 1 << 2;
const ALL_ROLES: u8 = DESTINATION_ROLE | FIRST_ROLE | SECOND_ROLE;
const LABEL_SEPARATOR: &str = " ; ";
const LINKS_OPEN: &str = " [";
const LINKS_CLOSE: char = ']';
const LINK_SEPARATOR: char = ',';
const LINK_ARROW: char = '>';

/// The row family an opcode's label decodes to (`base_region/decode.rs`), named in the list's
/// `opcode` lines: a value operation (`Binary`, `NumericAdd`, ... with a non-compare operator), a
/// compare-and-branch (with a compare operator) or a dense element read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Value,
    Branch,
    Index,
}

impl Family {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "value" => Some(Family::Value),
            "branch" => Some(Family::Branch),
            "index" => Some(Family::Index),
            _ => None,
        }
    }
}

macro_rules! relation_operator_kinds {
    ($($relation:ident => [$($operator:ident $symbol:literal),+ $(,)?], $stage_method:ident;)+) => {{
        let mut kinds: Vec<(&'static str, u8)> = Vec::new();
        $( kinds.extend([$($symbol),+].map(|symbol| (symbol, relation_kind(NumberRelation::$relation)))); )+
        kinds
    }};
}

macro_rules! arithmetic_operator_kinds {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        vec![$((stringify!($symbol), arithmetic_kind(NumberArithmetic::$operation))),+]
    };
}

macro_rules! bitwise_operator_kinds {
    ($($operation:ident => $operator:ident, $stage_method:ident, $symbol:literal;)+) => {
        vec![$(($symbol, bitwise_kind(NumberBitwise::$operation))),+]
    };
}

/// JavaScript operator symbols and the kind that computes them: each operation's kind, over the symbols its runtime row names.
fn operator_kinds() -> Vec<(&'static str, u8)> {
    let mut kinds = number_relation_rows!(relation_operator_kinds);
    kinds.extend(number_arithmetic_rows!(arithmetic_operator_kinds));
    kinds.extend(number_bitwise_rows!(bitwise_operator_kinds));
    kinds
}

/// The kind a relation's Number row computes.
fn relation_kind(relation: NumberRelation) -> u8 {
    match relation {
        NumberRelation::Equal => EQ,
        NumberRelation::NotEqual => NE,
        NumberRelation::LessThan => LT,
        NumberRelation::LessEqual => LE,
        NumberRelation::GreaterThan => GT,
        NumberRelation::GreaterEqual => GE,
    }
}

/// The kind an arithmetic row computes.
fn arithmetic_kind(operation: NumberArithmetic) -> u8 {
    match operation {
        NumberArithmetic::Add => ADD,
        NumberArithmetic::Subtract => SUB,
        NumberArithmetic::Multiply => MUL,
        NumberArithmetic::Divide => DIV,
    }
}

/// The kind a bitwise row computes.
fn bitwise_kind(operation: NumberBitwise) -> u8 {
    match operation {
        NumberBitwise::And => AND,
        NumberBitwise::Or => OR,
        NumberBitwise::Xor => XOR,
        NumberBitwise::ShiftLeft => SHL,
        NumberBitwise::ShiftRight => SHR,
        NumberBitwise::ShiftRightUnsigned => USHR,
    }
}

/// The kinds whose two operands may be exchanged without changing the result (`tile::oriented`).
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
    /// The other operand of a two-operand row, which a commutative row exchanges.
    pub const fn other(self) -> Self {
        match self {
            Operand::A => Operand::B,
            Operand::B => Operand::A,
        }
    }
}

/// The places each operand of a row may take (library codes), and for a heap row whether its
/// array is the entry record's (`entry`: bit `false` and bit `true`, set where the row may be that).
#[derive(Clone, Copy)]
pub struct NodePlaces {
    pub dst: &'static [u8],
    pub a: &'static [u8],
    pub b: &'static [u8],
    pub entry: u8,
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
/// The `entry` bit of a row whose array is not (`0`) or is (`1`) the entry record's.
const fn entry_bit(entry: bool) -> u8 {
    1 << entry as u8
}
const NO_ENTRY: u8 = entry_bit(false);
const BOTH_ENTRIES: u8 = entry_bit(false) | entry_bit(true);

/// An int32 arithmetic or bitwise row over pins, its second operand maybe an immediate.
pub const VALUE_FIRST: NodePlaces = NodePlaces { dst: &PINS, a: &PINS, b: &PINS_OR_IMM, entry: NO_ENTRY };
/// A dense element read: the array's place is the library's, the index a pin.
pub const INDEX_FIRST: NodePlaces = NodePlaces { dst: &PINS, a: &IGNORED, b: &PINS, entry: BOTH_ENTRIES };
pub const VALUE_SECOND: NodePlaces = NodePlaces { dst: &PINS, a: &PINS, b: &PINS, entry: NO_ENTRY };
/// A compare-and-branch writes nothing.
pub const BRANCH_SECOND: NodePlaces = NodePlaces { dst: &IGNORED, a: &PINS, b: &PINS, entry: NO_ENTRY };

/// The place set a list line names (`supernodes.txt`), or `None` for a name the rules do not define.
pub fn place_class(name: &str) -> Option<NodePlaces> {
    match name {
        "VALUE_FIRST" => Some(VALUE_FIRST),
        "INDEX_FIRST" => Some(INDEX_FIRST),
        "VALUE_SECOND" => Some(VALUE_SECOND),
        "BRANCH_SECOND" => Some(BRANCH_SECOND),
        _ => None,
    }
}

/// The operands a list line's `feeds` field names (`A`, `B` or `A,B`), `None` if it names none or another.
pub fn feeds_of(text: &str) -> Option<Vec<Operand>> {
    let feeds = text
        .split(',')
        .map(|name| match name.trim() {
            "A" => Some(Operand::A),
            "B" => Some(Operand::B),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!feeds.is_empty()).then_some(feeds)
}


/// A pattern as the stitcher matches it: the key of each node and the producer>consumer links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub keys: Vec<Key>,
    pub links: Vec<(usize, usize)>,
}

fn kind_of(operator: &str) -> Option<u8> {
    operator_kinds().into_iter().find(|(symbol, _)| *symbol == operator).map(|(_, kind)| kind)
}

/// The opcode name of one rendered label: `Name` or `Name(operator)`.
pub fn opcode_of_label(label: &str) -> &str {
    label.split_once('(').map_or(label, |(name, _)| name)
}

/// The key of one rendered label: `Name` or `Name(operator)`, where `Name` is an opcode of `families`.
fn key_of_label(label: &str, families: &[(&str, Family)]) -> Option<Key> {
    let (name, operator) = match label.split_once('(') {
        Some((name, rest)) => (name, Some(rest.strip_suffix(')')?)),
        None => (label, None),
    };
    let family = families.iter().find(|(opcode, _)| *opcode == name)?.1;
    match (family, operator) {
        (Family::Index, None) => Some(Key::Heap(INDEX_ROW)),
        (Family::Branch, Some(operator)) => kind_of(operator).filter(|&kind| is_compare(kind)).map(Key::Number),
        (Family::Value, Some(operator)) => kind_of(operator).filter(|&kind| !is_compare(kind)).map(Key::Number),
        _ => None,
    }
}

fn link_of(text: &str) -> Option<(usize, usize)> {
    let (producer, consumer) = text.split_once(LINK_ARROW)?;
    Some((producer.parse().ok()?, consumer.parse().ok()?))
}

/// The shape of a miner pattern text under the opcode `families`; `None` for a pattern the library
/// cannot build (an opcode with no family, an op or operator it has no row for, or not a chain of
/// `NODES` rows).
pub fn parse(pattern: &str, families: &[(&str, Family)]) -> Option<Shape> {
    let (labels, links) = pattern.rsplit_once(LINKS_OPEN)?;
    let links = links.strip_suffix(LINKS_CLOSE)?;
    let keys = labels.split(LABEL_SEPARATOR).map(|label| key_of_label(label, families)).collect::<Option<Vec<_>>>()?;
    let links = links.split(LINK_SEPARATOR).filter(|text| !text.is_empty()).map(link_of).collect::<Option<Vec<_>>>()?;
    let chain = links.iter().enumerate().all(|(at, link)| *link == (at, at + 1));
    (chain && keys.len() == NODES && links.len() + 1 == NODES).then_some(Shape { keys, links })
}

/// The hole roles a row reads through, as a bit mask: a place below `PIN` is a frame slot, an
/// immediate or a constant, which its hole supplies.
pub fn roles(node: &Node) -> u8 {
    let (used, places) = match *node {
        Node::Number((kind, dst, a, b, _)) => (read_roles(kind), [dst, a, b]),
        Node::Heap((_, _, dst, a, b)) => (ALL_ROLES, [dst, a, b]),
    };
    (0..ROLES).filter(|&role| used >> role & 1 != 0 && places[role] < PIN).fold(0, |mask, role| mask | 1 << role)
}

/// Which of (destination, first, second) a Number kind reads or writes, as a role mask.
fn read_roles(kind: u8) -> u8 {
    match kind {
        _ if is_compare(kind) || kind == BIT_TEST => FIRST_ROLE | SECOND_ROLE,
        NULLISH_EQ | NULLISH_NE | BOOL_TRUE | BOOL_FALSE => FIRST_ROLE,
        _ if is_copy(kind) => DESTINATION_ROLE | FIRST_ROLE,
        _ => ALL_ROLES,
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
pub fn variants(supernode: &Supernode, families: &[(&str, Family)]) -> BTreeSet<[Node; NODES]> {
    let shape = parse(supernode.pattern, families).expect("a supernode pattern in the miner's format");
    let [first, second] = &supernode.places;
    let mut found = BTreeSet::new();
    for &feed in supernode.feeds {
        let other = if feed == Operand::A { second.b } else { second.a };
        for entry in [false, true].into_iter().filter(|&entry| first.entry & entry_bit(entry) != 0) {
            for choice in product(&[first.dst, first.a, first.b, other, second.dst]) {
                found.extend(rows_of(&shape, feed, entry, &choice));
            }
        }
    }
    found
}

/// Every supernode stencil the list holds, once.
pub fn instances(supernodes: &[Supernode], families: &[(&str, Family)]) -> Vec<[Node; NODES]> {
    supernodes.iter().flat_map(|supernode| variants(supernode, families)).collect::<BTreeSet<_>>().into_iter().collect()
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

/// The type of one constituent row of a supernode (`supernode_rows.rs`): its kind and places as const arguments.
fn constituent(node: Node) -> String {
    match node {
        Node::Number((kind, dst, a, b, spec)) => format!("supernode_rows::NumberRow<{kind}, {dst}, {a}, {b}, {spec}>"),
        Node::Heap((heap, entry, dst, a, b)) => format!("supernode_rows::HeapRow<{heap}, {entry}, {dst}, {a}, {b}>"),
    }
}

/// The arguments of `supernode_row!` (`rows.rs`): the types of the first row, then the second, which is a Number row.
pub fn macro_args(nodes: &[Node; NODES]) -> String {
    assert!(matches!(nodes[1], Node::Number(_)), "the second row of a supernode is a Number row");
    format!("{}, {}", constituent(nodes[0]), constituent(nodes[1]))
}

