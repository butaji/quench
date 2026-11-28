//! Collection records: the layout of a collection, its array records and their headers.

use super::*;

/// A Map or Set record's size and where its object part starts.
#[derive(Clone, Copy)]
pub struct CollectionLayout { pub record_size: usize, pub object_offset: usize }

/// Proves `base` a Map or Set (a direct collection), whose records are at `record_base`, of exact
/// shape `shape` and prototype link `link`, else leaves at `miss`: a member read by name of it is
/// then that prototype's, where the shape holds no member of the name. `layout` is the record's
/// (`heap::NATIVE_COLLECTION_LAYOUT`).
#[inline(always)]
pub fn collection_of<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, layout: CollectionLayout, (shape, link): (u32, u32), miss: S::Miss) -> Option<()> {
    let is_collection = super::super::value::is_direct_collection(stage, base);
    stage.guard(is_collection, miss)?;
    let present = stage.ne_imm(record_base, 0u32);
    stage.guard(present, miss)?;
    let index = stage.and_imm(base, u64::from(u32::MAX));
    let offset = stage.mul_imm(index, layout.record_size as u64);
    let record = stage.add(record_base, offset);
    let object = stage.add_imm(record, layout.object_offset as u64);
    let exact = super::super::object::exact_shape(stage, object);
    let shaped = stage.eq_imm(exact, u64::from(shape));
    stage.guard(shaped, miss)?;
    let prototype = super::super::object::proto_link(stage, object);
    let linked = stage.eq_imm(prototype, u64::from(link));
    stage.guard(linked, miss)
}

/// The record of the direct array `base`, which the caller proved one, among the array records at
/// `record_base`, which it proved present: the record's address is its index, `base`'s low half,
/// times the record size past `record_base`.
#[inline(always)]
pub fn array_record<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word) -> S::Word {
    let index = stage.and_imm(base, u64::from(u32::MAX));
    let offset = stage.mul_imm(index, crate::heap::NATIVE_ARRAY_LAYOUT.record_size as u64);
    stage.add(record_base, offset)
}

/// The backing of the array record `record`: its header is one load (`array_header`), its
/// length word another, a narrow or double backing's flags (`ENCODED_LEN`) left in.
#[inline(always)]
pub fn record_backing<S: Stage>(stage: &mut S, record: S::Word) -> Backing<S::Word> {
    record_backing_as(stage, record, BackingAccess::MUTABLE)
}

/// `record_backing` reading the backing's header and length with `access`.
pub fn record_backing_as<S: Stage>(stage: &mut S, record: S::Word, access: BackingAccess) -> Backing<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let header = array_header(stage, record, access.header);
    let length = stage.load_u32(access.length, header, ARRAY.header_length_offset as i32);
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Backing { record, header, values, length }
}

/// The backing's header of the array record at `record`. Without extras, the record's object
/// keeps the header pointer as its payload word; with extras (an array with named properties,
/// a prototype, sparse or arguments facts), the word points at them and they hold it.
#[inline(always)]
pub fn array_header<S: Stage>(stage: &mut S, record: S::Word, payload: Access) -> S::Word {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let word = stage.load_u64(payload, record, (ARRAY.elements_offset + ARRAY.elements_header_offset) as i32);
    let extras = super::super::object::has_extras(stage, word);
    stage.choose(extras, #[inline(always)] |stage| {
        let extras = super::super::object::extras_pointer(stage, word);
        stage.load_u64(payload, extras, (ARRAY.extras_elements_offset + ARRAY.elements_header_offset) as i32)
    }, |_| word)
}
