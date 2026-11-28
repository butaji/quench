//! The dense array view a site proves once (`DenseView`), and the reads and replacements through it.

use super::*;

/// A dense array's backing as an access reads it, once proved: its first value and its length,
/// in one width.
#[derive(Clone, Copy)]
pub struct DenseView<W> {
    pub values: W,
    pub length: W,
    /// Whether a read needs no hole test of its own: the access consumes the element through a
    /// proof that rejects a hole's bits (a Number read, whose proof sees a NaN pattern and
    /// leaves). A fact of the proof, known before any run, so the read compiles without the test.
    pub packed: bool,
    /// Whether the access's index is proved below `length` (a range proof,
    /// `lane_region::int_ranges`), so only the hole test remains.
    pub within: bool,
    /// The backing's double flag (`heap::DOUBLE_LEN` of its length word), nonzero for a double
    /// backing, where the view's proof admitted one (`prove_view`): each access then takes the
    /// backing's form. `None` where the proof admitted a wide backing alone (a loop's hoisted
    /// view, a loop entry's).
    pub double: Option<W>,
    /// Whether the backing counts no others for this view (`OTHERS_UNKNOWN`, as a loop's hoisted
    /// writable view holds it: `emit_dense_array_uncounted`), so a store through it writes the
    /// element alone (`replace_uncounted_element`). A view proved by a store's own check keeps the
    /// count, `false`.
    pub uncounted: bool,
}

/// The view of the direct array `base` among the array records at `records`, writable where
/// `writable`, of a wide or a double backing (`DenseView::double`): `None` (leaving at `miss`)
/// for any other value or a narrow backing.
#[inline(always)]
pub fn prove_view<S: Stage>(stage: &mut S, records: S::Word, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DenseView<S::Word>> {
    let backing = if writable { writable_array_backing(stage, records, base, miss)? } else { flagged_backing(stage, records, base, miss, BackingAccess::MUTABLE)? };
    let (length, double) = wide_or_double(stage, backing.length, miss)?;
    Some(DenseView { values: backing.values, length, packed: false, within: false, double: Some(double), uncounted: false })
}

/// A backing's flagged length word (`ENCODED_LEN`) as a view reads it: the count, and the
/// double flag as a word (`DenseView::double`); `None` (leaving at `miss`) for a narrow
/// backing, whose decode only the interpreter's and the b-jit's reads make
/// (`any_backing_element`, `record_element`).
#[inline(always)]
pub(super) fn wide_or_double<S: Stage>(stage: &mut S, length: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    use crate::heap::{DOUBLE_LEN, ENCODED_LEN, NARROW_LEN};
    let narrow = stage.and_imm(length, u64::from(NARROW_LEN));
    let unnarrow = stage.eq_imm(narrow, 0u32);
    stage.guard(unnarrow, miss)?;
    let count = stage.and_imm(length, u64::from(!ENCODED_LEN));
    let double = stage.and_imm(length, u64::from(DOUBLE_LEN));
    Some((count, double))
}

/// The element `index` of `view`: `None` (leaving at `miss`) past the backing or, unless the view is
/// packed (a wide backing's proof), at a hole. A double backing's element is its Number's `Value`
/// (`double_element`).
#[inline(always)]
pub fn read_element<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let Some(flag) = view.double else { return wide_read(stage, view, index, miss) };
    let double = stage.ne_imm(flag, 0u32);
    stage.choose_leaving(double, #[inline(always)] |stage| {
        bounded(stage, view.within, view.length, index, miss)?;
        double_element(stage, view.values, index, miss)
    }, #[inline(always)] |stage| wide_read(stage, view, index, miss))
}

/// `read_element` of a view whose backing is wide.
#[inline(always)]
fn wide_read<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (_, value) = match (view.within, view.packed) {
        (false, false) => dense_element(stage, view.values, view.length, index, miss)?,
        (false, true) => packed_element(stage, view.values, view.length, index, miss)?,
        (true, false) => present_element(stage, view.values, index, miss)?,
        (true, true) => loaded_element(stage, view.values, index),
    };
    Some(value)
}

/// Replaces the present element `index` of `view` with `value`, which the caller proved needs
/// no barrier log; `None` (leaving at `miss`) past the backing or at a hole, where the store
/// would add an element. A double backing takes a Number that is no int32 in place
/// (`replace_double`); any other value leaves, for the VM's store to convert or make the
/// backing generic (`ArrayElements::replace_in_place`).
#[inline(always)]
pub fn replace_present<S: super::super::NumericStage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let Some(flag) = view.double else { return replace_wide(stage, view, index, value, miss) };
    let double = stage.ne_imm(flag, 0u32);
    stage.choose_leaving(double,
        #[inline(always)] |stage| replace_double(stage, view, index, value, miss).map(|()| value),
        #[inline(always)] |stage| replace_wide(stage, view, index, value, miss).map(|()| value))?;
    Some(())
}

/// `replace_present` of a view whose backing is wide.
#[inline(always)]
fn replace_wide<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let (address, old) = if view.within { present_element(stage, view.values, index, miss)? }
        else { dense_element(stage, view.values, view.length, index, miss)? };
    if view.uncounted { replace_uncounted_element(stage, address, value); }
    else { replace_element(stage, view.values, address, old, value); }
    Some(())
}

/// `replace_present` of a view whose backing is double (`heap::DOUBLE_LEN`): a Number that is
/// no int32 (`value::is_double`) is its double's bits, NaN canonical, which is the very word the
/// backing holds for it (`array_elements::double_word`), so the store writes the value's word; an
/// int32 writes its binary64, as the VM's store does (`ArrayElements::replace_encoded_in_place`),
/// so a whole Number (`i * 0.5` at an even `i`) stays in the compiled store. A present element is
/// replaced, so the backing's count (of holes) stays. Any other value leaves at `miss`.
#[inline(always)]
fn replace_double<S: super::super::NumericStage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let (double, int) = super::super::value::number_classes(stage, value);
    let number = stage.either(double, int);
    stage.guard(number, miss)?;
    bounded(stage, view.within, view.length, index, miss)?;
    let (address, old) = loaded_element(stage, view.values, index);
    let present = stage.ne_imm(old, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    stage.effect_if(int, #[inline(always)] |stage| {
        let integer = stage.narrow(value);
        let number = stage.i32_to_f64(integer);
        stage.store_f64(Region::Elements.mutable(), address, 0, number);
    }, #[inline(always)] |stage| stage.store_u64(Region::Elements.mutable(), address, 0, value));
    Some(())
}
