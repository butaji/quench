//! A flat string's units as a staged read of its record, and the string rows of `builtins`
//! over them where the width is the string's own: `charCodeAt`, `charAt` and `length` of a
//! string in a specializer's code. The record's form is read in place
//! (`vm::wtf16::native_string`): the two ASCII forms and the UTF-16 one; a rope, a slice, an
//! append-buffer prefix and a parts string are other tags, and leave at the row's `miss`.
//! The interpreter reads the same units through `FlatUnits::view` (`builtins::concrete_*`).

use super::builtins::{UnitWidth, char_at_ascii, char_code_at};
use super::region::Region;
use super::{Stage, value::is_direct_string};
use crate::Value;
use crate::vm::wtf16::native_string as form;

/// A flat string's units as the words a row reads: the address of the first, how many there
/// are, and whether each is two bytes (one) or one (zero).
#[derive(Clone, Copy)]
pub(crate) struct FlatString<W> {
    pub(crate) units: W,
    pub(crate) length: W,
    pub(crate) wide: W,
}

/// The flat string `value` names among the string records at `record_base`: `None` (leaving
/// at `miss`) for any other value, before the first string record exists (`record_base`
/// zero), or for a record in a form a row does not read. The record's address is its index,
/// `value`'s low half, times the record size past `record_base`.
#[inline(always)]
pub(crate) fn flat_string<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
    -> Option<FlatString<S::Word>> {
    let is_string = is_direct_string(stage, value);
    stage.guard(is_string, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
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
    // Every form keeps the words of both layouts inside the record, so each is read and the
    // form selects.
    let pointer = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_POINTER_OFFSET);
    let shared_units = stage.add_imm(pointer, form::shared_units_offset());
    let shared_length = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_LENGTH_OFFSET);
    let short_units = stage.add_imm(record, form::SHORT_UNITS_OFFSET);
    let short_length = stage.load_u8(Region::HeapMeta.mutable(), record, form::SHORT_LENGTH_OFFSET);
    let units = stage.select(short, short_units, shared_units);
    let length = stage.select(short, short_length, shared_length);
    let (one, zero) = (stage.constant(1), stage.constant(0));
    let wide = stage.select(wide, one, zero);
    Some(FlatString { units, length, wide })
}

/// `row` of `string` at its own width.
#[inline(always)]
fn at_width<S: Stage>(stage: &mut S, string: FlatString<S::Word>,
    row: impl Fn(&mut S, UnitWidth) -> Option<S::Word>) -> Option<S::Word> {
    let wide = stage.ne_imm(string.wide, 0u32);
    stage.choose_leaving(wide, |stage| row(stage, UnitWidth::Wide), |stage| row(stage, UnitWidth::Narrow))
}

/// `builtins::char_code_at` of `string`.
#[inline(always)]
pub(crate) fn string_char_code_at<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    at_width(stage, string, |stage, width| char_code_at(stage, width, string.units, string.length, index, miss))
}

/// `builtins::char_at_ascii` of `string`: the one-character string of the unit, from the
/// table of those strings at `table` (`Heap::ascii_unit_strings_base`, zero before the first
/// exists). `None` (leaving at `miss`) where the row leaves, or where the unit has no string
/// yet, which the native makes.
#[inline(always)]
pub(crate) fn string_char_at<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word, table: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let present = stage.ne_imm(table, 0u32);
    stage.guard(present, miss)?;
    let unit = at_width(stage, string, |stage, width| char_at_ascii(stage, width, string.units, string.length, index, miss))?;
    let offset = stage.mul_imm(unit, size_of::<Value>() as u64);
    let address = stage.add(table, offset);
    let character = stage.load_u64(Region::HeapMeta.mutable(), address, 0);
    let made = stage.ne_imm(character, Value::UNDEFINED.to_bits());
    stage.guard(made, miss)?;
    Some(character)
}

/// The interpreter's `string_char_code_at` of the string `value` among the records at
/// `record_base`: `None` for a form the row does not read, which the caller reads through the
/// string's units (`builtins::concrete_char_code_at`).
#[inline(always)]
pub(crate) fn concrete_string_char_code_at(record_base: *const crate::vm::wtf16::JsString, value: Value, index: Value) -> Option<Value> {
    let mut stage = super::Concrete::new(std::ptr::null());
    let string = flat_string(&mut stage, record_base as u64, value.to_bits(), ())?;
    string_char_code_at(&mut stage, string, index.to_bits(), ()).map(Value::from_bits)
}

/// The interpreter's `string_char_at` of the string `value`, from the table of one-unit strings
/// at `table`; `None` where the row leaves.
#[inline(always)]
pub(crate) fn concrete_string_char_at(record_base: *const crate::vm::wtf16::JsString, table: *const Value, value: Value, index: Value) -> Option<Value> {
    let mut stage = super::Concrete::new(std::ptr::null());
    let string = flat_string(&mut stage, record_base as u64, value.to_bits(), ())?;
    string_char_at(&mut stage, string, index.to_bits(), table as u64, ()).map(Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;
    use crate::staged::Concrete;
    use crate::staged::builtins::length_value;
    use crate::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY};

    fn stage() -> Concrete { Concrete::new(std::ptr::null()) }

    /// The string rows of `value` on the heap's own records at `index`, as words.
    fn read(heap: &Heap, value: Value, index: Value) -> Option<(Option<u64>, Option<u64>)> {
        let mut s = stage();
        let base = heap.string_record_base().map_or(0, |base| base as u64);
        let string = flat_string(&mut s, base, value.to_bits(), ())?;
        let code = string_char_code_at(&mut s, string, index.to_bits(), ());
        let length = length_value(&mut s, string.length, ());
        Some((code, length))
    }

    #[test]
    fn rows_read_every_flat_form_through_the_record() {
        let mut heap = Heap::new();
        let long = "k".repeat(SHORT_ASCII_CAPACITY + 5);
        let forms = [
            (heap.alloc_string(JsString::from_str("")), Vec::<u16>::new()),
            (heap.alloc_string(JsString::from_str("hello")), "hello".encode_utf16().collect()),
            (heap.alloc_string(JsString::from_str(&long)), long.encode_utf16().collect()),
            (heap.alloc_string(JsString::from_units(&[0x61, 0x20ac])), vec![0x61, 0x20ac]),
        ];
        for (value, units) in forms {
            for (at, unit) in units.iter().enumerate() {
                let read = read(&heap, value, Value::integer(at as i32)).unwrap();
                assert_eq!(read.0, Some(Value::integer(i32::from(*unit)).to_bits()));
                assert_eq!(read.1, Some(Value::integer(units.len() as i32).to_bits()));
            }
            let outside = read(&heap, value, Value::integer(units.len() as i32)).unwrap();
            assert_eq!(outside.0, Some(Value::NAN.to_bits()), "outside the string");
            assert_eq!(read(&heap, value, Value::number(0.5)).unwrap().0, None, "a double index leaves");
        }
    }

    #[test]
    fn a_slice_a_non_string_and_a_missing_base_leave() {
        let mut heap = Heap::new();
        let long = JsString::from_str(&"s".repeat(SHORT_ASCII_CAPACITY * 3));
        let slice = heap.alloc_string(long.substring(1..40));
        assert!(read(&heap, slice, Value::integer(0)).is_none(), "a slice is no form a row reads");
        assert!(read(&heap, Value::integer(3), Value::integer(0)).is_none());
        let empty = Heap::new();
        assert!(read(&empty, slice, Value::integer(0)).is_none(), "no record base");
    }
}
