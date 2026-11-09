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
