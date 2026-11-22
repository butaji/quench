//! A flat string's record and its `length` as a staged read of it. The record's form is read in
//! place (`vm::wtf16::native_string`): the two ASCII forms and the UTF-16 one; a rope, a slice,
//! an append-buffer prefix and a parts string are other tags, and a small string has no record:
//! each leaves at the row's `miss`. The interpreter reads the same word through
//! `concrete_string_length`, and the string rows the units (`strings::flat_string`).

use super::value::{is_direct_string, length_value};
use super::region::Region;
use super::Stage;
use crate::Value;
use crate::vm::wtf16::native_string as form;

/// The record of the flat string `value` names among the string records at `record_base`, with
/// whether its form is the short one and whether it is the wide one: `None` (leaving at `miss`)
/// for any other value, for a small string, which has no record (`Heap::small_string_text`), or
/// for a record in a form a row does not read. The record's address is its index, `value`'s low
/// half, times the record size past `record_base`. No zero test on `record_base`: a direct string
/// names a record only after its space is reserved (`records.rs`, `ensure_reserved` on every
/// allocation), so a zero base comes only with a small string, which the small-id test leaves.
#[inline(always)]
pub fn flat_record<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
    -> Option<(S::Word, S::Cond, S::Cond)> {
    let is_string = is_direct_string(stage, value);
    stage.guard(is_string, miss)?;
    // The small-id mask lies in the low word, so it reads `value` as it is.
    let small_bits = stage.and_imm(value, u64::from(crate::heap::SMALL_STRING_ID_MASK));
    let has_record = stage.ne_imm(small_bits, u64::from(crate::heap::SMALL_STRING_ID_BASE));
    stage.guard(has_record, miss)?;
    let index = stage.and_imm(value, u64::from(u32::MAX));
    let offset = stage.mul_imm(index, form::RECORD_SIZE as u64);
    let record = stage.add(record_base, offset);
    let tag = stage.load_u8(Region::HeapMeta.mutable(), record, form::TAG_OFFSET);
    let short = stage.eq_imm(tag, u64::from(form::TAG_SHORT));
    let ascii = stage.eq_imm(tag, u64::from(form::TAG_ASCII));
    let wide = stage.eq_imm(tag, u64::from(form::TAG_WIDE));
    let shared = stage.any(ascii, wide);
    let flat = stage.any(short, shared);
    stage.guard(flat, miss)?;
    Some((record, short, wide))
}

/// `flat_record`, or the record of an ASCII append-buffer prefix (`FlatUnits::AsciiPrefix`), whose
/// units a row reads through its buffer: the record, whether its form is the short one, whether it
/// is the wide one, and whether it is a prefix. A row that addresses the units itself, not through
/// `unit_words`, takes `flat_record`.
#[inline(always)]
pub(crate) fn unit_record<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
    -> Option<(S::Word, S::Cond, S::Cond, S::Cond)> {
    let is_string = is_direct_string(stage, value);
    stage.guard(is_string, miss)?;
    // The small-id mask lies in the low word, so it reads `value` as it is.
    let small_bits = stage.and_imm(value, u64::from(crate::heap::SMALL_STRING_ID_MASK));
    let has_record = stage.ne_imm(small_bits, u64::from(crate::heap::SMALL_STRING_ID_BASE));
    stage.guard(has_record, miss)?;
    let index = stage.and_imm(value, u64::from(u32::MAX));
    let offset = stage.mul_imm(index, form::RECORD_SIZE as u64);
    let record = stage.add(record_base, offset);
    let tag = stage.load_u8(Region::HeapMeta.mutable(), record, form::TAG_OFFSET);
    let short = stage.eq_imm(tag, u64::from(form::TAG_SHORT));
    let ascii = stage.eq_imm(tag, u64::from(form::TAG_ASCII));
    let wide = stage.eq_imm(tag, u64::from(form::TAG_WIDE));
    let prefix = stage.eq_imm(tag, u64::from(form::TAG_ASCII_PREFIX));
    let shared = stage.any(ascii, wide);
    let flat = stage.any(short, shared);
    let read = stage.any(flat, prefix);
    stage.guard(read, miss)?;
    Some((record, short, wide, prefix))
}

/// The address of the first unit and the unit count of the string whose record `unit_record`
/// read, in the form `short` or `prefix` names, else the shared form. Every form keeps the words
/// of the shared layout inside the record, so each is read and the form selects; a prefix's units
/// are its buffer's (`APPEND_UNITS_OFFSET` into its `Rc`'s value), which any other form reads from
/// its own record instead, so no load leaves memory a form owns.
#[inline(always)]
pub(crate) fn unit_words<S: Stage>(stage: &mut S, record: S::Word, short: S::Cond, prefix: S::Cond) -> (S::Word, S::Word) {
    let pointer = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_POINTER_OFFSET);
    let shared_units = stage.add_imm(pointer, form::shared_units_offset());
    let buffer = stage.select(prefix, shared_units, record);
    let prefix_units = stage.load_u64(Region::HeapMeta.mutable(), buffer, form::APPEND_UNITS_OFFSET);
    let short_units = stage.add_imm(record, form::SHORT_UNITS_OFFSET);
    let shared_length = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_LENGTH_OFFSET);
    let prefix_length = stage.load_u32(Region::HeapMeta.mutable(), record, form::PREFIX_LENGTH_OFFSET);
    let short_length = stage.load_u8(Region::HeapMeta.mutable(), record, form::SHORT_LENGTH_OFFSET);
    let long_units = stage.select(prefix, prefix_units, shared_units);
    let long_length = stage.select(prefix, prefix_length, shared_length);
    (stage.select(short, short_units, long_units), stage.select(short, short_length, long_length))
}

/// A flat string's units as the words a row reads: the address of the first, how many there
/// are, and whether each is two bytes (one) or one (zero).
#[derive(Clone, Copy)]
pub(crate) struct FlatString<W> {
    pub(crate) units: W,
    pub(crate) length: W,
    pub(crate) wide: W,
}

/// The flat string `value` names among the string records at `record_base`: its units, length
/// and width, read from its record (`flat_record`), or `None` (leaving at `miss`) where that leaves.
#[inline(always)]
pub(crate) fn flat_string<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
    -> Option<FlatString<S::Word>> {
    let (record, short, wide, prefix) = unit_record(stage, record_base, value, miss)?;
    let (units, length) = unit_words(stage, record, short, prefix);
    let (one, zero) = (stage.constant(1), stage.constant(0));
    let wide = stage.select(wide, one, zero);
    Some(FlatString { units, length, wide })
}

/// The unit count of the flat string `value` names among the string records at `record_base`
/// (`flat_record`), as an int32 word: `None` (leaving at `miss`) where `flat_record` leaves or
/// for a length past int32.
#[inline(always)]
pub(crate) fn string_length<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (record, short, _, prefix) = unit_record(stage, record_base, value, miss)?;
    // Every form keeps the words of the shared layout inside the record, so each is read and the
    // form selects.
    let short_length = stage.load_u8(Region::HeapMeta.mutable(), record, form::SHORT_LENGTH_OFFSET);
    let shared_length = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_LENGTH_OFFSET);
    let prefix_length = stage.load_u32(Region::HeapMeta.mutable(), record, form::PREFIX_LENGTH_OFFSET);
    let long_length = stage.select(prefix, prefix_length, shared_length);
    let length = stage.select(short, short_length, long_length);
    length_value(stage, length, miss)
}

/// ToBoolean of the string `value` among the string records at `record_base`, read from its
/// record without leaving: 1 where the flat string's length is nonzero, 0 where it is empty, and
/// 2 where no flat record reads (a value that is no string, a small string, a rope or a parts
/// string, or no record base yet), which the caller decides. The record is read only on the path
/// where `flat_record` would continue, so nothing is loaded from an address it would not read.
#[inline(always)]
pub fn flat_truth<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word) -> S::Word {
    let is_string = is_direct_string(stage, value);
    let present = stage.ne_imm(record_base, 0u32);
    let index = stage.and_imm(value, u64::from(u32::MAX));
    let small_bits = stage.and_imm(index, u64::from(crate::heap::SMALL_STRING_ID_MASK));
    let has_record = stage.ne_imm(small_bits, u64::from(crate::heap::SMALL_STRING_ID_BASE));
    let string = stage.all(is_string, present);
    let candidate = stage.all(string, has_record);
    stage.choose(candidate, #[inline(always)] |stage| {
        let offset = stage.mul_imm(index, form::RECORD_SIZE as u64);
        let record = stage.add(record_base, offset);
        let tag = stage.load_u8(Region::HeapMeta.mutable(), record, form::TAG_OFFSET);
        let short = stage.eq_imm(tag, u64::from(form::TAG_SHORT));
        let ascii = stage.eq_imm(tag, u64::from(form::TAG_ASCII));
        let wide = stage.eq_imm(tag, u64::from(form::TAG_WIDE));
        let shared = stage.any(ascii, wide);
        let flat = stage.any(short, shared);
        stage.choose(flat, #[inline(always)] |stage| {
            let short_length = stage.load_u8(Region::HeapMeta.mutable(), record, form::SHORT_LENGTH_OFFSET);
            let shared_length = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_LENGTH_OFFSET);
            let length = stage.select(short, short_length, shared_length);
            let nonempty = stage.ne_imm(length, 0u32);
            let (one, zero) = (stage.constant(1), stage.constant(0));
            stage.select(nonempty, one, zero)
        }, #[inline(always)] |stage| stage.constant(2))
    }, #[inline(always)] |stage| stage.constant(2))
}

/// The `length` of `value`, a direct array (among the array records at `arrays`) or a direct
/// string (among the string records at `strings`): `None` (leaving at `miss`) for any other value
/// or a form neither row reads.
#[inline(always)]
pub(crate) fn length_of<S: Stage>(stage: &mut S, (arrays, strings): (S::Word, S::Word), value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let array = super::value::is_direct_array(stage, value);
    stage.choose_leaving(array, #[inline(always)] |stage| super::elements::array_length(stage, arrays, value, miss), #[inline(always)] |stage| string_length(stage, strings, value, miss))
}

/// The interpreter's `string_length` of the string `value` among the records at `record_base`:
/// `None` for a form the row does not read, which the caller reads through the string's units.
#[inline(always)]
pub(crate) fn concrete_string_length(record_base: *const crate::vm::wtf16::JsString, value: Value) -> Option<Value> {
    let mut stage = super::Concrete::new(std::ptr::null());
    string_length(&mut stage, record_base as u64, value.to_bits(), ()).map(Value::from_bits)
}

/// The interpreter's `length_of` of `value`, among the array records at `arrays` and the string
/// records at `strings`.
#[inline(always)]
pub(crate) fn concrete_length_of(arrays: *const crate::heap::ArrayObject, strings: *const crate::vm::wtf16::JsString, value: Value) -> Option<Value> {
    let mut stage = super::Concrete::new(std::ptr::null());
    length_of(&mut stage, (arrays as u64, strings as u64), value.to_bits(), ()).map(Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;
    use crate::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY};

    /// The string row of `value` on the heap's own records.
    fn length(heap: &Heap, value: Value) -> Option<Value> {
        concrete_string_length(heap.string_record_base()?, value)
    }

    #[test]
    fn the_row_reads_the_length_of_every_flat_form_through_the_record() {
        let mut heap = Heap::new();
        let long = "k".repeat(SHORT_ASCII_CAPACITY + 5);
        let forms = [
            (heap.alloc_string(JsString::from_str("")), 0),
            (heap.alloc_string(JsString::from_str("hello")), 5),
            (heap.alloc_string(JsString::from_str(&long)), long.len()),
        ];
        for (value, units) in forms {
            assert_eq!(length(&heap, value), Some(Value::length(units)), "the length of a {units}-unit string");
        }
        // A UTF-16 string is held shared (`Repr::Shared`, so a rope can flatten in place), never
        // as a flat `Wide` record, so the row leaves for it and the caller reads its units.
        let wide = heap.alloc_string(JsString::from_units(&[0x61, 0x20ac]));
        assert_eq!(length(&heap, wide), None, "a UTF-16 string has no flat record");
        assert_eq!(length(&heap, Value::integer(3)), None, "no string");
    }

    #[test]
    fn a_slice_leaves() {
        let mut heap = Heap::new();
        let base = heap.alloc_string(JsString::from_str(&"s".repeat(SHORT_ASCII_CAPACITY * 3)));
        let sliced = heap.string(base).unwrap().substring(1..40);
        let slice = heap.alloc_string(sliced);
        assert_eq!(length(&heap, slice), None);
    }

    /// The row's only operands are the record bases: it names no per-site hole.
    #[test]
    fn the_row_has_no_per_site_hole() {
        let mut stage = crate::staged::trace::Trace::default();
        let (base, value) = (stage.constant(0), stage.constant(1));
        string_length(&mut stage, base, value, ());
        assert!(stage.holes.is_empty(), "{:?}", stage.holes);
    }
}
