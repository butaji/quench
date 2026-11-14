//! A loop entry record's words for the arrays its region indexes (`LoopEntry::push_array`): where
//! they lie, and a typed array's view read from them, as the stencil rows read it (the dense
//! arrays' backings are `entry::entry_backing`).

use super::region::Region;
use super::typed_elements::{TypedView, ViewKind};
use super::Stage;

/// The words a loop entry record holds for one array (`LoopEntry::push_array`): its elements' address and
/// their count, then a dense array's capacity or a typed array's kind (the `TypedArrayKind` discriminant,
/// which a typed view has no use of a capacity for).
pub(crate) const ARRAY_WORDS: usize = 3;
const WORD: i32 = core::mem::size_of::<u64>() as i32;
const ARRAY_COUNT_AT: i32 = WORD;
const ARRAY_KIND_AT: i32 = (ARRAY_WORDS as i32 - 1) * WORD;

/// The typed view of the array whose entry words start `offset` bytes into the loop entry record `entry`:
/// the elements' address, count and kind, which the entry proved once (`Vm::lane_typed_array_elements`) and
/// the loop keeps, as it makes no call that could detach or resize the buffer. The kind is the record's word.
#[inline(always)]
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/heap_rows.rs`), which includes this file
pub(crate) fn entry_typed_view<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> TypedView<S::Word> {
    let address = stage.add_imm(entry, offset);
    let values = stage.load_table(Region::LoopEntry.readonly(), address);
    let length = stage.load_u64(Region::LoopEntry.readonly(), address, ARRAY_COUNT_AT);
    let kind = stage.load_u64(Region::LoopEntry.readonly(), address, ARRAY_KIND_AT);
    TypedView { values, length, kind: ViewKind::Dynamic(kind) }
}
