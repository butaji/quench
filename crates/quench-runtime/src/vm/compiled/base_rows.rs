//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the staged row (`staged/{environment,elements,entry,value}.rs`)
//! run on the stencil stage and compiled by rustc at build time (`quench-bjit/stencils`), so a row
//! has one definition of its layout reads across the interpreter, the specializers and here.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1; a capture's slot and the slot's byte
//! offset). Every failed proof leaves through exit 0 with nothing written, so the interpreter
//! runs the instruction.

use super::NativeFieldLoad;
use quench_bjit::numeric::{library, Place};
use quench_bjit::{Stencil, HOLES};
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
    /// `dst = receiver.name` through one inline-cache entry that reads the receiver's own flat
    /// slot: the receiver word at hole 1, the entry's shape and slot offset after
    /// (`get_entry_holes`).
    CacheGet,
    /// As `CacheGet` for an entry whose slot the receiver's prototype holds.
    CacheHolderGet,
    /// `receiver.name = value` through one entry of a slot typed `ANY` or `NUMBER`: the receiver
    /// word at hole 0, the value at hole 1, the atom, shape and slot offset after
    /// (`set_entry_holes`).
    CacheSet,
    /// As `CacheSet` for an int32 slot, which takes int32 words only.
    CacheSetInt,
}

/// A heap row: its operation and where its operands and destination live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HeapRow {
    pub(super) heap: Heap,
    pub(super) dst: Place,
    pub(super) a: Place,
    pub(super) b: Place,
    /// For an array row, the backing is the entry record's (the offset at the array hole), not
    /// looked up per access. For a cache row, the entry is followed by another of its site: a
    /// miss falls to the next row and a hit takes the branch target past them.
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
            Heap::CacheGet => library::IC_GET_ROW,
            Heap::CacheHolderGet => library::IC_GET_HOLDER_ROW,
            Heap::CacheSet => library::IC_SET_ROW,
            Heap::CacheSetInt => library::IC_SET_INT_ROW,
        }
    }
}

/// The first hole of an entry's own operands in a cache row: holes 0 and 1 are its frame operands.
pub(super) const CACHE_ENTRY_HOLE: usize = 2;
/// The holes of one cache entry.
pub(super) type EntryHoles = [u64; HOLES - CACHE_ENTRY_HOLE];

/// The holes of a cache row: the two frame operands, then the entry's.
pub(super) fn cache_holes(operands: [u64; CACHE_ENTRY_HOLE], entry: EntryHoles) -> [u64; HOLES] {
    let mut holes = [0; HOLES];
    holes[..CACHE_ENTRY_HOLE].copy_from_slice(&operands);
    holes[CACHE_ENTRY_HOLE..].copy_from_slice(&entry);
    holes
}

/// The byte offset of `load`'s slot in a flat run of arena values.
fn slot_offset(load: &NativeFieldLoad) -> u64 {
    u64::from(load.slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64
}

/// A get entry's holes: the receiver's shape, then the slot's byte offset; an entry whose slot
/// the prototype holds names the holder's shape between them.
pub(super) fn get_entry_holes(load: &NativeFieldLoad) -> EntryHoles {
    match load.holder {
        Some(holder) => [u64::from(load.shape), u64::from(holder), slot_offset(load)],
        None => [u64::from(load.shape), slot_offset(load), 0],
    }
}

/// A store entry's holes: the atom a method cache may name, the receiver's shape and the slot's
/// byte offset.
pub(super) fn set_entry_holes(atom: crate::bytecode::Atom, load: &NativeFieldLoad) -> EntryHoles {
    [u64::from(atom), u64::from(load.shape), slot_offset(load)]
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

    /// The payload words and the arena header the cache rows read are the runtime's own.
    #[test]
    fn stencil_cache_layout_matches() {
        use stencil_layout as mirror;
        assert_eq!(
            [std::mem::offset_of!(JsStepDispatch, object_record_base), std::mem::offset_of!(JsStepDispatch, property_buffer),
                std::mem::offset_of!(JsStepDispatch, method_cache_atoms), std::mem::offset_of!(JsStepDispatch, method_cache_atom_words),
                std::mem::offset_of!(JsStepDispatch, stores_specialized), crate::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.base],
            [mirror::JS_STEP_OBJECT_RECORD_BASE, mirror::JS_STEP_PROPERTY_BUFFER, mirror::JS_STEP_METHOD_CACHE_ATOMS,
                mirror::JS_STEP_METHOD_CACHE_ATOM_WORDS, mirror::JS_STEP_STORES_SPECIALIZED, mirror::PROPERTY_BUFFER_BASE]
        );
    }

    fn observed(shape: u32, holder: Option<u32>) -> super::NativeFieldLoad {
        super::NativeFieldLoad {
            shape, holder, slot: 3, storage_len: 4, storage: Some(crate::heap::PlainPropertyStorage::Arena),
            ty: crate::typed_ir::Type::ANY, inherited: None, function_shape: false,
        }
    }

    /// An entry's holes are its shape ids and its slot's byte offset, in the order the stencil
    /// rows' staged ops name them (`stencils/ic_rows.rs`), after the row's two frame operands.
    #[test]
    fn cache_entry_holes_follow_the_staged_order() {
        use super::{cache_holes, get_entry_holes, set_entry_holes};
        let offset = 3 * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64;
        assert_eq!(get_entry_holes(&observed(7, None)), [7, offset, 0]);
        assert_eq!(get_entry_holes(&observed(7, Some(9))), [7, 9, offset]);
        assert_eq!(set_entry_holes(5, &observed(7, None)), [5, 7, offset]);
        assert_eq!(cache_holes([1, 2], [3, 4, 5]), [1, 2, 3, 4, 5]);
    }

    /// Every cache row, as the last entry of a site and as a chained one, was built.
    #[test]
    fn cache_rows_are_in_the_library() {
        use super::{stencil, Heap, HeapRow};
        for heap in [Heap::CacheGet, Heap::CacheHolderGet, Heap::CacheSet, Heap::CacheSetInt] {
            for entry in [false, true] {
                let row = HeapRow { heap, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry };
                assert!(stencil(row).is_ok(), "{row:?}");
            }
        }
    }
}
