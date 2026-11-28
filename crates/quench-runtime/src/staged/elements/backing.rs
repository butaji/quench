//! The backing of an array value: its record, header and values, and the element of a backing.

use super::*;

/// A dense array's backing as an access reaches it from the array value: the record, the
/// backing's header, its first value and its length.
#[derive(Clone, Copy)]
pub struct Backing<W> {
    pub record: W,
    pub header: W,
    pub values: W,
    pub length: W,
}

/// The backing of the direct array `base` among the array records at `record_base`: `None`
/// (leaving at `miss`) for any other value, or before the first array record exists
/// (`record_base` zero), or for a narrow or double backing (`ENCODED_LEN`).
/// The access of the words a backing's load reads: its header (`array_header`, the record's payload word and its
/// extras) and its length. A caller proves a word readonly where nothing in its code writes the word.
#[derive(Clone, Copy)]
pub struct BackingAccess { pub header: Access, pub length: Access }

impl BackingAccess {
    /// Mutable words, which any code may change: the access of every backing read a caller does not prove.
    pub const MUTABLE: Self = Self { header: Region::Payload.mutable(), length: Region::Elements.mutable() };
}

pub fn array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    array_backing_as(stage, record_base, base, miss, BackingAccess::MUTABLE)
}

/// `array_backing` reading the backing's words with `access`.
pub fn array_backing_as<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss, access: BackingAccess) -> Option<Backing<S::Word>> {
    let backing = flagged_backing(stage, record_base, base, miss, access)?;
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
pub fn record_element<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, index: S::Word, upper_words: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let backing = flagged_backing(stage, record_base, base, miss, BackingAccess::MUTABLE)?;
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
            super::super::object::decode_narrow_through(stage, narrow, upper_words)
        }, #[inline(always)] |stage| loaded_element(stage, backing.values, index).1);
        let present = stage.ne_imm(value, crate::Value::DELETED.to_bits());
        stage.guard(present, miss)?;
        Some(value)
    })
}

/// `array_backing` up to the length load: the backing with its length word's flags
/// (`ENCODED_LEN`) left in.
#[inline(always)]
pub(super) fn flagged_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss, access: BackingAccess) -> Option<Backing<S::Word>> {
    let record = super::super::proof::ArrayRecord::guard(stage, record_base, base, miss)?;
    Some(record_backing_as(stage, record.record(), access))
}

/// Proves `base` a direct array (among the array records at `record_base`) of no named property (the
/// empty shape, as `Heap::alloc_array` makes one) whose prototype's link is `link`, else leaves at
/// `miss`: a member read by name of it is its prototype's (`GlobalRead::NativeMember`'s `array_link`).
#[inline(always)]
pub fn plain_array_of<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, link: u32, miss: S::Miss) -> Option<()> {
    let record = super::super::proof::ArrayRecord::guard(stage, record_base, base, miss)?.record();
    let object = stage.add_imm(record, crate::heap::NATIVE_ARRAY_LAYOUT.object_offset as u64);
    let shape = super::super::object::exact_shape(stage, object);
    let unnamed = stage.eq_imm(shape, 0u32);
    stage.guard(unnamed, miss)?;
    let prototype = super::super::object::proto_link(stage, object);
    let linked = stage.eq_imm(prototype, u64::from(link));
    stage.guard(linked, miss)
}
