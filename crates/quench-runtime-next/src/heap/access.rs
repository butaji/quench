use super::{Cell, Heap};
use crate::value::Value;

impl Heap {
    pub fn get(&self, value: Value) -> Option<&Cell> {
        let index = value.heap_index()? as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the explicit length check proves the slab index is in range.
        unsafe { self.slots.get_unchecked(index).cell.as_ref() }
    }

    pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
        let index = value.heap_index()? as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the explicit length check proves the slab index is in range.
        unsafe { self.slots.get_unchecked_mut(index).cell.as_mut() }
    }
}
