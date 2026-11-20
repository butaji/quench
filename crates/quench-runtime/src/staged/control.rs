//! The staged `typeof`: the name index of a value its own word decides, written once for the
//! interpreter and every tier.

use super::*;

use super::value::{TYPEOF_BOOLEAN, TYPEOF_NUMBER, TYPEOF_OBJECT, TYPEOF_UNDEFINED, typeof_classes};
use crate::value::ValueLayout;

/// The `typeof` name index (int32) of `source`, into `destination`, where the value's own
/// word decides it: a Number, a Boolean, `null`, `undefined` or the deleted marker. A heap
/// value's name is the kind the frame proves of its cell (`Frame::type_of`); a frame that proves
/// no kind leaves at the miss edge.
#[inline(always)]
pub(super) fn typeof_index<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let bits = frame.tagged(stage, source);
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    // A heap value's name is its cell's kind; every other value's word decides it.
    let name = stage.choose_leaving(heap, #[inline(always)] |stage| {
        let kind = frame.type_of(stage, bits)?;
        Some(stage.i32_constant(kind.index() as i32))
    }, #[inline(always)] |stage| Some(immediate_name(stage, bits)));
    let Some(name) = name else { return Step::Unresolved };
    let result = frame.int_result(stage, name);
    let fact = frame.int_fact();
    frame.write(stage, destination, result, fact);
    Step::Value(result)
}

/// `typeof source` tested against the name at `index` in `TYPEOF_NAMES`, negated where `negated`
/// (`typeof x === "name"`): the boolean where the value's name is known (`name_word`).
#[inline(always)]
pub(super) fn typeof_test<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register, index: u64, negated: bool) -> Step<S::Word, F::Control> {
    let bits = frame.tagged(stage, source);
    let Some(name) = name_word(stage, frame, bits) else { return Step::Unresolved };
    let named = stage.eq_imm(name, index);
    let holds = if negated { stage.not(named) } else { named };
    let (word, fact) = frame.boolean(stage, holds);
    frame.write(stage, destination, word, fact);
    Step::Value(word)
}

/// `typeof source` as its result string, into `destination`: the frame's interned string of the
/// name `name_word` gives (`Frame::typeof_string`).
#[inline(always)]
pub(super) fn typeof_string<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let bits = frame.tagged(stage, source);
    let Some(name) = name_word(stage, frame, bits) else { return Step::Unresolved };
    let Some((text, fact)) = frame.typeof_string(stage, name) else { return Step::Unresolved };
    frame.write(stage, destination, text, fact);
    Step::Value(text)
}

/// The `typeof` name index of the word `bits` as a word of the form `immediate_typeof_index`
/// yields: where the tag class decides it, that index; a heap value's is the kind the frame proves
/// of its cell (`Frame::type_of`). `None` where neither decides, which leaves at the miss edge.
#[inline(always)]
fn name_word<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, bits: S::Word) -> Option<S::Word> {
    let miss = frame.miss();
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    stage.choose_leaving(heap, #[inline(always)] |stage| {
        let kind = frame.type_of(stage, bits)?;
        Some(stage.constant(kind.index()))
    }, #[inline(always)] |stage| super::value::immediate_typeof_index(stage, bits, miss))
}

/// The name index of the immediate `bits`: every immediate that is no Number,
/// Boolean or `null` is `undefined` or the deleted marker, whose name it is.
#[inline(always)]
fn immediate_name<S: NumericStage>(stage: &mut S, bits: S::Word) -> S::Word {
    let (number, boolean, null) = typeof_classes(stage, bits);
    let mut name = stage.i32_constant(TYPEOF_UNDEFINED as i32);
    for (holds, index) in [(null, TYPEOF_OBJECT), (boolean, TYPEOF_BOOLEAN), (number, TYPEOF_NUMBER)] {
        let named = stage.i32_constant(index as i32);
        name = stage.select(holds, named, name);
    }
    name
}
