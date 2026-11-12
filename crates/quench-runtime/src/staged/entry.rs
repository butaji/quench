//! A data field's read through the IC site's entry record (form b), own or held by the
//! receiver's prototype: the record's shape, the holder and the slot are read at run time from
//! the entry the site table keeps, not held as per-site operands. A specializer's only hole is
//! the entry's address, so the code never changes when the site's feedback does (the entry
//! record is the one feedback authority).

use super::object::{arena_run, decode_narrow, extras_pointer, has_extras, inline_storage, plain_payload};
use super::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;

// The entry's words are complemented by an all-ones word (a difference from it) for the shapes
// and by zero for the slot: the guards below read them without a xor.
const _: () = assert!(ENTRY.receiver_complement == u32::MAX && ENTRY.holder_complement == u32::MAX && ENTRY.slot_complement == 0);

/// Guards that the shape of the object record at `record` is the one the entry at `entry`
/// stores in its word at `offset`, complemented by `complement`: the stored word and the
/// record's shape complemented differ by zero.
#[inline(always)]
fn shape_matches<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, (offset, complement): (usize, u32), miss: S::Miss) -> Option<()> {
    let exact = stage.load_u32(record, OBJECT.auxiliary_offset as i32);
    let stored = stage.load_u32(entry, offset as i32);
    let complement = stage.constant(u64::from(complement));
    let wanted = stage.sub(complement, exact);
    let difference = stage.sub(stored, wanted);
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}

/// The slot the entry at `entry` names.
#[inline(always)]
fn entry_slot<S: Stage>(stage: &mut S, entry: S::Word) -> S::Word {
    let slot = stage.load_u32(entry, ENTRY.slot_offset as i32);
    stage.and_imm(slot, u64::from(u16::MAX))
}

/// The value at `slot` of the object record at `record`, whose shape the caller proved: `None`
/// (leaving at `miss`) where its values are not plain (extras displaced them). Packed values
/// decode from the record, flat ones read from the arena run.
#[inline(always)]
fn plain_value<S: Stage>(stage: &mut S, record: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(record, OBJECT.payload_offset as i32);
    let plain = plain_payload(stage, payload);
    stage.guard(plain, miss)?;
    let inline = inline_storage(stage, record);
    Some(stage.choose(inline, |stage| {
        let half = stage.mul_imm(slot, std::mem::size_of::<u32>() as u64);
        let address = stage.add(record, half);
        let narrow = stage.load_u32(address, OBJECT.payload_offset as i32);
        decode_narrow(stage, narrow)
    }, |stage| {
        let run = arena_run(stage, payload);
        let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
        let address = stage.add(run, offset);
        stage.load_u64(address, 0)
    }))
}

/// The value at `slot` of the prototype record `holder`, whose shape the caller proved: a
/// prototype keeps its values in its extras (`Object::mark_as_prototype`), wide and inline or as
/// a run in the arena, so `None` (leaving at `miss`) where it has none, or where an inline value
/// is past the inline ones.
#[inline(always)]
fn holder_value<S: Stage>(stage: &mut S, holder: S::Word, slot: S::Word, miss: S::Miss) -> Option<S::Word> {
    let payload = stage.load_u64(holder, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let pointer = extras_pointer(stage, payload);
    let inline = inline_storage(stage, holder);
    let limit = stage.constant(OBJECT.inline_property_count as u64);
    let below = stage.ult(slot, limit);
    let flat = stage.not(inline);
    let fits = stage.any(flat, below);
    stage.guard(fits, miss)?;
    let run = stage.choose(inline, |stage| stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64), |stage| {
        let start = stage.load_u32(pointer, OBJECT.extras_arena_start_offset as i32);
        arena_run(stage, start)
    });
    let offset = stage.mul_imm(slot, OBJECT.property_value_size as u64);
    let address = stage.add(run, offset);
    Some(stage.load_u64(address, 0))
}

/// The value an own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's or its values
/// are not plain. The entry stores each of its words complemented against the empty entry, so
/// a shape equal to the record's shape complemented is the guard.
#[inline(always)]
fn own_entry_value<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    shape_matches(stage, record, entry, (ENTRY.receiver_offset, ENTRY.receiver_complement), miss)?;
    let slot = entry_slot(stage, entry);
    plain_value(stage, record, slot, miss)
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

/// The value the own entry at `entry` (an address) names in the object record at `record`:
/// `None` (leaving at `miss`) where the record's exact shape is not the entry's, the entry
/// names a prototype holder (or is empty), or the record's values are not plain. A holder of
/// zero, the stored own holder, is the guard.
// Read by the tests; the b-jit stencil build reads `site_read`, which also takes holder entries.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn own_read_entry<S: Stage>(stage: &mut S, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let stored_holder = stage.load_u32(entry, ENTRY.holder_offset as i32);
    let holder = stage.eq_imm(stored_holder, u64::from(ENTRY.holder_complement ^ ENTRY.own_holder));
    stage.guard(holder, miss)?;
    own_entry_value(stage, record, entry, miss)
}

/// A field read through the cache entry `offset` bytes into the field cache table `entries`
/// (the running program's, as `Vm::program_field_entries` hands it), of the object `object`
/// names in the record table at `records`: the data value the entry holds, own or held by the
/// receiver's prototype, or `None` (leaving at `miss`) where either table is absent, the word is
/// no direct object, or the entry is no entry for the record (`own_entry_value`,
/// `holder_entry_value`). The offset is the row's only per-site operand, so a site is read as
/// the interpreter's cached read reads it.
// Read by the b-jit stencil build (`quench-bjit/stencils`), which includes this file.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn site_read<S: Stage>(stage: &mut S, (entries, records): (S::Word, S::Word), object: S::Word, offset: S::Imm, miss: S::Miss) -> Option<S::Word> {
    for base in [entries, records] {
        let present = stage.ne_imm(base, 0u32);
        stage.guard(present, miss)?;
    }
    let direct = super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = stage.and_imm(object, u64::from(u32::MAX));
    let record = super::object::object_record_at(stage, records, id);
    let entry = stage.add_imm(entries, offset);
    let stored_holder = stage.load_u32(entry, ENTRY.holder_offset as i32);
    let own = stage.eq_imm(stored_holder, u64::from(ENTRY.holder_complement ^ ENTRY.own_holder));
    stage.choose_leaving(own, |stage| own_entry_value(stage, record, entry, miss), |stage| holder_entry_value(stage, records, record, entry, miss))
}

/// The word the loop entry record holds at the per-site byte `offset`: a field value the loop's
/// entry recorded (`LoopEntry`), written before the run and never during it.
// Read by the b-jit stencil build (`quench-bjit/stencils`), which includes this file.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn entry_word<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> S::Word {
    stage.load_site(entry, offset)
}

/// The dense backing of an array the loop entry recorded: its first value, at the per-site
/// byte `offset` of the entry record (which never changes during a run, so the load is of an
/// immutable table), and its length, read from the backing's header.
// Read by the b-jit stencil build (`quench-bjit/stencils`), which includes this file.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn entry_backing<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> (S::Word, S::Word) {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let address = stage.add_imm(entry, offset);
    let values = stage.load_table(address);
    let length = stage.load_u32(values, ARRAY.header_length_offset as i32 - ARRAY.header_values_offset as i32);
    (values, length)
}

/// The interpreter's reading of `own_read_entry` over a site table entry.
///
/// # Safety
/// `object` is a live record, `entry` the address of a stored field cache entry, and
/// `property_values` the base of the arena that holds the record's run.
#[cfg(test)]
unsafe fn concrete_read_entry(object: &crate::heap::Object, entry: *const u32, property_values: *const crate::Value) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    own_read_entry(&mut stage, object as *const crate::heap::Object as u64, entry as u64, ()).map(crate::Value::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object};
    use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;
    use crate::Value;

    /// A field cache entry as the site table stores it: each word complemented.
    fn stored(shape: u32, holder: u32, slot: u32) -> [u32; 3] {
        let mut words = [0; 3];
        words[ENTRY.receiver_offset / 4] = shape ^ ENTRY.receiver_complement;
        words[ENTRY.holder_offset / 4] = holder ^ ENTRY.holder_complement;
        words[ENTRY.slot_offset / 4] = slot ^ ENTRY.slot_complement;
        words
    }

    fn object_of(heap: &mut Heap, count: usize) -> (Value, u32) {
        for shape in 1..=count as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
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
        let record = heap.object(object).unwrap();
        for slot in 0..count as u16 {
            let entry = stored(shape, ENTRY.own_holder, u32::from(slot));
            // SAFETY: a live record, a stored entry and its arena.
            let by_entry = unsafe { concrete_read_entry(record, entry.as_ptr(), heap.arena_values_base()) };
            // SAFETY: the hint's shape and slot are the record's own.
            let by_hint = unsafe { crate::staged::concrete_own_field_hit(record, heap.arena_values_base(), shape, slot) };
            assert_eq!(by_entry, by_hint, "slot {slot}");
            assert_eq!(by_entry, Some(Value::integer(10 * (i32::from(slot) + 1))));
        }
        let read = |entry: [u32; 3]| unsafe { concrete_read_entry(record, entry.as_ptr(), heap.arena_values_base()) };
        assert_eq!(read(stored(shape + 1, ENTRY.own_holder, 0)), None, "another shape");
        assert_eq!(read(stored(shape, 7, 0)), None, "a holder entry");
        assert_eq!(read([0; 3]).is_some(), shape == ENTRY.receiver_complement, "the empty entry names no shape");
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
        let stray = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        for count in [crate::value_vec::INLINE_PROPERTY_COUNT, wide] {
            let prototype = heap.alloc(Cell::Object(Object::new(Value::NULL)));
            for shape in 1..=count as u32 {
                heap.property_add(prototype, Value::integer(70 + shape as i32), shape);
            }
            heap.object_record_mut(prototype).unwrap().mark_as_prototype();
            let receiver = heap.alloc(Cell::Object(Object::new(prototype)));
            let (holder_shape, receiver_shape) = (shape_of(&heap, prototype), shape_of(&heap, receiver));
            let records = heap.object_record_base().expect("object records exist") as u64;
            let read = |object: Value, entry: [u32; 3]| {
                let mut stage = crate::staged::Concrete::new(heap.arena_values_base());
                site_read(&mut stage, (entry.as_ptr() as u64, records), object.to_bits(), 0u64, ()).map(Value::from_bits)
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

    /// The row has no per-site hole of its own: the entry address is the caller's word.
    #[test]
    fn the_entry_read_has_no_per_site_hole() {
        let mut stage = crate::staged::trace::Trace::default();
        let (record, entry) = (stage.constant(0), stage.constant(1));
        own_read_entry(&mut stage, record, entry, ());
        assert!(stage.holes.is_empty(), "{:?}", stage.holes);
    }

    #[test]
    fn the_loop_entry_rows_read_the_recorded_field_and_array_backing() {
        let mut backing = crate::heap::ArrayElements::from_slice(&[Value::integer(1), Value::number(0.5), Value::integer(3)]);
        let values = backing.make_mut().as_mut_ptr() as u64;
        let record = [Value::integer(7).to_bits(), values];
        let mut stage = crate::staged::Concrete::new(std::ptr::null());
        let entry = record.as_ptr() as u64;
        assert_eq!(entry_word(&mut stage, entry, 0), Value::integer(7).to_bits());
        let (first, length) = entry_backing(&mut stage, entry, std::mem::size_of::<u64>() as u64);
        assert_eq!((first, length), (values, 3));
        let mut trace = crate::staged::trace::Trace::default();
        let entry = trace.constant(0);
        let (offset, other) = (trace.site(8), trace.site(16));
        entry_word(&mut trace, entry, offset);
        entry_backing(&mut trace, entry, other);
        assert_eq!(trace.holes, [8, 16], "the offsets are the rows' only operands");
    }
}
