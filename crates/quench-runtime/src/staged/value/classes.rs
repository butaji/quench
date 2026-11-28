//! The value classes a tag names: languages, undefined, null, arrays, collections and functions.

use super::*;

/// The language bits (`language`) a word's tag alone sets: a Number (an int32 or a double), a Boolean,
/// `null`, `undefined`, a direct object family record (`OBJECT`, and `OBJECT_PLAIN` for a plain one), a
/// direct array (`ARRAY`, with the `OBJECT` bit, as the family test the object family makes), a direct
/// function (`FUNCTION`), a direct string (`STRING`, the exact family test `is_direct_string` makes). A heap value
/// no tag decides (a rope or heap string, a symbol, a BigInt) sets none.
#[inline(always)]
pub fn tag_languages<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    let plain = family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG);
    let array = family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG);
    let function = family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG);
    let string = is_direct_string(stage, bits);
    let mut object = stage.any(plain, array);
    object = stage.any(object, function);
    for tag in [ValueLayout::DIRECT_PROMISE_TAG, ValueLayout::DIRECT_ITERATOR_TAG] {
        let family = family_is(stage, bits, tag);
        object = stage.any(object, family);
    }
    let int = is_int(stage, bits);
    let double = is_double(stage, bits);
    let number = stage.any(int, double);
    let unflagged = stage.and_imm(bits, !(crate::Value::TRUE.to_bits() ^ crate::Value::FALSE.to_bits()));
    let boolean = stage.eq_imm(unflagged, crate::Value::FALSE.to_bits());
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    let undefined = stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits());
    let classes = [(number, language::NUMBER), (boolean, language::BOOLEAN), (null, language::NULL), (undefined, language::UNDEFINED),
        (object, language::OBJECT), (plain, language::OBJECT_PLAIN), (array, language::ARRAY), (function, language::FUNCTION),
        (string, language::STRING)];
    let mut languages = stage.constant(0);
    for (hit, bit) in classes {
        let flag = stage.constant(u64::from(bit));
        let none = stage.constant(0);
        let added = stage.select(hit, flag, none);
        languages = stage.or(languages, added);
    }
    languages
}

/// Whether `bits` is `true` or `false` (`Value::as_bool`).
#[inline(always)]
pub fn is_boolean<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let (truthy, falsy) = (stage.eq_imm(bits, crate::Value::TRUE.to_bits()), stage.eq_imm(bits, crate::Value::FALSE.to_bits()));
    stage.any(truthy, falsy)
}

/// Whether `bits` is `undefined`.
#[inline(always)]
pub fn is_undefined<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    use crate::typed_ir::Type;
    let undefined = Type::language(crate::ValueType::Undefined);
    if let Some(known) = decided(stage, bits, |fact| fact.only_languages(undefined), |fact| fact.may_be_languages(undefined)) { return known; }
    stage.eq_imm(bits, crate::Value::UNDEFINED.to_bits())
}

/// Whether `bits` is `null`.
#[inline(always)]
pub fn is_null<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    use crate::typed_ir::Type;
    let null = Type::language(crate::ValueType::Null);
    if let Some(known) = decided(stage, bits, |fact| fact.only_languages(null), |fact| fact.may_be_languages(null)) { return known; }
    stage.eq_imm(bits, crate::Value::NULL.to_bits())
}

/// The value word of an int32 whose 64-bit `payload` is its zero-extended bits (`Value::int`).
#[inline(always)]
pub fn int_word<S: Stage>(stage: &mut S, payload: S::Word) -> S::Word {
    stage.or_imm(payload, ValueLayout::INT_TAG)
}

/// Whether `bits` is a direct string reference (`Value::as_direct_string`).
#[inline(always)]
pub fn is_direct_string<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_STRING_TAG)
}

/// The int32 value word of a `length`: `None` (leaving at `miss`) past int32, which
/// `Value::length` holds as a double.
#[inline(always)]
pub fn length_value<S: Stage>(stage: &mut S, length: S::Word, miss: S::Miss) -> Option<S::Word> {
    let beyond = stage.ugt_imm(length, i32::MAX as u64);
    let within = stage.not(beyond);
    stage.guard(within, miss)?;
    Some(stage.or_imm(length, ValueLayout::INT_TAG))
}

/// Whether `bits` is a direct array reference (`Value::as_direct_array`).
#[inline(always)]
pub fn is_direct_array<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG)
}

/// Whether `bits` is a direct Map or Set reference (`Value::direct_collection`).
#[inline(always)]
pub fn is_direct_collection<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_COLLECTION_TAG)
}

/// Whether `bits` is a legacy slot reference (`Value::legacy_heap_index`): a heap value of no
/// record family, whose low half is its slot.
#[inline(always)]
pub fn is_legacy_slot<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::HEAP_TAG)
}

/// Whether `bits` is a direct function reference (`Value::as_direct_function`).
#[inline(always)]
pub fn is_direct_function<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG)
}
