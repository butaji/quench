//! The generational write barrier: an old (marked) cell written to joins the remembered set once, so a
//! young collection scans it for references to young cells.
use crate::heap::Heap;
use crate::value::{HeapReference, Value};

impl Heap {
    /// `remember` of `value`'s cell, for a write made through a pointer to its storage.
    #[inline(always)]
    pub(crate) fn remember_cell(&mut self, value: Value) {
        match value.heap_reference() {
            Some(HeapReference::LegacySlot(index)) => self.remember(index as usize),
            // An environment record passes the record barrier: unmarked and logged once.
            Some(HeapReference::EnvironmentRecord(record_id)) => self.environment_records.remember(record_id),
            _ => {}
        }
    }

    /// Whether `value`'s cell is in the remembered set: its writes need no barrier until the
    /// next collection.
    pub(crate) fn is_remembered_cell(&self, value: Value) -> bool {
        if let Some(record_id) = value.direct_environment_index() {
            // An unmarked record (young, or logged since the last collection) takes stores
            // with no barrier (`RecordSpace::store_logs`).
            return !self.environment_records.is_marked(record_id);
        }
        let Some(HeapReference::LegacySlot(index)) = value.heap_reference() else {
            return false;
        };
        let (word, bit) = crate::heap::slot_bit(index as usize);
        self.remembered_marks.get(word).is_some_and(|marks| marks & bit != 0)
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
