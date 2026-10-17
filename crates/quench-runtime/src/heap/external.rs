use super::{Cell, Heap};

impl Cell {
    pub(super) fn external_bytes(&self) -> usize {
        match self {
            Self::ArrayBuffer { bytes, .. } => bytes.capacity(),
            _ => 0,
        }
    }
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
