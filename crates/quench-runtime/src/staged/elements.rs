//! A dense array backing's element read as a staged operation: the interpreter reads it on
//! `Concrete`, a specializer emits it.

use super::{Concrete, Stage};
use super::region::Region;

/// Element `index` of the dense backing of `length` values at `values`, with its address:
/// `None` (leaving at `miss`) past the backing or at a hole (`Value::DELETED`), which the
/// caller resolves the long way. `index` and `length` share one width; the address widens
/// the index first, as an in-bounds int32 index's byte offset can pass `u32::MAX`.
#[inline(always)]
pub(crate) fn dense_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    present_element(stage, values, index, miss)
}

/// `dense_element` where the hole's bits are a value like another, which the caller's proof
/// rejects (`DenseView::packed`): only the backing's end leaves at `miss`.
#[inline(always)]
pub(crate) fn packed_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    Some(loaded_element(stage, values, index))
}

/// `dense_element` of an `index` the caller proved below the backing's length (a range
/// proof, `lane_region::int_ranges`): only a hole leaves at `miss`.
#[inline(always)]
pub(crate) fn present_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word,
    miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let (address, value) = loaded_element(stage, values, index);
    let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    Some((address, value))
}

/// The address and bits of element `index` of the backing at `values`.
#[inline(always)]
fn loaded_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word) -> (S::Word, S::Word) {
    let offset = stage.uextend32(index);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    (address, stage.load_u64(Region::Elements.mutable(), address, 0))
}

/// A dense array's backing as an access reads it, once proved: its first value and its length,
/// in one width.
#[derive(Clone, Copy)]
pub(crate) struct DenseView<W> {
    pub(crate) values: W,
    pub(crate) length: W,
    /// Whether a read needs no hole test of its own: the access consumes the element through a
    /// proof that rejects a hole's bits (a Number read, whose proof sees a NaN pattern and
    /// leaves). A fact of the proof, known before any run, so the read compiles without the test.
    pub(crate) packed: bool,
    /// Whether the access's index is proved below `length` (a range proof,
    /// `lane_region::int_ranges`), so only the hole test remains.
    pub(crate) within: bool,
}

/// The view of the direct array `base` among the array records at `records`, writable where
/// `writable`: `None` (leaving at `miss`) for any other value.
#[inline(always)]
pub(crate) fn prove_view<S: Stage>(stage: &mut S, records: S::Word, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DenseView<S::Word>> {
    let backing = array_backing(stage, records, base, miss)?;
    if writable { writable_backing(stage, backing, miss)?; }
    Some(DenseView { values: backing.values, length: backing.length, packed: false, within: false })
}

/// The element `index` of `view`: `None` (leaving at `miss`) past the backing or, unless the view is
/// packed, at a hole.
#[inline(always)]
pub(crate) fn read_element<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
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
/// would add an element.
#[inline(always)]
pub(crate) fn replace_present<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let (address, old) = if view.within { present_element(stage, view.values, index, miss)? }
        else { dense_element(stage, view.values, view.length, index, miss)? };
    replace_element(stage, view.values, address, old, value);
    Some(())
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
/// (`record_base` zero).
#[inline(always)]
pub(crate) fn array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    let is_array = super::value::is_direct_array(stage, base);
    stage.guard(is_array, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let record = array_record(stage, record_base, base);
    Some(record_backing(stage, record))
}

/// The record of the direct array `base`, which the caller proved one, among the array records at
/// `record_base`, which it proved present: the record's address is its index, `base`'s low half,
/// times the record size past `record_base`.
#[inline(always)]
pub(crate) fn array_record<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word) -> S::Word {
    let index = stage.and_imm(base, u64::from(u32::MAX));
    let offset = stage.mul_imm(index, crate::heap::NATIVE_ARRAY_LAYOUT.record_size as u64);
    stage.add(record_base, offset)
}

/// The backing of the array record `record`: its header is one load, its length another.
#[inline(always)]
pub(crate) fn record_backing<S: Stage>(stage: &mut S, record: S::Word) -> Backing<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let header = stage.load_u64(Region::Records.mutable(), record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Backing { record, header, values, length }
}

/// `backing` as an in-place replacement needs it: its elements owned by this array alone, the
/// array not frozen and without extras (indexed descriptors, an arguments map, sparse
/// elements or a prototype watch), as `Vm::try_replace_record_element` requires. `None`
/// leaves at `miss`.
#[inline(always)]
pub(crate) fn writable_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, miss: S::Miss) -> Option<()> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    unbarred_backing(stage, backing, ARRAY.frozen_mask, miss)
}

/// `writable_backing` of an array that is also extensible, which a change of its length
/// (`push`, `pop`) needs: `Object::holds_ordinary_elements`' integrity bits are all clear.
#[inline(always)]
pub(crate) fn resizable_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, miss: S::Miss) -> Option<()> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    unbarred_backing(stage, backing, ARRAY.integrity_mask, miss)
}

/// `backing` unshared, plain, and with none of the meta bits `barred`.
#[inline(always)]
fn unbarred_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, barred: u32, miss: S::Miss) -> Option<()> {
    use crate::heap::{NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};
    let owners = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_owners_offset as i32);
    let unique = stage.eq_imm(owners, 1u32);
    stage.guard(unique, miss)?;
    let meta = stage.load_u32(Region::Records.mutable(), backing.record, (ARRAY.object_offset + OBJECT.meta_offset) as i32);
    let held = stage.and_imm(meta, u64::from(barred));
    let clear = stage.eq_imm(held, 0u32);
    stage.guard(clear, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), backing.record, (ARRAY.object_offset + OBJECT.payload_offset) as i32);
    let plain = super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)
}

/// The `length` of the direct array `base` as the int32 word it reads as: its dense backing's
/// count, which an array without extras (an arguments object, sparse elements) has as its
/// length. `None` (leaving at `miss`) for any other value, an array with extras, or a length
/// past int32.
#[inline(always)]
pub(crate) fn array_length<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<S::Word> {
    use crate::heap::{NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};
    let backing = array_backing(stage, record_base, base, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), backing.record, (ARRAY.object_offset + OBJECT.payload_offset) as i32);
    let plain = super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    super::value::length_value(stage, backing.length, miss)
}

/// Replaces the present element at `address` of the dense backing at `values`, which held
/// `old`, with `value`: the backing's count of values that are no int32 (its others) follows
/// the write, and a count native code made unknown (`OTHERS_UNKNOWN`) stays unknown. The
/// count subtracts first, so an exact count never overflows on the way.
#[inline(always)]
pub(crate) fn replace_element<S: Stage>(stage: &mut S, values: S::Word, address: S::Word, old: S::Word, value: S::Word) {
    let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
    let (added, removed) = (other(stage, value), other(stage, old));
    let others = recount(stage, others, added, removed);
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, others);
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
}

/// Marks the backing at `values` as counting no others (`OTHERS_UNKNOWN`), so stores through a
/// view of it need not keep the count (`replace_uncounted_element`).
#[inline(always)]
pub(crate) fn uncount_backing<S: Stage>(stage: &mut S, values: S::Word) {
    let unknown = stage.constant(u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.store_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES, unknown);
}

/// `replace_element` on a backing whose count is `OTHERS_UNKNOWN`: the count stays unknown
/// (`recount`), so the write is the element's alone.
#[inline(always)]
pub(crate) fn replace_uncounted_element<S: Stage>(stage: &mut S, address: S::Word, value: S::Word) {
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
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

/// Appends `value`, a Number word whose store passes the write barrier unlogged, at the length
/// of the writable `backing` (`writable_backing`) while its capacity word proves a slot past the
/// length; `None` (leaving at `miss`, before any write) for a full backing, which the VM grows.
/// The capacity word changes only where the backing is allocated or grown, so room is a layout
/// fact the row reads, not a VM reserve.
#[inline(always)]
pub(crate) fn append_in_room<S: Stage>(stage: &mut S, backing: Backing<S::Word>, value: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let word = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_capacity_offset as i32);
    let capacity = stage.and_imm(word, u64::from(ARRAY.capacity_mask));
    append_element(stage, backing.values, backing.length, capacity, backing.length, value, miss)
}

/// A backing's count of other values once a write added `added` and overwrote `removed`;
/// `OTHERS_UNKNOWN` stays unknown. Subtracting first keeps an exact count from overflowing.
#[inline(always)]
pub(super) fn recount<S: Stage>(stage: &mut S, others: S::Word, added: S::Word, removed: S::Word) -> S::Word {
    let removed_first = stage.sub(others, removed);
    let recounted = stage.add(removed_first, added);
    let unknown = stage.eq_imm(others, u64::from(crate::heap::OTHERS_UNKNOWN));
    stage.select(unknown, others, recounted)
}

/// One where `value` counts among a backing's others (it is no int32), else zero.
#[inline(always)]
pub(super) fn other<S: Stage>(stage: &mut S, value: S::Word) -> S::Word {
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

/// The interpreter's reading of `append_in_room`: `value` appended to the direct array `array` among
/// the array records at `record_base` where its backing is writable and has room; false (changing
/// nothing) leaves the append to the VM, which grows the backing. A reference passes the write
/// barrier and a hole is no element, so neither is a word the row appends.
#[inline(always)]
pub(crate) fn concrete_append_in_room(record_base: *const crate::heap::ArrayObject, array: crate::Value, value: crate::Value) -> bool {
    if value.heap_index().is_some() || value.is_deleted() {
        return false;
    }
    let mut stage = Concrete::new(std::ptr::null());
    array_backing(&mut stage, record_base as u64, array.to_bits(), ()).is_some_and(|backing| {
        writable_backing(&mut stage, backing, ()).is_some() && append_in_room(&mut stage, backing, value.to_bits(), ()).is_some()
    })
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

/// The interpreter's reading of `array_length`: the `length` of the direct array `base` among the
/// array records at `record_base`, as the int32 it reads as.
#[inline(always)]
pub(crate) fn concrete_array_length(record_base: *const crate::heap::ArrayObject, base: crate::Value) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    array_length(&mut stage, record_base as u64, base.to_bits(), ()).map(crate::Value::from_bits)
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

    #[test]
    fn a_packed_view_reads_a_hole_as_the_value_its_consumer_rejects_and_still_leaves_past_the_backing() {
        let elements = [Value::number(1.5), Value::DELETED];
        let view = |packed| DenseView { values: elements.as_ptr() as u64, length: elements.len() as u64, packed, within: false };
        let read = |packed, index| read_element(&mut Concrete::new(std::ptr::null()), view(packed), index, ());
        assert_eq!(read(false, 1), None, "an unpacked read leaves at a hole");
        assert_eq!(read(true, 1), Some(Value::DELETED.to_bits()), "a packed read has no hole test");
        assert_eq!(read(true, 0), Some(Value::number(1.5).to_bits()));
        assert_eq!(read(true, 2), None, "past the backing leaves either way");
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
            Concrete::new(std::ptr::null()).load_u32(crate::staged::Region::Elements.mutable(), elements.as_mut_ptr() as u64,
                crate::heap::OTHERS_FROM_VALUES)
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
        crate::staged::Stage::store_u32(&mut Concrete::new(std::ptr::null()), crate::staged::Region::Elements.mutable(),
            elements.as_mut_ptr() as u64,
            crate::heap::OTHERS_FROM_VALUES, u64::from(crate::heap::OTHERS_UNKNOWN));
        elements.reserve(1);
        assert_eq!(append(elements, full, Value::number(0.25)), Some(full as u64 + 1));
        assert_eq!(others(elements), u64::from(crate::heap::OTHERS_UNKNOWN), "an unknown count stays unknown");
    }

    /// `append_in_room` on `Concrete` over `elements`' own backing, whose header sits
    /// `header_values_offset` before its values.
    fn append_in_room_of(elements: &mut crate::heap::UniqueElements, value: Value) -> Option<u64> {
        use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
        let values = elements.as_mut_ptr() as u64;
        let header = values - ARRAY.header_values_offset as u64;
        let backing = Backing { record: 0, header, values, length: elements.len() as u64 };
        append_in_room(&mut Concrete::new(std::ptr::null()), backing, value.to_bits(), ())
    }

    #[test]
    fn an_append_in_room_reads_the_capacity_word_and_leaves_where_the_backing_is_full() {
        let mut backing = crate::heap::ArrayElements::from_slice(&[Value::integer(1)]);
        let elements = backing.make_mut();
        elements.reserve(2);
        let room = (elements.capacity() - elements.len()) as u64;
        assert!(room >= 2);
        for grown in 2..2 + room {
            assert_eq!(append_in_room_of(elements, Value::integer(0)), Some(grown));
        }
        assert_eq!(elements.len(), elements.capacity());
        assert_eq!(append_in_room_of(elements, Value::integer(0)), None, "a full backing leaves for the VM to grow it");
        assert_eq!(elements.len(), elements.capacity(), "the leave wrote nothing");
    }

    #[test]
    fn the_interpreter_appends_in_room_and_leaves_references_holes_and_full_backings() {
        use crate::heap::{ArrayElements, ArrayObject, Cell, Heap};
        let mut heap = Heap::new();
        let mut backing = ArrayElements::from_slice(&[Value::integer(1)]);
        backing.make_mut().reserve(2);
        let array = heap.alloc(Cell::Array { object: ArrayObject::fresh(Value::NULL, backing) });
        let records = heap.array_record_base().expect("an array record");
        assert!(concrete_append_in_room(records, array, Value::number(2.5)));
        assert!(!concrete_append_in_room(records, array, array), "a reference passes the write barrier");
        assert!(!concrete_append_in_room(records, array, Value::DELETED), "a hole is no element");
        assert!(!concrete_append_in_room(records, Value::NULL, Value::integer(1)), "no array");
        let mut length = 2;
        while concrete_append_in_room(records, array, Value::integer(0)) {
            length += 1;
            assert!(length < 64, "the room is the capacity word's");
        }
        let elements = heap.array(array).expect("the array").elements();
        assert_eq!((elements.len(), elements.as_slice()[1]), (length, Value::number(2.5)));
    }
}
