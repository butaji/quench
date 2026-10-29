use super::{Cell, Heap};
use crate::value::Value;

impl Cell {
    pub(super) fn external_bytes(&self) -> usize {
        match self {
            Self::ArrayBuffer { bytes, .. } => bytes.capacity(),
            _ => 0,
        }
    }

    /// The memory a cell owns beyond its slot, which allocation weight and live weight
    /// charge: array element storage, Map and Set entries, and external buffers. A string
    /// record's units are charged by `JsString::flat_unit_bytes` (`Heap::alloc_string`).
    pub(super) fn payload_bytes(&self) -> usize {
        match self {
            Self::Array { object } => array_payload_bytes(object),
            Self::Map { entries, .. } => entries.payload_bytes(),
            Self::Set { entries, .. } => entries.payload_bytes(),
            cell => cell.external_bytes(),
        }
    }
}

/// Element storage an array owns.
pub(super) fn array_payload_bytes(array: &super::ArrayObject) -> usize {
    array.elements().capacity() * size_of::<super::Value>()
}

impl Heap {
    #[allow(dead_code)]
    pub fn stats(&self) -> (u64, u64, usize, usize, usize, usize) {
        (
            self.total_allocations,
            self.collections,
            self.peak_live.max(self.live_slots()),
            self.peak_survivors,
            self.max_threshold,
            self.external_bytes,
        )
    }

    /// Changes the legacy cell `cell` through `change`, charging what its payload
    /// (`Cell::payload_bytes`) grew toward the next collection, as `alloc` charges a new
    /// cell's: a Map or Set filled after it was made makes collections due like its
    /// allocation would have.
    pub(crate) fn grow_cell_payload<R>(&mut self, cell: Value, change: impl FnOnce(&mut Cell) -> R) -> Option<R> {
        let cell = self.get_mut(cell)?;
        let before = cell.payload_bytes();
        let result = change(cell);
        let grown = cell.payload_bytes().saturating_sub(before);
        self.charge_payload_growth(grown);
        Some(result)
    }

    /// Records a buffer, Wasm memory or table resizing outside `alloc`. Growth counts toward
    /// the next collection like any other payload (TOD-125).
    pub(crate) fn adjust_external_bytes(&mut self, before: usize, after: usize) {
        match after.cmp(&before) {
            std::cmp::Ordering::Greater => {
                self.charge_external_growth(after - before);
                self.charge_payload_growth(after - before);
            }
            std::cmp::Ordering::Less => {
                self.external_bytes = self.external_bytes.saturating_sub(before - after)
            }
            std::cmp::Ordering::Equal => {}
        }
    }
}
