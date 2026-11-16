//! Value-word reads every tier shares: which heap family a value names.

use super::region::Region;
use super::Stage;
use crate::value::ValueLayout;

/// The family field is the word's top bits, so a family test is one shift and a compare with
/// a small immediate, with no 64-bit mask or tag to load.
const FAMILY_SHIFT: u32 = ValueLayout::HEAP_FAMILY_MASK.trailing_zeros();
const _: () = assert!(ValueLayout::HEAP_FAMILY_MASK == u64::MAX << FAMILY_SHIFT);

#[inline(always)]
fn family_is<S: Stage>(stage: &mut S, bits: S::Word, tag: u64) -> S::Cond {
    let family = stage.shr_imm(bits, FAMILY_SHIFT);
    stage.eq_imm(family, tag >> FAMILY_SHIFT)
}

/// Whether `bits` is a direct object reference, whose low half names its record
/// (`Value::as_direct_object`).
#[inline(always)]
pub(crate) fn is_direct_object<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_OBJECT_TAG)
}

/// The id of the object record a direct object reference names, its low half (`Value::direct_object_index`).
#[inline(always)]
pub(crate) fn direct_object_id<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    stage.and_imm(bits, u64::from(u32::MAX))
}

/// Whether `bits` is an int32 (`Value::as_int`); an int32 has one tagged form, so two
/// distinct int-tagged words are two distinct values.
#[inline(always)]
pub(crate) fn is_int<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    stage.eq_imm(tag, ValueLayout::INT_TAG)
}

/// Whether `bits` is a direct string reference (`Value::as_direct_string`).
#[inline(always)]
pub(crate) fn is_direct_string<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let family = stage.and_imm(bits, ValueLayout::HEAP_FAMILY_MASK);
    stage.eq_imm(family, ValueLayout::DIRECT_STRING_TAG)
}

/// The int32 value word of a `length`: `None` (leaving at `miss`) past int32, which
/// `Value::length` holds as a double.
#[inline(always)]
pub(crate) fn length_value<S: Stage>(stage: &mut S, length: S::Word, miss: S::Miss) -> Option<S::Word> {
    let beyond = stage.ugt_imm(length, i32::MAX as u64);
    let within = stage.not(beyond);
    stage.guard(within, miss)?;
    Some(stage.or_imm(length, ValueLayout::INT_TAG))
}

/// Whether `bits` is a direct array reference (`Value::as_direct_array`).
#[inline(always)]
pub(crate) fn is_direct_array<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_ARRAY_TAG)
}

/// Whether `bits` is a legacy slot reference (`Value::legacy_heap_index`): a heap value of no
/// record family, whose low half is its slot.
#[inline(always)]
pub(crate) fn is_legacy_slot<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::HEAP_TAG)
}

/// Whether `bits` is a direct function reference (`Value::as_direct_function`).
#[inline(always)]
pub(crate) fn is_direct_function<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    family_is(stage, bits, ValueLayout::DIRECT_FUNCTION_TAG)
}

/// Whether `bits` is a binary64 Number: past the span of the tag fields the immediates use.
#[inline(always)]
pub(crate) fn is_double<S: Stage>(stage: &mut S, bits: S::Word) -> S::Cond {
    let field = stage.shr_imm(bits, ValueLayout::TAG_SHIFT);
    let offset = stage.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    stage.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN)
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
    let integer_truthy = stage.ne_imm(payload, 0u32);
    let past = is_double(stage, bits);
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

/// The indexes in `bytecode::TYPEOF_NAMES` of the names a value's tag alone decides: `typeof` of
/// `undefined`, of `null` (an object) and a boolean and a Number. A test pins each to its name.
pub(crate) const TYPEOF_UNDEFINED: u64 = 0;
pub(crate) const TYPEOF_OBJECT: u64 = 1;
pub(crate) const TYPEOF_BOOLEAN: u64 = 2;
pub(crate) const TYPEOF_NUMBER: u64 = 3;

/// The index in `bytecode::TYPEOF_NAMES` of `typeof` of the value `bits` where its tag alone decides
/// it (`undefined` and the deleted marker, `null`, a boolean, an int32 or a double): a heap value,
/// whose cell decides (a string, BigInt, symbol, object or function), or an internal marker leaves
/// at `miss`.
#[inline(always)]
pub(crate) fn immediate_typeof_index<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let tag = stage.and_imm(bits, ValueLayout::TAG_MASK);
    let integer = stage.eq_imm(tag, ValueLayout::INT_TAG);
    let field = stage.shr_imm(bits, ValueLayout::TAG_SHIFT);
    let offset = stage.add_imm(field, ValueLayout::FIRST_TAG_FIELD.wrapping_neg());
    let double = stage.ugt_imm(offset, ValueLayout::TAG_FIELD_SPAN);
    let number = stage.any(integer, double);
    let boolean = stage.eq_imm(tag, crate::Value::TRUE.to_bits() & ValueLayout::TAG_MASK);
    let null = stage.eq_imm(bits, crate::Value::NULL.to_bits());
    let unset = [crate::Value::UNDEFINED, crate::Value::DELETED].map(|value| stage.eq_imm(bits, value.to_bits()));
    let decided = [number, boolean, null, unset[0], unset[1]].into_iter().reduce(|any, each| stage.any(any, each)).expect("five tag classes");
    stage.guard(decided, miss)?;
    let [undefined, object, boolean_name, number_name] = [TYPEOF_UNDEFINED, TYPEOF_OBJECT, TYPEOF_BOOLEAN, TYPEOF_NUMBER].map(|index| stage.constant(index));
    let word = stage.select(boolean, boolean_name, undefined);
    let word = stage.select(null, object, word);
    Some(stage.select(number, number_name, word))
}

/// The `typeof` result string of the value `bits` where its tag alone decides it: the entry of `names`,
/// the address of the table of the eight result strings (`Vm::typeof_names`, interned when the heap is
/// made and indexed as `bytecode::TYPEOF_NAMES`), at the value's index; a heap value leaves at `miss`.
#[inline(always)]
pub(crate) fn immediate_typeof_string<S: Stage>(stage: &mut S, bits: S::Word, names: S::Word, miss: S::Miss) -> Option<S::Word> {
    let index = immediate_typeof_index(stage, bits, miss)?;
    let offset = stage.mul_imm(index, core::mem::size_of::<crate::Value>() as u64);
    let entry = stage.add(names, offset);
    Some(stage.load_table(Region::HeapMeta.readonly(), entry))
}

/// The interpreter's `immediate_typeof_index` of `value`.
#[inline(always)]
pub(crate) fn concrete_immediate_typeof_index(value: crate::Value) -> Option<usize> {
    let mut stage = crate::staged::Concrete::new(std::ptr::null());
    immediate_typeof_index(&mut stage, value.to_bits(), ()).map(|index| index as usize)
}

#[cfg(test)]
mod typeof_tests {
    use super::*;
    use crate::Value;
    use crate::bytecode::TYPEOF_NAMES;

    /// The tag classes answer the index of their `typeof` name; a heap value or an internal marker has none.
    #[test]
    fn the_tag_classes_name_their_typeof_index() {
        let named = |value: Value| concrete_immediate_typeof_index(value).map(|index| TYPEOF_NAMES[index]);
        assert_eq!(named(Value::UNDEFINED), Some("undefined"));
        assert_eq!(named(Value::DELETED), Some("undefined"));
        assert_eq!(named(Value::NULL), Some("object"));
        assert_eq!([named(Value::TRUE), named(Value::FALSE)], [Some("boolean"); 2]);
        assert_eq!([named(Value::integer(-3)), named(Value::number(2.5)), named(Value::number(f64::NAN))], [Some("number"); 3]);
        assert_eq!(TYPEOF_NAMES[TYPEOF_UNDEFINED as usize], "undefined");
        assert_eq!(TYPEOF_NAMES[TYPEOF_OBJECT as usize], "object");
        assert_eq!(TYPEOF_NAMES[TYPEOF_BOOLEAN as usize], "boolean");
        assert_eq!(TYPEOF_NAMES[TYPEOF_NUMBER as usize], "number");
    }
}
