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

/// Proves the dense backing at `values` counts no others (`OTHERS_FROM_VALUES` zero): every value
/// below its length is an int32 and none is a hole, so each element's word is an int32's and needs
/// no tag test (`int_element`). Leaves at `miss` otherwise, as a count of others does.
#[inline(always)]
pub(crate) fn int_only_backing<S: Stage>(stage: &mut S, values: S::Word, miss: S::Miss) -> Option<()> {
    let others = stage.load_u32(Region::Elements.mutable(), values, crate::heap::OTHERS_FROM_VALUES);
    let none = stage.eq_imm(others, 0u32);
    stage.guard(none, miss)
}

/// Element `index` of an int-only backing (`int_only_backing`) of `length` values at `values`, as
/// its word: no hole test, since none is present. An `index` the caller did not prove below `length`
/// (`within` false) leaves at `miss` past it.
#[inline(always)]
pub(crate) fn int_element<S: Stage>(stage: &mut S, values: S::Word, length: S::Word, index: S::Word,
    within: bool, miss: S::Miss) -> Option<S::Word> {
    bounded(stage, within, length, index, miss)?;
    Some(loaded_element(stage, values, index).1)
}

/// The address and bits of element `index` of the backing at `values`.
#[inline(always)]
fn loaded_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word) -> (S::Word, S::Word) {
    let offset = stage.uextend32(index);
    let offset = stage.mul_imm(offset, std::mem::size_of::<crate::Value>() as u64);
    let address = stage.add(values, offset);
    (address, stage.load_u64(Region::Elements.mutable(), address, 0))
}

/// Element `index`, which the caller proved below the length, of the double backing
/// (`heap::DOUBLE_LEN`) at `values`: the `Value` of the Number its word holds
/// (`Stage::box_number`, canonical: 3.0's bits read as the int32 3); `None` (leaving at `miss`)
/// at a hole, `Value::DELETED`'s word, which no box may canonicalize away. A double word is as
/// wide as a `Value`, so the element's address is a wide element's.
#[inline(always)]
fn double_element<S: Stage>(stage: &mut S, values: S::Word, index: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (address, word) = loaded_element(stage, values, index);
    let present = stage.ne_imm(word, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    let number = stage.load_f64(Region::Elements.mutable(), address, 0);
    Some(stage.box_number(number))
}

/// The guard of an `index` against `length` a view did not prove below it (`DenseView::within`).
#[inline(always)]
fn bounded<S: Stage>(stage: &mut S, within: bool, length: S::Word, index: S::Word, miss: S::Miss) -> Option<()> {
    if within { return Some(()); }
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)
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
    /// The backing's double flag (`heap::DOUBLE_LEN` of its length word), nonzero for a double
    /// backing, where the view's proof admitted one (`prove_view`): each access then takes the
    /// backing's form. `None` where the proof admitted a wide backing alone (a loop's hoisted
    /// view, a loop entry's).
    pub(crate) double: Option<W>,
    /// Whether the backing counts no others for this view (`OTHERS_UNKNOWN`, as a loop's hoisted
    /// writable view holds it: `emit_dense_array_uncounted`), so a store through it writes the
    /// element alone (`replace_uncounted_element`). A view proved by a store's own check keeps the
    /// count, `false`.
    pub(crate) uncounted: bool,
}

/// The view of the direct array `base` among the array records at `records`, writable where
/// `writable`, of a wide or a double backing (`DenseView::double`): `None` (leaving at `miss`)
/// for any other value or a narrow backing.
#[inline(always)]
pub(crate) fn prove_view<S: Stage>(stage: &mut S, records: S::Word, base: S::Word, writable: bool,
    miss: S::Miss) -> Option<DenseView<S::Word>> {
    let backing = if writable { writable_array_backing(stage, records, base, miss)? } else { flagged_backing(stage, records, base, miss)? };
    let (length, double) = wide_or_double(stage, backing.length, miss)?;
    Some(DenseView { values: backing.values, length, packed: false, within: false, double: Some(double), uncounted: false })
}

/// A backing's flagged length word (`ENCODED_LEN`) as a view reads it: the count, and the
/// double flag as a word (`DenseView::double`); `None` (leaving at `miss`) for a narrow
/// backing, whose decode only the interpreter's and the b-jit's reads make
/// (`any_backing_element`, `record_element`).
#[inline(always)]
fn wide_or_double<S: Stage>(stage: &mut S, length: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
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
pub(crate) fn read_element<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word,
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
pub(crate) fn replace_present<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
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
/// backing holds for it (`array_elements::double_word`), so the store writes the value's word.
/// A present element is replaced, so the backing's count (of holes) stays. An int32 or any
/// other value leaves at `miss`.
#[inline(always)]
fn replace_double<S: Stage>(stage: &mut S, view: DenseView<S::Word>, index: S::Word, value: S::Word,
    miss: S::Miss) -> Option<()> {
    let number = super::value::is_double(stage, value);
    stage.guard(number, miss)?;
    bounded(stage, view.within, view.length, index, miss)?;
    let (address, old) = loaded_element(stage, view.values, index);
    let present = stage.ne_imm(old, crate::Value::DELETED.to_bits());
    stage.guard(present, miss)?;
    stage.store_u64(Region::Elements.mutable(), address, 0, value);
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
/// (`record_base` zero), or for a narrow or double backing (`ENCODED_LEN`).
#[inline(always)]
pub(crate) fn array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    let backing = flagged_backing(stage, record_base, base, miss)?;
    // A narrow or double backing (`ArrayElements::is_encoded`) flags one of its length's top
    // two bits: it misses.
    let encoded = stage.ugt_imm(backing.length, u64::from(!crate::heap::ENCODED_LEN));
    let wide = stage.not(encoded);
    stage.guard(wide, miss)?;
    Some(backing)
}

/// Element `index` of the direct array `base` (among the array records at `record_base`) of any
/// backing: a wide one's value, a double one's Number (`double_element`), or a narrow one's,
/// decoded through the upper-word table at `upper_words` (`object::decode_narrow_through`);
/// `None` (leaving at `miss`) past the backing's end or at a hole.
#[inline(always)]
pub(crate) fn record_element<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, index: S::Word, upper_words: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let backing = flagged_backing(stage, record_base, base, miss)?;
    backing_element(stage, backing, index, upper_words, miss)
}

/// `record_element` past the backing's loads (`flagged_backing`).
#[inline(always)]
fn backing_element<S: Stage>(stage: &mut S, backing: Backing<S::Word>, index: S::Word, upper_words: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    use crate::heap::{DOUBLE_LEN, ENCODED_LEN, NARROW_LEN};
    let length = stage.and_imm(backing.length, u64::from(!ENCODED_LEN));
    let in_bounds = stage.ult(index, length);
    stage.guard(in_bounds, miss)?;
    let flag = stage.and_imm(backing.length, u64::from(DOUBLE_LEN));
    let double = stage.ne_imm(flag, 0u32);
    stage.choose_leaving(double, #[inline(always)] |stage| double_element(stage, backing.values, index, miss), #[inline(always)] |stage| {
        let flag = stage.and_imm(backing.length, u64::from(NARROW_LEN));
        let narrow = stage.ne_imm(flag, 0u32);
        let value = stage.choose(narrow, #[inline(always)] |stage| {
            let offset = stage.mul_imm(index, std::mem::size_of::<u32>() as u64);
            let address = stage.add(backing.values, offset);
            let narrow = stage.load_u32(Region::Elements.mutable(), address, 0);
            super::object::decode_narrow_through(stage, narrow, upper_words)
        }, #[inline(always)] |stage| loaded_element(stage, backing.values, index).1);
        let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
        stage.guard(present, miss)?;
        Some(value)
    })
}

/// `array_backing` up to the length load: the backing with its length word's flags
/// (`ENCODED_LEN`) left in.
#[inline(always)]
fn flagged_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
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

/// The backing of the array record `record`: its header is one load (`array_header`), its
/// length word another, a narrow or double backing's flags (`ENCODED_LEN`) left in.
#[inline(always)]
pub(crate) fn record_backing<S: Stage>(stage: &mut S, record: S::Word) -> Backing<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let header = array_header(stage, record);
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Backing { record, header, values, length }
}

/// The backing's header of the array record at `record`. Without extras, the record's object
/// keeps the header pointer as its payload word; with extras (an array with named properties,
/// a prototype, sparse or arguments facts), the word points at them and they hold it.
#[inline(always)]
pub(crate) fn array_header<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let word = stage.load_u64(Region::Payload.mutable(), record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let extras = super::object::has_extras(stage, word);
    stage.choose(extras, #[inline(always)] |stage| {
        let extras = super::object::extras_pointer(stage, word);
        stage.load_u64(Region::Payload.mutable(), extras, (ARRAY.extras_elements_offset + ARRAY.elements_header_offset) as i32)
    }, |_| word)
}

/// `flagged_backing` and `writable_backing` in one: the array's payload word is proved plain (no
/// extras) first, so it is the backing's header with no choice between the two words
/// (`array_header`), and the plain test is made once. The length word keeps its form's flags.
#[inline(always)]
fn writable_array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    use crate::heap::{NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};
    let is_array = super::value::is_direct_array(stage, base);
    stage.guard(is_array, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let record = array_record(stage, record_base, base);
    // Without extras, the object's payload word is the backing's header (`array_header`).
    let header = stage.load_u64(Region::Payload.mutable(), record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let plain = super::object::plain_payload(stage, header);
    stage.guard(plain, miss)?;
    // The length word keeps its flags: `prove_view` reads the backing's form from them.
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    let owners = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_owners_offset as i32);
    let unique = stage.eq_imm(owners, 1u32);
    stage.guard(unique, miss)?;
    let object = stage.add_imm(record, ARRAY.object_offset as u64);
    let meta = super::object::header_u32(stage, Region::Records.mutable(), object, OBJECT.meta_offset);
    let held = stage.and_imm(meta, u64::from(ARRAY.frozen_mask));
    let clear = stage.eq_imm(held, 0u32);
    stage.guard(clear, miss)?;
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Some(Backing { record, header, values, length })
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
    let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
    let meta = super::object::header_u32(stage, Region::ShapeWord.mutable(), object, OBJECT.meta_offset);
    let held = stage.and_imm(meta, u64::from(barred));
    let clear = stage.eq_imm(held, 0u32);
    stage.guard(clear, miss)?;
    // Without extras, this word is the backing's header (`array_header`).
    let payload = stage.load_u64(Region::Payload.mutable(), backing.record, (ARRAY.object_offset + OBJECT.payload_offset) as i32);
    let plain = super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)
}

/// The `length` of the direct array `base` as the int32 word it reads as: its dense backing's
/// count, of any form, which an array without extras (an arguments object, sparse elements) has
/// as its length. `None` (leaving at `miss`) for any other value, an array with extras, or a
/// length past int32.
#[inline(always)]
pub(crate) fn array_length<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let is_array = super::value::is_direct_array(stage, base);
    stage.guard(is_array, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let record = array_record(stage, record_base, base);
    // The record's payload word is the backing's header when it holds no extras, which is the only
    // case this read takes: one load gives the plain test and the header, and extras miss at once.
    let header = stage.load_u64(Region::Payload.mutable(), record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let plain = super::object::plain_payload(stage, header);
    stage.guard(plain, miss)?;
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    // A narrow or double backing flags its length's top bits: it misses.
    let encoded = stage.ugt_imm(length, u64::from(!crate::heap::ENCODED_LEN));
    let wide = stage.not(encoded);
    stage.guard(wide, miss)?;
    super::value::length_value(stage, length, miss)
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
    let int = super::value::is_int(stage, value);
    let (zero, one) = (stage.constant(0), stage.constant(1));
    stage.select(int, zero, one)
}

#[cfg(test)]
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
    // A value of the old one's kind (both int32, or both others) leaves the count as it is,
    // exact or unknown, so only the element is written: a double over a double (V8-v7
    // NavierStokes) skips the count's load, recount and store.
    if other(&mut stage, value.to_bits()) == other(&mut stage, old.to_bits()) {
        replace_uncounted_element(&mut stage, address as u64, value.to_bits());
        return;
    }
    replace_element(&mut stage, values as u64, address as u64, old.to_bits(), value.to_bits());
}

/// `concrete_replace_element` of the elements at `first + step * k` by `values`'s items, the
/// backing's count of others recounted once for all of them.
///
/// # Safety
/// As `concrete_replace_element`, for every index.
#[inline(always)]
pub(crate) unsafe fn concrete_replace_elements(backing: *mut crate::Value, first: isize, step: isize, values: impl Iterator<Item = crate::Value>) {
    let mut stage = Concrete::new(std::ptr::null());
    let others = stage.load_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES);
    let counted = others != u64::from(crate::heap::OTHERS_UNKNOWN);
    let (mut added, mut removed) = (0u64, 0u64);
    for (k, value) in values.enumerate() {
        // SAFETY: the caller's contract.
        let address = unsafe { backing.offset(first + step * k as isize) };
        if counted {
            added += other(&mut stage, value.to_bits());
            removed += other(&mut stage, unsafe { address.read() }.to_bits());
        }
        replace_uncounted_element(&mut stage, address as u64, value.to_bits());
    }
    if added != removed {
        let others = recount(&mut stage, others, added, removed);
        stage.store_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES, others);
    }
}

/// `concrete_replace_elements` of the contiguous elements from `first` by `values`, in
/// passes the compiler vectorizes: count the others leaving and arriving, then copy.
///
/// # Safety
/// As `concrete_replace_element`, for every index.
#[inline(always)]
pub(crate) unsafe fn concrete_replace_run(backing: *mut crate::Value, first: usize, values: &[u64]) {
    let mut stage = Concrete::new(std::ptr::null());
    // SAFETY: the caller's contract.
    let elements = unsafe { std::slice::from_raw_parts_mut(backing.add(first).cast::<u64>(), values.len()) };
    let others = stage.load_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES);
    if others != u64::from(crate::heap::OTHERS_UNKNOWN) {
        let count = |bits: &[u64]| bits.iter().map(|&bits| other(&mut Concrete::new(std::ptr::null()), bits)).sum::<u64>();
        let (added, removed) = (count(values), count(elements));
        if added != removed {
            let others = recount(&mut stage, others, added, removed);
            stage.store_u32(Region::Elements.mutable(), backing as u64, crate::heap::OTHERS_FROM_VALUES, others);
        }
    }
    elements.copy_from_slice(values);
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

/// Element `index` of the direct array `base` among the array records at `record_base`, of any
/// backing: a wide one's value, a double backing's Number (`Stage::box_number`, canonical) or a
/// narrow backing's value decoded (`object::decode_narrow`); `None` (leaving at `miss`) for any
/// other base, past the elements or at a hole.
#[inline(always)]
pub(crate) fn any_backing_element<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, index: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    use crate::heap::{DOUBLE_LEN, ENCODED_LEN};
    let Backing { values, length, .. } = flagged_backing(stage, record_base, base, miss)?;
    // A narrow or double backing flags one of its length's top two bits (`ArrayElements::is_encoded`).
    let encoded = stage.ugt_imm(length, u64::from(!ENCODED_LEN));
    stage.choose_leaving(encoded, #[inline(always)] |stage| {
        let count = stage.and_imm(length, u64::from(!ENCODED_LEN));
        let in_bounds = stage.ult(index, count);
        stage.guard(in_bounds, miss)?;
        let flag = stage.and_imm(length, u64::from(DOUBLE_LEN));
        let double = stage.ne_imm(flag, 0u32);
        stage.choose_leaving(double, #[inline(always)] |stage| double_element(stage, values, index, miss), #[inline(always)] |stage| {
            // A narrow backing (`ArrayElements::is_narrow`): the element decoded from its narrow form.
            let offset = stage.uextend32(index);
            let offset = stage.mul_imm(offset, std::mem::size_of::<u32>() as u64);
            let address = stage.add(values, offset);
            let narrow = stage.load_u32(Region::Elements.mutable(), address, 0);
            let value = super::object::decode_narrow(stage, narrow);
            let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
            stage.guard(present, miss)?;
            Some(value)
        })
    }, #[inline(always)] |stage| dense_element(stage, values, length, index, miss).map(|(_, value)| value))
}

/// `any_backing_element` on `Concrete`: the interpreter's read of element `index` of `base`.
#[inline(always)]
pub(crate) fn concrete_record_element(record_base: *const crate::heap::ArrayObject, base: crate::Value, index: usize) -> Option<crate::Value> {
    let mut stage = Concrete::new(std::ptr::null());
    any_backing_element(&mut stage, record_base as u64, base.to_bits(), index as u64, ()).map(crate::Value::from_bits)
}

#[cfg(test)]
mod tests;
