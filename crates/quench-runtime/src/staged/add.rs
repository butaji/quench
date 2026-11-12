//! A property add as a staged operation: an extensible ordinary object of a proved exact shape
//! takes a value at the next slot and moves to the shape the transition names. The interpreter
//! runs it on `Concrete`; a stencil instantiates the same row.
//!
//! A shape's transitions are append-only edges, (shape, atom, attributes) to (shape', slot): an
//! edge, once made, never changes, so a resolved one is a Stable fact that needs no dependency of
//! its own. What the object proves at each use is the before-shape (the guard), its integrity
//! (extensible, not frozen), its plain storage, and the store gate. Whether the slot fits is a
//! fact of the layout, not a read: a packed object has a free inline half below the inline
//! count, and an arena range's capacity derives from its length alone (`range_has_room`), so the
//! planner (`add_plan`) decides which form can take the add and the row never grows storage. Any
//! other add is the VM's, which allocates and grows.

use super::entry::direct_record;
use super::object::{Form, plain_payload, proto_link, store_slot};
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// One resolved transition as the words a stage computes on: the shape the object must hold, the
/// shape it moves to, and the slot the value takes, which is the storage length of the first.
#[derive(Clone, Copy)]
pub(crate) struct Edge<W> {
    pub(crate) before: W,
    pub(crate) after: W,
    pub(crate) slot: W,
}

/// What the object at `record` (id `record_id`) must hold for the add of `value`: exactly the
/// before-shape (which also excludes a dictionary and exotic access), no integrity bit (it is
/// extensible and not frozen), plain storage (no extras, so it never was a prototype and no
/// shape reader or method cache can name it) and an open store gate (no slot representation
/// observes the write, no barrier log is due). Answers the payload word the storage reads from.
#[inline(always)]
fn admitted<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, before: S::Word, value: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    let exact = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    let moved = stage.sub(exact, before);
    let same = stage.eq_imm(moved, 0u32);
    stage.guard(same, miss)?;
    let meta = stage.load_u32(record, OBJECT.meta_offset as i32);
    let integrity = stage.and_imm(meta, u64::from(OBJECT.integrity_mask));
    let open = stage.eq_imm(integrity, 0u32);
    stage.guard(open, miss)?;
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    let gate = stage.store_gate(record_id, value);
    stage.guard(gate, miss)?;
    Some(payload)
}

/// Adds `value` at the slot of `edge` to the object record at `record` (id `record_id`) and moves
/// it to the edge's after-shape, where it holds the before-shape, is extensible and plain, the
/// store gate admits the write and `form` holds its values; `None` (leaving at `miss`, before any
/// write) otherwise. The value is written before the shape, so no reader sees a shape without its
/// slot; the transition retires no shape reader because only an object with extras, which a
/// prototype or holder has, is read through one.
#[inline(always)]
pub(crate) fn add_property<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, edge: Edge<S::Word>,
    form: Form, value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted(stage, record, record_id, edge.before, value, miss)?;
    store_slot(stage, record, payload, edge.slot, form, value, miss)?;
    stage.store_u32(record, OBJECT.auxiliary_offset as i32, edge.after);
    Some(())
}

/// `add_property` for the object word `object` of the record table at `records`, the way a
/// compiled site holds it: the word is a direct object (else `None`, leaving at `miss`), and where
/// `link` is given (an add the prototype chain can intercept, whose chain proof the code holds as
/// a Stable fact for exactly that prototype) the object's prototype link is it. The chain's own
/// shapes are not read: a change to any prototype's layout retires the code
/// (`heap::subscribe_prototype_values`), so only what varies per receiver, its link, is tested.
#[inline(always)]
pub(crate) fn site_add<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, (edge, link): (Edge<S::Word>, Option<S::Word>),
    form: Form, value: S::Word, miss: S::Miss) -> Option<()> {
    let present = stage.ne_imm(records, 0u32);
    stage.guard(present, miss)?;
    let (id, record) = direct_record(stage, records, object, miss)?;
    if let Some(link) = link {
        let held = proto_link(stage, record);
        let moved = stage.sub(held, link);
        let same = stage.eq_imm(moved, 0u32);
        stage.guard(same, miss)?;
    }
    add_property(stage, record, id, edge, form, value, miss)
}
