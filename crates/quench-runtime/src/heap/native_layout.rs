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
    #[inline]
    pub(crate) fn native_layout_epoch(&self) -> u64 {
        self.collections + (self.layout_id << Self::LAYOUT_ID_SHIFT)
            + (self.object_records.block_count() + self.function_records.block_count() + self.array_records.block_count()
                + self.string_records.block_count()) as u64
    }

    /// The fixed base of the string record range once it has been reserved: native readers
    /// read a string's form in place (`vm::wtf16::native_string`) from a current live, rooted
    /// direct-string Value, and finish before any safepoint (a parts string is written out
    /// there, and its form is none a row reads).
    #[inline(always)]
    pub(crate) fn string_record_base(&self) -> Option<*const JsString> {
        self.string_records.base_ptr()
    }

    /// `string_record_base` untested, for a read that proves its receiver a direct string with a
    /// record before it addresses the table.
    #[inline(always)]
    pub(crate) fn live_string_records(&self) -> *const JsString {
        self.string_records.live_base()
    }

    /// The fixed base of the Wasm GC record range once it has been reserved (`NATIVE_WASM_GC_LAYOUT`
    /// gives a record's payload words, the record's stride is `size_of::<WasmGcRecord>()`). Native code
    /// combines it with a live, rooted direct Wasm GC Value's record ID; the base never moves once reserved.
    #[inline(always)]
    pub(crate) fn wasm_gc_record_base(&self) -> Option<*const WasmGcRecord> {
        self.wasm_gc_records.base_ptr()
    }

    /// The table of one-unit ASCII strings by unit (`ascii_unit_strings`), `UNDEFINED` for a
    /// unit with no string yet: where native code reads `charAt`'s result. A collection
    /// empties it, so a reader takes the base again after any safepoint.
    pub(crate) fn ascii_unit_strings_base(&self) -> Option<*const Value> {
        self.ascii_unit_strings.as_ref().map(|strings| strings.as_ptr())
    }
}
