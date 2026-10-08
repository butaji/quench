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
            self.peak_live,
            self.peak_survivors,
            self.max_threshold,
            self.external_bytes,
        )
    }

    pub(crate) fn adjust_external_bytes(&mut self, before: usize, after: usize) {
        match after.cmp(&before) {
            std::cmp::Ordering::Greater => self.external_bytes += after - before,
            std::cmp::Ordering::Less => {
                self.external_bytes = self.external_bytes.saturating_sub(before - after)
            }
            std::cmp::Ordering::Equal => {}
        }
    }
}
