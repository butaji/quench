//! A function's inferred name as a staged operation: a function record of a proved exact shape
//! whose own `name` still holds the empty string takes the name's string in that slot and moves
//! to the shape that records the original name (`ShapeTransition::FunctionName`). The interpreter
//! runs it on `Concrete`; a stencil and the Cranelift specializer instantiate the same row.
//!
//! The edge (before-shape, after-shape, name slot) is the one the VM's slow arm made for this
//! site, and shapes' transitions are append-only, so a resolved edge needs no dependency: the row
//! proves what varies per function, the before-shape (which fixes the name slot's attributes and
//! that the shape already records an original name, so no construction memo needs clearing), its
//! plain storage, the store gate of the function record space, and that the slot holds the empty
//! string the VM keeps (`empty`, a word the caller reads from the dispatch payload or the VM).

use super::region::Region;
use super::add::{Edge, admitted};
use super::entry::plain_value;
use super::object::Form;
use crate::typed_ir::Representation;
use super::slot_store::store_slot;
use super::{StagedRecordSpace, Stage};
use crate::heap::{NATIVE_FUNCTION_LAYOUT as FUNCTION, NATIVE_OBJECT_LAYOUT as OBJECT};

/// The storage form the name slot is in: a slot below the inline count is packed or an arena run
/// as the record says at run time; any other is an arena run. The slot already holds a value, so
/// no room is needed, unlike an add (`add_plan::Transition::form`).
#[inline(always)]
pub fn name_form(slot: u16) -> Form {
    if usize::from(slot) < OBJECT.inline_property_count { Form::Either } else { Form::Arena }
}

/// Names the function whose `Object` part is at `record` (function record `record_id`): where it
/// holds the edge's before-shape, plain storage, an empty name in the edge's slot and the function
/// space's store gate admits the write, `name` replaces the empty name and the record moves to the
/// edge's after-shape; `None` (leaving at `miss`, before any write) otherwise. The name is written
/// before the shape, as the add row writes its value, so no reader sees the shape without it.
#[inline(always)]
pub(crate) fn set_function_name<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, (edge, empty): (Edge<S::Word>, S::Word),
    form: Form, name: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted(stage, StagedRecordSpace::Function, record, record_id, edge.before, name, miss)?;
    let current = plain_value(stage, record, edge.slot, miss)?;
    let moved = stage.sub(current, empty);
    let unnamed = stage.eq_imm(moved, 0u32);
    stage.guard(unnamed, miss)?;
    store_slot(stage, record, payload, edge.slot, form, (name, Representation::Tagged), miss)?;
    stage.store_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32, edge.after);
    Some(())
}

/// `set_function_name` for the function word `function` of the function record table at `records`,
/// the way a compiled site holds it: a reserved table, and the word names a direct function (else
/// `None`, leaving at `miss`).
#[inline(always)]
pub fn site_name<S: Stage>(stage: &mut S, records: S::Word, function: S::Word, naming: (Edge<S::Word>, S::Word),
    form: Form, name: S::Word, miss: S::Miss) -> Option<()> {
    let present = stage.ne_imm(records, 0u32);
    stage.guard(present, miss)?;
    let direct = super::value::is_direct_function(stage, function);
    stage.guard(direct, miss)?;
    let id = stage.and_imm(function, u64::from(u32::MAX));
    let offset = stage.mul_imm(id, FUNCTION.record_size as u64);
    let record = stage.add(records, offset);
    let object = stage.add_imm(record, FUNCTION.object_offset as u64);
    set_function_name(stage, object, id, naming, form, name, miss)
}
