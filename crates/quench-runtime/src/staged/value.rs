//! Value-word reads every tier shares: which heap family a value names.

use super::Stage;
use crate::value::ValueLayout;

/// Whether `bits` is a direct object reference, whose low half names its record
/// (`Value::as_direct_object`).
#[inline(always)]
pub(crate) fn is_direct_object<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_OBJECT_TAG)
}

/// Whether `bits` is a direct array reference (`Value::as_direct_array`).
#[inline(always)]
pub(crate) fn is_direct_array<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_ARRAY_TAG)
}

/// Whether `bits` is a direct function reference (`Value::as_direct_function`).
#[inline(always)]
pub(crate) fn is_direct_function<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_FUNCTION_TAG)
}

/// ToBoolean of `bits` as far as its own word decides it (`Value::truthiness`):
/// `(undecided, truthy)`. A heap value other than an object or array record is undecided, its
/// cell deciding (a string's length, a BigInt, the IsHTMLDDA object); `truthy` holds for any
/// other truthy value: a record, a nonzero int32 or Number, or an immediate but `null`,
/// `undefined`, `false` and the deleted marker.
#[inline(always)]
pub(crate) fn tagged_truthiness<S: Stage>(stage: &mut S, bits: S::Word) -> (S::Cond, S::Cond) {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    let (object, array) = (is_direct_object(stage, bits), is_direct_array(stage, bits));
    let record = stage.any(object, array);
    let unheaped = stage.not(heap);
    let decided = stage.any(unheaped, record);
    let undecided = stage.not(decided);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let payload = stage.and_imm(bits, u64::from(u32::MAX));
    let integer_truthy = stage.ne_imm(payload, 0);
    let field = stage.shr_imm(bits, ValueLayout::TAG_SHIFT);
    let offset = stage.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    let past = stage.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN);
    let tagged = stage.not(past);
    let falsy = [crate::Value::NULL, crate::Value::UNDEFINED, crate::Value::DELETED, crate::Value::FALSE]
        .map(|value| stage.eq_imm(bits, value.to_bits()));
    let falsy = falsy.into_iter().reduce(|any, each| stage.any(any, each)).expect("four falsy immediates");
    let immediate_truthy = stage.not(falsy);
    let double_truthy = stage.number_truthy(bits);
    let tagged_truthy = stage.pick(integer, integer_truthy, immediate_truthy);
    let unheaped_truthy = stage.pick(tagged, tagged_truthy, double_truthy);
    (undecided, stage.pick(heap, record, unheaped_truthy))
}
