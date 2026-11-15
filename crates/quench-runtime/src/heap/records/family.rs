use super::*;

/// What the collector does to every record family alike, whatever its record type: the
/// per-collection passes iterate `Heap::record_families` instead of naming each space.
pub(in crate::heap) trait RecordFamily {
    /// Every record becomes young: a full collection's starting state.
    fn clear_marks(&mut self);
    /// The records the barrier logged since the last collection; each is unmarked.
    fn take_remembered(&mut self) -> Vec<RecordId>;
    /// Before a minor collection marks under `age`: which records are newborn
    /// (`RecordSpace::prepare_minor`), with the barrier's `logged` records, old, among them.
    fn prepare_minor(&mut self, logged: &[RecordId], age: PromotionAge);
    /// The records the last minor sweep promoted from aged, emptied.
    fn take_promoted(&mut self) -> Vec<RecordId>;
    /// Every aged record becomes old: a full collection promotes every survivor.
    fn forget_ages(&mut self);
    /// `RecordSpace::begin_satb`, `end_satb` and `take_satb_values`.
    fn begin_satb(&mut self);
    fn end_satb(&mut self);
    fn take_satb_values(&mut self) -> Vec<Value>;
    fn satb_values_empty(&self) -> bool;
    fn satb_values_len(&self) -> usize;
    /// What minor sweeps found of the aged records since the last call, emptied.
    fn take_aged_fates(&mut self) -> AgedFates;
    fn live_count(&self) -> usize;
    /// Bytes of the occupied ids: live plus unswept garbage before a sweep, the survivors
    /// after it (pacing's resident measure).
    fn occupied_bytes(&self) -> usize;
    /// Visits every live record's id.
    fn for_each_live_id(&self, visit: &mut dyn FnMut(RecordId));
    #[cfg(feature = "profile-aggregate")]
    fn high_water(&self) -> usize;
    #[cfg(feature = "profile-aggregate")]
    fn remembered_count(&self) -> usize;
    #[cfg(debug_assertions)]
    fn snapshot_marks(&self) -> Vec<u64>;
    #[cfg(debug_assertions)]
    fn restore_marks(&mut self, marks: &[u64]);
    #[cfg(debug_assertions)]
    fn for_each_marked(&self, visit: &mut dyn FnMut(RecordId));
}

impl<T, const CAPACITY: usize, const HOLE: usize> RecordFamily for RecordSpace<T, CAPACITY, HOLE> {
    fn clear_marks(&mut self) {
        RecordSpace::clear_marks(self);
    }
    fn take_remembered(&mut self) -> Vec<RecordId> {
        RecordSpace::take_remembered(self)
    }
    fn prepare_minor(&mut self, logged: &[RecordId], age: PromotionAge) {
        RecordSpace::prepare_minor(self, logged, age);
    }
    fn take_promoted(&mut self) -> Vec<RecordId> {
        std::mem::take(&mut self.promoted)
    }
    fn forget_ages(&mut self) {
        RecordSpace::forget_ages(self);
    }
    fn begin_satb(&mut self) {
        RecordSpace::begin_satb(self);
    }
    fn end_satb(&mut self) {
        RecordSpace::end_satb(self);
    }
    fn take_satb_values(&mut self) -> Vec<Value> {
        RecordSpace::take_satb_values(self)
    }
    fn satb_values_empty(&self) -> bool {
        self.satb_values.is_empty()
    }
    fn satb_values_len(&self) -> usize {
        self.satb_values.len()
    }
    fn take_aged_fates(&mut self) -> AgedFates {
        std::mem::take(&mut self.aged_fates)
    }
    fn live_count(&self) -> usize {
        RecordSpace::live_count(self)
    }
    fn occupied_bytes(&self) -> usize {
        self.live.saturating_mul(size_of::<T>())
    }
    fn for_each_live_id(&self, visit: &mut dyn FnMut(RecordId)) {
        self.for_each_live(|id, _| visit(id));
    }
    #[cfg(feature = "profile-aggregate")]
    fn high_water(&self) -> usize {
        RecordSpace::high_water(self)
    }
    #[cfg(feature = "profile-aggregate")]
    fn remembered_count(&self) -> usize {
        RecordSpace::remembered_count(self)
    }
    #[cfg(debug_assertions)]
    fn snapshot_marks(&self) -> Vec<u64> {
        RecordSpace::snapshot_marks(self)
    }
    #[cfg(debug_assertions)]
    fn restore_marks(&mut self, marks: &[u64]) {
        RecordSpace::restore_marks(self, marks);
    }
    #[cfg(debug_assertions)]
    fn for_each_marked(&self, visit: &mut dyn FnMut(RecordId)) {
        RecordSpace::for_each_marked(self, visit);
    }
}
