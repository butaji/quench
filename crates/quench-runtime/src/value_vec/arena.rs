//! The arena's lifecycle: its buffer's offset and native view, and its reset to empty.
use super::*;

impl ValueArena {
    pub(crate) const BUFFER_OFFSET: usize = std::mem::offset_of!(Self, values);
    /// The current storage for out-of-line object properties.
    ///
    /// Native readers remain confined to no-safepoint execution: reserved storage
    /// keeps its base, but compaction relocates ranges and allocation fallback can
    /// move the buffer. Every derived slot must be checked against `len`.
    pub(crate) fn native_values(&self) -> (*const Value, usize) {
        (self.values.as_ptr(), self.values.len())
    }

    pub(crate) fn reset(&mut self) {
        self.widened_shapes.clear();
        self.shape_readers.reset();
        self.values.clear();
        self.free = [FreeRuns::EMPTY; BUCKETS];
        self.released_values = 0;
        self.settled_values = 0;
        self.shape_lengths.truncate(1);
        self.shape_lengths[0] = 0;
        self.dictionaries.clear();
        self.free_dictionaries.clear();
        self.next_dictionary = 0;
        self.nursery = Nursery::default();
        self.forget_reservations();
    }
}
