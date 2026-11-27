//! The span primitives the string rows call: the first and the last place a run of units occurs in
//! another (`Stage::find_span`, `Stage::rfind_span`) and whether a run occurs at a given place
//! (`Stage::span_equal_at`). Each is a leaf a row names, like a libm function: the bodies here, by
//! rows over the units one at a time, are what a specializer emits by default, and a stage with a
//! tuned search runs that instead (`Concrete`: memchr, memmem and the block filter of
//! `vm::wtf16::search`), so the rows around them state the semantics once and every tier keeps its
//! fastest scan.

use super::Stage;
use super::code_units::code_unit;
use super::region::Region;

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



/// The span primitives the text rows write with: copying a run of units (`Stage::copy_span`),
/// filling a run with one unit (`Stage::fill_span`) and the ASCII case map of a run of bytes
/// (`Stage::case_span`, `Stage::case_changes_span`). The bodies below are rows over the units one at
/// a time; `Concrete` runs memcpy, memset and the slice loops instead.
#[inline(always)]
fn unit_at<S: Stage>(stage: &mut S, wide: bool, units: S::Word, index: S::Word) -> S::Word {
    let offset = stage.mul_imm(index, if wide { 2u64 } else { 1 });
    stage.add(units, offset)
}

/// Stores `unit` at `index` of the units at `units`, two bytes a unit where `wide`.
#[inline(always)]
fn store_unit<S: Stage>(stage: &mut S, wide: bool, units: S::Word, index: S::Word, unit: S::Word) {
    let address = unit_at(stage, wide, units, index);
    if wide {
        stage.store_u16(Region::HeapMeta.mutable(), address, 0, unit);
    } else {
        stage.store_u8(Region::HeapMeta.mutable(), address, 0, unit);
    }
}

/// Runs `body` for each index below `length`, one unit at a time; nothing for an empty run.
#[inline(always)]
fn each_unit<S: Stage>(stage: &mut S, length: S::Word, mut body: impl FnMut(&mut S, S::Word)) {
    let some = stage.ne_imm(length, 0u64);
    stage.choose(some, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let [_] = stage.loop_words([zero], #[inline(always)] |stage, [index]| {
            body(stage, index);
            let next = stage.add_imm(index, 1u64);
            (stage.ult(next, length), [next])
        });
        stage.constant(0)
    }, #[inline(always)] |stage| stage.constant(0));
}

/// The `length` units at `source` written to `target`, both of one width; the runs do not overlap.
#[inline(always)]
pub fn copy_by_rows<S: Stage>(stage: &mut S, wide: bool, target: S::Word, source: S::Word, length: S::Word) {
    each_unit(stage, length, #[inline(always)] |stage, index| {
        let unit = code_unit(stage, wide, source, index);
        store_unit(stage, wide, target, index, unit);
    });
}

/// `unit` written to each of the `length` units at `target`.
#[inline(always)]
pub fn fill_by_rows<S: Stage>(stage: &mut S, wide: bool, target: S::Word, unit: S::Word, length: S::Word) {
    each_unit(stage, length, #[inline(always)] |stage, index| store_unit(stage, wide, target, index, unit));
}

/// The ASCII case map of `byte`, upper or lower: a letter of the other case moves by 32, any
/// other byte is itself.
#[inline(always)]
pub fn case_byte<S: Stage>(stage: &mut S, byte: S::Word, upper: bool) -> S::Word {
    let first = if upper { b'a' } else { b'A' };
    let from_first = stage.add_imm(byte, u64::from(first).wrapping_neg());
    let letters = stage.constant(26);
    let letter = stage.ult(from_first, letters);
    let moved = stage.add_imm(byte, if upper { 32u64.wrapping_neg() } else { 32 });
    stage.select(letter, moved, byte)
}

/// The ASCII case map of the `length` bytes at `source` written to `target`.
#[inline(always)]
pub fn case_by_rows<S: Stage>(stage: &mut S, target: S::Word, source: S::Word, length: S::Word, upper: bool) {
    each_unit(stage, length, #[inline(always)] |stage, index| {
        let byte = code_unit(stage, false, source, index);
        let mapped = case_byte(stage, byte, upper);
        store_unit(stage, false, target, index, mapped);
    });
}

/// Whether the ASCII case map changes any of the `length` bytes at `source`.
#[inline(always)]
pub fn case_changes_by_rows<S: Stage>(stage: &mut S, source: S::Word, length: S::Word, upper: bool) -> S::Cond {
    let some = stage.ne_imm(length, 0u64);
    let changed = stage.choose(some, #[inline(always)] |stage| {
        let zero = stage.constant(0);
        let [index] = stage.loop_words([zero], #[inline(always)] |stage, [index]| {
            let byte = code_unit(stage, false, source, index);
            let mapped = case_byte(stage, byte, upper);
            let same = stage.eq(byte, mapped);
            let next = stage.add_imm(index, 1u64);
            let more = stage.ult(next, length);
            // A change stops at its own index, below the length; no change stops at the length.
            (stage.all(same, more), [stage.select(same, next, index)])
        });
        let unchanged = stage.eq(index, length);
        let (none, one) = (stage.constant(0), stage.constant(1));
        stage.select(unchanged, none, one)
    }, #[inline(always)] |stage| stage.constant(0));
    stage.ne_imm(changed, 0u64)
}
#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
