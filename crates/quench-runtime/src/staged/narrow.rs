//! A value's narrow form as staged words: the packed half a plain object keeps in its payload
//! word, or `NOT_NARROW` where the value has none. The interpreter runs it on `Concrete`; a
//! specializer emits it from the same rows.

use super::Stage;
use crate::typed_ir::Type;
use crate::value::{
    HEAP_FAMILY_BITS, HEAP_FAMILY_SHIFT, LOWER_WORD, NARROW_HEAP_FAMILIES, NarrowLayout, TAG_BOOL, TAG_DELETED, TAG_NULL, TAG_UNDEFINED, ValueLayout,
};

/// The upper words of the narrow constants, their codes following the families' codes.
const CONSTANT_UPPER_WORDS: [u64; 4] = [TAG_UNDEFINED, TAG_NULL, TAG_BOOL, TAG_DELETED];
const FIRST_CONSTANT_CODE: u64 = NARROW_HEAP_FAMILIES as u64;
/// A bit mask's complement in the low word: the upper word of a value.
pub(super) const UPPER_WORD: u64 = !(u32::MAX as u64);
/// The word a value that has no narrow form (a Number, a wide reference) yields.
pub(super) const NOT_NARROW: u64 = u64::MAX;

/// Whether the int32 `value`'s biased low word is below the narrow integer span: it has a narrow form.
#[inline(always)]
pub fn integer_fits<S: Stage>(stage: &mut S, value: S::Word) -> S::Cond {
    let lower = stage.and_imm(value, LOWER_WORD);
    let span = 1_u64 << NarrowLayout::INTEGER_BITS;
    let biased = stage.add_imm(lower, span >> 1);
    let biased = stage.and_imm(biased, LOWER_WORD);
    let over = stage.ugt_imm(biased, span - 1);
    stage.not(over)
}

/// The narrow word of the int32 `value` that `integer_fits` holds for: its code under the integer flag.
#[inline(always)]
pub fn integer_code<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let lower = stage.and_imm(value, LOWER_WORD);
    let shifted = stage.mul_imm(lower, 1_u64 << NarrowLayout::CODE_SHIFT);
    let shifted = stage.and_imm(shifted, LOWER_WORD);
    stage.or_imm(shifted, u64::from(NarrowLayout::INTEGER_FLAG))
}

/// `narrow_form` of an int32 `value`: its code where `integer_fits`, else `NOT_NARROW`.
#[inline(always)]
fn integer_form<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let fits = integer_fits(stage, value);
    let integer = integer_code(stage, value);
    let refused = stage.constant(NOT_NARROW);
    stage.select(fits, integer, refused)
}

/// The narrow form of `value` (`Value::to_narrow`) where it has one: an integer in range, a
/// reference whose index fits, or a constant; `NOT_NARROW` for any other value, which the
/// store leaves to the general path. A `fact` that proves an int32 selects the integer arm
/// alone: the tag test and the reference arm are not emitted. Each form computes only its own arm.
#[inline(always)]
pub(crate) fn narrow_form<S: Stage>(stage: &mut S, value: S::Word, fact: Type) -> S::Word {
    if fact.is_subtype(Type::INT32) { return integer_form(stage, value); }
    let upper = stage.and_imm(value, UPPER_WORD);
    let is_int = stage.eq_imm(upper, ValueLayout::INT_TAG);
    // A stencil has no stack to call a shared arm out of, and both forms are pure: it computes both and
    // selects. Any other stage runs only the arm taken.
    #[cfg(stencil_build)]
    {
        let integer = integer_form(stage, value);
        let reference = reference_form(stage, value, upper);
        stage.select(is_int, integer, reference)
    }
    #[cfg(not(stencil_build))]
    stage.choose(is_int, #[inline(always)] |stage| integer_form(stage, value), #[inline(always)] |stage| reference_form(stage, value, upper))
}

/// `narrow` where it is a narrow form; `None` (leaving at `miss`) where it is `NOT_NARROW`.
#[inline(always)]
pub(crate) fn storable_form<S: Stage>(stage: &mut S, narrow: S::Word, miss: S::Miss) -> Option<S::Word> {
    let storable = stage.ne_imm(narrow, NOT_NARROW);
    stage.guard(storable, miss)?;
    Some(narrow)
}

/// The word a store of `value` writes into a slot of a record whose values are packed
/// (`inline`): its narrow form, or `None` (leaving at `miss`) where it has none. A flat slot takes
/// the value whole, and its word here is zero.
#[inline(always)]
pub(crate) fn packed_form<S: Stage>(stage: &mut S, inline: S::Cond, value: S::Word, fact: Type, miss: S::Miss) -> Option<S::Word> {
    // A stencil has no stack to call a shared arm out of, and the narrow form is pure: it computes the form
    // and selects. Any other stage runs only the arm taken.
    #[cfg(stencil_build)]
    let narrow = {
        let packed = narrow_form(stage, value, fact);
        let flat = stage.constant(0);
        stage.select(inline, packed, flat)
    };
    #[cfg(not(stencil_build))]
    let narrow = stage.choose(inline, #[inline(always)] |stage| narrow_form(stage, value, fact), #[inline(always)] |stage| stage.constant(0));
    storable_form(stage, narrow, miss)
}

/// `narrow_form` of a value that is no integer: its code is its family bits, a constant's
/// follows them, and the lower word is the index.
#[inline(always)]
fn reference_form<S: Stage>(stage: &mut S, value: S::Word, upper: S::Word) -> S::Word {
    let lower = stage.and_imm(value, LOWER_WORD);
    let family = stage.and_imm(upper, HEAP_FAMILY_BITS);
    let base = stage.and_imm(upper, !HEAP_FAMILY_BITS);
    let mut named = stage.eq_imm(base, ValueLayout::HEAP_TAG);
    let mut code = stage.shr_imm(family, HEAP_FAMILY_SHIFT);
    for (offset, word) in CONSTANT_UPPER_WORDS.into_iter().enumerate() {
        let is_constant = stage.eq_imm(upper, word);
        let constant_code = stage.constant(FIRST_CONSTANT_CODE + offset as u64);
        code = stage.select(is_constant, constant_code, code);
        named = stage.any(named, is_constant);
    }
    let index_high = stage.shr_imm(lower, NarrowLayout::INDEX_BITS);
    let index_fits = stage.eq_imm(index_high, 0u32);
    let fits = stage.pick(named, index_fits, named);
    let index = stage.mul_imm(lower, 1_u64 << NarrowLayout::INDEX_SHIFT);
    let coded = stage.mul_imm(code, 1_u64 << NarrowLayout::CODE_SHIFT);
    let reference = stage.or(index, coded);
    let refused = stage.constant(NOT_NARROW);
    stage.select(fits, reference, refused)
}
