//! Element writes: replacing and appending elements, filling holes, and recounting a backing.

use super::*;

/// Replaces the present element at `address` of the dense backing at `values`, which held
/// `old`, with `value`: the backing's count of values that are no int32 (its others) follows
/// the write, and a count native code made unknown (`OTHERS_UNKNOWN`) stays unknown. The
/// count subtracts first, so an exact count never overflows on the way.
#[inline(always)]
pub fn replace_element<S: Stage>(stage: &mut S, values: S::Word, address: S::Word, old: S::Word, value: S::Word) {
    let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
    let (added, removed) = (other(stage, value), other(stage, old));
    let others = recount(stage, others, added, removed);
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
}

/// Marks the backing at `values` as counting no others (`OTHERS_UNKNOWN`), so stores through a
/// view of it need not keep the count (`replace_uncounted_element`).
#[inline(always)]
pub fn uncount_backing<S: Stage>(stage: &mut S, values: S::Word) {
    let unknown = stage.constant(u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, unknown);
}

/// `replace_element` on a backing whose count is `OTHERS_UNKNOWN`: the count stays unknown
/// (`recount`), so the write is the element's alone.
#[inline(always)]
pub fn replace_uncounted_element<S: Stage>(stage: &mut S, address: S::Word, value: S::Word) {
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
}

/// Appends `value` at `index` of the dense backing at `values`, which holds `length` values
/// and room for `capacity`, as its owner proved: `None` (leaving at `miss`) unless `index` is
/// the length and the room has a slot past it. The new element counts among the backing's
/// others as `replace_element` counts one, and the backing's length advances; the new
/// length is the answer. `index`, `length` and `capacity` share one width.
#[inline(always)]
pub fn append_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, capacity: S::Word,
    index: S::Word, value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let past = stage.sub(index, length);
    let at_end = stage.eq_imm(past, 0u32);
    stage.guard(at_end, miss)?;
    let room = stage.ult(length, capacity);
    stage.guard(room, miss)?;
    let offset = stage.uextend32(length);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
    let (added, none) = (other(stage, value), stage.constant(0));
    let others = recount(stage, others, added, none);
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
    let grown = stage.add_imm(length, 1u32);
    let word = stage.uextend32(grown);
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::LENGTH_FROM_VALUES, word);
    Some(grown)
}

/// Fills the hole at the int32 `key` (a value word) of the direct array `base` among the array
/// records at `record_base` with `value`, whose store the caller proved passes the write barrier
/// unlogged: OrdinarySet then defines a plain data element in the hole, one write in place, where
/// the backing is wide or double, the array's own, extensible and plain (`resizable_backing`) and its
/// prototype's link is `chain`, the held index-free chain proof's (`heap::index_free_chain_link_address`,
/// zero while none is held). The hole counted among a wide backing's others, so the count is
/// `replace_element`'s; a double backing's hole takes a Number as `fill_double_hole` writes it. `None` (leaving at `miss`, before any write) for any other store.
#[inline(always)]
pub fn fill_hole<S: super::super::NumericStage>(stage: &mut S, record_base: S::Word, chain: S::Word, (base, key, value): (S::Word, S::Word, S::Word),
    miss: S::Miss) -> Option<()> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let index = stage.add_imm(key, crate::value::ValueLayout::INT_TAG.wrapping_neg());
    let over = stage.ugt_imm(index, u64::from(i32::MAX as u32));
    let within_int = stage.not(over);
    stage.guard(within_int, miss)?;
    // A wide or a double backing (`new Array(n)`'s holes start double); a narrow one leaves.
    let flagged = flagged_backing(stage, record_base, base, miss, BackingAccess::MUTABLE)?;
    let (length, double) = wide_or_double(stage, flagged.length, miss)?;
    let backing = Backing { length, ..flagged };
    resizable_backing(stage, backing, miss)?;
    let held = stage.ne_imm(chain, 0u32);
    stage.guard(held, miss)?;
    let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
    let link = super::super::object::proto_link(stage, object);
    let linked = stage.eq(link, chain);
    stage.guard(linked, miss)?;
    let in_bounds = stage.ult(index, backing.length);
    stage.guard(in_bounds, miss)?;
    let (address, old) = loaded_element(stage, backing.values, index);
    let hole = stage.eq_imm(old, crate::Value::DELETED.to_bits());
    stage.guard(hole, miss)?;
    let is_double = stage.ne_imm(double, 0u32);
    stage.choose_leaving(is_double,
        #[inline(always)] |stage| fill_double_hole(stage, backing, address, value, miss).map(|()| value),
        #[inline(always)] |stage| { replace_element(stage, backing.values, address, old, value); Some(value) })?;
    Some(())
}

/// `fill_hole`'s write into the hole at `address` of a double backing (`heap::DOUBLE_LEN`), as the
/// VM's store makes it (`ArrayElements::replace_encoded_in_place`): a Number's binary64 (an int32
/// converted, any other Number its own word), the backing counting one hole fewer (a double
/// backing's count is of its holes). Holes alone taking an int32 leave, for the VM to make them
/// wide holes; so do any other value and a backing whose count is unknown.
#[inline(always)]
fn fill_double_hole<S: super::super::NumericStage>(stage: &mut S, backing: Backing<S::Word>, address: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let (double, int) = super::super::value::number_classes(stage, value);
    let number = stage.either(double, int);
    let holes = stage.load_u32(Region::Elements.mutable(), backing.values, crate::heap::OTHERS_FROM_VALUES);
    let counted = stage.ne_imm(holes, u64::from(crate::heap::OTHERS_UNKNOWN));
    let admitted = stage.both(number, counted);
    stage.guard(admitted, miss)?;
    let all_holes = stage.eq(holes, backing.length);
    let widens = stage.both(int, all_holes);
    stage.guard_false(widens, miss)?;
    let fewer = stage.add_imm(holes, u64::from(u32::MAX));
    let fewer = stage.and_imm(fewer, u64::from(u32::MAX));
    stage.store_u32(Region::Elements.mutable(), backing.values, crate::heap::OTHERS_FROM_VALUES, fewer);
    stage.effect_if(int, #[inline(always)] |stage| {
        let integer = stage.narrow(value);
        let number = stage.i32_to_f64(integer);
        stage.store_f64(Region::Elements.mutable(), address, 0, number);
    }, #[inline(always)] |stage| stage.store_u64(Region::Elements.mutable(), address, 0, value));
    Some(())
}

/// Appends `value`, a Number word whose store passes the write barrier unlogged, at the length
/// of the writable `backing` (`writable_backing`) while its capacity word proves a slot past the
/// length; `None` (leaving at `miss`, before any write) for a full backing, which the VM grows.
/// The capacity word changes only where the backing is allocated or grown, so room is a layout
/// fact the row reads, not a VM reserve.
#[inline(always)]
pub fn append_in_room<S: Stage>(stage: &mut S, backing: Backing<S::Word>, value: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let word = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_capacity_offset as i32);
    let capacity = stage.and_imm(word, u64::from(ARRAY.capacity_mask));
    append_element(stage, backing.values, backing.length, capacity, backing.length, value, miss)
}

/// A backing's count of other values once a write added `added` and overwrote `removed`;
/// `OTHERS_UNKNOWN` stays unknown. Subtracting first keeps an exact count from overflowing.
#[inline(always)]
pub fn recount<S: Stage>(stage: &mut S, others: S::Word, added: S::Word, removed: S::Word) -> S::Word {
    let removed_first = stage.sub(others, removed);
    let recounted = stage.add(removed_first, added);
    let unknown = stage.eq_imm(others, u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.select(unknown, others, recounted)
}

/// One where `value` counts among a backing's others (it is no int32), else zero.
#[inline(always)]
pub fn other<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let int = super::super::value::is_int(stage, value);
    let (zero, one) = (stage.constant(0), stage.constant(1));
    stage.select(int, zero, one)
}
