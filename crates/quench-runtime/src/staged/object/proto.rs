//! The prototype link word and the null link.

use super::*;

/// The record's encoded prototype link (`Object::proto_link`).
#[inline(always)]
pub fn proto_link<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let meta = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.meta_offset);
    stage.and_imm(meta, u64::from(OBJECT.proto_mask))
}

/// The link of a `null` prototype: the all-ones link field (`ProtoLink::NULL`).
pub const NULL_LINK: u32 = OBJECT.proto_mask;
