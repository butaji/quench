//! The own-slot reads of a proved receiver (`run_owned`): through the layout the frame kept, or
//! the one a read proves and keeps, one compare of the record's auxiliary word where the form is
//! a plain arena run.

use super::super::ic::{self, Reached};
use super::super::object::{read_slot_value, Form, Layout};
use super::super::Stage;
use crate::typed_ir::Representation;
use super::{Owner, PropertyFrame};

/// The value at `slot` of `owner`'s object, which must have exact `shape` and keep its values in
/// a plain wide arena run: one compare of the record's auxiliary word, whose storage-form bits
/// (inline, extras, narrow) are all clear in that form, proves both, as the interpreter's formed
/// read does (`object::formed_own_layout`); the frame keeps the shape and the run. Only a direct
/// object's record keeps those bits current: a function's record is read through `kept_slot`.
#[inline(always)]
pub fn formed_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct, shape): (Owner, bool, u32),
    object: S::Word, at: &Reached<S::Word, S::Cond>, (slot, rep, storage_len): (u16, Representation, usize), miss: S::Miss) -> Option<S::Word> {
    // The word the receiver's header read, where it read one.
    let word = at.auxiliary.unwrap_or_else(#[inline(always)] || super::super::object::header_u32(stage, super::super::region::Region::ShapeWord.mutable(), at.record,
        crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset));
    let same = { let shape = stage.shape_site(shape); stage.eq_imm(word, shape) };
    stage.guard(same, miss)?;
    let shaped = super::super::Shaped::of_guard(at.record);
    frame.prove_shape(stage, owner, (shape, shaped), direct);
    let payload = stage.load_u64(super::super::region::Region::Payload.mutable(), at.record, crate::heap::NATIVE_OBJECT_LAYOUT.payload_offset as i32);
    let layout = Layout { slots: super::super::object::Slots::Flat(super::super::object::arena_run(stage, payload)), storage_len };
    frame.keep_layout(owner, object, (shape, shaped.fact()), layout);
    Some(read_slot_value(stage, layout, slot, rep))
}

/// The value at `slot` of `owner`'s object of exact `shape`, through the layout the frame kept
/// where it covers the slot, else the one this read proves and keeps.
#[inline(always)]
pub fn kept_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, shape): (Owner, u32), object: S::Word,
    at: &Reached<S::Word, S::Cond>, (slot, form, storage_len, rep): (u16, Form, usize, Representation), miss: S::Miss) -> Option<S::Word> {
    let layout = match frame.kept_layout(owner, object, shape).filter(|layout| usize::from(slot) < layout.storage_len) {
        Some(layout) => layout,
        None => {
            let layout = ic::slot_layout(stage, at, form, storage_len, miss)?;
            frame.keep_layout(owner, object, (shape, at.shaped.expect("a slot read follows the guard of its shape").fact()), layout);
            layout
        }
    };
    Some(read_slot_value(stage, layout, slot, rep))
}

/// The value at `slot` of `owner`'s object whose layout id is `word`, read as `rep` says (a raw
/// double's slot boxes its double, as `formed_slot` does): through the layout the frame kept for
/// its shape, else one compare of the id proves the shape and the storage form, whose layout the
/// read keeps, testing no form bit. The layout id proves the exact shape, and the slot's type is
/// the dependency the compiled code holds on it (`NativeFieldLoad::slot_dependency`).
#[inline(always)]
pub fn word_formed_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool), object: S::Word,
    at: &Reached<S::Word, S::Cond>, word: u32, (slot, rep, storage_len): (u16, Representation, usize), miss: S::Miss) -> Option<S::Word> {
    let shape = ic::layout_shape(word);
    if frame.proven_shape(stage, owner).map(|(proven, _)| proven) == Some(shape)
        && let Some(layout) = frame.kept_layout(owner, object, shape).filter(|layout| usize::from(slot) < layout.storage_len) {
        return Some(read_slot_value(stage, layout, slot, rep));
    }
    let shaped = ic::guard_layout(stage, at, word, miss)?;
    let bits = ic::layout_form(word);
    frame.prove_shape(stage, owner, (shape, shaped), direct);
    let formed = Reached { formed: Some(bits), shaped: Some(shaped), ..*at };
    let layout = ic::slot_layout(stage, &formed, ic::form_of_layout(word), storage_len, miss)?;
    frame.keep_layout(owner, object, (shape, shaped.fact()), layout);
    Some(read_slot_value(stage, layout, slot, rep))
}
