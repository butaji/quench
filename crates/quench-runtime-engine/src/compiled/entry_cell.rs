//! The one entry word of a function's lane body, which native callers read.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;


pub(crate) use quench_runtime::vm::engine_types::EntryCell;

pub(crate) use quench_runtime::vm::engine_types::RowEntries;

const _: () = {
    use quench_runtime::staged::call::ENTRY_CELL;
    assert!(std::mem::size_of::<EntryCell>() == 128);
    assert!(std::mem::offset_of!(EntryCell, entry) as i32 == ENTRY_CELL.entry_offset);
    assert!(std::mem::offset_of!(EntryCell, body) as i32 == ENTRY_CELL.body_offset);
    assert!(std::mem::offset_of!(EntryCell, facts) as i32 == ENTRY_CELL.facts_offset);
    assert!(std::mem::offset_of!(EntryCell, recovery) as i32 == ENTRY_CELL.recovery_offset);
    assert!(std::mem::offset_of!(EntryCell, counters) as i32 == ENTRY_CELL.counters_offset);
    assert!(std::mem::offset_of!(EntryCell, scalar) as i32 == ENTRY_CELL.scalar_offset);
    assert!(std::mem::offset_of!(EntryCell, rows) as i32 == ENTRY_CELL.rows_offset);
    assert!(std::mem::offset_of!(EntryCell, proven) as i32 == ENTRY_CELL.proven_offset);
    assert!(std::mem::offset_of!(EntryCell, numbers) as i32 == ENTRY_CELL.numbers_offset);
    assert!(std::mem::offset_of!(EntryCell, uses_this) as i32 == ENTRY_CELL.uses_this_offset);
};









#[quench_macros::ext(EntryCellOps1)]
impl EntryCell {
    /// A cell of an unpublished function, whose entry is `stub`, the engine's entry stub (`lane_body_abi::entry_stub`).
    pub(crate) fn new(stub: usize) -> Self {
        let cell = Self { entry: Default::default(), recovery: Default::default(), body: Default::default(), facts: Default::default(), counters: Default::default(), scalar: Default::default(), rows: Default::default(), proven: Default::default(), numbers: Default::default(), uses_this: std::cell::Cell::new(1),
            proven_entry: Default::default(), proven_body: Default::default(), stub: std::cell::Cell::new(stub) };
        cell.unpublish();
        cell
    }

    /// Records the function's facts, which its native callers admit a call by.
    pub(crate) fn set_facts(&self, facts: quench_runtime::staged::call::CellFacts) { self.facts.set(facts.word()) }

}

pub(crate) use quench_bjit::shared_words::CAPTURE_ADDRESS_BITS;
/// The bit of the address a store tag takes: slots are aligned past it (`CAPTURE_STORES`).
const CAPTURE_STORES_BIT: u64 = crate::dispatch_fast::CAPTURE_STORES as u64;
/// The mask of the address bits.
pub(crate) const CAPTURE_ADDRESS_MASK: i64 = ((1 << CAPTURE_ADDRESS_BITS) - 1) & !(CAPTURE_STORES_BIT as i64);

/// The one word a body's entry takes its captures in: the dense slots' address and count, and
/// whether they admit every direct store (`dispatch_fast::capture_stores`).
#[inline]
pub(crate) fn capture_word(values: *const Value, len: usize, stores: bool) -> u64 {
    debug_assert!(values.addr() >> CAPTURE_ADDRESS_BITS == 0 && len >> (u64::BITS - CAPTURE_ADDRESS_BITS) == 0);
    values.addr() as u64 | u64::from(stores) * CAPTURE_STORES_BIT | (len as u64) << CAPTURE_ADDRESS_BITS
}

/// Grows the scratch window to `width` registers for a body a run may enter: an inlined leave
/// inside it fills its window. A run finds the window sized by its body's publication.
pub(crate) fn reserve_lane_scratch(width: usize) {
    let frame = super::frame_in_lane();
    // SAFETY: as `lane_frame`; native code reads the window's address from the dispatch
    // record at each use, so the record follows a resize.
    unsafe {
        if (*frame).scratch.len() < width {
            (*frame).scratch.resize(width, Value::UNDEFINED);
            (*frame).dispatch.inlined = (*frame).scratch.as_mut_ptr().addr();
        }
    }
}
