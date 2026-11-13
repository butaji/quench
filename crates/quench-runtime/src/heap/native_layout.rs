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
    /// Equal values mean the four record bases and the mark blocks are as last read. A
    /// snapshot restore or another heap draws a new `layout_id`, so equal values are never a different layout.
    pub(crate) fn native_layout_epoch(&self) -> u64 {
        self.collections + (self.layout_id << Self::LAYOUT_ID_SHIFT)
            + (self.object_records.block_count() + self.function_records.block_count() + self.array_records.block_count()
                + self.string_records.block_count()) as u64
    }

    /// The fixed base of the string record range once it has been reserved: native readers
    /// read a string's form in place (`vm::wtf16::native_string`) from a current live, rooted
    /// direct-string Value, and finish before any safepoint (a parts string is written out
    /// there, and its form is none a row reads).
    pub(crate) fn string_record_base(&self) -> Option<*const JsString> {
        self.string_records.base_ptr()
    }

    /// The table of one-unit ASCII strings by unit (`ascii_unit_strings`), `UNDEFINED` for a
    /// unit with no string yet: where native code reads `charAt`'s result. A collection
    /// empties it, so a reader takes the base again after any safepoint.
    pub(crate) fn ascii_unit_strings_base(&self) -> Option<*const Value> {
        self.ascii_unit_strings.as_ref().map(|strings| strings.as_ptr())
    }
}
