//! Dense array element reads: the element at an index, its loaded value and its bounds.

use super::*;

/// Element `index` of the dense backing of `length` values at `values`, with its address:
/// `None` (leaving at `miss`) past the backing or at a hole (`Value::DELETED`), which the
/// caller resolves the long way. `index` and `length` share one width; the address widens
/// the index first, as an in-bounds int32 index's byte offset can pass `u32::MAX`.
#[inline(always)]
pub fn dense_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    present_element(stage, values, index, miss)
}

/// `dense_element` where the hole's bits are a value like another, which the caller's proof
/// rejects (`DenseView::packed`): only the backing's end leaves at `miss`.
#[inline(always)]
pub fn packed_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    Some(loaded_element(stage, values, index))
}

/// `dense_element` of an `index` the caller proved below the backing's length (a range
/// proof, `lane_region::int_ranges`): only a hole leaves at `miss`.
#[inline(always)]
pub fn present_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let (address, value) = loaded_element(stage, values, index);
    let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    Some((address, value))
}

/// Proves the dense backing at `values` counts no others (`OTHERS_FROM_VALUES` zero): every value
/// below its length is an int32 and none is a hole, so each element's word is an int32's and needs
/// no tag test (`int_element`). Leaves at `miss` otherwise, as a count of others does.
#[inline(always)]
pub fn int_only_backing<S: Stage>(stage: &mut S, values: S::Word, miss: S::Miss) -> Option<()> {
    let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
    let none = stage.eq_imm(others, 0u32);
    stage.guard(none, miss)
}

/// Element `index` of an int-only backing (`int_only_backing`) of `length` values at `values`, as
/// its word: no hole test, since none is present. An `index` the caller did not prove below `length`
/// (`within` false) leaves at `miss` past it.
#[inline(always)]
pub fn int_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    within: bool, miss: S::Miss) -> Option<S::Word> {
    bounded(stage, within, length, index, miss)?;
    Some(loaded_element(stage, values, index).1)
}

/// The address and bits of element `index` of the backing at `values`.
#[inline(always)]
pub(super) fn loaded_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word) -> (S::Word, S::Word) {
    let offset = stage.uextend32(index);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    (address, stage.load_u64(Region::Elements.mutable(), address, 0))
}

/// Element `index`, which the caller proved below the length, of the double backing
/// (`heap::DOUBLE_LEN`) at `values`: the `Value` of the Number its word holds, as a raw double word
/// (`Stage::box_raw_number`, decision 4.1: 3.0 stays a double); `None` (leaving at `miss`)
/// at a hole, `Value::DELETED`'s word, which no box may canonicalize away. A double word is as
/// wide as a `Value`, so the element's address is a wide element's.
#[inline(always)]
pub(super) fn double_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (address, word) = loaded_element(stage, values, index);
    let present = stage.ne_imm(word, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    let number = stage.load_f64(Region::Elements.mutable(), address, 0);
    Some(stage.box_raw_number(super::super::Float64::of_row(number)))
}

/// The guard of an `index` against `length` a view did not prove below it (`DenseView::within`).
#[inline(always)]
pub(super) fn bounded<S: Stage>(stage: &mut S, within: bool, length: S::Word, index: S::Word, miss: S::Miss) -> Option<()> {
    if within { return Some(()); }
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)
}
