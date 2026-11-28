//! A flat string's record and its `length` as a staged read of it. The record's form is read in
//! place (`vm::wtf16::native_string`): the two ASCII forms and the UTF-16 one; a rope, a slice,
//! an append-buffer prefix and a parts string are other tags, and a small string has no record:
//! each leaves at the row's `miss`. The interpreter reads the same word through
//! `concrete_string_length`, and the string rows the units (`strings::flat_string`).

use super::value::{is_direct_string, length_value};
use super::region::Region;
use super::Stage;
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
pub fn unit_record<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
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
pub fn unit_words<S: Stage>(stage: &mut S, record: S::Word, short: S::Cond, prefix: S::Cond) -> (S::Word, S::Word) {
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
pub struct FlatString<W> {
    pub units: W,
    pub length: W,
    pub wide: W,
}

/// The flat string `value` names among the string records at `record_base`: its units, length
/// and width, read from its record (`flat_record`), or `None` (leaving at `miss`) where that leaves.
#[inline(always)]
pub fn flat_string<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
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
pub fn string_length<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss) -> Option<S::Word> {
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
pub fn length_of<S: Stage>(stage: &mut S, (arrays, strings): (S::Word, S::Word), value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let array = super::value::is_direct_array(stage, value);
    stage.choose_leaving(array, #[inline(always)] |stage| super::elements::array_length(stage, arrays, value, miss), #[inline(always)] |stage| string_length(stage, strings, value, miss))
}



#[cfg(test)]
mod tests;

/// Whether `===` of two heap words that `generic::decide` left open compares contents: one family, and a
/// direct string or a legacy cell (a BigInt). Any other open pair is unequal.
#[inline(always)]
pub fn content_pair<S: Stage>(stage: &mut S, [left, right]: [S::Word; 2]) -> S::Cond {
    let family = stage.and_imm(left, crate::value::ValueLayout::HEAP_FAMILY_MASK);
    let right_family = stage.and_imm(right, crate::value::ValueLayout::HEAP_FAMILY_MASK);
    let one_family = stage.eq(family, right_family);
    let string = is_direct_string(stage, left);
    let legacy = stage.eq_imm(family, crate::value::ValueLayout::HEAP_TAG);
    let contents = stage.any(string, legacy);
    stage.all(one_family, contents)
}

/// `===` of two strings both held inline in their records (the short form, zero-padded): equal exactly when
/// their records' words are. Any other pair (a small string, another form) leaves at `miss`, the VM's compare.
#[inline(always)]
pub fn short_strings_equal<S: Stage>(stage: &mut S, record_base: S::Word, [left, right]: [S::Word; 2], miss: S::Miss) -> Option<S::Cond> {
    let (left_record, left_short, _) = flat_record(stage, record_base, left, miss)?;
    let (right_record, right_short, _) = flat_record(stage, record_base, right, miss)?;
    let shorts = stage.all(left_short, right_short);
    stage.guard(shorts, miss)?;
    let same_word = |stage: &mut S, word: usize| {
        let at = (word * 8) as i32;
        let (a, b) = (stage.load_u64(Region::HeapMeta.mutable(), left_record, at), stage.load_u64(Region::HeapMeta.mutable(), right_record, at));
        stage.eq(a, b)
    };
    let first = same_word(stage, 0);
    Some((1..form::RECORD_SIZE / 8).fold(first, |equal, word| { let same = same_word(stage, word); stage.all(equal, same) }))
}
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
