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

/// A string native over a flat string: `charCodeAt`, and `charAt` from the table of one-unit
/// strings. The interpreter (`concrete_char_code_at`, `concrete_char_at`) and the b-jit's builtin
/// rows run it through the one step, `step_string_native_of`.
#[derive(Clone, Copy)]
pub enum StringNative<W> {
    /// `charCodeAt` of `string` at the index value word `index`.
    CharCodeAt { string: FlatString<W>, index: W },
    /// `charAt` of `string` at `index`, from the one-unit strings at `table`.
    CharAt { string: FlatString<W>, index: W, table: W },
}

/// The value word `native` answers: the one step of a string native, on any stage. `None`
/// (leaving at `miss`) where its row leaves (`string_char_code_at`, `string_char_at`).
#[inline(always)]
pub fn step_string_native_of<S: Stage>(stage: &mut S, native: StringNative<S::Word>, miss: S::Miss) -> Option<S::Word> {
    match native {
        StringNative::CharCodeAt { string, index } => string_char_code_at(stage, string, index, miss),
        StringNative::CharAt { string, index, table } => string_char_at(stage, string, index, table, miss),
    }
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
    step_string_native_of(&mut stage, StringNative::CharCodeAt { string, index: index.to_bits() }, ()).map(Value::from_bits)
}

#[cfg(not(stencil_build))]
/// The interpreter's `string_char_at` of `string` at `index`, from the table of one-unit strings
/// at `table`; `None` where the row leaves.
#[inline(always)]
pub fn concrete_char_at(string: ConcreteString<'_>, table: *const Value, index: Value) -> Option<Value> {
    let string = string.flat()?;
    let mut stage = super::Concrete::new(std::ptr::null());
    step_string_native_of(&mut stage, StringNative::CharAt { string, index: index.to_bits(), table: table as u64 }, ()).map(Value::from_bits)
}

/// The position a relative `index` (an integer word) names in a run of `length` elements or
/// units, and whether it lies inside: a negative index counts back from the end where `from_end`
/// (`at`), and is outside otherwise (`codePointAt`). The one body of `String.prototype.at`,
/// `codePointAt`, `Array.prototype.at` and `with` for every tier.
#[inline(always)]
pub fn relative_index_row<S: Stage>(stage: &mut S, index: S::Word, length: S::Word, from_end: bool) -> (S::Cond, S::Word) {
    let position = if from_end {
        let sign = stage.shr_imm(index, 63u64);
        let below = stage.ne_imm(sign, 0u64);
        let counted = stage.add(length, index);
        stage.select(below, counted, index)
    } else {
        index
    };
    // A position below zero is a huge unsigned word, past any length.
    (stage.ult(position, length), position)
}

/// `relative_index_row` on `Concrete`: the position `index` names in a run of `length`, or `None`.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn relative_index(index: i64, length: usize, from_end: bool) -> Option<usize> {
    let mut stage = super::Concrete::new(std::ptr::null());
    let (inside, position) = relative_index_row(&mut stage, index as u64, length as u64, from_end);
    inside.then_some(position as usize)
}

/// The unit index that `String.prototype.at` (`from_end`) or `codePointAt` reads for the relative
/// `index` of `units` (`relative_index_row`); `None` where it lies outside.
#[cfg(not(stencil_build))]
pub(crate) fn relative_unit(units: Units<'_>, index: i64, from_end: bool) -> Option<usize> {
    relative_index(index, units.len(), from_end)
}

/// The code point that `codePointAt` answers at the unit `index` (inside the `length` units at
/// `units`, two bytes a unit where `wide`): the supplementary code point of a high surrogate
/// followed by a low one, else the unit itself. The one body for every tier.
#[inline(always)]
pub fn code_point_at_row<S: Stage>(stage: &mut S, wide: bool, units: S::Word, length: S::Word, index: S::Word) -> S::Word {
    let first = super::code_units::code_unit(stage, wide, units, index);
    if !wide {
        return first;
    }
    let from_high = stage.add_imm(first, 0xD800u64.wrapping_neg());
    let span = stage.constant(0x400);
    let high = stage.ult(from_high, span);
    let next = stage.add_imm(index, 1u64);
    let room = stage.ult(next, length);
    let pairable = stage.all(high, room);
    stage.choose(pairable, #[inline(always)] |stage| {
        let second = super::code_units::code_unit(stage, true, units, next);
        let from_low = stage.add_imm(second, 0xDC00u64.wrapping_neg());
        let span = stage.constant(0x400);
        let low = stage.ult(from_low, span);
        let upper = stage.mul_imm(from_high, 0x400u64);
        let joined = stage.add(upper, from_low);
        let point = stage.add_imm(joined, 0x10000u64);
        stage.select(low, point, first)
    }, #[inline(always)] |_| first)
}

/// `code_point_at_row` on `Concrete` over `units`; `None` past the end.
#[cfg(not(stencil_build))]
pub(crate) fn code_point_at(units: Units<'_>, index: usize) -> Option<u32> {
    if index >= units.len() {
        return None;
    }
    let span = super::spans::concrete_span(units);
    let mut stage = super::Concrete::new(std::ptr::null());
    Some(code_point_at_row(&mut stage, span.form.wide(), span.units, span.length, index as u64) as u32)
}

/// The code unit `String.fromCharCode` stores for an argument whose ToNumber is `number`: its
/// ToUint16, the low sixteen bits of the number's integer part (`value::number_to_u32`).
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn from_char_code_unit(number: f64) -> u16 {
    crate::value::number_to_u32(number) as u16
}

/// Whether the code unit `unit` is ECMAScript WhiteSpace or LineTerminator (what `trim` strips):
/// U+0009..U+000D, U+0020, U+00A0, and, of a wide string, U+1680, U+2000..U+200A, U+2028, U+2029,
/// U+202F, U+205F, U+3000 and U+FEFF. A narrow unit is at most U+00FF, so it tests the first three.
#[inline(always)]
pub fn is_trim_space<S: Stage>(stage: &mut S, width: UnitWidth, unit: S::Word) -> S::Cond {
    let in_range = |stage: &mut S, first: u64, count: u64| {
        let offset = stage.add_imm(unit, first.wrapping_neg());
        let bound = stage.constant(count);
        stage.ult(offset, bound)
    };
    let controls = in_range(stage, 0x09, 5);
    let space = stage.eq_imm(unit, 0x20u64);
    let no_break = stage.eq_imm(unit, 0xA0u64);
    let narrow = stage.any(controls, space);
    let narrow = stage.any(narrow, no_break);
    if width == UnitWidth::Narrow {
        return narrow;
    }
    let mut found = narrow;
    let spaces = in_range(stage, 0x2000, 11);
    found = stage.any(found, spaces);
    let separators = in_range(stage, 0x2028, 2);
    found = stage.any(found, separators);
    for single in [0x1680u64, 0x202F, 0x205F, 0x3000, 0xFEFF] {
        let same = stage.eq_imm(unit, single);
        found = stage.any(found, same);
    }
    found
}

/// The units `trim` keeps of the flat string of `width` at `units`, `length` long: the start and end
/// left once the whitespace (`is_trim_space`) at the chosen ends is stripped. `trimStart` keeps the
/// end, `trimEnd` the start. The one body of `trim`, `trimStart` and `trimEnd` for every tier.
#[inline(always)]
pub fn trim_bounds<S: Stage>(stage: &mut S, width: UnitWidth, units: S::Word, length: S::Word,
    (from_start, from_end): (bool, bool)) -> (S::Word, S::Word) {
    let zero = stage.constant(0);
    let start = if from_start {
        let [start] = stage.loop_words([zero], #[inline(always)] |stage, [index]| {
            let inside = stage.ult(index, length);
            let space = stage.choose(inside, #[inline(always)] |stage| {
                let unit = super::code_units::code_unit(stage, width == UnitWidth::Wide, units, index);
                let space = is_trim_space(stage, width, unit);
                let (one, none) = (stage.constant(1), stage.constant(0));
                stage.select(space, one, none)
            }, #[inline(always)] |stage| stage.constant(0));
            let more = stage.ne_imm(space, 0u64);
            (more, [stage.add(index, space)])
        });
        start
    } else {
        zero
    };
    let end = if from_end {
        let [end] = stage.loop_words([length], #[inline(always)] |stage, [end]| {
            let inside = stage.ult(start, end);
            let space = stage.choose(inside, #[inline(always)] |stage| {
                let last = stage.add_imm(end, u64::MAX);
                let unit = super::code_units::code_unit(stage, width == UnitWidth::Wide, units, last);
                let space = is_trim_space(stage, width, unit);
                let (one, none) = (stage.constant(1), stage.constant(0));
                stage.select(space, one, none)
            }, #[inline(always)] |stage| stage.constant(0));
            let more = stage.ne_imm(space, 0u64);
            (more, [stage.sub(end, space)])
        });
        end
    } else {
        length
    };
    (start, end)
}

/// `count` units of a pattern already at the start of the `length` units at `out` (`count`
/// at most `length`, above zero) doubled through the rest: each copy takes the units written so
/// far, or the cut still left, so a run of n units takes about log2(n / count) copies.
#[inline(always)]
fn double_through<S: Stage>(stage: &mut S, wide: bool, out: S::Word, length: S::Word, count: S::Word) {
    let unit_bytes = if wide { 2u64 } else { 1 };
    let more = stage.ult(count, length);
    stage.choose(more, #[inline(always)] |stage| {
        let [_] = stage.loop_words([count], #[inline(always)] |stage, [written]| {
            let left = stage.sub(length, written);
            let short = stage.ult(left, written);
            let step = stage.select(short, left, written);
            let offset = stage.mul_imm(written, unit_bytes);
            let at = stage.add(out, offset);
            stage.copy_span(wide, at, out, step);
            let next = stage.add(written, step);
            (stage.ult(next, length), [next])
        });
        stage.constant(0)
    }, #[inline(always)] |stage| stage.constant(0));
}

/// `padStart` or `padEnd`: the `text_length` units at `text` and repeats of the `fill_length`
/// (non-empty) units at `fill` written to the `length` units at `out` (`length` above
/// `text_length`), the filler at the end when `at_end`, else at the start. All are of one width.
/// A one-unit filler is a fill; a longer one is copied once, or its cut, then doubled. The one
/// body of both builtins for every tier, over the span primitives.
#[inline(always)]
pub fn pad_into_row<S: Stage>(stage: &mut S, wide: bool, out: S::Word, length: S::Word, text: S::Word, text_length: S::Word,
    fill: S::Word, fill_length: S::Word, at_end: bool) {
    let unit_bytes = if wide { 2u64 } else { 1 };
    let pad_length = stage.sub(length, text_length);
    let (text_out, pad_out) = if at_end {
        let after = stage.mul_imm(text_length, unit_bytes);
        (out, stage.add(out, after))
    } else {
        let after = stage.mul_imm(pad_length, unit_bytes);
        (stage.add(out, after), out)
    };
    stage.copy_span(wide, text_out, text, text_length);
    let single = stage.eq_imm(fill_length, 1u64);
    stage.choose(single, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let unit = super::code_units::code_unit(stage, wide, fill, zero);
        stage.fill_span(wide, pad_out, unit, pad_length);
        stage.constant(0)
    }, #[inline(always)] |stage| {
        let short = stage.ult(pad_length, fill_length);
        let first = stage.select(short, pad_length, fill_length);
        stage.copy_span(wide, pad_out, fill, first);
        double_through(stage, wide, pad_out, pad_length, first);
        stage.constant(0)
    });
}

/// `repeat`: the `unit_length` (non-empty) bytes at `unit` copied through the `length` bytes at
/// `out`, a whole multiple of them: one copy, then doubled. The one body of the builtin for every tier.
#[inline(always)]
pub fn repeat_into_row<S: Stage>(stage: &mut S, out: S::Word, length: S::Word, unit: S::Word, unit_length: S::Word) {
    let some = stage.ne_imm(length, 0u64);
    stage.choose(some, #[inline(always)] |stage| {
        stage.copy_span(false, out, unit, unit_length);
        double_through(stage, false, out, length, unit_length);
        stage.constant(0)
    }, #[inline(always)] |stage| stage.constant(0));
}

/// One past the first surrogate unit at or after `from` (`from < length`) in the `length` wide
/// units at `units`, or `length` where there is none: the hot scan of `lone_surrogate_row`, which
/// tells the two apart by the unit before the answer.
#[inline(always)]
fn past_surrogate<S: Stage>(stage: &mut S, units: S::Word, length: S::Word, from: S::Word) -> S::Word {
    let [past] = stage.loop_words([from], #[inline(always)] |stage, [index]| {
        let surrogate = surrogate_at(stage, units, index);
        let next = stage.add_imm(index, 1u64);
        let room = stage.ult(next, length);
        let plain = stage.not(surrogate);
        (stage.all(plain, room), [next])
    });
    past
}

/// Whether the wide unit at `index` is a surrogate (0xD800..=0xDFFF).
#[inline(always)]
fn surrogate_at<S: Stage>(stage: &mut S, units: S::Word, index: S::Word) -> S::Cond {
    let unit = super::code_units::code_unit(stage, true, units, index);
    let from_high = stage.add_imm(unit, 0xD800u64.wrapping_neg());
    let span = stage.constant(0x800);
    stage.ult(from_high, span)
}

/// The index of the first lone surrogate at or after `from` in the `length` wide units at
/// `units` (a high surrogate with no low one after it, or a low one with no high one before it),
/// or `length` where there is none. A pair is stepped over whole. The one body of
/// `isWellFormed` and `toWellFormed` for every tier; a narrow string holds no surrogate.
#[inline(always)]
pub fn lone_surrogate_row<S: Stage>(stage: &mut S, units: S::Word, length: S::Word, from: S::Word) -> S::Word {
    let some = stage.ult(from, length);
    stage.choose(some, #[inline(always)] |stage| {
        // Each turn scans to the next surrogate; a pair resumes after it, a lone one or the end
        // leaves with its index marked by the top bit.
        let [found] = stage.loop_words([from], #[inline(always)] |stage, [start]| {
            let past = past_surrogate(stage, units, length, start);
            let index = stage.add_imm(past, u64::MAX);
            let surrogate = surrogate_at(stage, units, index);
            let step = stage.choose(surrogate, #[inline(always)] |stage| {
                let unit = super::code_units::code_unit(stage, true, units, index);
                let from_high = stage.add_imm(unit, 0xD800u64.wrapping_neg());
                let quarter = stage.constant(0x400);
                let high = stage.ult(from_high, quarter);
                let room = stage.ult(past, length);
                let pairable = stage.all(high, room);
                let paired = stage.choose(pairable, #[inline(always)] |stage| {
                    let low = super::code_units::code_unit(stage, true, units, past);
                    let from_low = stage.add_imm(low, 0xDC00u64.wrapping_neg());
                    let quarter = stage.constant(0x400);
                    let is_low = stage.ult(from_low, quarter);
                    let (one, none) = (stage.constant(1), stage.constant(0));
                    stage.select(is_low, one, none)
                }, #[inline(always)] |stage| stage.constant(0));
                let pair = stage.ne_imm(paired, 0u64);
                let after = stage.add_imm(past, 1u64);
                let lone = stage.or_imm(index, 1u64 << 63);
                stage.select(pair, after, lone)
            }, #[inline(always)] |stage| stage.or_imm(length, 1u64 << 63));
            let mark = stage.shr_imm(step, 63u64);
            let lone = stage.ne_imm(mark, 0u64);
            let room = stage.ult(step, length);
            let clear = stage.not(lone);
            (stage.all(clear, room), [step])
        });
        let mark = stage.shr_imm(found, 63u64);
        let lone = stage.ne_imm(mark, 0u64);
        let index = stage.and_imm(found, u64::MAX >> 1);
        stage.select(lone, index, length)
    }, #[inline(always)] |_| length)
}

/// `toWellFormed` of the `length` wide units at `units` written to the `length` units at `out`:
/// each run up to a lone surrogate copied, the surrogate replaced by U+FFFD.
#[inline(always)]
pub fn to_well_formed_row<S: Stage>(stage: &mut S, units: S::Word, length: S::Word, out: S::Word) {
    let some = stage.ne_imm(length, 0u64);
    stage.choose(some, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let [_] = stage.loop_words([zero], #[inline(always)] |stage, [from]| {
            let lone = lone_surrogate_row(stage, units, length, from);
            let count = stage.sub(lone, from);
            let offset = stage.mul_imm(from, 2u64);
            let (source, target) = (stage.add(units, offset), stage.add(out, offset));
            stage.copy_span(true, target, source, count);
            let inside = stage.ult(lone, length);
            stage.choose(inside, #[inline(always)] |stage| {
                let offset = stage.mul_imm(lone, 2u64);
                let at = stage.add(out, offset);
                let replacement = stage.constant(0xFFFD);
                stage.store_u16(Region::HeapMeta.mutable(), at, 0, replacement);
                stage.constant(0)
            }, #[inline(always)] |stage| stage.constant(0));
            let next = stage.add_imm(lone, 1u64);
            (stage.ult(next, length), [next])
        });
        stage.constant(0)
    }, #[inline(always)] |stage| stage.constant(0));
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

    /// The ECMAScript WhiteSpace and LineTerminator units, as the specification lists them.
    fn spec_space(unit: u16) -> bool {
        matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
    }

    #[test]
    fn trim_space_is_the_specification_set_for_every_unit() {
        for unit in 0..=u16::MAX {
            assert_eq!(is_trim_space(&mut stage(), UnitWidth::Wide, u64::from(unit)), spec_space(unit), "{unit:#x}");
        }
        for unit in 0..=u8::MAX {
            assert_eq!(is_trim_space(&mut stage(), UnitWidth::Narrow, u64::from(unit)), spec_space(u16::from(unit)), "{unit:#x}");
        }
    }

    #[test]
    fn trim_bounds_strip_the_chosen_ends() {
        let narrow = |text: &[u8], ends| {
            let (start, end) = trim_bounds(&mut stage(), UnitWidth::Narrow, text.as_ptr() as u64, text.len() as u64, ends);
            (start, end)
        };
        assert_eq!(narrow(b"", (true, true)), (0, 0));
        assert_eq!(narrow(b"   ", (true, true)), (3, 3));
        assert_eq!(narrow(b"   ", (false, true)), (0, 0));
        assert_eq!(narrow(b" \t a b \n", (true, true)), (3, 6));
        assert_eq!(narrow(b" \t a b \n", (true, false)), (3, 9));
        assert_eq!(narrow(b" \t a b \n", (false, true)), (0, 6));
        assert_eq!(narrow(b"\xa0x\xa0", (true, true)), (1, 2));
        let wide: Vec<u16> = [0xFEFF, 0x3000, 0x41, 0x2028, 0x42, 0x205F].to_vec();
        let (start, end) = trim_bounds(&mut stage(), UnitWidth::Wide, wide.as_ptr() as u64, wide.len() as u64, (true, true));
        assert_eq!((start, end), (2, 5));
    }
}
