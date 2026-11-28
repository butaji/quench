//! The truthiness of a value word: undecided, falsy and truthy, by tag.

use super::*;

/// The states `tagged_truthiness` sorts a word into: undecided by the word, falsy, or truthy.
const UNDECIDED: u64 = 0;

const FALSY: u64 = 1;

const TRUTHY: u64 = 2;

/// ToBoolean of `bits` as far as its own word decides it: `(undecided, truthy)`. A heap value
/// other than an object or array record is undecided, its cell deciding (a string's length, a
/// BigInt, the IsHTMLDDA object); `truthy` holds for any other truthy value: a record, a nonzero
/// int32 or Number, or an immediate but `null`, `undefined`, `false` and the deleted marker.
/// `Value::truthiness` is this at `Concrete`, so every stage decides the same cases.
#[inline(always)]
pub fn tagged_truthiness<S: Stage>(stage: &mut S, bits: S::Word) -> (S::Cond, S::Cond) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    // The word's truth state: a heap word is decided by its record's test alone (its record is truthy,
    // else undecided), so the record tests and the immediate and Number ladder below run only in their own
    // arm. The arms compute the same truth as before.
    let state = stage.choose(heap, #[inline(always)] |stage| {
        let (object, array) = (is_direct_object(stage, bits), is_direct_array(stage, bits));
        let record = stage.any(object, array);
        let (truthy, undecided) = (stage.constant(TRUTHY), stage.constant(UNDECIDED));
        stage.select(record, truthy, undecided)
    }, #[inline(always)] |stage| {
        // An int32 is decided by its payload alone, so its arm runs no immediate or Number test.
        let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
        stage.choose(integer, #[inline(always)] |stage| {
            let payload = stage.and_imm(bits, u64::from(u32::MAX));
            let integer_truthy = stage.ne_imm(payload, 0u32);
            let (yes, no) = (stage.constant(TRUTHY), stage.constant(FALSY));
            stage.select(integer_truthy, yes, no)
        }, #[inline(always)] |stage| {
            let past = is_double(stage, bits);
            let tagged = stage.not(past);
            // Written out, not an iterator fold: a stencil inlines every step (no call to `reduce` or `expect`).
            let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
            let undefined = stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits());
            let deleted = stage.eq_imm(bits, crate::Value::DELETED.to_bits());
            let false_word = stage.eq_imm(bits, crate::Value::FALSE.to_bits());
            let nullish = stage.any(null, undefined);
            let absent = stage.any(deleted, false_word);
            let falsy = stage.any(nullish, absent);
            let immediate_truthy = stage.not(falsy);
            let double_truthy = stage.number_truthy(bits);
            let unheaped_truthy = stage.pick(tagged, immediate_truthy, double_truthy);
            let (yes, no) = (stage.constant(TRUTHY), stage.constant(FALSY));
            stage.select(unheaped_truthy, yes, no)
        })
    });
    (stage.eq_imm(state, UNDECIDED), stage.eq_imm(state, TRUTHY))
}

/// ToBoolean of a Boolean's `bits`: `true` alone is truthy.
#[inline(always)]
pub fn boolean_truthy<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    stage.eq_imm(bits, crate::Value::TRUE.to_bits())
}

/// ToBoolean of an integer word (an int32 or a 64-bit integer, as held): nonzero.
#[inline(always)]
pub fn int_truthy<S: Stage>(stage: &mut S, word: S::Word) -> S::Cond {
    stage.ne_imm(word, 0u32)
}
