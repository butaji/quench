//! The object record header: the layout id, inline storage, extras and the proved exact shape.

use super::*;

/// The address of the table entry of the record's layout (`heap::layouts`), which holds its
/// header words: the layout id is the one word of them the record keeps.
#[inline(always)]
pub(crate) fn layout_entry<S: Stage>(stage: &mut S, at: super::super::region::Access, record: S::Word) -> S::Word {
    let id = stage.load_u32(at, record, OBJECT.layout_offset as i32);
    let base = stage.table(super::super::Table::Layouts);
    let offset = stage.mul_imm(id, 8u64);
    stage.add(base, offset)
}

/// The record's header word at `offset` in its layout's entry (`OBJECT.meta_offset`,
/// `OBJECT.auxiliary_offset`), its layout id read as `at`.
#[inline(always)]
pub(crate) fn header_u32<S: Stage>(stage: &mut S, at: super::super::region::Access, record: S::Word, offset: usize) -> S::Word {
    let entry = layout_entry(stage, at, record);
    stage.load_u32(Region::HeapMeta.readonly(), entry, offset as i32)
}

/// The record's inline-storage flag (`Object::meta`), true where its values are inline.
#[inline(always)]
pub fn inline_storage<S: Stage>(stage: &mut S, record: S::Word) -> S::Cond {
    let meta = header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.meta_offset);
    let inline = stage.and_imm(meta, u64::from(OBJECT.inline_storage_mask));
    stage.ne_imm(inline, 0u32)
}

/// Whether an object's payload word points at its extras (`Object::payload`).
#[inline(always)]
pub fn has_extras<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.eq_imm(tag, OBJECT.extras_tag)
}

/// Whether an object's payload word holds plain storage, no extras: `has_extras` negated.
#[inline(always)]
pub fn plain_payload<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.extras_tag_mask);
    stage.ne_imm(tag, OBJECT.extras_tag)
}

/// The extras an object's payload word points at, once `has_extras` holds.
#[inline(always)]
pub fn extras_pointer<S: Stage>(stage: &mut S, payload: S::Word) -> S::Word {
    stage.and_imm(payload, !OBJECT.extras_tag_mask)
}

/// The record's exact shape as every cache records it (`Object::exact_shape`): its auxiliary
/// word without the storage-form bits (`storage_form`).
#[inline(always)]
pub(crate) fn exact_shape<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    let word = aux_word(stage, record);
    recorded_shape(stage, word)
}

/// The record's whole auxiliary word: its shape and storage-form bits (`recorded_shape`, `storage_form`).
#[inline(always)]
pub(crate) fn aux_word<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset)
}

/// `exact_shape` of an auxiliary word already loaded.
#[inline(always)]
pub(crate) fn recorded_shape<S: Stage>(stage: &mut S, word: S::Word) -> S::Word {
    stage.and_imm(word, u64::from(!(OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form)))
}

/// Whether the storage-form bit `bit` (`OBJECT.inline_form`, `OBJECT.extras_form`) is set in
/// the auxiliary word `word` loaded from a record. Every writer of the header's inline flag or
/// the payload's extras tag keeps these bits current (`Object::storage_form`), so a record's
/// word answers `inline_storage` and `has_extras` without loading the header or the payload.
#[inline(always)]
pub(crate) fn storage_form<S: Stage>(stage: &mut S, word: S::Word, bit: u32) -> S::Cond {
    let set = stage.and_imm(word, u64::from(bit));
    stage.ne_imm(set, 0u32)
}
