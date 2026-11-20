//! The store barrier's gate, as the staged store reads it.
use super::{Heap, Object};
use crate::staged::StagedRecordSpace;
use crate::Value;

impl Heap {
    /// Whether a store of `value` into record `record_id` of `space` needs neither the arena's
    /// observation (`ValueArena::tracks`) nor a barrier log (`RecordSpace::store_logs`): the
    /// store's gate (`staged::Stage::store_gate`).
    #[inline(always)]
    pub(crate) fn store_unlogged(&self, space: StagedRecordSpace, record_id: u32, value: Value) -> bool {
        // The common store: nothing watches, and the value names no record a log could need.
        if !self.store_watched && value.heap_index().is_none() {
            return true;
        }
        let logs = match space {
            StagedRecordSpace::Object => self.object_records.store_logs(record_id, value),
            StagedRecordSpace::Function => self.function_records.store_logs(record_id, value),
        };
        !self.tracks_slot_values() && !logs
    }

    /// Recomputes `store_watched` after a gate input changed: the snapshot barrier
    /// reference counts (`set_rc`) or slot observation (`track_slot_representations`).
    pub(super) fn refresh_store_watch(&mut self) {
        self.store_watched = self.rc_active || self.properties.tracks();
    }

    /// The word native stores read as their gate (`JsStepHead::store_watch`): `store_watched`,
    /// split into its inputs, slot observation (`STORE_WATCH_TRACKS`) and reference counts
    /// (`STORE_WATCH_COUNTS`). Zero exactly when `store_watched` is clear.
    #[inline(always)]
    pub(crate) fn store_watch_word(&self) -> usize {
        use crate::staged::layout_offsets::{STORE_WATCH_COUNTS, STORE_WATCH_TRACKS};
        let tracks = if self.properties.tracks() { STORE_WATCH_TRACKS } else { 0 };
        let counts = if self.rc_active { STORE_WATCH_COUNTS } else { 0 };
        (tracks | counts) as usize
    }

    /// Whether a store of a value that names no heap record needs no barrier log: always, unless
    /// old records keep counts (`RecordSpace::store_logs`). What a staged store proved immediate
    /// still asks (`staged::Stage::immediate_store_open`).
    #[inline(always)]
    pub(crate) fn immediate_store_unlogged(&self) -> bool {
        !self.rc_active
    }

    /// Whether the property arena records its slots' value classes (`ValueArena::tracks`).
    #[inline(always)]
    pub(crate) fn tracks_slot_values(&self) -> bool {
        self.properties.tracks()
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

    /// The `Object` part of function record `record_id` for a store that `store_unlogged` admitted.
    ///
    /// # Safety
    /// `record_id` names a live function record, and `&mut self` is exclusive.
    #[inline(always)]
    pub(crate) unsafe fn unlogged_function_object(&mut self, record_id: u32) -> *mut Object {
        // SAFETY: the caller's contract; the barrier logs nothing for the admitted store.
        let record: *mut _ = unsafe { self.function_records.get_live_mut_unbarriered(record_id) };
        // SAFETY: a function record holds its `Object` at the layout's offset (`NATIVE_FUNCTION_LAYOUT`).
        unsafe { record.cast::<u8>().add(super::NATIVE_FUNCTION_LAYOUT.object_offset).cast() }
    }
}
