//! The inline-cache reads of a receiver whose entry proved its exact shape in flat form (`GuardShape` at `Form::Arena`,
//! `base_region::proofs`): the slot is the arena run's, with no compare of the auxiliary word. The shape-checking read is
//! `ic::own_field`; a stencil's flat row (`quench-bjit/stencils/ic_rows.rs`) reads through here.

use super::Stage;
use super::object::{Layout, Slots, arena_run, read_slot};
use super::region::Region;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::Representation;

/// The own field of the object record at `record` whose exact shape and flat form the entry proved: the arena run's slot
/// (`slot`, its byte offset in a stencil's hole), with no compare (the flat arm of `ic::own_field` alone).
#[inline(always)]
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/ic_rows.rs`)
pub(crate) fn own_field_flat<S: Stage>(stage: &mut S, shaped: super::Shaped<S::Word>, slot: u16) -> S::Word {
    let record = shaped.record();
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let run = arena_run(stage, payload);
    let layout = Layout { slots: Slots::Flat(run), storage_len: usize::from(slot) + 1 };
    read_slot(stage, layout, slot, Representation::Tagged)
}
