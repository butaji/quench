//! The prototype link word and its record and object ids.

use super::*;

/// The record's encoded prototype link (`Object::proto_link`).
#[inline(always)]
pub fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.meta_offset);
    stage.and_imm(meta, u64::from(OBJECT.proto_mask))
}

/// The span of the object record range of prototype links. The range starts at a multiple of its
/// power-of-two span, so a link in it is the base in its high bits, one mask and compare, and
/// holds the id in its low bits, a mask that addresses the record without the subtracted base.
const RECORD_LINKS: u32 = OBJECT.proto_record_end - OBJECT.proto_record_base;

const _: () = assert!(RECORD_LINKS.is_power_of_two() && OBJECT.proto_record_base.is_multiple_of(RECORD_LINKS));

/// The link of a `null` prototype: the all-ones link field (`ProtoLink::NULL`).
pub(super) const NULL_LINK: u32 = OBJECT.proto_mask;

/// Whether `link` names an object record (`ProtoLink::object_record_id`).
#[inline(always)]
pub(crate) fn proto_names_record<S: Stage>(stage: &mut S, link: S::Word) -> S::Cond {
    let high = stage.and_imm(link, !u64::from(RECORD_LINKS - 1));
    stage.eq_imm(high, u64::from(OBJECT.proto_record_base))
}

/// The id of the object record `link` names, which `proto_names_record` proved it names.
#[inline(always)]
pub(crate) fn proto_record_id<S: Stage>(stage: &mut S, link: S::Word) -> S::Word {
    stage.and_imm(link, u64::from(RECORD_LINKS - 1))
}

/// The id of the object record `link` names, or `miss` where it names none or its object is live.
#[inline(always)]
pub fn proto_object_record_id<S: Stage>(stage: &mut S, link: S::Word, miss: S::Miss) -> Option<S::Word> {
    let inside = proto_names_record(stage, link);
    stage.guard(inside, miss)?;
    // The link's low bits are the granule; the record id is its header's hop.
    let granule = proto_record_id(stage, link);
    departed_record_id(stage, granule, miss)
}
