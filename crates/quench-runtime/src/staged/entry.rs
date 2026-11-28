//! A data field's read through the IC site's entry record (form b), own or held by the
//! receiver's prototype: the record's shape, the holder and the slot are read at run time from
//! the entry the site table keeps, not held as per-site operands. A specializer's only hole is
//! the entry's address, so the code never changes when the site's feedback does (the entry
//! record is the one feedback authority).

use super::object::{arena_run, decode_narrow, extras_pointer, has_extras, inline_storage, plain_payload};
use super::{Stage, StagedRecordSpace};
use super::region::Region;
use super::field_cache_layout::CLASS_KEY;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;

// The entry's words are complemented by an all-ones word (a difference from it) for the shapes
// and by zero for the slot: the guards below read them without a xor.
const _: () = assert!(ENTRY.receiver_complement == u32::MAX && ENTRY.slot_complement == 0);

#[path = "entry/plain.rs"]
mod plain;
#[path = "entry/site.rs"]
mod site;
#[path = "entry/direct.rs"]
mod direct;
#[path = "entry/holder.rs"]
mod holder;
#[path = "entry/class.rs"]
mod class;
#[path = "entry/words.rs"]
mod words;
pub use plain::*;
pub use site::*;
pub use direct::*;
pub use holder::*;
pub use class::*;
pub use words::*;

#[cfg(test)]
mod tests;
