
#[allow(unused_imports)]
use crate::ext_traits::*;
/// The numbers the stencil rows are compiled with (`quench-bjit/stencils/layout.rs`) are
/// the runtime's own.
use super::super::CallContext;
use quench_runtime::vm::wtf16::native_string as form;
#[allow(dead_code)]
#[path = "../../../../quench-bjit/stencils/layout.rs"]
pub(crate) mod stencil_layout;
mod stencil_units {
    include!("../../../../quench-bjit/stencils/units.rs");
}


/// Every payload field a stencil row reads, as (name, the runtime's offset, the stencil layout's).
fn payload_offsets() -> Vec<(&'static str, usize, usize)> {
    use stencil_layout as m;
    use std::mem::offset_of as at;
    let own = vec![
        ("adapter_payload", at!(CallContext, head.adapter_payload), m::CALL_CONTEXT_ADAPTER_PAYLOAD),
        ("property_buffer_base", quench_runtime::value_vec::NATIVE_PROPERTY_BUFFER_LAYOUT.base, m::PROPERTY_BUFFER_BASE),
        ("activation_kind", crate::dispatch_fast::KIND_OFFSET, m::ACTIVATION_KIND_OFFSET),
        ("cell_entry", at!(super::super::entry_cell::EntryCell, entry), m::CELL_ENTRY_AT),
        ("cell_recovery", at!(super::super::entry_cell::EntryCell, recovery), m::CELL_RECOVERY_AT),
        ("cell_body", at!(super::super::entry_cell::EntryCell, body), m::CELL_BODY_AT),
        ("cell_counters", at!(super::super::entry_cell::EntryCell, counters), m::CELL_COUNTERS_AT),
        ("raised_run", super::super::entry_cell::RAISED_RUN as usize, m::RAISED_RUN as usize),
        ("frame_bytes", super::super::FRAME_BYTES, m::ACTIVATION_BYTES),
        ("layout_table_base", quench_runtime::tier_surface::layouts::table_base().addr(), m::LAYOUT_TABLE_BASE as usize),
        ("short_units", form::SHORT_UNITS_OFFSET as usize, m::string_form::SHORT_UNITS_OFFSET as usize),
        ("shared_pointer", form::SHARED_POINTER_OFFSET as usize, m::string_form::SHARED_POINTER_OFFSET as usize),
        ("shared_units", form::shared_units_offset() as usize, m::string_form::shared_units_offset() as usize),
    ];
    own
}

/// The value and narrow-value words the stencils are compiled with are the runtime's own.
fn value_words_match() {
    use quench_runtime::value::{NarrowLayout, ValueLayout};
    use stencil_layout as mirror;
    use mirror::words as w;
    assert_eq!(
        [ValueLayout::TAG_MASK, ValueLayout::INT_TAG, ValueLayout::INTERNAL_TAG, ValueLayout::HEAP_TAG, ValueLayout::DIRECT_ARRAY_TAG, ValueLayout::DIRECT_OBJECT_TAG, ValueLayout::DIRECT_FUNCTION_TAG, ValueLayout::DIRECT_PROMISE_TAG, ValueLayout::DIRECT_ITERATOR_TAG, ValueLayout::HEAP_FAMILY_MASK, ValueLayout::DIRECT_STRING_TAG, u64::from(ValueLayout::TAG_SHIFT), ValueLayout::FIRST_TAG_FIELD, ValueLayout::TAG_FIELD_SPAN, ValueLayout::CANONICAL_NAN, quench_runtime::Value::NULL.to_bits(), quench_runtime::Value::UNDEFINED.to_bits(), quench_runtime::Value::DELETED.to_bits(), quench_runtime::Value::FALSE.to_bits(), quench_runtime::Value::TRUE.to_bits()],
        [w::TAG_MASK, w::INT_TAG, w::INTERNAL_TAG, w::HEAP_TAG, w::DIRECT_ARRAY_TAG, w::DIRECT_OBJECT_TAG, w::DIRECT_FUNCTION_TAG, w::DIRECT_PROMISE_TAG, w::DIRECT_ITERATOR_TAG, w::HEAP_FAMILY_MASK, w::DIRECT_STRING_TAG, u64::from(w::TAG_SHIFT), w::FIRST_TAG_FIELD, w::TAG_FIELD_SPAN, w::CANONICAL_NAN, w::NULL, w::UNDEFINED, w::DELETED, w::FALSE, w::TRUE]
    );
    assert_eq!(
        [quench_runtime::value::LOWER_WORD, quench_runtime::value::HEAP_FAMILY_BITS, u64::from(quench_runtime::value::HEAP_FAMILY_SHIFT), quench_runtime::value::NARROW_HEAP_FAMILIES as u64, quench_runtime::value::TAG_UNDEFINED, quench_runtime::value::TAG_NULL, quench_runtime::value::TAG_BOOL, quench_runtime::value::TAG_DELETED],
        [w::LOWER_WORD, w::HEAP_FAMILY_BITS, u64::from(w::HEAP_FAMILY_SHIFT), w::NARROW_HEAP_FAMILIES as u64, w::UNDEFINED, w::NULL, w::FALSE, w::DELETED]
    );
    assert_eq!(
        [u64::from(NarrowLayout::INTEGER_FLAG), u64::from(NarrowLayout::INTEGER_BITS), u64::from(NarrowLayout::CODE_SHIFT), u64::from(NarrowLayout::CODE_MASK), u64::from(NarrowLayout::INDEX_SHIFT), u64::from(NarrowLayout::INDEX_BITS), NarrowLayout::DIRECT_OBJECT_UPPER, u64::from(NarrowLayout::DIRECT_OBJECT_CODE)],
        [u64::from(w::NARROW_INT_FLAG), u64::from(w::NARROW_INTEGER_BITS), u64::from(w::NARROW_CODE_SHIFT), u64::from(w::NARROW_CODE_MASK), u64::from(w::NARROW_INDEX_SHIFT), u64::from(w::NARROW_INDEX_BITS), w::DIRECT_OBJECT_UPPER, u64::from(w::DIRECT_OBJECT_NARROW_CODE)]
    );
}

/// The typed-array kinds the stencils name are the runtime's, in its order, with its widths.
fn typed_kinds_match() {
    use quench_runtime::tier_surface::TypedArrayKind as Mirror;
    let mirrored = Mirror::ALL;
    assert_eq!(usize::from(stencil_layout::LOOP_ENTRY_ARRAY_COUNT_AT as u8), std::mem::size_of::<u64>(), "the count follows the elements' address");
    let native_kinds: Vec<u8> = (0..quench_bjit::numeric::library::TYPED_KINDS).filter(|kind| !quench_bjit::numeric::library::NON_NATIVE_KINDS.contains(kind)).collect();
    let runtime_native: Vec<u8> = quench_runtime::tier_surface::TYPED_ARRAYS.iter().filter(|row| quench_runtime::staged::typed_elements::NativeKind::new(row.kind).is_some()).map(|row| row.kind as u8).collect();
    assert_eq!(native_kinds, runtime_native);
    assert_eq!(quench_runtime::tier_surface::TYPED_ARRAYS.len(), mirrored.len());
    for (row, mirror) in quench_runtime::tier_surface::TYPED_ARRAYS.iter().zip(mirrored) {
        assert_eq!((row.kind as usize, row.width), (mirror as usize, mirror.width()), "{}", row.name);
    }
}

/// The conversions the stencils make of a binary64's bits are the runtime's own of its value.
#[test]
fn stencil_units_match() {
    let samples = [
        0.0, -0.0, 0.5, -0.5, 1.0, 1.5, -1.5, 2.5, 254.5, 255.0, 255.5, 256.0, 0.499_999_999_999_999_94, 0.500_000_000_000_000_1,
        2_147_483_648.0, -2_147_483_649.0, 4_294_967_295.0, 4_294_967_296.5, 1e10, -1e10, 9_007_199_254_740_993.0, 1e19, 1e300,
        f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
    ];
    for value in samples {
        assert_eq!(stencil_units::to_int32(value), quench_runtime::units::to_int32(value), "ToInt32 of {value}");
        let clamped = if value.is_nan() { 0 } else { value.clamp(f64::from(u8::MIN), f64::from(u8::MAX)).round_ties_even() as u64 };
        assert_eq!(stencil_units::clamp_u8(value.to_bits()), clamped, "clamp of {value}");
        assert_eq!(stencil_units::clamp_u8(value.to_bits()), u64::from(quench_runtime::value::number_to_uint8_clamp(value)), "ToUint8Clamp of {value}");
    }
}

#[test]
fn stencil_layout_matches() {
    use stencil_layout as mirror;
    eprintln!("OFFSETS {:?}", payload_offsets());
    assert_eq!(format!("{:?}", quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT), format!("{:?}", mirror::NATIVE_OBJECT_LAYOUT));
    assert_eq!(format!("{:?}", quench_runtime::tier_surface::NATIVE_FUNCTION_LAYOUT), format!("{:?}", mirror::NATIVE_FUNCTION_LAYOUT));
    assert_eq!(format!("{:?}", quench_runtime::tier_surface::NATIVE_ARRAY_LAYOUT), format!("{:?}", mirror::NATIVE_ARRAY_LAYOUT));
    assert_eq!(format!("{:?}", quench_runtime::tier_surface::NATIVE_TYPED_ARRAY_LAYOUT), format!("{:?}", mirror::NATIVE_TYPED_ARRAY_LAYOUT));
    assert_eq!(format!("{:?}", quench_runtime::tier_surface::NATIVE_ENVIRONMENT_LAYOUT), format!("{:?}", mirror::NATIVE_ENVIRONMENT_LAYOUT));
    let (cache, copy) = (quench_runtime::vm::NATIVE_FIELD_CACHE_LAYOUT, mirror::NATIVE_FIELD_CACHE_LAYOUT);
    assert_eq!([cache.entry_size, cache.receiver_offset, cache.holder_offset, cache.slot_offset, cache.holder_cell_offset],
        [copy.entry_size, copy.receiver_offset, copy.holder_offset, copy.slot_offset, copy.holder_cell_offset]);
    assert_eq!(
        [cache.receiver_complement, cache.holder_complement, cache.slot_complement, cache.own_holder],
        [copy.receiver_complement, copy.holder_complement, copy.slot_complement, copy.own_holder]
    );
    assert_eq!(
        [quench_runtime::heap::NARROW_LEN as u64, quench_runtime::tier_surface::DOUBLE_LEN as u64, quench_runtime::tier_surface::ENCODED_LEN as u64, quench_runtime::tier_surface::OTHERS_UNKNOWN as u64, quench_runtime::tier_surface::LENGTH_FROM_VALUES as i64 as u64, quench_runtime::tier_surface::OTHERS_FROM_VALUES as i64 as u64, quench_runtime::value_vec::INLINE_PROPERTY_COUNT as u64, u64::from(quench_runtime::tier_surface::SMALL_STRING_ID_MASK), u64::from(quench_runtime::tier_surface::SMALL_STRING_ID_BASE)],
        [mirror::NARROW_LEN as u64, mirror::DOUBLE_LEN as u64, mirror::ENCODED_LEN as u64, mirror::OTHERS_UNKNOWN as u64, mirror::LENGTH_FROM_VALUES as i64 as u64, mirror::OTHERS_FROM_VALUES as i64 as u64, mirror::INLINE_PROPERTIES as u64, u64::from(mirror::SMALL_STRING_ID_MASK), u64::from(mirror::SMALL_STRING_ID_BASE)]
    );
    value_words_match();
    typed_kinds_match();
    assert_eq!(super::super::NATIVE_JS_STOPPED, mirror::call_record::NATIVE_JS_STOPPED);
    for (name, runtime, copy) in payload_offsets() {
        assert_eq!(runtime, copy, "{name}: the runtime's offset is the stencil layout's");
    }
    assert_eq!(
        [form::RECORD_SIZE as i32, form::SHORT_LENGTH_OFFSET, form::SHARED_LENGTH_OFFSET],
        [mirror::string_form::RECORD_SIZE as i32, mirror::string_form::SHORT_LENGTH_OFFSET, mirror::string_form::SHARED_LENGTH_OFFSET]
    );
}
