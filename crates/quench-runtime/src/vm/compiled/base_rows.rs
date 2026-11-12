//! Stencil rows that read the heap: a captured value, a dense array element read or replaced, an
//! entry-record field. Each row is the existing staged / emit helpers lowered into a stencil
//! (`quench_bjit::generate`) beside the Cranelift specializer, so a row has one definition of
//! its layout reads; the stencils are generated on first use and shared for the process.
//!
//! Operands are holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the
//! operands' (a field row's entry-record byte offset in 1). Every failed proof leaves through exit
//! 0 with nothing written, so the interpreter runs the instruction.

use super::{emit_dense_array_view, DenseArrayView, emit_dense_element, emit_dense_element_replace, emit_js_payload_word, CallContext, JsStepDispatch};
use super::object_slots::EmitImm;
use crate::staged::{self, Stage};
use cranelift_codegen::ir;
use quench_bjit::numeric::{self, Place};
use quench_bjit::row::{Operand, RowBuilder, Source, Ty};
use quench_bjit::{Stencil, StencilBuilder, generate};
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

/// The activation's dispatch payload: the context's adapter pointer, which never changes during a run (so the load may merge).
fn payload(b: &mut StencilBuilder) -> ir::Value {
    let mut stage = super::object_slots::emit(b.b);
    let address = stage.add_imm(b.context, std::mem::offset_of!(CallContext, adapter_payload) as u64);
    stage.load_table(address)
}

fn capture(b: &mut StencilBuilder, payload: ir::Value, slot: u16, miss: ir::Block) -> ir::Value {
    let values = emit_js_payload_word(b.b, payload, std::mem::offset_of!(JsStepDispatch, capture_values));
    let len = emit_js_payload_word(b.b, payload, std::mem::offset_of!(JsStepDispatch, capture_len));
    let mut stage = super::object_slots::emit(b.b);
    staged::environment::dense_capture(&mut stage, values, len, slot, miss).expect("the emit stage continues past its guards")
}

/// The view of the array whose entry-record element pointer sits at `hole` (`staged::entry::entry_backing`).
fn entry_view(b: &mut StencilBuilder, payload: ir::Value, hole: u32, writable: bool) -> DenseArrayView {
    let entry = emit_js_payload_word(b.b, payload, std::mem::offset_of!(JsStepDispatch, loop_entry));
    let at = b.operand(Operand::Hole(hole), Ty::I64);
    let (values, length) = staged::entry::entry_backing(&mut super::object_slots::emit(b.b), entry, EmitImm::Value(at));
    DenseArrayView { values, length, writable, uncounted: false }
}

fn element(b: &mut StencilBuilder, payload: ir::Value, row: HeapRow, miss: ir::Block) -> ir::Value {
    let index = numeric::index_of(b, row.b, numeric::SECOND);
    let view = if row.entry {
        entry_view(b, payload, 1, false)
    } else {
        let base = b.read(Source::Frame(Operand::Hole(1)), Ty::Word);
        emit_dense_array_view(b.b, payload, base, false, miss)
    };
    emit_dense_element(b.b, view, index, miss).1
}

fn store(b: &mut StencilBuilder, payload: ir::Value, row: HeapRow, miss: ir::Block) {
    let index = numeric::index_of(b, row.b, numeric::SECOND);
    let value = numeric::number_word(b, row.a, 1);
    let view = if row.entry {
        entry_view(b, payload, 0, true)
    } else {
        let base = b.read(Source::Frame(Operand::Hole(0)), Ty::Word);
        emit_dense_array_view(b.b, payload, base, true, miss)
    };
    let (address, old) = emit_dense_element(b.b, view, index, miss);
    emit_dense_element_replace(b.b, view, address, old, value);
    b.next();
}

fn field(b: &mut StencilBuilder, payload: ir::Value) -> ir::Value {
    let entry = emit_js_payload_word(b.b, payload, std::mem::offset_of!(JsStepDispatch, loop_entry));
    let at = b.operand(Operand::Hole(1), Ty::I64);
    staged::entry::entry_word(&mut super::object_slots::emit(b.b), entry, EmitImm::Value(at))
}

fn emit(b: &mut StencilBuilder, row: HeapRow) {
    let miss = b.exit_block(0);
    let payload = payload(b);
    let word = match row.heap {
        Heap::Capture(slot) => capture(b, payload, slot, miss),
        Heap::Index => element(b, payload, row, miss),
        Heap::Field => field(b, payload),
        Heap::Store => return store(b, payload, row, miss),
        Heap::Array => {
            let base = b.read(Source::Frame(Operand::Hole(1)), Ty::Word);
            let is_array = staged::value::is_direct_array(&mut super::object_slots::emit(b.b), base);
            b.guard(is_array, 0);
            return b.next();
        }
    };
    numeric::deliver(b, row.dst, word);
}

/// The stencil of `row`: generated on first use, then shared for the process.
pub(super) fn stencil(row: HeapRow) -> Result<&'static Stencil, String> {
    static TABLE: Mutex<Option<HashMap<HeapRow, &'static Stencil>>> = Mutex::new(None);
    let mut table = TABLE.lock().map_err(|e| e.to_string())?;
    let table = table.get_or_insert_with(HashMap::new);
    if let Some(found) = table.get(&row) {
        return Ok(found);
    }
    let started = std::time::Instant::now();
    let made = numeric::isa().and_then(|isa| generate(isa, |b| emit(b, row)));
    numeric::add_generation_ns(started.elapsed().as_nanos() as u64);
    let made = made?;
    let stencil: &'static Stencil = Box::leak(Box::new(made));
    table.insert(row, stencil);
    Ok(stencil)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers the stencil rows are compiled with (`quench-bjit/stencils/layout.rs`) are
    /// the runtime's own.
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
        assert_eq!(format!("{:?}", crate::vm::NATIVE_FIELD_CACHE_LAYOUT), format!("{:?}", mirror::NATIVE_FIELD_CACHE_LAYOUT));
        assert_eq!(
            [crate::heap::OTHERS_UNKNOWN as u64, crate::heap::LENGTH_FROM_VALUES as i64 as u64, crate::heap::OTHERS_FROM_VALUES as i64 as u64, crate::value_vec::INLINE_PROPERTY_COUNT as u64],
            [mirror::OTHERS_UNKNOWN as u64, mirror::LENGTH_FROM_VALUES as i64 as u64, mirror::OTHERS_FROM_VALUES as i64 as u64, mirror::INLINE_PROPERTY_COUNT as u64]
        );
        use mirror::words as w;
        assert_eq!(
            [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, crate::Value::NULL.to_bits(), crate::Value::UNDEFINED.to_bits(), crate::Value::DELETED.to_bits(), crate::Value::FALSE.to_bits()],
            [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE]
        );
        assert_eq!(
            [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
            [u64::from(w::NARROW_INTEGER_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
        );
        assert_eq!(
            [std::mem::offset_of!(CallContext, adapter_payload), std::mem::offset_of!(JsStepDispatch, loop_entry), std::mem::offset_of!(JsStepDispatch, capture_values), std::mem::offset_of!(JsStepDispatch, capture_len), std::mem::offset_of!(JsStepDispatch, array_record_base)],
            [mirror::CALL_CONTEXT_ADAPTER_PAYLOAD, mirror::JS_STEP_LOOP_ENTRY, mirror::JS_STEP_CAPTURE_VALUES, mirror::JS_STEP_CAPTURE_LEN, mirror::JS_STEP_ARRAY_RECORD_BASE]
        );
    }

    /// The field-read row's code size and relocations; with `STENCIL_DUMP=dir` its bytes are written there.
    #[test]
    fn field_read_row_generates() {
        let stencil = stencil(HeapRow { heap: Heap::Field, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false }).unwrap();
        eprintln!("field-read: {} bytes, relocs {:?}, imm32 {:?}", stencil.code.len(), stencil.relocs, stencil.imm32);
        if let Ok(dir) = std::env::var("STENCIL_DUMP") {
            std::fs::write(format!("{dir}/field_read_cranelift.bin"), &stencil.code).unwrap();
        }
    }
}
