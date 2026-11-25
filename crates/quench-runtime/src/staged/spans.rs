//! The span primitives the string rows call: the first and the last place a run of units occurs in
//! another (`Stage::find_span`, `Stage::rfind_span`) and whether a run occurs at a given place
//! (`Stage::span_equal_at`). Each is a leaf a row names, like a libm function: the bodies here, by
//! rows over the units one at a time, are what a specializer emits by default, and a stage with a
//! tuned search runs that instead (`Concrete`: memchr, memmem and the block filter of
//! `vm::wtf16::search`), so the rows around them state the semantics once and every tier keeps its
//! fastest scan.

use super::Stage;
use super::code_units::code_unit;

/// How a span's units are stored: one byte a unit, all ASCII (`Ascii`) or any Latin-1 (`Latin1`), or
/// two bytes a unit (`Wide`). A static value, so each form's code is its own in a specializer's output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpanForm {
    Ascii,
    Latin1,
    Wide,
}

impl SpanForm {
    /// Whether the units are two bytes each.
    #[inline(always)]
    pub const fn wide(self) -> bool {
        matches!(self, SpanForm::Wide)
    }
}

/// A flat run of string units as the words a primitive reads: its form, the address of its first
/// unit and how many units it has.
#[derive(Clone, Copy)]
pub struct UnitSpan<W> {
    pub form: SpanForm,
    pub units: W,
    pub length: W,
}

/// Whether the units of `needle` are the units of `text` from `start`, one unit at a time; an empty
/// needle matches. The caller proves `start + needle.length <= text.length`.
#[inline(always)]
pub fn equal_at_by_rows<S: Stage>(stage: &mut S, text: UnitSpan<S::Word>, needle: UnitSpan<S::Word>, start: S::Word) -> S::Cond {
    let nonempty = stage.ne_imm(needle.length, 0u64);
    let same = stage.choose(nonempty, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let [index] = stage.loop_words([zero], #[inline(always)] |stage, [index]| {
            let at = stage.add(start, index);
            let left = code_unit(stage, text.form.wide(), text.units, at);
            let right = code_unit(stage, needle.form.wide(), needle.units, index);
            let same = stage.eq(left, right);
            let next = stage.add_imm(index, 1u64);
            let more = stage.ult(next, needle.length);
            let go_on = stage.all(same, more);
            // A mismatch stops at its own index, below the length; a full match stops at the length.
            (go_on, [stage.select(same, next, index)])
        });
        let full = stage.eq(index, needle.length);
        let (one, none) = (stage.constant(1), stage.constant(0));
        stage.select(full, one, none)
    }, #[inline(always)] |stage| stage.constant(1));
    stage.ne_imm(same, 0u64)
}

/// The last start a needle of `needle` units can begin at in a text of `length`, and whether it fits at all.
#[inline(always)]
fn last_start<S: Stage>(stage: &mut S, length: S::Word, needle: S::Word) -> (S::Cond, S::Word) {
    let short = stage.ult(length, needle);
    let fits = stage.not(short);
    let room = stage.sub(length, needle);
    let zero = stage.constant(0);
    (fits, stage.select(fits, room, zero))
}

/// The first index at or after `from` where `needle` occurs in `text`, and whether one does. `from` is
/// at most the text's length (the rows clamp it).
#[inline(always)]
pub fn find_by_rows<S: Stage>(stage: &mut S, text: UnitSpan<S::Word>, needle: UnitSpan<S::Word>, from: S::Word) -> (S::Cond, S::Word) {
    let (fits, last) = last_start(stage, text.length, needle.length);
    let past = stage.ult(last, from);
    let empty = stage.not(fits);
    let none = stage.any(empty, past);
    let searched = stage.choose(none, #[inline(always)] |stage| stage.constant(u64::MAX), #[inline(always)] |stage| {
        let [at] = stage.loop_words([from], #[inline(always)] |stage, [at]| {
            let matched = equal_at_by_rows(stage, text, needle, at);
            let room = stage.ult(at, last);
            let unmatched = stage.not(matched);
            let go_on = stage.all(unmatched, room);
            let next = stage.add_imm(at, 1u64);
            let no_room = stage.not(room);
            let missed = stage.all(unmatched, no_room);
            let ended = stage.constant(u64::MAX);
            let stepped = stage.select(go_on, next, at);
            (go_on, [stage.select(missed, ended, stepped)])
        });
        at
    });
    (stage.ne_imm(searched, u64::MAX), searched)
}

/// The last index at or before `from` where `needle` occurs in `text`, and whether one does: `from`
/// is first clamped to the last start the needle fits at.
#[inline(always)]
pub fn rfind_by_rows<S: Stage>(stage: &mut S, text: UnitSpan<S::Word>, needle: UnitSpan<S::Word>, from: S::Word) -> (S::Cond, S::Word) {
    let (fits, last) = last_start(stage, text.length, needle.length);
    let past = stage.ult(last, from);
    let from = stage.select(past, last, from);
    let empty = stage.not(fits);
    let searched = stage.choose(empty, #[inline(always)] |stage| stage.constant(u64::MAX), #[inline(always)] |stage| {
        let [at] = stage.loop_words([from], #[inline(always)] |stage, [at]| {
            let matched = equal_at_by_rows(stage, text, needle, at);
            let room = stage.ne_imm(at, 0u64);
            let unmatched = stage.not(matched);
            let go_on = stage.all(unmatched, room);
            let previous = stage.add_imm(at, u64::MAX);
            let no_room = stage.not(room);
            let missed = stage.all(unmatched, no_room);
            let ended = stage.constant(u64::MAX);
            let stepped = stage.select(go_on, previous, at);
            (go_on, [stage.select(missed, ended, stepped)])
        });
        at
    });
    (stage.ne_imm(searched, u64::MAX), searched)
}

/// The units of a span on `Concrete`, as the tuned searches take them.
#[cfg(not(stencil_build))]
pub(crate) fn concrete_units<'a>(span: UnitSpan<u64>) -> crate::vm::wtf16::Units<'a> {
    use crate::vm::wtf16::Units;
    let length = span.length as usize;
    // SAFETY: a Concrete span names the live units of a string the caller holds for the call.
    unsafe {
        match span.form {
            SpanForm::Ascii => Units::Ascii(std::slice::from_raw_parts(span.units as *const u8, length)),
            SpanForm::Latin1 => Units::Latin1(std::slice::from_raw_parts(span.units as *const u8, length)),
            SpanForm::Wide => Units::Wide(std::slice::from_raw_parts(span.units as *const u16, length)),
        }
    }
}

/// The span of `units` on `Concrete`: its form, address and length.
#[cfg(not(stencil_build))]
pub(crate) fn concrete_span(units: crate::vm::wtf16::Units<'_>) -> UnitSpan<u64> {
    use crate::vm::wtf16::Units;
    match units {
        Units::Ascii(bytes) => UnitSpan { form: SpanForm::Ascii, units: bytes.as_ptr() as u64, length: bytes.len() as u64 },
        Units::Latin1(bytes) => UnitSpan { form: SpanForm::Latin1, units: bytes.as_ptr() as u64, length: bytes.len() as u64 },
        Units::Wide(wide) => UnitSpan { form: SpanForm::Wide, units: wide.as_ptr() as u64, length: wide.len() as u64 },
    }
}
