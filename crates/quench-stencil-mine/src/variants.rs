//! The explosion cost of a supernode: the stencil rows of each constituent operation
//! multiply over operand places, less the places of every intermediate the fusion keeps in a
//! register. Row counts are read from the bjit library itself (`library.rs`), not copied.
#[allow(dead_code, clippy::all)]
#[path = "../../quench-bjit/stencils/library.rs"]
mod library;

use crate::model::{Label, Pattern};
use std::collections::{BTreeMap, BTreeSet};

/// JavaScript operator symbols and the library kind that computes them.
const OPERATOR_KINDS: &[(&str, u8)] = &[
    ("+", library::ADD), ("-", library::SUB), ("*", library::MUL), ("/", library::DIV),
    ("&", library::AND), ("|", library::OR), ("^", library::XOR),
    ("<<", library::SHL), (">>", library::SHR), (">>>", library::USHR),
    ("<", library::LT), ("<=", library::LE), (">", library::GT), (">=", library::GE),
    ("==", library::EQ), ("===", library::EQ), ("!=", library::NE), ("!==", library::NE),
];

/// Operator-free opcodes and the library kind that computes them.
const OPCODE_KINDS: &[(&str, u8)] = &[
    ("Move", library::MOVE), ("LoadLocalPlain", library::COPY), ("StoreLocalPlain", library::COPY),
    ("Jump", library::JUMP),
];

/// The factor of an operation the Number library has no row for (heap and call operations
/// have their own tables, `heap_library`): counted as one variant, so a product of them
/// understates the true cost.
const UNMAPPED_VARIANTS: f64 = 1.0;

pub struct PlaceTable {
    rows: BTreeMap<u8, usize>,
    destinations: BTreeMap<u8, usize>,
    pub library_rows: usize,
}

fn kind_of(label: &Label) -> Option<u8> {
    let by_operator = label.operator.and_then(|symbol| OPERATOR_KINDS.iter().find(|(s, _)| *s == symbol));
    by_operator.or_else(|| OPCODE_KINDS.iter().find(|(name, _)| *name == label.name)).map(|&(_, kind)| kind)
}

impl PlaceTable {
    pub fn new() -> Self {
        let rows = library::library();
        let mut per_kind: BTreeMap<u8, usize> = BTreeMap::new();
        let mut places: BTreeMap<u8, BTreeSet<u8>> = BTreeMap::new();
        for &(kind, destination, ..) in &rows {
            *per_kind.entry(kind).or_insert(0) += 1;
            places.entry(kind).or_default().insert(destination);
        }
        let destinations = places.into_iter().map(|(kind, set)| (kind, set.len())).collect();
        Self { rows: per_kind, destinations, library_rows: rows.len() }
    }

    fn rows_of(&self, label: &Label) -> f64 {
        kind_of(label).and_then(|kind| self.rows.get(&kind)).map_or(UNMAPPED_VARIANTS, |&rows| rows as f64)
    }

    fn fused_places(&self, label: &Label) -> f64 {
        kind_of(label).and_then(|kind| self.destinations.get(&kind)).map_or(1.0, |&places| places as f64)
    }

    /// Stencil variants the pattern implies.
    pub fn variants(&self, pattern: &Pattern) -> u64 {
        let product: f64 = pattern.labels.iter().map(|label| self.rows_of(label)).product();
        let fused: f64 = pattern.links.iter().map(|link| self.fused_places(&pattern.labels[link.producer as usize])).product();
        (product / fused).round().max(1.0) as u64
    }
}

impl Default for PlaceTable {
    fn default() -> Self {
        Self::new()
    }
}
