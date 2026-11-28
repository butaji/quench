//! Object storage layouts as staged reads: where a record of a proved exact shape keeps its
//! named values, and how a narrow value decodes. The interpreter reads them on `Concrete`; a
//! specializer emits them.

use super::{Concrete, Stage, Table};
use super::region::Region;
use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, PlainPropertyStorage};
use super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use super::object_encoding::{CLASS_HEADER_BYTES, DEPARTED_CLASS};
use crate::typed_ir::Representation;
use crate::value::{NarrowLayout, ValueLayout};

/// Bytes in one half of a packed payload word, the stride of a packed slot.
pub(crate) const HALF_BYTES: u64 = std::mem::size_of::<u32>() as u64;

/// What a site proved about where an object of an exact shape keeps its values. It is a
/// static input: a specializer reads no word to learn it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Form {
    /// Packed narrow values in the payload word (`PlainPropertyStorage::PackedInline`).
    Packed,
    /// A flat run in the property arena (`PlainPropertyStorage::Arena`).
    Arena,
    /// Either, as the record's inline flag says at run time.
    Either,
}

impl Form {
    /// The form a site's observed storage and its shape's length leave possible: only a shape
    /// that fits the payload word can be packed.
    #[inline(always)]
    pub fn of(storage: Option<PlainPropertyStorage>, storage_len: usize) -> Self {
        match storage {
            Some(PlainPropertyStorage::PackedInline) => Self::Packed,
            Some(PlainPropertyStorage::Arena) => Self::Arena,
            None if storage_len <= OBJECT.inline_property_count => Self::Either,
            None => Self::Arena,
        }
    }
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// How packed values are read: from the record at each read, since a store may reach them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PackedAccess {
    Reload,
}

/// Packed narrow values: the record that holds them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Packed<W> {
    Record(W),
}

/// Where an object of a proved exact shape keeps its named values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Slots<W, C> {
    Packed(Packed<W>),
    /// The address of value zero of a flat run of wide values: the arena's, or the inline
    /// values extras displaced.
    Flat(W),
    /// Packed or flat, as `inline` says; `flat` is the run's address where it is flat.
    Either { packed: Packed<W>, inline: C, flat: W },
    /// The address of word zero of a narrow run (`value_vec::NARROW_FORM`): value `i` is the
    /// narrow word at byte `4 * i` (little-endian halves, the lower first). Only a stage that
    /// decides the form at run time (the interpreter) reads one; a specializer's residual
    /// leaves at its miss instead.
    Narrow(W),
}

/// The layout of one object record and how many values its shape holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Layout<W, C> {
    pub slots: Slots<W, C>,
    pub storage_len: usize,
}

#[path = "object/header.rs"]
mod header;
#[path = "object/proto.rs"]
mod proto;
#[path = "object/walk.rs"]
mod walk;
#[path = "object/records.rs"]
mod records;
#[path = "object/arena.rs"]
mod arena;
#[path = "object/own_layout.rs"]
mod own_layout;
#[path = "object/read.rs"]
mod read;
#[path = "object/narrow.rs"]
mod narrow;
pub use header::*;
pub use proto::*;
pub use walk::*;
pub use records::*;
pub use arena::*;
pub use own_layout::*;
pub use read::*;
pub use narrow::*;
