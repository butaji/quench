//! What a property value write observes: the shape cache's slot note.

use super::*;

impl Heap {
    pub(super) fn note_value_slot(&mut self, object: Value, slot: usize, value: Value) {
        if !self.properties.tracks() { return; }
        if let Some(data) = self.object(object) {
            let shape = data.exact_shape();
            if shape == data.shape() && slot < self.properties.shape_length(shape) {
                self.properties.note_slot_value(shape, slot, value);
            }
        }
    }
}
