//! The generational write barrier: an old (marked) cell written to joins the remembered set once, so a
//! young collection scans it for references to young cells.
use crate::heap::Heap;
use crate::value::{HeapReference, Value};

impl Heap {
    /// `remember` of `value`'s cell, for a write made through a pointer to its storage.
    #[inline(always)]
    pub(crate) fn remember_cell(&mut self, value: Value) {
        if let Some(HeapReference::LegacySlot(index)) = value.heap_reference() {
            self.remember(index as usize);
        }
    }

    /// The write barrier: an old (marked) cell written to joins the remembered set once.
    #[inline(always)]
    pub(in crate::heap) fn remember(&mut self, index: usize) {
        if !self.tracks_young_cells()
            || index >= self.slots.len()
            || !Heap::marked(&self.marks, index)
        {
            return;
        }
        let (word, bit) = crate::heap::slot_bit(index);
        // A cell already remembered needs nothing more: the common repeated write.
        if self.remembered_marks.get(word).is_some_and(|marks| marks & bit != 0) {
            return;
        }
        self.remember_first(word, bit, index);
    }

    /// `remember` of a cell not yet in the remembered set.
    #[inline(never)]
    fn remember_first(&mut self, word: usize, bit: u64, index: usize) {
        self.remembered_marks.resize(self.marks.len(), 0);
        self.remembered_marks[word] |= bit;
        self.remembered.push(index as u32);
    }
}
