//! The store barrier's gate, as the staged store reads it.
use super::{Heap, Object};
use crate::Value;

impl Heap {
    /// Whether a store of `value` into object record `record_id` needs neither the arena's
    /// observation (`ValueArena::tracks`) nor a barrier log (`RecordSpace::store_logs`): the
    /// store's gate (`staged::Stage::store_gate`).
    #[inline(always)]
    pub(crate) fn store_unlogged(&self, record_id: u32, value: Value) -> bool {
        !self.properties.tracks() && !self.object_records.store_logs(record_id, value)
    }

    /// The first arena value, which a flat run's position offsets (`staged::Table::PropertyValues`).
    #[inline(always)]
    pub(crate) fn arena_values_base(&self) -> *const Value {
        self.properties.values_base()
    }

    /// Object record `record_id` for a store that `store_unlogged` admitted.
    ///
    /// # Safety
    /// `record_id` names a live object record, and `&mut self` is exclusive.
    #[inline(always)]
    pub(crate) unsafe fn unlogged_object_record(&mut self, record_id: u32) -> *mut Object {
        // SAFETY: the caller's contract; the barrier logs nothing for the admitted store.
        unsafe { self.object_records.get_live_mut_unbarriered(record_id) }
    }
}
