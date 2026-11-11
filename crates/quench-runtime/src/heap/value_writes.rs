//! What a property value write observes: the shape cache's slot note, and the epoch a folded
//! construct value watches (a write to the object it names retires the fold).

use super::*;

impl Heap {
    pub(super) fn note_value_slot(&mut self, object: Value, slot: usize, value: Value) {
        self.note_value_dependents(object);
        if !self.properties.tracks() { return; }
        if let Some(data) = self.object(object) {
            let shape = data.exact_shape();
            if shape == data.shape() && slot < self.properties.shape_length(shape) {
                self.properties.note_slot_value(shape, slot, value);
            }
        }
    }

    /// The objects a folded property value names: a write to one advances `value_epoch`.
    pub(crate) fn value_epoch(&self) -> u64 { self.value_epoch }
    pub(crate) fn mark_value_dependents(&mut self, object: Value) {
        if let Some(data) = self.object_store_mut(object, Value::UNDEFINED) { data.mark_value_dependents(); }
    }
    pub(super) fn note_value_dependents(&mut self, object: Value) {
        if self.object(object).is_some_and(|data| data.has_value_dependents()) {
            self.value_epoch = self.value_epoch.wrapping_add(1);
        }
    }
}
