//! The pieces of an own data field's store that a flat run of arena values shares: the guards
//! that admit the write and the write itself. `store.rs` composes them with the packed arm; a
//! stencil row for a site whose storage the hint proved flat (`Form::Arena`) composes them
//! alone, so its slot operand is the run's byte offset and nothing else.

use super::narrow::packed_form;
use super::object::{arena_run, flat_run, inline_storage, recorded_shape, storage_form, Form, HALF_BYTES};
use super::region::Region;
use super::{Stage, Table};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::{Representation, Type};

/// The auxiliary word of the record at `record` once its exact shape is `shape` and its values
/// are plain (no extras, no narrow run); `None` (leaving at `miss`) otherwise. The word tells
/// the storage form by its form bits (`object::storage_form`).
///
/// A `hinted` store's `shape` is a lane record's hint, a layout id (`Object::hint_shape`):
/// one compare of the record's own word proves the shape and the form, which the id names, so
/// the values are plain with no test, and the word is the id. Any other `shape` is a recorded
/// one, which the word equals without its form bits, and the extras bit is tested apart.
#[inline(always)]
fn admitted_word<S: Stage>(stage: &mut S, record: S::Word, shape: u32, hinted: bool, miss: S::Miss) -> Option<S::Word> {
    if hinted {
        let id = super::ic::layout_id(stage, record);
        let same = { let shape = stage.shape_site(shape); stage.eq_imm(id, shape) };
        stage.guard(same, miss)?;
        // A hint of extras or a narrow run admits no store here: the form is the id's fact,
        // read once the id matched (a cleared hint names no layout).
        if super::ic::layout_form(shape) & (OBJECT.extras_form | OBJECT.narrow_form) != 0 {
            let never = stage.constant(0);
            let refused = stage.ne_imm(never, 0u32);
            stage.guard(refused, miss)?;
        }
        return Some(id);
    }
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let exact = recorded_shape(stage, word);
    let shape = stage.shape_site(shape);
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    // Neither extras nor a narrow run: the payload is a packed word or a wide run's start.
    let displaced = storage_form(stage, word, OBJECT.extras_form | OBJECT.narrow_form);
    let plain = stage.not(displaced);
    stage.guard(plain, miss)?;
    Some(word)
}

/// Whether a store admits in place: `specialized` is the dispatch's word that the run stores specialized (a run
/// that does not leaves), and the atom at the next site hole stays numbered. That no method cache names the atom
/// is a Stable fact the code holds (`held_atoms`): naming it retires the code, and a run already inside learns
/// that from the invalidation word its rows test after a call, so no bitset is read here.
#[allow(dead_code)] // the stencil rows' admission; the interpreter's stores hold no per-store test
#[inline(always)]
pub(crate) fn store_admitted<S: Stage>(stage: &mut S, specialized: S::Word, miss: S::Miss) -> Option<()> {
    let on = stage.ne_imm(specialized, 0u32);
    stage.guard(on, miss)?;
    let _atom = stage.site(0);
    Some(())
}

/// The type a store of `value` plans by: the narrower of `proven` (what the row proved of it) and
/// `stage.fact(value)` (what the stage holds), or `proven` where neither is the narrower one.
#[inline(always)]
pub(crate) fn store_fact<S: Stage>(stage: &S, value: S::Word, proven: Type) -> Type {
    let known = stage.fact(value);
    if known.is_subtype(proven) { known } else { proven }
}

/// The word a store writes into a slot of class `holds` (decision 4.1, R4-M1): a Number-class slot
/// (`repr` Float64) holds raw double bits, so an int32 word converts to its double (`Stage::raw_number`);
/// any other slot, and an untyped one (`None`), stores the word as it is.
#[inline(always)]
pub(crate) fn class_word<S: Stage>(stage: &mut S, value: S::Word, holds: Option<Type>) -> S::Word {
    match holds {
        Some(class) if crate::typed_ir::repr(class) == Representation::Float64 => stage.raw_number(value),
        _ => value,
    }
}

/// The payload word of the record at `record` (id `record_id`) once a store of `value` may
/// proceed: its exact shape is `shape` (`admitted_word`), the store gate admits the write and
/// its values are plain; `None` (leaving at `miss`) otherwise, changing nothing.
#[inline(always)]
pub(crate) fn admitted_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, hinted: bool,
    value: S::Word, miss: S::Miss) -> Option<S::Word> {
    admitted_word(stage, record, shape, hinted, miss)?;
    let fact = store_fact(stage, value, Type::ANY);
    gated_payload(stage, record, record_id, (value, fact), miss)
}

/// Continues where a store of `value`, which the frame proved of type `fact`, into record
/// `record_id` needs no barrier log. An immediate type (`Type::IMMEDIATE`) names no heap
/// record, so it asks only whether old records keep counts (`Stage::immediate_store_open`), under
/// which overwriting a reference still logs; any other value asks the stage's gate.
#[inline(always)]
pub(crate) fn gate_open<S: Stage>(stage: &mut S, record_id: S::Word, (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    if fact.is_subtype(Type::IMMEDIATE) {
        let open = stage.immediate_store_open();
        return stage.guard(open, miss);
    }
    let open = stage.store_gate(super::StagedRecordSpace::Object, record_id, value);
    stage.guard(open, miss)
}

/// `gate_open` of a store whose value the slot's type holds (`Stage::held_store_gate`): the
/// barrier's log alone, no slot observation.
#[inline(always)]
pub(crate) fn held_gate_open<S: Stage>(stage: &mut S, record_id: S::Word, (value, fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
    if fact.is_subtype(Type::IMMEDIATE) {
        let open = stage.immediate_store_open();
        return stage.guard(open, miss);
    }
    let open = stage.held_store_gate(super::StagedRecordSpace::Object, record_id, value);
    stage.guard(open, miss)
}

/// The store gate of a gated `value` into object record `id` over the slot's current value `old`: an int over
/// an int widens no observed slot type, whose join already holds `old` (tracking joins every live value and
/// every store), so only the barrier's log is asked (`Stage::held_store_gate`); any other store asks the whole
/// gate (`Stage::store_gate`), which refuses while slot types are tracked.
#[inline(always)]
pub(crate) fn int_over_int_gate<S: Stage>(stage: &mut S, (id, old): (S::Word, S::Word), value: S::Word) -> S::Cond {
    let old_int = super::value::is_int(stage, old);
    let new_int = super::value::is_int(stage, value);
    let same = stage.all(old_int, new_int);
    let held = stage.held_store_gate(super::StagedRecordSpace::Object, id, value);
    let whole = stage.store_gate(super::StagedRecordSpace::Object, id, value);
    stage.pick(same, held, whole)
}

/// The payload word of the record at `record` once the gate admits `value` (of `fact`) and its
/// values are plain (no extras); the exact shape is the caller's proof (`property_write::in_place`).
#[inline(always)]
pub(crate) fn gated_payload<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, value: (S::Word, Type),
    miss: S::Miss) -> Option<S::Word> {
    gate_open(stage, record_id, value, miss)?;
    Some(stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32))
}

/// The payload of the arena-form record `record` (`record_id`) for a tracked store of `value` at `slot`
/// through the hint `id`: the layout compare, then the slot owner's held type admitting `value`
/// (`held_holds`, which reads the owner tables of the hint's shape, so it runs only once the record
/// proved it), then the barrier's log alone (`held_gate_open`): a store the slot's type already holds
/// observes nothing, which the full gate (`gate_open`) refuses wherever slots are tracked.
#[inline(always)]
pub(crate) fn held_payload<S: Stage, const HELD: u8>(stage: &mut S, record: S::Word, record_id: S::Word, (id, slot): (u32, u16),
    value: S::Word, miss: S::Miss) -> Option<S::Word> {
    admitted_word(stage, record, id, true, miss)?;
    held_holds::<S, HELD>(stage, super::ic::layout_shape(id), slot, value, miss)?;
    let fact = store_fact(stage, value, Type::ANY);
    held_gate_open(stage, record_id, (value, fact), miss)?;
    Some(stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32))
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// Stores `value`, a word in `rep`'s encoding, at `slot` of the flat run the plain `payload` word
/// addresses; the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
pub(crate) fn store_flat<S: Stage>(stage: &mut S, payload: S::Word, slot: u16, (value, rep): (S::Word, Representation)) {
    let run = arena_run(stage, payload);
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    let address = stage.add_imm(run, offset);
    store_word(stage, address, value, rep);
}

/// `own_store` for a record whose values are a flat run (`Form::Arena`): the guards of
/// `admitted_payload` (the gate passes a `fact` proved immediate), then the run's slot; `None` (leaving at `miss`) where the record keeps
/// its values inline, changing nothing.
#[inline(always)]
pub(crate) fn own_flat_store<S: Stage>(stage: &mut S, record: S::Word, record_id: S::Word, shape: u32, slot: u16,
    (value, rep, fact): (S::Word, Representation, Type), miss: S::Miss) -> Option<()> {
    let word = admitted_word(stage, record, shape, false, miss)?;
    let fact = store_fact(stage, value, fact);
    let payload = gated_payload(stage, record, record_id, (value, fact), miss)?;
    let inline = storage_form(stage, word, OBJECT.inline_form);
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    store_flat(stage, payload, slot, (value, rep));
    Some(())
}

/// Proves the record at `record` of the exact shape `shape` with plain wide flat values in one compare:
/// its auxiliary word is the shape with no storage-form bit set (`recorded_shape`); `None`, leaving at
/// `miss`, otherwise. What the access of a receiver known to be a direct object runs (`base_region::proofs`).
#[inline(always)]
pub(crate) fn own_flat_known<S: Stage>(stage: &mut S, record: S::Word, shape: u32, miss: S::Miss) -> Option<()> {
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let shape = stage.shape_site(shape);
    let same = stage.eq_imm(word, shape);
    stage.guard(same, miss)
}

/// `value` written at flat `slot` of the record at `record`, which `own_flat_known` proved of the slot's
/// shape with plain flat values; the value is one the slot's observed type holds, so the store gate does not apply.
#[inline(always)]
pub(crate) fn proved_flat_store<S: Stage>(stage: &mut S, record: S::Word, slot: u16, (value, rep): (S::Word, Representation)) {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    store_flat(stage, payload, slot, (value, rep));
}

/// `value`, a word in `rep`'s encoding, written at `slot`, a word (the shapes' storage length or a cache
/// entry's slot), of the plain object at `record` whose `payload` word the caller read and proved plain,
/// with its values in `form`:
/// a half of the payload word for a packed object, which needs the value's narrow form, or the arena
/// run's slot. A form the site proved is guarded, not decoded; `None` (leaving at `miss`, nothing
/// written) where the object is in the other form, a packed slot cannot hold the value, or the
/// run is narrow (`object::narrow_run`), whose halves take no wide value.
#[inline(always)]
pub(crate) fn store_slot<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, slot: S::Word, form: Form, (value, rep): (S::Word, Representation),
    miss: S::Miss) -> Option<()> {
    debug_assert!(rep != Representation::Float64 || form == Form::Arena, "a packed slot holds a narrow value, never a raw double");
    let inline = inline_storage(stage, record);
    match form {
        Form::Packed => stage.guard(inline, miss)?,
        Form::Arena => {
            let flat = stage.not(inline);
            stage.guard(flat, miss)?;
        }
        Form::Either => {}
    }
    // A packed slot needs the value's narrow form; a flat one takes any value.
    let narrow = if form == Form::Arena { stage.constant(0) } else { packed_form(stage, inline, value, store_fact(stage, value, Type::ANY), miss)? };
    stage.choose_leaving(inline, #[inline(always)] |stage| {
        let half = stage.mul_imm(slot, HALF_BYTES);
        let address = stage.add(record, half);
        stage.store_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32, narrow);
        Some(stage.constant(0))
    }, #[inline(always)] move |stage| {
        let run = flat_run(stage, payload, miss)?;
        let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
        let address = stage.add(run, offset);
        store_word(stage, address, value, rep);
        Some(stage.constant(0))
    })?;
    Some(())
}

/// The slot types a tracked store row is installed for (`SlotType`'s discriminants, which
/// `slot_type.rs` asserts): the held type the row compares the slot's owner with, and so the one
/// class test it makes of the value. `HELD_NONE` names no row: the store is the VM's.
pub const HELD_NONE: u8 = 0;
pub const HELD_INT32: u8 = 1;
pub const HELD_OBJECT: u8 = 6;
pub const HELD_OBJECT_OR_NULL: u8 = 7;
pub const HELD_TAGGED: u8 = 12;

/// Whether a tracked store through the hint `id` (`StorePlan::Hinted`) is an arena-form store a row
/// makes in place: not narrow, the layout's form is the arena's, and it carries no constant mark (a
/// marked object's store retires the code that holds its values, the VM's store). A record with no
/// hint yet (`NO_FIELD_RECEIVER`, past the layout table) names no layout: its store is the VM's.
#[inline(always)]
pub(crate) fn hinted_arena(id: u32, narrow: bool) -> bool {
    !narrow && (id as usize) < crate::heap::layouts::LAYOUT_CAPACITY && super::ic::layout_form(id) == 0
        && crate::heap::layouts::auxiliary(id) & crate::value_vec::CONSTANT_FORM == 0
}

/// Continues where slot `slot` of the exact shape `shape` still holds the type `HELD` the row was
/// installed for (the slot owner tables: the shape's chain, the slot's owner, the owner's type) and
/// `value` is of that type, so the store widens nothing and observes nothing: one compare of the
/// owner's type and the one class test `HELD` names (none for a tagged slot). A slot whose type
/// widened since misses, and the plain row stores and installs the row of its type now. The edge's
/// fact checked where the data changes: nothing is joined on a store that holds its slot's class.
///
/// The tables are read only by a stage that stores (`Stage::SLOT_TABLES`), which holds its heap.
#[inline(always)]
pub(crate) fn held_holds<S: Stage, const HELD: u8>(stage: &mut S, shape: u32, slot: u16, value: S::Word, miss: S::Miss) -> Option<()> {
    let at = Region::HeapMeta.mutable();
    let chains = stage.table(Table::SlotChains);
    let chain_at = stage.add_imm(chains, u64::from(shape) * 4);
    let chain = stage.load_u32(at, chain_at, 0);
    let owners = stage.table(Table::ChainOwners);
    let slot_index = stage.add_imm(chain, u64::from(slot));
    let owner_offset = stage.mul_imm(slot_index, 4u64);
    let owner_at = stage.add(owners, owner_offset);
    let owner = stage.load_u32(at, owner_at, 0);
    let types = stage.table(Table::OwnerTypes);
    let held_at = stage.add(types, owner);
    let held = stage.load_u8(at, held_at, 0);
    let same = stage.eq_imm(held, u64::from(HELD));
    stage.guard(same, miss)?;
    let admitted = if const { HELD == HELD_TAGGED } { return Some(()) }
        else if const { HELD == HELD_INT32 } { super::value::is_int(stage, value) }
        else if const { HELD == HELD_OBJECT } { super::value::is_direct_object(stage, value) }
        else {
            let object = super::value::is_direct_object(stage, value);
            let null = super::value::is_null(stage, value);
            stage.any(object, null)
        };
    stage.guard(admitted, miss)
}

/// `value`, a word in `rep`'s encoding, written at the arena slot `address`.
#[inline(always)]
pub(crate) fn store_word<S: Stage>(stage: &mut S, address: S::Word, value: S::Word, rep: Representation) {
    match rep {
        Representation::Float64 => stage.store_f64(Region::ObjectSlots.mutable(), address, 0, value),
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.store_u64(Region::ObjectSlots.mutable(), address, 0, value),
    }
}
