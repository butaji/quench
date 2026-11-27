//! A data field's read through the IC site's entry record (form b), own or held by the
//! receiver's prototype: the record's shape, the holder and the slot are read at run time from
//! the entry the site table keeps, not held as per-site operands. A specializer's only hole is
//! the entry's address, so the code never changes when the site's feedback does (the entry
//! record is the one feedback authority).

use super::object::{Form, arena_run, decode_narrow, extras_pointer, has_extras, inline_storage, plain_payload};
use crate::typed_ir::Representation;
use super::slot_store::store_slot;
use super::Stage;
use super::region::Region;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;

// The entry's words are complemented by an all-ones word (a difference from it) for the shapes
// and by zero for the slot: the guards below read them without a xor.
const _: () = assert!(ENTRY.receiver_complement == u32::MAX && ENTRY.slot_complement == 0);

/// Guards that the shape of the object record at `record` is the one the entry at `entry`
/// stores in its word at `offset`, complemented by `complement`: the stored word and the
/// record's shape complemented differ by zero.
#[inline(always)]
fn shape_matches<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, (offset, complement): (usize, u32), miss: S::Miss) -> Option<()> {
    let exact = super::object::exact_shape(stage, record);
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, offset as i32);
    let complement = stage.constant(u64::from(complement));
    let wanted = stage.sub(complement, exact);
    let difference = stage.sub(stored, wanted);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// Whether the entry at `entry` is an own entry: its holder word is the stored own holder.
#[inline(always)]
fn own_entry<S: Stage>(stage: &mut S, entry: S::Word) -> S::Cond {
    let stored = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.holder_offset as i32);
    stage.eq_imm(stored, u64::from(ENTRY.holder_complement ^ ENTRY.own_holder))
}

/// The slot the entry at `entry` names.
#[inline(always)]
pub fn entry_slot<S: Stage>(stage: &mut S, entry: S::Word) -> S::Word {
    let slot = stage.load_u32(Region::HeapMeta.mutable(), entry, ENTRY.slot_offset as i32);
    stage.and_imm(slot, u64::from(u16::MAX))
}

/// The value at `slot` of the object record at `record`, whose shape the caller proved: `None`
/// (leaving at `miss`) where its values are not plain (extras displaced them). Packed values
/// decode from the record, flat ones read from the arena run.
#[inline(always)]
pub(super) fn plain_value<S: Stage>(stage: &mut S, record: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    plain_slot(stage, record, payload, slot, miss)
}

/// The value at `slot` of the record at `record` whose payload word `payload` holds plain
/// storage (`plain_payload`): packed values decode from the record, flat ones read from the arena
/// run; `None` (leaving at `miss`) for a narrow run (`object::flat_run`).
#[inline(always)]
fn plain_slot<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let inline = inline_storage(stage, record);
    stage.choose_leaving(inline, #[inline(always)] |stage| {
        let half = stage.mul_imm(slot, std::mem::size_of::<u32>() as u64);
        let address = stage.add(record, half);
        let narrow = stage.load_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32);
        Some(decode_narrow(stage, narrow))
    }, #[inline(always)] |stage| {
        let run = super::object::flat_run(stage, payload, miss)?;
        let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
        let address = stage.add(run, offset);
        Some(stage.load_u64(Region::ObjectSlots.mutable(), address, 0))
    })
}

/// A field read through two cache entries: the first names a value the second entry's site reads from.
#[inline(always)]
pub(crate) fn site_chain<S: Stage>(stage: &mut S, (entries, records, functions): (S::Word, S::Word, S::Word), object: S::Word, (first, second): (S::Imm, S::Imm), miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, (entries, records), miss)?;
    let middle = site_read_in(stage, (entries, records, functions), object, first, miss)?;
    site_read_in(stage, (entries, records, functions), middle, second, miss)
}

/// Stores `value` at `slot` of the object record at `record`, whose shape the caller proved and
/// whose store gate (`Stage::store_gate`) admitted the write: a packed slot takes the value's
/// narrow form, a flat one the value (`store_slot`); `None` (leaving at `miss`) where its values are
/// not plain or a packed slot cannot hold the value, nothing written.
#[inline(always)]
fn plain_store<S: Stage>(stage: &mut S, record: S::Word, slot: S::Word, value: S::Word, miss: S::Miss) -> Option<()> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    store_slot(stage, record, payload, slot, Form::Either, (value, Representation::Tagged), miss)
}

/// A store of `value` through the own cache entry `offset` bytes into the table of `tables`: the
/// record's shape is the entry's, the gate admits the write where it is `gated`, and the slot is
/// plain. A value the caller proved immediate and of the slot's observed type is not `gated`: it
/// needs no barrier log and widens no observation (`slot_store::gate_open`).
///
/// That proof names one shape, `proven`: the shape whose slot type the caller read. The entry is the
/// site's live primary entry, which the interpreter rewrites to the latest shape it resolved
/// (`Vm::record_field_cache`), so an ungated store also guards that the record is of the `proven`
/// shape: a slot of another shape may not admit the value, and an ungated write there would leave
/// its slot type unjoined (`ValueArena::join_slot_owner`) under code that trusts it. A gated store
/// passes `None`: the gate itself refuses every store that may widen a tracked slot type (`slot_store::int_over_int_gate`).
#[inline(always)]
pub(crate) fn site_store<S: Stage>(stage: &mut S, tables: (S::Word, S::Word), object: S::Word, offset: S::Imm,
    (value, gated, proven): (S::Word, bool, Option<S::Imm>), miss: S::Miss) -> Option<()> {
    site_tables(stage, tables, miss)?;
    let (id, record, entry) = site_record(stage, tables, object, offset, miss)?;
    let own = own_entry(stage, entry);
    stage.guard(own, miss)?;
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    if let Some(shape) = proven {
        let exact = super::object::exact_shape(stage, record);
        let same = stage.eq_imm(exact, shape);
        stage.guard(same, miss)?;
    }
    let slot = entry_slot(stage, entry);
    let open = if gated {
        let old = plain_value(stage, record, slot, miss)?;
        super::slot_store::int_over_int_gate(stage, (id, old), value)
    } else { stage.immediate_store_open() };
    stage.guard(open, miss)?;
    plain_store(stage, record, slot, value, miss)
}

/// The value the own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's, the entry
/// names a prototype holder (or is empty), or the record's values are not plain. The entry
/// stores each of its words complemented against the empty entry, so a holder of zero and a
/// shape equal to the record's shape complemented are the guards.
// Read by the b-jit stencil generator (thread 7) once it derives GetField from this row; the
// tests run it on `Concrete` meanwhile.

/// The value at `slot` of the prototype record `holder`, whose shape the caller proved: a
/// prototype keeps its values in its extras (`Object::mark_as_prototype`), wide and inline or as
/// a run in the arena, so `None` (leaving at `miss`) where it has none, or where an inline value
/// is past the inline ones.
#[inline(always)]
fn holder_value<S: Stage>(stage: &mut S, holder: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(Region::Payload.mutable(), holder, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let pointer = extras_pointer(stage, payload);
    let inline = inline_storage(stage, holder);
    let limit = stage.constant(OBJECT.inline_property_count as u64);
    let below = stage.ult(slot, limit);
    let flat = stage.not(inline);
    let fits = stage.any(flat, below);
    stage.guard(fits, miss)?;
    let run = stage.choose(inline, #[inline(always)] |stage| stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64), #[inline(always)] |stage| {
        let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
        arena_run(stage, start)
    });
    let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
    let address = stage.add(run, offset);
    Some(stage.load_u64(Region::ObjectSlots.mutable(), address, 0))
}

/// The value an own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's. A record whose
/// extras displaced its values (an intrinsic such as `Math`, a prototype) answers from them
/// (`holder_value`). The entry stores each of its words complemented against the empty entry, so
/// a shape equal to the record's shape complemented is the guard.
#[inline(always)]
fn own_entry_value<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    let slot = entry_slot(stage, entry);
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.choose_leaving(plain, #[inline(always)] |stage| plain_slot(stage, record, payload, slot, miss),
        #[inline(always)] |stage| holder_value(stage, record, slot, miss))
}

/// The value a prototype-held entry at `entry` names for the object record at `record`
/// (`Vm::ordinary_holder_entry_value`): the receiver's shape is the entry's, its prototype link
/// names an object record of the entry's holder shape, and the value is the holder's at the
/// entry's slot. `None` (leaving at `miss`) for any other receiver, link or holder, so a changed
/// prototype is a miss.
#[inline(always)]
fn holder_entry_value<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    let link = super::object::proto_link(stage, record);
    let id = super::object::proto_object_record_id(stage, link, miss)?;
    let holder = super::object::object_record_at(stage, records, id);
    shape_matches(stage, holder, entry, (ENTRY.holder_offset, ENTRY.holder_complement), miss)?;
    let slot = entry_slot(stage, entry);
    holder_value(stage, holder, slot, miss)
}

/// Guards that the field cache table `entries` (the running program's, as
/// `Vm::program_field_entries` hands it) and the object record table `records` are present.
#[inline(always)]
fn site_tables<S: Stage>(stage: &mut S, (entries, records): (S::Word, S::Word), miss: S::Miss) -> Option<()> {
    for base in [entries, records] {
        let present = stage.ne_imm(base, 0u32);
        stage.guard(present, miss)?;
    }
    Some(())
}

/// The id and object record that `object` names in the record table at `records`, whose presence
/// the caller proved: `None` (leaving at `miss`) where the word is no direct object or its object is live.
#[inline(always)]
pub(super) fn direct_record<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let direct = super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = super::object::object_record_id_of(stage, object, miss)?;
    Some((id, super::object::object_record_at(stage, records, id)))
}

/// The record, its id and the cache entry `offset` bytes into the table `entries` that a site
/// access of `object` names, whose presence the caller proved (`site_tables`): `None` (leaving at
/// `miss`) where the word is no direct object.
#[inline(always)]
fn site_record<S: Stage>(stage: &mut S, (entries, records): (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss)
    -> Option<(S::Word, S::Word, S::Word)> {
    let (id, record) = direct_record(stage, records, object, miss)?;
    let entry = stage.add_imm(entries, offset);
    Some((id, record, entry))
}

/// The folded value of a prototype-held read (`PrototypeProof::Linked`): the receiver `record` has
/// exact shape `shape` and prototype link `link`, so its holder is the one the read was folded
/// against and `value` is the folded constant (a Stable fact a code's live check holds). `None`
/// (leaving at `miss`) for any other receiver. Shape and link are the caller's values, compared here.
#[inline(always)]
pub(crate) fn held_constant<S: Stage>(stage: &mut S, _receiver: super::Linked<S::Word>, value: u64) -> S::Word {
    stage.constant(value)
}

/// The receiver check of a prototype-held read (`held_constant`, `guard::prototype`): the record `record` has
/// exact shape `shape` and prototype link `link`, else the caller leaves at `miss`. The one definition of the pair.
#[inline(always)]
pub(crate) fn shape_link_matches<S: Stage>(stage: &mut S, record: S::Word, (shape, link): (u64, u64), miss: S::Miss) -> Option<super::Linked<S::Word>> {
    let exact = super::object::exact_shape(stage, record);
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    let prototype = super::object::proto_link(stage, record);
    let same = stage.eq_imm(prototype, link);
    stage.guard(same, miss)?;
    Some(super::Linked::of_guard(record))
}

/// A field read through the cache entry `offset` bytes into the table `entries`, of the object
/// `object` names in the record table at `records`, whose presence the caller proved
/// (`site_tables`): the data value the entry holds, own or held by the receiver's prototype, or
/// `None` (leaving at `miss`) where the word is no direct object or the entry is no entry for
/// the record (`own_entry_value`, `holder_entry_value`).
#[inline(always)]
fn site_read_in<S: Stage>(stage: &mut S, (entries, records, functions): (S::Word, S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    let record = readable_record(stage, (records, functions), object, miss)?;
    let entry = stage.add_imm(entries, offset);
    let own = own_entry(stage, entry);
    stage.choose_leaving(own, #[inline(always)] |stage| own_entry_value(stage, record, entry, miss),
        #[inline(always)] |stage| holder_entry_value(stage, records, record, entry, miss))
}

/// The record whose own properties a read of `object` takes: a direct object's in the record table
/// `records`, or, where the function record table `functions` is present, a function's `Object` (at
/// `NATIVE_FUNCTION_LAYOUT.object_offset`), which a cache entry keys by its shape as any object's.
/// `None` (leaving at `miss`) for any other word.
#[inline(always)]
fn readable_record<S: Stage>(stage: &mut S, (records, functions): (S::Word, S::Word), object: S::Word, miss: S::Miss) -> Option<S::Word> {
    let direct = super::value::is_direct_object(stage, object);
    let id = super::value::direct_object_id(stage, object);
    stage.choose_leaving(direct, #[inline(always)] |stage| super::object::object_record_of(stage, records, object, miss), #[inline(always)] |stage| {
        let function = super::value::is_direct_function(stage, object);
        stage.guard(function, miss)?;
        let present = stage.ne_imm(functions, 0u32);
        stage.guard(present, miss)?;
        let offset = stage.mul_imm(id, super::layout_offsets::NATIVE_FUNCTION_LAYOUT.record_size as u64);
        let record = stage.add(functions, offset);
        Some(stage.add_imm(record, super::layout_offsets::NATIVE_FUNCTION_LAYOUT.object_offset as u64))
    })
}

/// `site_read_in` of an own entry alone: `None` (leaving at `miss`) for an entry that names a
/// prototype holder, which the interpreter's own-field handlers (`Vm::primary_own_field_hit`) do
/// not answer, since they keep the receiver's slot as their hint.
#[inline(always)]
fn site_own_in<S: Stage>(stage: &mut S, tables: (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    let (_, record, entry) = site_record(stage, tables, object, offset, miss)?;
    let own = own_entry(stage, entry);
    stage.guard(own, miss)?;
    own_entry_value(stage, record, entry, miss)
}

/// `site_read` of an own entry alone (`site_own_in`).
#[inline(always)]
pub(crate) fn site_own_read<S: Stage>(stage: &mut S, tables: (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, tables, miss)?;
    site_own_in(stage, tables, object, offset, miss)
}

/// `site_chain` of own entries alone (`site_own_in`).
#[inline(always)]
pub(crate) fn site_own_chain<S: Stage>(stage: &mut S, tables: (S::Word, S::Word), object: S::Word, (first, second): (S::Imm, S::Imm), miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, tables, miss)?;
    let middle = site_own_in(stage, tables, object, first, miss)?;
    site_own_in(stage, tables, middle, second, miss)
}

/// A field read through the cache entry `offset` bytes into the field cache table `entries`
/// of the object `object` names in the record table at `records`: the data value the entry
/// holds (`site_read_in`), or `None` (leaving at `miss`) where either table is absent too. The
/// offset is the row's only per-site operand, so a site is read as the interpreter's cached
/// read reads it.
#[inline(always)]
pub(crate) fn site_read<S: Stage>(stage: &mut S, (entries, records, functions): (S::Word, S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    site_tables(stage, (entries, records), miss)?;
    site_read_in(stage, (entries, records, functions), object, offset, miss)
}

/// A nested lookup's chain of two reads, `object.first.second` (`FieldLookup::Site`): the value
/// the first entry reads is the object the second reads from, and the tables are proved once.
#[inline(always)]
pub(crate) fn entry_word<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> S::Word {
    stage.load_site(Region::LoopEntry.readonly(), entry, offset)
}

/// A store of `value` through the cache entry `offset` bytes into the field cache table `entries`
/// of the object `object` names in the record table at `records`: where the entry is an own entry
/// for the record's exact shape, its values are plain and the store gate admits the write, the
/// value goes to the entry's slot (`plain_store`). `None` (leaving at `miss`) otherwise, or where
/// either table is absent or the word is no direct object, nothing written. Whether the program
/// may store in place at all (a specialized run, no method cache naming the field) is the
/// caller's.
#[inline(always)]
pub(crate) fn entry_backing<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> (S::Word, S::Word) {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let address = stage.add_imm(entry, offset);
    let values = stage.load_table(Region::LoopEntry.readonly(), address);
    let length = stage.load_u32(Region::Elements.mutable(), values, ARRAY.header_length_offset as i32 - ARRAY.header_values_offset as i32);
    // The entry proved a wide backing; one the VM's store has since turned double in place
    // (`ArrayElements::replace_in_place`) flags its length word (`ENCODED_LEN`), and its words
    // are no `Value`s: the view then reads as empty, so every access leaves at its bounds test.
    let encoded = stage.ugt_imm(length, u64::from(!crate::heap::ENCODED_LEN));
    let empty = stage.constant(0);
    (values, stage.select(encoded, empty, length))
}

/// The interpreter's `site_read` of `object` through the stored field cache entry `entry` (at
/// byte 0 of its table), over the heap's own tables.
#[cfg(test)]
fn concrete_read_entry(heap: &crate::heap::Heap, object: crate::Value, entry: &[u32; 4]) -> Option<crate::Value> {
    let records = heap.object_record_base()? as u64;
    // SAFETY: a stored entry, and the heap's live record table and arena.
    unsafe { super::ic::concrete_site_read(heap, (entry.as_ptr() as u64, records), object, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Heap, Object};
    use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;
    use crate::Value;

    /// A field cache entry as the site table stores it: each word complemented.
    fn stored(shape: u32, holder: u32, slot: u32) -> [u32; 4] {
        let mut words = [0; 4];
        words[ENTRY.receiver_offset / 4] = shape ^ ENTRY.receiver_complement;
        words[ENTRY.holder_offset / 4] = holder ^ ENTRY.holder_complement;
        words[ENTRY.slot_offset / 4] = slot ^ ENTRY.slot_complement;
        words
    }

    fn object_of(heap: &mut Heap, count: usize) -> (Value, u32) {
        for shape in 1..=count as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc_object(Object::new(Value::NULL));
        for shape in 1..=count as u32 {
            heap.property_add(object, Value::integer(shape as i32 * 10), shape);
        }
        let shape = heap.object(object).unwrap().exact_shape();
        (object, shape)
    }

    #[test]
    fn an_entry_read_matches_the_hinted_read_in_both_storage_forms_and_misses_on_the_rest() {
        let mut heap = Heap::new();
        let count = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
        let (object, shape) = object_of(&mut heap, count);
        let record = heap.object(object).unwrap().record_only().unwrap();
        for slot in 0..count as u16 {
            let entry = stored(shape, ENTRY.own_holder, u32::from(slot));
            let by_entry = concrete_read_entry(&heap, object, &entry);
            // SAFETY: the hint's shape and slot are the record's own.
            let by_hint = unsafe { crate::staged::concrete_own_field_hit(record, &heap, shape, slot) };
            assert_eq!(by_entry, by_hint, "slot {slot}");
            assert_eq!(by_entry, Some(Value::integer(10 * (i32::from(slot) + 1))));
        }
        let read = |entry: [u32; 4]| concrete_read_entry(&heap, object, &entry);
        assert_eq!(read(stored(shape + 1, ENTRY.own_holder, 0)), None, "another shape");
        assert_eq!(read(stored(shape, 7, 0)), None, "a holder entry");
        assert_eq!(read([0; 4]).is_some(), shape == ENTRY.receiver_complement, "the empty entry names no shape");
    }

    /// A holder entry reads the prototype's slot through the receiver's link, in both of a
    /// prototype's storage forms, and misses where the receiver's shape, its prototype or the
    /// holder's shape is not the entry's.
    #[test]
    fn a_site_read_follows_a_holder_entry_to_the_prototype_and_misses_on_any_other() {
        let mut heap = Heap::new();
        let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
        for shape in 1..=wide as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let shape_of = |heap: &Heap, object: Value| heap.object(object).unwrap().exact_shape();
        let stray = heap.alloc_object(Object::new(Value::NULL));
        for count in [crate::value_vec::INLINE_PROPERTY_COUNT, wide] {
            let prototype = heap.alloc_object(Object::new(Value::NULL));
            for shape in 1..=count as u32 {
                heap.property_add(prototype, Value::integer(70 + shape as i32), shape);
            }
            heap.object_record_mut(prototype).unwrap().mark_as_prototype();
            let receiver = heap.alloc_object(Object::new(prototype));
            let (holder_shape, receiver_shape) = (shape_of(&heap, prototype), shape_of(&heap, receiver));
            let records = heap.object_record_base().expect("object records exist") as u64;
            let read = |object: Value, entry: [u32; 4]| {
                let mut stage = crate::staged::Concrete::over(&heap);
                site_read(&mut stage, (entry.as_ptr() as u64, records, 0), object.to_bits(), 0u64, ()).map(Value::from_bits)
            };
            for slot in 0..count as u32 {
                assert_eq!(read(receiver, stored(receiver_shape, holder_shape, slot)), Some(Value::integer(71 + slot as i32)), "{count} values, slot {slot}");
            }
            assert_eq!(read(receiver, stored(receiver_shape, holder_shape + 1, 0)), None, "another holder shape");
            assert_eq!(read(receiver, stored(receiver_shape + 1, holder_shape, 0)), None, "another receiver shape");
            assert_eq!(read(stray, stored(shape_of(&heap, stray), holder_shape, 0)), None, "a receiver of another prototype");
            assert_eq!(read(Value::integer(1), stored(receiver_shape, holder_shape, 0)), None, "no object");
        }
    }

    /// A chain reads the second entry's field of the object the first entry reads, and misses
    /// where the first value is no object.
    #[test]
    fn a_site_chain_reads_the_second_field_of_the_first_value() {
        let mut heap = Heap::new();
        heap.register_property_shape(1, 1, None);
        let leaf = heap.alloc_object(Object::new(Value::NULL));
        heap.property_add(leaf, Value::integer(9), 1);
        let (outer, plain) = (heap.alloc_object(Object::new(Value::NULL)), heap.alloc_object(Object::new(Value::NULL)));
        heap.property_add(outer, leaf, 1);
        heap.property_add(plain, Value::integer(4), 1);
        let shape = heap.object(leaf).unwrap().exact_shape();
        let entries = [stored(shape, ENTRY.own_holder, 0), stored(shape, ENTRY.own_holder, 0)];
        let records = heap.object_record_base().expect("object records exist") as u64;
        let read = |object: Value| {
            let mut stage = crate::staged::Concrete::over(&heap);
            site_chain(&mut stage, (entries.as_ptr() as u64, records, 0), object.to_bits(), (0u64, ENTRY.entry_size as u64), ()).map(Value::from_bits)
        };
        assert_eq!(read(outer), Some(Value::integer(9)));
        assert_eq!(read(plain), None, "an integer has no fields");
    }

    /// A store through an own entry writes the entry's slot in both storage forms, a value a
    /// packed slot cannot hold or an entry that is not the record's own leaves, and a leave
    /// writes nothing.
    #[test]
    fn a_site_store_writes_the_entry_slot_and_leaves_without_writing_on_any_miss() {
        for count in [crate::value_vec::INLINE_PROPERTY_COUNT, crate::value_vec::INLINE_PROPERTY_COUNT + 2] {
            let mut heap = Heap::new();
            let (object, shape) = object_of(&mut heap, count);
            let records = heap.object_record_base().expect("object records exist") as u64;
            let store = |heap: &Heap, entry: [u32; 4], value: Value| {
                let mut stage = crate::staged::Concrete::over(heap);
                site_store(&mut stage, (entry.as_ptr() as u64, records), object.to_bits(), 0u64, (value.to_bits(), true, None), ()).is_some()
            };
            let read = |heap: &Heap, entry: [u32; 4]| concrete_read_entry(heap, object, &entry);
            for slot in 0..count as u32 {
                let entry = stored(shape, ENTRY.own_holder, slot);
                assert!(store(&heap, entry, Value::integer(-7)), "{count} values, slot {slot}");
                assert_eq!(read(&heap, entry), Some(Value::integer(-7)));
                let before = read(&heap, entry);
                assert_eq!(store(&heap, entry, Value::number(1.5)), count > crate::value_vec::INLINE_PROPERTY_COUNT, "a double needs a flat slot");
                assert_eq!(read(&heap, entry) == before, count <= crate::value_vec::INLINE_PROPERTY_COUNT);
            }
            let entry = stored(shape, ENTRY.own_holder, 0);
            let before = read(&heap, entry);
            assert!(!store(&heap, stored(shape + 1, ENTRY.own_holder, 0), Value::integer(1)), "another shape");
            assert!(!store(&heap, stored(shape, 7, 0), Value::integer(1)), "a holder entry");
            assert_eq!(read(&heap, entry), before, "a leave writes nothing");
        }
    }

    /// The site read's only per-site operand is the entry's byte offset.
    #[test]
    fn the_site_read_has_one_per_site_hole() {
        let mut stage = crate::staged::trace::Trace::default();
        let (entries, records, object, functions) = (stage.constant(0), stage.constant(1), stage.constant(2), stage.constant(3));
        let offset = stage.site(ENTRY.entry_size as u64);
        site_read(&mut stage, (entries, records, functions), object, offset, ());
        assert_eq!(stage.holes, [ENTRY.entry_size as u64], "{:?}", stage.holes);
    }
}
