//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the staged row (`staged/{environment,elements,entry,value}.rs`)
//! run on the stencil stage and compiled by rustc at build time (`quench-bjit/stencils`), so a row
//! has one definition of its layout reads across the interpreter, the specializers and here.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1; a capture's slot and the slot's byte
//! offset; a site field row packs its object register's frame offset above the destination's in
//! 0, takes the narrow upper-word table in 1 and the cache entry's byte offset in 2). Every failed proof leaves through exit 0 with nothing written, so the interpreter
//! runs the instruction.

use quench_bjit::numeric::{library, Place};
use quench_bjit::Stencil;
use std::collections::HashMap;
use std::sync::Mutex;

/// What a heap row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Heap {
    /// Depth-0 capture `slot` of the running activation.
    Capture(u16),
    /// `dst = array[index]` of a dense array: the array word at hole 1, the index at hole 2.
    Index,
    /// `array[index] = value` replacing a present element in place: the array word at hole 0, the
    /// value at hole 1, the index at hole 2.
    Store,
    /// The field value the loop entry recorded: its byte offset in the record at hole 1.
    Field,
    /// The array word at hole 1 is a direct array (what `RequireObjectCoercible` of one proves).
    Array,
    /// The data field the site's field cache entry names, own or held by the receiver's prototype
    /// (`entry::site_read`), read from the object register whose frame offset sits above the
    /// destination's in hole 0 (hole 2: the entry's byte offset).
    Site,
    /// A chain of two such reads, `object.first.second` (a nested field lookup), whose entry offsets
    /// share hole 2: the first below `CHAIN_SECOND_AT` bits, the second above them.
    SiteChain,
    /// `Site` and `SiteChain` of the activation's receiver, which the payload holds: hole 0 is the
    /// destination alone.
    ThisSite,
    ThisChain,
    /// The `length` of the direct array at hole 1, an int32 (what the array's record counts).
    Length,
    /// Stores the value operand (hole 1) into the own data field the site's cache entry (hole 2) names
    /// of the object register whose frame offset is hole 0, where the store gate admits it.
    SiteStore,
    /// `SiteStore` into the activation's receiver: hole 0 is unused.
    ThisStore,
    /// Stores the value operand (hole 0) into depth-0 capture `slot` of an activation whose slots
    /// admit every direct store; the slot is hole 1 and its byte offset hole 2.
    CaptureSet(u16),
    /// The running activation's `this`.
    This,
    /// The activation's local slot `slot`, whose number is hole 1.
    LoadLocal(u16),
    /// The first operand written to the activation's local slot `slot`, whose number is hole 2.
    StoreLocal(u16),
    /// The register at the frame offset of hole 1 holds exactly the word of hole 2.
    Identity,
    /// A prototype-held field folded to the value its holder held when the code was compiled (hole 2):
    /// the object register whose frame offset sits above the destination's in hole 0 is a direct object
    /// of the exact shape in hole 1's low half and the prototype link in its high half, else the row
    /// leaves. A Stable fact holds the value (`entry::held_receiver`).
    Held,
    /// `Held` of the activation's receiver, which the payload holds: hole 0 is the destination alone.
    ThisHeld,
    /// Leaves unless the invalidation word at the address in hole 1 is zero (`code_dependencies::Invalidation`):
    /// the check before a row that holds a Stable fact where a call may have changed it.
    Live,
    /// The global object's data slot (hole 2: its byte offset in the arena run behind its extras) read into the
    /// destination, the object being the record id of hole 1: a Stable fact holds the object's layout.
    GlobalRead,
    /// The first operand stored to that slot (hole 0: the record id), where the row's `entry` variant takes the
    /// store gate for an operand that may be a heap reference, and a Number operand needs none.
    GlobalStore,
    /// The tagged word of hole 1 into the destination: an immutable global, a folded `typeof` name, and
    /// the `undefined` receiver of a name call.
    Word,
    /// The realm proof entry rows make once: the VM's active realm field (hole 1: its byte offset) names the
    /// global object word of hole 2; a mismatch fails the entry.
    Realm,
}

/// A heap row: its operation and where its operands and destination live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HeapRow {
    pub(super) heap: Heap,
    pub(super) dst: Place,
    pub(super) a: Place,
    pub(super) b: Place,
    /// The row's variant: an array's backing is the entry record's (the offset at the array hole), not
    /// looked up per access (`Index`, `Store`); a site read is a chain of two reads (`SiteChain`).
    pub(super) entry: bool,
}

impl Heap {
    /// The row's code in `quench_bjit::numeric::library`.
    pub(super) fn code(self) -> u8 {
        match self {
            Heap::Capture(_) => library::CAPTURE_ROW,
            Heap::Index => library::INDEX_ROW,
            Heap::Store => library::STORE_ROW,
            Heap::Field => library::FIELD_ROW,
            Heap::Array => library::ARRAY_ROW,
            Heap::Site | Heap::SiteChain => library::SITE_ROW,
            Heap::ThisSite | Heap::ThisChain => library::THIS_SITE_ROW,
            Heap::Length => library::LENGTH_ROW,
            Heap::CaptureSet(_) => library::CAPTURE_SET_ROW,
            Heap::SiteStore => library::SITE_STORE_ROW,
            Heap::ThisStore => library::THIS_STORE_ROW,
            Heap::This => library::THIS_ROW,
            Heap::LoadLocal(_) => library::LOAD_LOCAL_ROW,
            Heap::StoreLocal(_) => library::STORE_LOCAL_ROW,
            Heap::Identity => library::IDENTITY_ROW,
            Heap::Held | Heap::ThisHeld => library::HELD_ROW,
            Heap::Live => library::LIVE_ROW,
            Heap::GlobalRead => library::GLOBAL_READ_ROW,
            Heap::GlobalStore => library::GLOBAL_STORE_ROW,
            Heap::Word => library::CONSTANT_ROW,
            Heap::Realm => library::REALM_ROW,
        }
    }

    /// Whether the row reads a snapshot of the heap the loop's entry took (the record's captures,
    /// fields and array views) or tables a helper may move: not the activation's `this` and local
    /// slots, which every run reads live, and the rows that read no heap.
    pub(super) fn reads_snapshot(self) -> bool {
        !matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::StoreLocal(_) | Heap::Identity | Heap::Live | Heap::Word | Heap::Realm)
    }

    /// Whether the row's result is a tagged word whatever the registers around it hold: a value
    /// the activation holds, not a Number the region proved.
    pub(super) fn yields_tagged(self) -> bool {
        matches!(self, Heap::This | Heap::LoadLocal(_) | Heap::Word)
    }
}

/// The stencil of `row`, built at compile time by rustc; a row the library lacks declines the region.
pub(super) fn stencil(row: HeapRow) -> Result<&'static Stencil, String> {
    static TABLE: Mutex<Option<HashMap<library::HeapCode, &'static Stencil>>> = Mutex::new(None);
    let code = library::heap_canonical((row.heap.code(), row.entry, row.dst.code(), row.a.code(), row.b.code()));
    let mut table = TABLE.lock().map_err(|e| e.to_string())?;
    let table = table.get_or_insert_with(HashMap::new);
    if let Some(found) = table.get(&code) {
        return Ok(found);
    }
    let found = quench_bjit::rustc::stencil(&library::heap_name(code)).ok_or_else(|| format!("no stencil row {row:?}"))?;
    let stencil: &'static Stencil = Box::leak(Box::new(found));
    table.insert(code, stencil);
    Ok(stencil)
}

#[cfg(test)]
mod tests {

    /// The numbers the stencil rows are compiled with (`quench-bjit/stencils/layout.rs`) are
    /// the runtime's own.
    use super::super::{CallContext, JsStepDispatch};
    #[allow(dead_code)]
    mod stencil_layout {
        include!("../../../../quench-bjit/stencils/layout.rs");
    }

    #[test]
    fn stencil_layout_matches() {
        use crate::value::{NarrowLayout, ValueLayout};
        use stencil_layout as mirror;
        eprintln!("OFFSETS {} {} {} {} {} {}", std::mem::offset_of!(CallContext, adapter_payload), std::mem::offset_of!(JsStepDispatch, loop_entry), std::mem::offset_of!(JsStepDispatch, capture_values), std::mem::offset_of!(JsStepDispatch, capture_len), std::mem::offset_of!(JsStepDispatch, array_record_base), std::mem::offset_of!(JsStepDispatch, returned_pc));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_OBJECT_LAYOUT), format!("{:?}", mirror::NATIVE_OBJECT_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_ARRAY_LAYOUT), format!("{:?}", mirror::NATIVE_ARRAY_LAYOUT));
        assert_eq!(format!("{:?}", crate::heap::NATIVE_ENVIRONMENT_LAYOUT), format!("{:?}", mirror::NATIVE_ENVIRONMENT_LAYOUT));
        let (cache, copy) = (crate::vm::NATIVE_FIELD_CACHE_LAYOUT, mirror::NATIVE_FIELD_CACHE_LAYOUT);
        assert_eq!([cache.entry_size, cache.receiver_offset, cache.holder_offset, cache.slot_offset], [copy.entry_size, copy.receiver_offset, copy.holder_offset, copy.slot_offset]);
        assert_eq!(
            [cache.receiver_complement, cache.holder_complement, cache.slot_complement, cache.own_holder],
            [copy.receiver_complement, copy.holder_complement, copy.slot_complement, copy.own_holder]
        );
        assert_eq!(
            [crate::heap::OTHERS_UNKNOWN as u64, crate::heap::LENGTH_FROM_VALUES as i64 as u64, crate::heap::OTHERS_FROM_VALUES as i64 as u64, crate::value_vec::INLINE_PROPERTY_COUNT as u64],
            [mirror::OTHERS_UNKNOWN as u64, mirror::LENGTH_FROM_VALUES as i64 as u64, mirror::OTHERS_FROM_VALUES as i64 as u64, mirror::INLINE_PROPERTIES as u64]
        );
        use mirror::words as w;
        assert_eq!(
            [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, crate::Value::NULL.to_bits(), crate::Value::UNDEFINED.to_bits(), crate::Value::DELETED.to_bits(), crate::Value::FALSE.to_bits(), crate::Value::TRUE.to_bits()],
            [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE, w::TRUE]
        );
        assert_eq!(
            [crate::value::LOWER_WORD, crate::value::HEAP_FAMILY_BITS, u64::from(crate::value::HEAP_FAMILY_SHIFT), crate::value::NARROW_HEAP_FAMILIES as u64, crate::value::TAG_UNDEFINED, crate::value::TAG_NULL, crate::value::TAG_BOOL, crate::value::TAG_DELETED],
            [w::LOWER_WORD, w::HEAP_FAMILY_BITS, u64::from(w::HEAP_FAMILY_SHIFT), w::NARROW_HEAP_FAMILIES as u64, w::UNDEFINED, w::NULL, w::FALSE, w::DELETED]
        );
        assert_eq!(
            [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
            [u64::from(w::NARROW_INT_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
        );
        assert_eq!(
            [std::mem::offset_of!(CallContext, adapter_payload), std::mem::offset_of!(JsStepDispatch, loop_entry), std::mem::offset_of!(JsStepDispatch, capture_values), std::mem::offset_of!(JsStepDispatch, capture_len), std::mem::offset_of!(JsStepDispatch, array_record_base), std::mem::offset_of!(JsStepDispatch, returned_pc)],
            [mirror::CALL_CONTEXT_ADAPTER_PAYLOAD, mirror::JS_STEP_LOOP_ENTRY, mirror::JS_STEP_CAPTURE_VALUES, mirror::JS_STEP_CAPTURE_LEN, mirror::JS_STEP_ARRAY_RECORD_BASE, mirror::JS_STEP_RETURNED_PC]
        );
        assert_eq!(
            [std::mem::offset_of!(JsStepDispatch, object_record_base), std::mem::offset_of!(JsStepDispatch, property_buffer), std::mem::offset_of!(JsStepDispatch, field_entries), crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.base, super::super::entry_cell::CAPTURE_ADDRESS_BITS as usize, std::mem::offset_of!(JsStepDispatch, capture_stores), super::super::super::dispatch_fast::CAPTURE_STORES,
             std::mem::offset_of!(JsStepDispatch, object_mark_blocks), std::mem::offset_of!(JsStepDispatch, object_mark_block_count), std::mem::offset_of!(JsStepDispatch, slots_tracked), std::mem::offset_of!(JsStepDispatch, string_record_base), std::mem::offset_of!(JsStepDispatch, receiver), std::mem::offset_of!(JsStepDispatch, native_locals), std::mem::offset_of!(JsStepDispatch, local_count)],
            [mirror::JS_STEP_OBJECT_RECORD_BASE, mirror::JS_STEP_PROPERTY_BUFFER, mirror::JS_STEP_FIELD_ENTRIES, mirror::PROPERTY_BUFFER_BASE, mirror::CAPTURE_ADDRESS_BITS as usize, mirror::JS_STEP_CAPTURE_STORES, mirror::CAPTURE_STORES_BIT as usize,
             mirror::JS_STEP_OBJECT_MARK_BLOCKS, mirror::JS_STEP_OBJECT_MARK_BLOCK_COUNT, mirror::JS_STEP_SLOTS_TRACKED, mirror::JS_STEP_STRING_RECORD_BASE, mirror::JS_STEP_RECEIVER, mirror::JS_STEP_NATIVE_LOCALS, mirror::JS_STEP_LOCAL_COUNT]
        );
        use crate::vm::wtf16::native_string as form;
        assert_eq!(
            [form::RECORD_SIZE as i32, form::TAG_OFFSET, i32::from(form::TAG_SHORT), i32::from(form::TAG_ASCII), i32::from(form::TAG_WIDE), form::SHORT_LENGTH_OFFSET, form::SHARED_LENGTH_OFFSET],
            [mirror::string_form::RECORD_SIZE as i32, mirror::string_form::TAG_OFFSET, i32::from(mirror::string_form::TAG_SHORT), i32::from(mirror::string_form::TAG_ASCII), i32::from(mirror::string_form::TAG_WIDE), mirror::string_form::SHORT_LENGTH_OFFSET, mirror::string_form::SHARED_LENGTH_OFFSET]
        );
    }
}
