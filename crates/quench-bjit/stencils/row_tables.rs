//! The Number kinds' codes and the heap rows' operand tables (`HEAP_TABLES`): the half of the row catalog
//! (`row_catalog.rs`) that `build.rs` reads. The heap rows' names are `row_names.rs`, which `build.rs` does not mount.

use super::TYPED_KINDS;

/// How a heap row reads its entry flag: it ignores it, keeps the site's, or is always an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Never,
    Kept,
    Always,
}

/// The places one operand of a heap row ranges over: a row that does not read the operand fixes it to `MEM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Unread,
    /// Any destination: every int place and every double register.
    Dst,
    /// A typed element read's destination: a double register or `MEM`.
    Float,
    /// Any operand: every int place, the constant and immediate, and every double register.
    Src,
}

/// The places a heap row reads: its entry flag and the places each operand ranges over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uses {
    pub entry: Entry,
    pub dst: Slot,
    pub a: Slot,
    pub b: Slot,
}

catalog!(row_catalog(tables));
