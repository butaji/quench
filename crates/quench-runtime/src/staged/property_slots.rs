//! The own-slot reads of a proved receiver (`run_owned`): through the layout the frame kept, or
//! the one a read proves and keeps, one compare of the record's auxiliary word where the form is
//! a plain arena run.

use super::super::ic::{self, Reached};
use super::super::object::{read_slot_value, Form, Layout, Rep};
use super::super::Stage;
use super::{Owner, PropertyFrame};

/// The value at `slot` of `owner`'s object, which must have exact `shape` and keep its values in
/// a plain wide arena run: one compare of the record's auxiliary word, whose storage-form bits
/// (inline, extras, narrow) are all clear in that form, proves both, as the interpreter's formed
/// read does (`object::formed_own_layout`); the frame keeps the shape and the run. Only a direct
/// object's record keeps those bits current: a function's record is read through `kept_slot`.
pub(super) fn formed_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct, shape): (Owner, bool, u32),
    at: &Reached<S::Word, S::Cond>, (slot, rep, storage_len): (u16, Rep, usize), miss: S::Miss) -> Option<S::Word> {
    let word = super::super::object::header_u32(stage, super::super::region::Region::ShapeWord.mutable(), at.record,
        crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset);
    let same = { let shape = stage.site(u64::from(shape)); stage.eq_imm(word, shape) };
    stage.guard(same, miss)?;
    frame.prove_shape(stage, owner, shape, direct);
    let payload = stage.load_u64(super::super::region::Region::Payload.mutable(), at.record, crate::heap::NATIVE_OBJECT_LAYOUT.payload_offset as i32);
    let layout = Layout { slots: super::super::object::Slots::Flat(super::super::object::arena_run(stage, payload)), storage_len };
    frame.keep_layout(owner, shape, layout);
    Some(read_slot_value(stage, layout, slot, rep))
}

/// The value at `slot` of `owner`'s object of exact `shape`, through the layout the frame kept
/// where it covers the slot, else the one this read proves and keeps.
pub(super) fn kept_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, shape): (Owner, u32),
    at: &Reached<S::Word, S::Cond>, (slot, form, storage_len, rep): (u16, Form, usize, Rep), miss: S::Miss) -> Option<S::Word> {
    let layout = match frame.kept_layout(owner, shape).filter(|layout| usize::from(slot) < layout.storage_len) {
        Some(layout) => layout,
        None => {
            let layout = ic::slot_layout(stage, at, form, storage_len, miss)?;
            frame.keep_layout(owner, shape, layout);
            layout
        }
    };
    Some(read_slot_value(stage, layout, slot, rep))
}
