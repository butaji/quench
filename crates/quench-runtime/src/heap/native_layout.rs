//! What native code may cache of the record spaces.
use super::*;

impl Heap {
    /// One monotone counter over every change to the record bases and mark blocks native code
    /// caches: a collection, or a new block of any record family (the first reserves the
    /// family's range; each can move the block list `native_object_mark_blocks` names).
    /// Equal values mean the three record bases and the mark blocks are as last read. A
    /// snapshot restore resets it, so a reader must drop its cached value there.
    #[expect(dead_code, reason = "the first caller is the lane entry that skips refresh_object_bases")]
    pub(crate) fn native_layout_epoch(&self) -> u64 {
        self.collections
            + (self.object_records.block_count() + self.function_records.block_count() + self.array_records.block_count()) as u64
    }
}
