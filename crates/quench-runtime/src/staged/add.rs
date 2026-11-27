//! A property add as a staged operation: an extensible ordinary object of a proved exact shape
//! takes a value at the next slot and moves to the shape the transition names. The interpreter
//! runs it on `Concrete`; a stencil instantiates the same row.
//!
//! A shape's transitions are append-only edges, (shape, atom, attributes) to (shape', slot): an
//! edge, once made, never changes, so a resolved one is a Stable fact that needs no dependency of
//! its own. What the object proves at each use is its before-layout (one guard of its layout id,
//! `heap::layouts`, which holds the shape, the prototype link and the integrity bits the site
//! recorded: extensible, not frozen), its plain storage, and the store gate; the add moves it to
//! the after-layout (`Object::layout_with_shape`), one id store. Whether the slot fits is a
//! fact of the layout, not a read: a packed object has a free inline half below the inline
//! count, and an arena range's capacity derives from its length alone (`range_has_room`), so the
//! planner (`add_plan`) decides which form can take the add and the row never grows storage. Any
//! other add is the VM's, which allocates and grows.

use super::region::Region;
use super::entry::direct_record;
use super::object::{Form, plain_payload, proto_link};
use crate::typed_ir::Representation;
use super::slot_store::store_slot;
use super::{StagedRecordSpace, Stage};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// One resolved transition as the words a stage computes on: the layout id the object must hold,
/// the layout id it moves to, and the slot the value takes, which is the storage length of the
/// first's shape.
#[derive(Clone, Copy)]
pub struct Edge<W> {
    pub before: W,
    pub after: W,
    pub slot: W,
}

/// What the object at `record` (id `record_id`) must hold for the add of `value`: exactly the
/// before-layout (whose shape excludes a dictionary and exotic access and which holds no integrity
/// bit: the site recorded it from an extensible, unfrozen object), plain storage (no extras, so it never was a prototype and no
/// shape reader or method cache can name it) and an open store gate (no slot representation
/// observes the write, no barrier log is due). Answers the payload word the storage reads from.
#[inline(always)]
pub fn admitted<S: Stage>(stage: &mut S, space: StagedRecordSpace, record: S::Word, record_id: S::Word, before: S::Word,
    value: S::Word, miss: S::Miss) -> Option<S::Word> {
    admitted_gated::<S, false>(stage, space, record, record_id, before, value, miss)
}

/// `admitted`, gated by the barrier's log alone where `HELD` (`Stage::held_store_gate`): the
/// slot's observed type already holds the value, so slot observation would see nothing of it.
#[inline(always)]
fn admitted_gated<S: Stage, const HELD: bool>(stage: &mut S, space: StagedRecordSpace, record: S::Word, record_id: S::Word, before: S::Word,
    value: S::Word, miss: S::Miss) -> Option<S::Word> {
    let layout = stage.load_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32);
    let moved = stage.sub(layout, before);
    let same = stage.eq_imm(moved, 0u32);
    stage.guard(same, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    let gate = if HELD { stage.held_store_gate(space, record_id, value) } else { stage.store_gate(space, record_id, value) };
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
pub fn add_property<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, edge: Edge<S::Word>,
    form: Form, value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted(stage, StagedRecordSpace::Object, record, record_id, edge.before, value, miss)?;
    store_slot(stage, record, payload, edge.slot, form, (value, Representation::Tagged), miss)?;
    stage.store_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32, edge.after);
    Some(())
}

/// `add_property` of a value the after-shape's slot observation already holds
/// (`Heap::add_observation_holds`): the barrier's log alone gates the write (`admitted_gated`).
#[inline(always)]
pub fn add_property_held<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, edge: Edge<S::Word>,
    form: Form, value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = admitted_gated::<S, true>(stage, StagedRecordSpace::Object, record, record_id, edge.before, value, miss)?;
    store_slot(stage, record, payload, edge.slot, form, (value, Representation::Tagged), miss)?;
    stage.store_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32, edge.after);
    Some(())
}

/// `add_property` for the object word `object` of the record table at `records`, the way a
/// compiled site holds it: the word is a direct object (else `None`, leaving at `miss`), and where
/// `link` is given (an add the prototype chain can intercept, whose chain proof the code holds as
/// a Stable fact for exactly that prototype) the object's prototype link is it. The chain's own
/// shapes are not read: a change to the layout of a prototype on it retires the code
/// (`Heap::subscribe_prototype_chain`), so only what varies per receiver, its link, is tested.
#[inline(always)]
pub fn site_add<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, (edge, link): (Edge<S::Word>, Option<S::Word>),
    form: Form, value: S::Word, miss: S::Miss) -> Option<()> {
    let direct = super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    // A live cell's class is its layout: the before-class fixes the link `link` proves, so no link test is made.
    let granule = super::value::direct_object_id(stage, object);
    let cell = super::object::cell_at_granule(stage, granule);
    let class = super::object::cell_class(stage, cell);
    let live = super::object::class_is_live(stage, class);
    stage.choose_leaving(live, #[inline(always)] |stage| {
        add_cell_property(stage, (granule, cell, class), edge, value, miss)?;
        Some(stage.constant(0))
    }, #[inline(always)] |stage| {
        let present = stage.ne_imm(records, 0u32);
        stage.guard(present, miss)?;
        let (id, record) = direct_record(stage, records, object, miss)?;
        if let Some(link) = link {
            let held = proto_link(stage, record);
            let moved = stage.sub(held, link);
            let same = stage.eq_imm(moved, 0u32);
            stage.guard(same, miss)?;
        }
        add_property(stage, record, id, edge, form, value, miss)?;
        Some(stage.constant(0))
    })?;
    Some(())
}

/// The add of `value` at the slot of `edge` to the live class cell `cell` at `granule`, in place: it holds the
/// before-class (its header is the layout word, so one compare proves the shape, link and flags), the field
/// fits its region's capacity, and the store gate admits the write (the class space's barrier, as
/// `Heap::class_cell_add`). The field is stored before the header takes the after-class, so no reader sees the
/// new class without its slot. `None` (leaving at `miss`, before any write) otherwise, where the VM departs the
/// cell or grows nothing. The after-class is the transition's, which keeps the before-class's link and flags.
#[inline(always)]
fn add_cell_property<S: Stage>(stage: &mut S, (granule, cell, class): (S::Word, S::Word, S::Word), edge: Edge<S::Word>,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let moved = stage.sub(class, edge.before);
    let same = stage.eq_imm(moved, 0u32);
    stage.guard(same, miss)?;
    // The region's capacity is `2^region - 1`, so `slot < capacity` is `slot + 1 < 2^region`, which is `(slot + 1) >> region == 0`.
    let region = stage.shr_imm(granule, u64::from(super::object_encoding::REGION_SHIFT));
    let next = stage.add_imm(edge.slot, 1u32);
    let beyond = stage.shr(next, region);
    let fits = stage.eq_imm(beyond, 0u32);
    stage.guard(fits, miss)?;
    // Always the whole gate: the cell space's gate refuses while slot representations are tracked, so a stored
    // word is never a Number-slot's raw double that needs converting (`slot_store::class_word`).
    let open = stage.store_gate(StagedRecordSpace::Class, granule, value);
    stage.guard(open, miss)?;
    super::entry::class_store(stage, cell, edge.slot, value);
    stage.store_u32(Region::ShapeWord.mutable(), cell, super::layout_offsets::NATIVE_CLASS_CELL_LAYOUT.class_offset as i32, edge.after);
    Some(())
}
