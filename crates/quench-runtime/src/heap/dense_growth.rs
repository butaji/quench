//! How dense element storage grows past its holes: the gap a write may leave, and a fill of a
//! whole sparse tail.
use super::*;

/// The longest dense storage built in one resize: a quarter of the capacity word's range, so
/// the growth policy's rounding stays within it (`ArrayElements::allocate`).
pub(crate) const MAX_RESIZE_LENGTH: usize = (array_elements::BIASED / 4) as usize;

/// Holes a write may leave below `index` in dense storage. Within the dense hole budget a
/// gap this wide is bounded memory; past it a hole must cost no more than a sparse entry,
/// else a strided write pattern (`a[i * 1000]`) would take dense storage of its whole length.
pub(crate) fn dense_write_gap_limit(index: usize) -> usize {
    const GAP_WITHIN_BUDGET: usize = 1024;
    if index < MAX_DENSE_ARRAY_HOLE_LENGTH { GAP_WITHIN_BUDGET } else { SPARSE_ENTRY_SLOTS - 1 }
}

impl Heap {
    /// Whether `indices` is everything from the array's dense length to its end, which a
    /// whole-tail fill replaces (`fill_array_tail`) within the longest resize.
    pub(crate) fn fills_sparse_tail(&self, array: Value, indices: &std::ops::Range<usize>) -> bool {
        let dense = self.array_elements(array).map_or(0, |dense| dense.len());
        indices.start <= dense
            && indices.end > dense
            && indices.end == self.sparse_length(array).unwrap_or(dense)
            && indices.end <= MAX_RESIZE_LENGTH
    }

    /// Makes every index from the dense length up to the array's length hold `value`, as a
    /// run of stores would, in dense storage taking the whole length at once: the sparse
    /// elements all lie in the range being overwritten, so none survive.
    pub(crate) fn fill_array_tail(&mut self, array: Value, value: Value) {
        self.note_prototype_elements(array);
        self.write_barrier(array);
        let length = self.sparse_length(array).or_else(|| self.array_elements(array).map(|dense| dense.len()));
        if let Some(arrays) = &mut self.sparse_arrays {
            arrays.remove(&array);
            if arrays.is_empty() {
                self.sparse_arrays = None;
            }
        }
        self.grow_array_elements(array, |elements| elements.resize(length.unwrap_or(0), value));
    }
}
