//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the staged row (`staged/{environment,elements,entry,value}.rs`)
//! run on the stencil stage and compiled by rustc at build time (`quench-bjit/stencils`), so a row
//! has one definition of its layout reads across the interpreter, the specializers and here.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1; a capture's slot and the slot's byte
//! offset). Every failed proof leaves through exit 0 with nothing written, so the interpreter
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
}

/// A heap row: its operation and where its operands and destination live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HeapRow {
    pub(super) heap: Heap,
    pub(super) dst: Place,
    pub(super) a: Place,
    pub(super) b: Place,
    /// The array's backing is the entry record's (the offset at the array hole), not looked up per access.
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
        }
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

#[cfg(all(test, target_arch = "x86_64"))]
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
        eprintln!("OFFSETS {} {} {} {} {}", std::mem::offset_of!(CallContext, adapter_payload), std::mem::offset_of!(JsStepDispatch, loop_entry), std::mem::offset_of!(JsStepDispatch, capture_values), std::mem::offset_of!(JsStepDispatch, capture_len), std::mem::offset_of!(JsStepDispatch, array_record_base));
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
            [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, crate::Value::NULL.to_bits(), crate::Value::UNDEFINED.to_bits(), crate::Value::DELETED.to_bits(), crate::Value::FALSE.to_bits()],
            [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE]
        );
        assert_eq!(
            [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
            [u64::from(w::NARROW_INT_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
        );
        assert_eq!(
            [std::mem::offset_of!(CallContext, adapter_payload), std::mem::offset_of!(JsStepDispatch, loop_entry), std::mem::offset_of!(JsStepDispatch, capture_values), std::mem::offset_of!(JsStepDispatch, capture_len), std::mem::offset_of!(JsStepDispatch, array_record_base)],
            [mirror::CALL_CONTEXT_ADAPTER_PAYLOAD, mirror::JS_STEP_LOOP_ENTRY, mirror::JS_STEP_CAPTURE_VALUES, mirror::JS_STEP_CAPTURE_LEN, mirror::JS_STEP_ARRAY_RECORD_BASE]
        );
    }
}
