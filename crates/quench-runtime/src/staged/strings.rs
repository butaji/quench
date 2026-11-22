//! A flat string's units as a staged read of its record, and the string rows of `builtins`
//! over them where the width is the string's own: `charCodeAt`, `charAt` and `length` of a
//! string in a specializer's code. The record's form is read in place
//! (`vm::wtf16::native_string`): the two ASCII forms and the UTF-16 one; a rope, a slice, an
//! append-buffer prefix and a parts string are other tags, and leave at the row's `miss`.
//! The interpreter reads the same units through `FlatUnits::view`: `concrete_char_code_at` and
//! `concrete_char_at` take the string in either form (`ConcreteString`).

use super::builtins::{UnitWidth, char_at_ascii, char_at_ascii_within, char_code_at, char_code_at_within};
use super::region::Region;
use super::Stage;
use crate::Value;
use crate::vm::wtf16::native_string as form;
use crate::vm::wtf16::JsString;
#[cfg(not(stencil_build))]
use crate::vm::wtf16::Units;

/// A flat string's units as the words a row reads: the address of the first, how many there
/// are, and whether each is two bytes (one) or one (zero).
#[derive(Clone, Copy)]
pub struct FlatString<W> {
    pub units: W,
    pub length: W,
    pub wide: W,
}

/// The flat string `value` names among the string records at `record_base`: its units, length
/// and width, read from its record (`string_length::unit_record`: a flat form or an ASCII prefix),
/// or `None` (leaving at `miss`) where that leaves.
#[inline(always)]
pub fn flat_string<S: Stage>(stage: &mut S, record_base: S::Word, value: S::Word, miss: S::Miss)
    -> Option<FlatString<S::Word>> {
    let (record, short, wide, prefix) = super::string_length::unit_record(stage, record_base, value, miss)?;
    let (units, length) = super::string_length::unit_words(stage, record, short, prefix);
    let (one, zero) = (stage.constant(1), stage.constant(0));
    let wide = stage.select(wide, one, zero);
    Some(FlatString { units, length, wide })
}

/// The units of the flat string whose record is `record`, in the form `short` names: the record's
/// own, else its shared buffer's. Every form keeps the words of both layouts inside the record, so
/// each is read and the form selects. A record's suballocations may change when the VM runs, so a
/// row reads them where it uses them, not held across a safepoint.
#[inline(always)]
pub fn record_units<S: Stage>(stage: &mut S, record: S::Word, short: S::Cond) -> S::Word {
    let pointer = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_POINTER_OFFSET);
    let shared_units = stage.add_imm(pointer, form::shared_units_offset());
    let short_units = stage.add_imm(record, form::SHORT_UNITS_OFFSET);
    stage.select(short, short_units, shared_units)
}

/// The length of the flat string whose record is `record`, in the form `short` names. A string's
/// length never changes, so a loop may hold it (`loop_strings`).
#[inline(always)]
pub fn record_length<S: Stage>(stage: &mut S, record: S::Word, short: S::Cond) -> S::Word {
    let shared_length = stage.load_u64(Region::HeapMeta.mutable(), record, form::SHARED_LENGTH_OFFSET);
    let short_length = stage.load_u8(Region::HeapMeta.mutable(), record, form::SHORT_LENGTH_OFFSET);
    stage.select(short, short_length, shared_length)
}

/// `row` of `string` at its own width.
#[inline(always)]
fn at_width<S: Stage>(stage: &mut S, string: FlatString<S::Word>,
    row: impl Fn(&mut S, UnitWidth) -> Option<S::Word>) -> Option<S::Word> {
    let wide = stage.ne_imm(string.wide, 0u32);
    stage.choose_leaving(wide, #[inline(always)] |stage| row(stage, UnitWidth::Wide), #[inline(always)] |stage| row(stage, UnitWidth::Narrow))
}

/// `builtins::char_code_at` of `string`.
#[inline(always)]
pub fn string_char_code_at<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    at_width(stage, string, #[inline(always)] |stage, width| char_code_at(stage, width, string.units, string.length, index, miss))
}

/// `string_char_code_at` of `string` for an index the caller proves below its length: the row with no
/// bounds test (`builtins::char_code_at_within`).
#[inline(always)]
pub fn string_char_code_at_within<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    at_width(stage, string, #[inline(always)] |stage, width| char_code_at_within(stage, width, string.units, index, miss))
}

/// `string_char_at` for an index the caller proves below the string's length (`builtins::char_at_ascii_within`).
#[inline(always)]
pub fn string_char_at_within<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word, table: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let present = stage.ne_imm(table, 0u32);
    stage.guard(present, miss)?;
    let unit = at_width(stage, string, #[inline(always)] |stage, width| char_at_ascii_within(stage, width, string.units, index, miss))?;
    character_of_unit(stage, table, unit, miss)
}

/// `builtins::char_at_ascii` of `string`: the one-character string of the unit, from the
/// table of those strings at `table` (`Heap::ascii_unit_strings_base`, zero before the first
/// exists). `None` (leaving at `miss`) where the row leaves, or where the unit has no string
/// yet, which the native makes.
#[inline(always)]
pub fn string_char_at<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word, table: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let present = stage.ne_imm(table, 0u32);
    stage.guard(present, miss)?;
    let unit = at_width(stage, string, #[inline(always)] |stage, width| char_at_ascii(stage, width, string.units, string.length, index, miss))?;
    character_of_unit(stage, table, unit, miss)
}

/// The unit `string_char_at` names, before the table maps it to its one-character string: the
/// `char_at_ascii` row of `string`, leaving at `miss` where that row leaves. A comparison of the
/// one-character string with a one-unit constant decides on the unit alone, so the string is
/// never made (`Lowering::unit_compare`).
#[inline(always)]
pub fn string_unit_at<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    at_width(stage, string, |stage, width| char_at_ascii(stage, width, string.units, string.length, index, miss))
}

/// `string_unit_at` for an index the caller proves below the string's length (`char_at_ascii_within`).
#[inline(always)]
pub fn string_unit_at_within<S: Stage>(stage: &mut S, string: FlatString<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    at_width(stage, string, |stage, width| char_at_ascii_within(stage, width, string.units, index, miss))
}

/// The one-character string of the ASCII `unit` from the table at `table`, which the unit has once
/// it is made (`leaving at miss` where it has none yet).
#[inline(always)]
fn character_of_unit<S: Stage>(stage: &mut S, table: S::Word, unit: S::Word, miss: S::Miss) -> Option<S::Word> {
    let offset = stage.mul_imm(unit, size_of::<Value>() as u64);
    let address = stage.add(table, offset);
    let character = stage.load_u64(Region::HeapMeta.mutable(), address, 0);
    let made = stage.ne_imm(character, Value::UNDEFINED.to_bits());
    stage.guard(made, miss)?;
    Some(character)
}

#[cfg(not(stencil_build))]
/// A flat string a concrete caller holds, in either form: its record among the string records
/// at `records`, read in place, or its units already viewed, which covers the forms a record
/// read leaves.
#[derive(Clone, Copy)]
pub enum ConcreteString<'a> {
    /// The string `value` among the string records at `records`.
    Record { records: *const JsString, value: Value },
    /// The units of a flat string.
    Units(Units<'a>),
}

#[cfg(not(stencil_build))]
impl ConcreteString<'_> {
    /// This string's units as the words a row reads, or `None` where a record read leaves.
    #[inline(always)]
    fn flat(self) -> Option<FlatString<u64>> {
        match self {
            Self::Record { records, value } => {
                let mut stage = super::Concrete::new(std::ptr::null());
                flat_string(&mut stage, records as u64, value.to_bits(), ())
            }
            Self::Units(units) => {
                let wide = u64::from(matches!(units, Units::Wide(_)));
                let (base, length) = match units {
                    Units::Ascii(bytes) => (bytes.as_ptr() as u64, bytes.len() as u64),
                    Units::Wide(units) => (units.as_ptr() as u64, units.len() as u64),
                    // Latin-1 is no native row's form: the row leaves at its miss.
                    Units::Latin1(_) => return None,
                };
                Some(FlatString { units: base, length, wide })
            }
        }
    }
}

#[cfg(not(stencil_build))]
/// The interpreter's `string_char_code_at` of `string` at `index`; `None` where the row leaves,
/// which the caller answers through the native.
#[inline(always)]
pub fn concrete_char_code_at(string: ConcreteString<'_>, index: Value) -> Option<Value> {
    let string = string.flat()?;
    let mut stage = super::Concrete::new(std::ptr::null());
    string_char_code_at(&mut stage, string, index.to_bits(), ()).map(Value::from_bits)
}

#[cfg(not(stencil_build))]
/// The interpreter's `string_char_at` of `string` at `index`, from the table of one-unit strings
/// at `table`; `None` where the row leaves.
#[inline(always)]
pub fn concrete_char_at(string: ConcreteString<'_>, table: *const Value, index: Value) -> Option<Value> {
    let string = string.flat()?;
    let mut stage = super::Concrete::new(std::ptr::null());
    string_char_at(&mut stage, string, index.to_bits(), table as u64, ()).map(Value::from_bits)
}

/// The unit index that `String.prototype.at` (`from_end`) or `codePointAt` reads for the relative
/// `index`: a negative `at` index counts back from the end, and any other index is taken as given.
/// `None` where the index lies outside `units`. Plain over `Units`, which the stencils do not
/// name, so the row callers stay on the runtime.
#[cfg(not(stencil_build))]
pub(crate) fn relative_unit(units: Units<'_>, index: i64, from_end: bool) -> Option<usize> {
    let index = if from_end && index < 0 { units.len() as i64 + index } else { index };
    usize::try_from(index).ok().filter(|&index| index < units.len())
}

/// The code point that `codePointAt` answers at the unit `index` of `units`: the supplementary
/// code point of a high surrogate followed by a low one, else the unit itself. `None` past the end.
#[cfg(not(stencil_build))]
pub(crate) fn code_point_at(units: Units<'_>, index: usize) -> Option<u32> {
    let first = units.get(index)?;
    let pair = units.get(index + 1).and_then(|low| crate::unicode::decode_surrogate_pair(first, low));
    Some(pair.unwrap_or_else(|| u32::from(first)))
}

/// The code unit `String.fromCharCode` stores for an argument whose ToNumber is `number`: its
/// ToUint16, the low sixteen bits of the number's integer part (`value::number_to_u32`).
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn from_char_code_unit(number: f64) -> u16 {
    crate::value::number_to_u32(number) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;
    use crate::staged::Concrete;
    use crate::staged::value::length_value;
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
        let wide = heap.alloc_string(JsString::from_units(&[0x61, 0x20ac]));
        assert!(read(&heap, wide, Value::integer(0)).is_none(), "wide units are behind `Repr::Shared`: the row leaves");
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

    #[test]
    fn char_code_at_reads_units_of_either_width_and_answers_nan_outside() {
        let narrow = Units::Ascii(b"aZ");
        let wide = [0x61u16, 0x20ac];
        let wide = Units::Wide(&wide);
        let at = |units, index: Value| concrete_char_code_at(ConcreteString::Units(units), index).map(Value::to_bits);
        assert_eq!(at(narrow, Value::integer(1)), Some(Value::integer(0x5a).to_bits()));
        assert_eq!(at(wide, Value::integer(1)), Some(Value::integer(0x20ac).to_bits()));
        for index in [2, 100, -1, i32::MIN] {
            assert_eq!(at(narrow, Value::integer(index)), Some(Value::NAN.to_bits()), "{index} is outside");
        }
        assert_eq!(at(Units::Ascii(b""), Value::integer(0)), Some(Value::NAN.to_bits()));
        assert_eq!(at(narrow, Value::number(0.5)), None, "a double index leaves to the native");
        assert_eq!(at(narrow, Value::UNDEFINED), None, "an absent index leaves to the native");
    }

    #[test]
    fn char_at_answers_ascii_units_from_the_table_and_leaves_the_rest() {
        let mut table = [Value::UNDEFINED; 128];
        table[usize::from(b'y')] = Value::integer(7);
        table[0x7f] = Value::integer(9);
        let table = table.as_ptr();
        let wide = [0x61u16, 0x20ac, 0x7f];
        let wide = Units::Wide(&wide);
        let at = |units, index: i32| concrete_char_at(ConcreteString::Units(units), table, Value::integer(index));
        assert_eq!(at(Units::Ascii(b"xy"), 1), Some(Value::integer(7)));
        assert_eq!(at(Units::Ascii(b"xy"), 0), None, "a unit with no string yet leaves to the native");
        assert_eq!(at(wide, 1), None, "a unit past ASCII leaves");
        assert_eq!(at(wide, 2), Some(Value::integer(9)), "the last ASCII unit stays");
        assert_eq!(at(wide, 3), None, "outside leaves for the native's empty string");
        assert_eq!(at(wide, -1), None);
        assert_eq!(concrete_char_at(ConcreteString::Units(wide), table, Value::number(1.5)), None);
        let no_table: *const Value = std::ptr::null();
        assert_eq!(concrete_char_at(ConcreteString::Units(wide), no_table, Value::integer(2)), None, "no table yet");
    }
}
