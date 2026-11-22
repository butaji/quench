//! Raw-double views of double-backed arrays (`heap::DOUBLE_LEN`): what a loop proves once at its
//! entering edge and reads and writes as a `Float64Array`'s elements (`lane_region::ElementViews::Double`).

use super::Stage;
use super::elements::{Backing, prove_view, record_backing};
use super::region::Region;

/// The raw doubles of the direct array `base` among the array records at `records`, whose backing
/// is double (`heap::DOUBLE_LEN`) and counts no hole, writable where `writable`: their first word
/// and count. `None` (leaving at `miss`) for any other value or backing.
#[inline(always)]
pub fn double_view<S: Stage>(stage: &mut S, records: S::Word, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let view = prove_view(stage, records, base, writable, miss)?;
    let flag = view.double?;
    let double = stage.ne_imm(flag, 0u32);
    stage.guard(double, miss)?;
    // A double backing's count of others is of its holes (`heap::DOUBLE_LEN`).
    let holes = stage.load_u32(Region::Elements.mutable(), view.values, crate::heap::OTHERS_FROM_VALUES);
    let packed = stage.eq_imm(holes, 0u32);
    stage.guard(packed, miss)?;
    Some((view.values, view.length))
}

/// A `double_view` of the array record `record` read again after a helper that may have changed
/// its backing: its first word, and its count while the backing is still double, else zero (its
/// words are `Value`s again, `ArrayElements::make_mut`), so every access through the view leaves
/// at its bounds test. A double backing gains no hole in place: its writers write Numbers alone.
#[inline(always)]
pub fn reloaded_double_view<S: Stage>(stage: &mut S, record: S::Word) -> (S::Word, S::Word) {
    use crate::heap::{DOUBLE_LEN, ENCODED_LEN};
    let Backing { values, length, .. } = record_backing(stage, record);
    let flag = stage.and_imm(length, u64::from(DOUBLE_LEN));
    let double = stage.ne_imm(flag, 0u32);
    let count = stage.and_imm(length, u64::from(!ENCODED_LEN));
    let empty = stage.constant(0);
    (values, stage.select(double, count, empty))
}
