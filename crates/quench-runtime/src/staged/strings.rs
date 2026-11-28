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
pub fn relative_index(index: i64, length: usize, from_end: bool) -> Option<usize> {
    let mut stage = super::Concrete::pure();
    let (inside, position) = relative_index_row(&mut stage, index as u64, length as u64, from_end);
    inside.then_some(position as usize)
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

/// The UTF-16 units of the code point `point` (at most U+10FFFF): one unit, the point itself,
/// for a point up to U+FFFF (a lone surrogate too), else the two units of its surrogate pair,
/// the high one in the low 16 bits and the low one above it. Nonzero high bits mean two units,
/// since a low surrogate is never zero. The inverse of `code_point_at_row`.
#[inline(always)]
pub fn code_point_units_row<S: Stage>(stage: &mut S, point: S::Word) -> S::Word {
    let limit = stage.constant(0x10000);
    let single = stage.ult(point, limit);
    stage.choose(single, #[inline(always)] |_| point, #[inline(always)] |stage| {
        let payload = stage.add_imm(point, 0x10000u64.wrapping_neg());
        let top = stage.shr_imm(payload, 10u64);
        let high = stage.add_imm(top, 0xD800u64);
        let bottom = stage.and_imm(payload, 0x3FFu64);
        let low = stage.add_imm(bottom, 0xDC00u64);
        let shifted = stage.mul_imm(low, 0x10000u64);
        stage.or(high, shifted)
    })
}

/// `code_point_units_row` on `Concrete`: `point`'s units pushed to `units`.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn push_code_point_units(units: &mut Vec<u16>, point: u32) {
    let mut stage = super::Concrete::pure();
    let packed = code_point_units_row(&mut stage, u64::from(point));
    units.push(packed as u16);
    if packed >> 16 != 0 {
        units.push((packed >> 16) as u16);
    }
}


/// The code unit `String.fromCharCode` stores for an argument whose ToNumber is `number`: its
/// ToUint16, the low sixteen bits of the number's integer part (`value::number_to_u32`).
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn from_char_code_unit(number: f64) -> u16 {
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
mod tests;
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
