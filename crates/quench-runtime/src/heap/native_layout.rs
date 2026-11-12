//! What native code may cache of the record spaces.
use super::*;

impl Heap {
    /// The layout id sits above any collection and block count in `native_layout_epoch`.
    const LAYOUT_ID_SHIFT: u32 = 32;

    pub(super) fn next_layout_id() -> u64 {
        static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// One monotone counter over every change to the record bases and mark blocks native code
    /// caches: a collection, or a new block of any record family (the first reserves the
    /// family's range; each can move the block list `native_object_mark_blocks` names).
    /// Equal values mean the three record bases and the mark blocks are as last read. A
    /// snapshot restore or another heap draws a new `layout_id`, so equal values are never a different layout.
    pub(crate) fn native_layout_epoch(&self) -> u64 {
        self.collections + (self.layout_id << Self::LAYOUT_ID_SHIFT)
            + (self.object_records.block_count() + self.function_records.block_count() + self.array_records.block_count()) as u64
    }
}
