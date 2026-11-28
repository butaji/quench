use super::*;
use crate::heap::{Heap, Object};
use crate::vm::NATIVE_FIELD_CACHE_LAYOUT as ENTRY;
use crate::Value;

/// The entry `entry` read through `Concrete` on `object` at site offset zero (own or class-held).
fn concrete_read_entry(heap: &crate::heap::Heap, object: crate::Value, entry: &[u32; 4]) -> Option<crate::Value> {
    // SAFETY: a stored entry, and the heap's live table.
    unsafe { crate::staged::ic::concrete_site_read(heap, entry.as_ptr() as u64, object, 0) }
}

/// A field cache entry as the site table stores it: each word complemented.
fn stored(key: u32, holder: u32, slot: u32) -> [u32; 4] {
    let mut words = [0; 4];
    words[ENTRY.receiver_offset / 4] = key ^ ENTRY.receiver_complement;
    words[ENTRY.holder_offset / 4] = holder ^ ENTRY.holder_complement;
    words[ENTRY.slot_offset / 4] = slot ^ ENTRY.slot_complement;
    words
}

/// A class-held entry: the receiver's class key, and the holder cell's granule where its field lives
/// (`ENTRY.holder_cell_offset`, not complemented). Its holder word is no own holder, so it is held.
fn stored_held(key: u32, holder_granule: u32, slot: u32) -> [u32; 4] {
    let mut words = stored(key, 0, slot);
    words[ENTRY.holder_cell_offset / 4] = holder_granule;
    words
}

/// An object with `count` own fields, each a distinct integer, in the class its layout names.
fn object_of(heap: &mut Heap, count: usize) -> Value {
    for shape in 1..=count as u32 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    let object = heap.alloc_object(Object::new(Value::NULL));
    for shape in 1..=count as u32 {
        heap.property_add(object, Value::integer(shape as i32 * 10), shape);
    }
    object
}

/// The class key of a live cell, which a class-keyed entry holds as its receiver.
fn key_of(heap: &Heap, object: Value) -> u32 {
    heap.live_class_key(object).expect("a live class cell names its key")
}

#[test]
fn an_entry_read_reads_the_cell_field_at_every_slot_and_misses_on_the_rest() {
    let mut heap = Heap::new();
    let count = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
    let object = object_of(&mut heap, count);
    let key = key_of(&heap, object);
    // Every ordinary object is a class cell, so the entry reads the cell's field at every slot, inline or spilled.
    for slot in 0..count as u16 {
        let entry = stored(key, ENTRY.own_holder, u32::from(slot));
        let by_entry = concrete_read_entry(&heap, object, &entry);
        assert_eq!(by_entry, Some(Value::integer(10 * (i32::from(slot) + 1))), "slot {slot}");
    }
    let stray = heap.alloc_object(Object::new(Value::NULL));
    assert_ne!(heap.live_class_key(stray), Some(key), "another class");
    let read = |entry: [u32; 4]| concrete_read_entry(&heap, object, &entry);
    assert_eq!(read(stored(key.wrapping_add(1), ENTRY.own_holder, 0)), None, "another class key");
    assert_eq!(read(stored(key, 7, 0)), None, "a holder entry of an own field's class");
    assert_eq!(read([0; 4]).is_some(), key == ENTRY.receiver_complement, "the empty entry names no class");
}

/// A class-held entry reads the prototype cell's field through the receiver's class key, at every slot of the
/// prototype's storage, and misses where the receiver's class is not the entry's or the entry holds no holder cell.
#[test]
fn a_class_held_entry_reads_the_prototype_field_and_misses_on_any_other_class() {
    let mut heap = Heap::new();
    let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide as u32 {
        heap.register_property_shape(shape, shape as usize, None);
    }
    let stray = heap.alloc_object(Object::new(Value::NULL));
    for count in [crate::value_vec::INLINE_PROPERTY_COUNT, wide] {
        let prototype = heap.alloc_object(Object::new(Value::NULL));
        for shape in 1..=count as u32 {
            heap.property_add(prototype, Value::integer(70 + shape as i32), shape);
        }
        heap.ensure_cell_extras(prototype);
        heap.object_mut(prototype).unwrap().mark_as_prototype();
        let receiver = heap.alloc_object(Object::new(prototype));
        let (holder_granule, receiver_key) = (prototype.to_bits() as u32, key_of(&heap, receiver));
        let read = |object: Value, entry: [u32; 4]| {
            let mut stage = crate::staged::Concrete::over(&heap);
            site_read(&mut stage, (entry.as_ptr() as u64, 0), object.to_bits(), 0u64, ()).map(Value::from_bits)
        };
        for slot in 0..count as u32 {
            assert_eq!(read(receiver, stored_held(receiver_key, holder_granule, slot)), Some(Value::integer(71 + slot as i32)), "{count} values, slot {slot}");
        }
        assert_eq!(read(receiver, stored_held(key_of(&heap, stray), holder_granule, 0)), None, "a receiver of another class");
        assert_eq!(read(Value::integer(1), stored_held(receiver_key, holder_granule, 0)), None, "no object");
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
    let entries = [stored(key_of(&heap, outer), ENTRY.own_holder, 0), stored(key_of(&heap, leaf), ENTRY.own_holder, 0)];
    let read = |object: Value| {
        let mut stage = crate::staged::Concrete::over(&heap);
        site_chain(&mut stage, (entries.as_ptr() as u64, 0), object.to_bits(), (0u64, ENTRY.entry_size as u64), ()).map(Value::from_bits)
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
        let object = object_of(&mut heap, count);
        let key = key_of(&heap, object);
        let store = |heap: &Heap, entry: [u32; 4], value: Value| {
            let mut stage = crate::staged::Concrete::over(heap);
            site_store(&mut stage, entry.as_ptr() as u64, object.to_bits(), 0u64, (value.to_bits(), true, None), ()).is_some()
        };
        let read = |heap: &Heap, entry: [u32; 4]| concrete_read_entry(heap, object, &entry);
        for slot in 0..count as u32 {
            let entry = stored(key, ENTRY.own_holder, slot);
            assert!(store(&heap, entry, Value::integer(-7)), "{count} values, slot {slot}");
            assert_eq!(read(&heap, entry), Some(Value::integer(-7)));
            let before = read(&heap, entry);
            assert_eq!(store(&heap, entry, Value::number(1.5)), count > crate::value_vec::INLINE_PROPERTY_COUNT, "a double needs a flat slot");
            assert_eq!(read(&heap, entry) == before, count <= crate::value_vec::INLINE_PROPERTY_COUNT);
        }
        let entry = stored(key, ENTRY.own_holder, 0);
        let before = read(&heap, entry);
        assert!(!store(&heap, stored(key.wrapping_add(1), ENTRY.own_holder, 0), Value::integer(1)), "another class");
        assert!(!store(&heap, stored(key, 7, 0), Value::integer(1)), "a holder entry");
        assert_eq!(read(&heap, entry), before, "a leave writes nothing");
    }
}

/// A class-keyed entry (its receiver word the live cell's class key) reads and stores the live cell's field,
/// and misses on an object of another class, whose key the entry does not hold.
#[test]
fn a_class_keyed_entry_reads_and_stores_the_live_cell_field_and_misses_on_another_class() {
    let mut heap = Heap::new();
    let count = crate::value_vec::INLINE_PROPERTY_COUNT;
    let object = object_of(&mut heap, count);
    let other = heap.alloc_object(Object::new(Value::NULL));
    let key = key_of(&heap, object);
    assert_ne!(heap.live_class_key(other), Some(key), "another class");
    let read = |heap: &Heap, object: Value, entry: [u32; 4]| {
        let mut stage = crate::staged::Concrete::over(heap);
        site_read(&mut stage, (entry.as_ptr() as u64, 0), object.to_bits(), 0u64, ()).map(Value::from_bits)
    };
    for slot in 0..count as u32 {
        let entry = stored(key, ENTRY.own_holder, slot);
        assert_eq!(read(&heap, object, entry), Some(Value::integer(10 * (slot as i32 + 1))), "slot {slot}");
        assert_eq!(read(&heap, other, entry), None, "another class, slot {slot}");
    }
    let entry = stored(key, ENTRY.own_holder, 0);
    let store = |heap: &Heap, object: Value, value: Value| {
        let mut stage = crate::staged::Concrete::over(heap);
        site_store(&mut stage, entry.as_ptr() as u64, object.to_bits(), 0u64, (value.to_bits(), true, None), ()).is_some()
    };
    assert!(store(&heap, object, Value::integer(-7)), "the class's own cell");
    assert_eq!(read(&heap, object, entry), Some(Value::integer(-7)));
    assert!(!store(&heap, other, Value::integer(1)), "another class leaves");
    assert_eq!(read(&heap, object, entry), Some(Value::integer(-7)), "a leave writes nothing");
}

/// The site read's only per-site operand is the entry's byte offset.
#[test]
fn the_site_read_has_one_per_site_hole() {
    let mut stage = crate::staged::trace::Trace::default();
    let (entries, functions, object) = (stage.constant(0), stage.constant(3), stage.constant(2));
    let offset = stage.site(ENTRY.entry_size as u64);
    site_read(&mut stage, (entries, functions), object, offset, ());
    assert_eq!(stage.holes, [ENTRY.entry_size as u64], "{:?}", stage.holes);
}
