//! A dense array backing's element read as a staged operation: the interpreter reads it on
//! `Concrete`, a specializer emits it.

use super::{Concrete, Stage};

/// Element `index` of the dense backing of `length` values at `values`, with its address:
/// `None` (leaving at `miss`) past the backing or at a hole (`Value::DELETED`), which the
/// caller resolves the long way. `index` and `length` share one width; the address widens
/// the index first, as an in-bounds int32 index's byte offset can pass `u32::MAX`.
#[inline(always)]
pub(crate) fn dense_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    let offset = stage.uextend32(index);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    let value = stage.load_u64(address, 0);
    let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    Some((address, value))
}

/// A dense array's backing as an access reaches it from the array value: the record, the
/// backing's header, its first value and its length.
#[derive(Clone, Copy)]
pub(crate) struct Backing<W> {
    pub(crate) record: W,
    pub(crate) header: W,
    pub(crate) values: W,
    pub(crate) length: W,
}

/// The backing of the direct array `base` among the array records at `record_base`: `None`
/// (leaving at `miss`) for any other value, or before the first array record exists
/// (`record_base` zero). The record's address is its index, `base`'s low half, times the
/// record size past `record_base`; the backing's header is one load, its length another.
#[inline(always)]
pub(crate) fn array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let is_array = super::value::is_direct_array(stage, base);
    stage.guard(is_array, miss)?;
    let index = stage.and_imm(base, u64::from(u32::MAX));
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let offset = stage.mul_imm(index, ARRAY.record_size as u64);
    let record = stage.add(record_base, offset);
    let header = stage.load_u64(record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let length = stage.load_u32(header, ARRAY.header_length_offset as i32);
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Some(Backing { record, header, values, length })
}

/// Replaces the present element at `address` of the dense backing at `values`, which held
/// `old`, with `value`: the backing's count of values that are no int32 (its others) follows
/// the write, and a count native code made unknown (`OTHERS_UNKNOWN`) stays unknown. The
/// count subtracts first, so an exact count never overflows on the way.
#[inline(always)]
pub(crate) fn replace_element<S: Stage>(stage: &mut S, values: S::Word, address: S::Word, old: S::Word, value: S::Word) {
    let others = stage.load_u32(values, crate::heap::OTHERS_FROM_VALUES);
    let (added, removed) = (other(stage, value), other(stage, old));
    let others = recount(stage, others, added, removed);
    stage.store_u32(values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(address, 0, value);
}

/// `replace_element` on a backing whose count is `OTHERS_UNKNOWN`: the count stays unknown
/// (`recount`), so the write is the element's alone.
#[inline(always)]
pub(crate) fn replace_uncounted_element<S: Stage>(stage: &mut S, address: S::Word, value: S::Word) {
    stage.store_u64(address, 0, value);
}

/// Appends `value` at `index` of the dense backing at `values`, which holds `length` values
/// and room for `capacity`, as its owner proved: `None` (leaving at `miss`) unless `index` is
/// the length and the room has a slot past it. The new element counts among the backing's
/// others as `replace_element` counts one, and the backing's length advances; the new
/// length is the answer. `index`, `length` and `capacity` share one width.
#[inline(always)]
pub(crate) fn append_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, capacity: S::Word,
    index: S::Word, value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let past = stage.sub(index, length);
    let at_end = stage.eq_imm(past, 0u32);
    stage.guard(at_end, miss)?;
    let room = stage.ult(length, capacity);
    stage.guard(room, miss)?;
    let offset = stage.uextend32(length);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    let others = stage.load_u32(values, crate::heap::OTHERS_FROM_VALUES);
    let (added, none) = (other(stage, value), stage.constant(0));
    let others = recount(stage, others, added, none);
    stage.store_u32(values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(address, 0, value);
    let grown = stage.add_imm(length, 1u32);
    let word = stage.uextend32(grown);
    stage.store_u32(values, crate::heap::LENGTH_FROM_VALUES, word);
    Some(grown)
}

/// A backing's count of other values once a write added `added` and overwrote `removed`;
/// `OTHERS_UNKNOWN` stays unknown. Subtracting first keeps an exact count from overflowing.
#[inline(always)]
fn recount<S: Stage>(stage: &mut S, others: S::Word, added: S::Word, removed: S::Word) -> S::Word {
    let removed_first = stage.sub(others, removed);
    let recounted = stage.add(removed_first, added);
    let unknown = stage.eq_imm(others, u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.select(unknown, others, recounted)
}

/// One where `value` counts among a backing's others (it is no int32), else zero.
#[inline(always)]
fn other<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
    let tag = stage.and_imm(value, crate::value::ValueLayout::TAG_MASK);
    let int = stage.eq_imm(tag, crate::value::ValueLayout::INT_TAG);
    let (zero, one) = (stage.constant(0), stage.constant(1));
    stage.select(int, zero, one)
}

/// The interpreter's reading of `replace_element` at `index` of the backing whose first value
/// `values` addresses.
///
/// # Safety
/// The backing is its array's own, alive, and holds `index`.
#[inline(always)]
pub(crate) unsafe fn concrete_replace_element(values: *mut crate::Value, index: usize, value: crate::Value) {
    let mut stage = Concrete::new(std::ptr::null());
    // SAFETY: the caller's contract.
    let address = unsafe { values.add(index) };
    let old = unsafe { address.read() };
    replace_element(&mut stage, values as u64, address as u64, old.to_bits(), value.to_bits());
}

/// The interpreter's reading of `other`.
#[inline(always)]
pub(crate) fn concrete_other(value: crate::Value) -> u32 {
    other(&mut Concrete::new(std::ptr::null()), value.to_bits()) as u32
}

/// The interpreter's reading of `recount`.
#[inline(always)]
pub(crate) fn concrete_recount(others: u32, added: u32, removed: u32) -> u32 {
    recount(&mut Concrete::new(std::ptr::null()), u64::from(others), u64::from(added), u64::from(removed)) as u32
}

/// The interpreter's reading of `dense_element` over `elements`.
#[cfg(test)]
#[inline(always)]
pub(crate) fn concrete_dense_element(elements: &[crate::Value], index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    dense_element(&mut stage, elements.as_ptr() as u64, elements.len() as u64, index as u64, ())
        .map(|(_, value)| crate::Value::from_bits(value))
}

/// The interpreter's reading of `array_backing` then `dense_element`: element `index` of the
/// direct array `base` among the array records at `record_base`.
#[inline(always)]
pub(crate) fn concrete_record_element(record_base: *const crate::heap::ArrayObject, base: crate::Value, index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    let backing = array_backing(&mut stage, record_base as u64, base.to_bits(), ())?;
    dense_element(&mut stage, backing.values, backing.length, index as u64, ())
        .map(|(_, value)| crate::Value::from_bits(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn a_dense_element_reads_present_values_and_leaves_past_the_backing_or_at_a_hole() {
        let elements = [Value::number(1.5), Value::DELETED, Value::UNDEFINED];
        let read = |index| concrete_dense_element(&elements, index).map(Value::to_bits);
        assert_eq!(read(0), Some(Value::number(1.5).to_bits()));
        assert_eq!(read(1), None, "a hole leaves");
        assert_eq!(read(2), Some(Value::UNDEFINED.to_bits()));
        assert_eq!(read(3), None, "past the backing leaves");
    }

    /// `append_element` on `Concrete` over `elements`' own backing, with its room.
    fn append(elements: &mut crate::heap::UniqueElements, index: usize, value: Value) -> Option<u64> {
        let (length, capacity) = (elements.len() as u64, elements.capacity() as u64);
        let values = elements.as_mut_ptr() as u64;
        append_element(&mut Concrete::new(std::ptr::null()), values, length, capacity, index as u64, value.to_bits(), ())
    }

    #[test]
    fn an_append_writes_the_slot_past_the_length_within_room_and_counts_it() {
        let mut backing = crate::heap::ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5)]);
        let elements = backing.make_mut();
        elements.reserve(2);
        let others = |elements: &mut crate::heap::UniqueElements| {
            Concrete::new(std::ptr::null()).load_u32(elements.as_mut_ptr() as u64, crate::heap::OTHERS_FROM_VALUES)
        };
        assert_eq!(others(elements), 1);
        assert_eq!(append(elements, 3, Value::integer(7)), None, "past the length leaves");
        assert_eq!(append(elements, 1, Value::integer(7)), None, "below the length leaves");
        assert_eq!(append(elements, 2, Value::integer(7)), Some(3));
        assert_eq!(append(elements, 3, Value::number(2.5)), Some(4));
        assert_eq!(elements.len(), 4);
        assert_eq!(others(elements), 2, "the double counts, the int32 does not");
        let read = |index| concrete_dense_element(elements, index).map(Value::to_bits);
        assert_eq!(read(2), Some(Value::integer(7).to_bits()));
        assert_eq!(read(3), Some(Value::number(2.5).to_bits()));
        while elements.len() < elements.capacity() { elements.push(Value::integer(0)); }
        let full = elements.len();
        assert_eq!(append(elements, full, Value::integer(7)), None, "a full backing leaves");
        crate::staged::Stage::store_u32(&mut Concrete::new(std::ptr::null()), elements.as_mut_ptr() as u64,
            crate::heap::OTHERS_FROM_VALUES, u64::from(crate::heap::OTHERS_UNKNOWN));
        elements.reserve(1);
        assert_eq!(append(elements, full, Value::number(0.25)), Some(full as u64 + 1));
        assert_eq!(others(elements), u64::from(crate::heap::OTHERS_UNKNOWN), "an unknown count stays unknown");
    }
}
