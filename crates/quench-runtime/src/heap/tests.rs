use super::*;
use std::rc::Rc;

mod parallel;
mod records_and_weak;
mod storage_and_promotion;
mod census_and_slots;
mod concurrent;

fn plain_object() -> Object {
    Object::new(Value::NULL)
}

fn collect_minor(heap: &mut Heap, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
    heap.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, false)
}

/// A heap whose minor collections promote at `age`.
fn heap_at(age: PromotionAge) -> Heap {
    let mut heap = Heap::new();
    heap.set_promotion_age_for_test(age);
    heap
}

/// Runs `scenario` on a heap of each promotion age.
fn for_each_promotion_age(mut scenario: impl FnMut(&mut Heap, PromotionAge)) {
    for age in PromotionAge::ALL {
        scenario(&mut heap_at(age), age);
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn direct_array_record_base_resolves_the_current_live_array_value() {
    let mut heap = Heap::new();
    assert!(heap.array_record_base().is_none());

    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), vec![Value::integer(7)]),
    });
    let id = array
        .direct_array_index()
        .expect("ordinary array uses a direct record");
    let base = heap
        .array_record_base()
        .expect("direct array allocation reserves its record range");
    // SAFETY: `array` is a live direct-array Value owned by `heap`; its ID resolves within
    // the base returned by that same heap.
    let record = unsafe { &*base.add(id as usize) };
    assert!(std::ptr::eq(record, heap.array(array).expect("live array record")));

    let shared_elements = heap
        .array_elements(array)
        .expect("array elements")
        .clone();
    let shared = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), shared_elements),
    });
    heap.array_elements_mut(array)
        .expect("array elements")
        .make_mut()
        .push(Value::integer(9));
    // Growing the elements may move the backing or keep it (the allocator decides); the
    // record range stays put and each array sees its own elements.
    assert_eq!(heap.array_record_base(), Some(base));
    assert_eq!(heap.array(array).unwrap().elements().as_slice(), &[Value::integer(7), Value::integer(9)][..]);
    assert_eq!(heap.array(shared).unwrap().elements().as_slice(), &[Value::integer(7)][..]);
}

#[test]
fn minor_collection_traces_remembered_object_and_sparse_array_edges() {
    for_each_promotion_age(|heap, _| minor_collection_traces_remembered_edges(heap));
}

fn minor_collection_traces_remembered_edges(heap: &mut Heap) {
    let owner = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), vec![Value::UNDEFINED]),
    });
    let sparse = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
    heap.collect([owner, sparse]);

    let child = heap.alloc(Cell::Error("dense child".into()));
    if let Some(elements) = heap.array_elements_mut(owner) {
        elements.make_mut().set(0, child);
    }
    let sparse_child = heap.alloc(Cell::Error("sparse child".into()));
    let sparse_index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(sparse, sparse_index, sparse_child);
    let garbage = heap.alloc(Cell::Error("unreachable young".into()));

    collect_minor(heap, [owner, sparse]);
    assert!(heap.get(child).is_some());
    assert_eq!(heap.sparse_get(sparse, sparse_index), Some(sparse_child));
    assert!(heap.get(sparse_child).is_some());
    assert!(heap.get(garbage).is_none());

    if let Some(elements) = heap.array_elements_mut(owner) {
        elements.make_mut().set(0, Value::UNDEFINED);
    }
    heap.collect([owner, sparse]);
    assert!(heap.get(child).is_none());
}

#[test]
fn minor_collection_handles_old_weak_maps_and_weak_refs() {
    for_each_promotion_age(|heap, _| minor_collection_handles_old_weak_cells(heap));
}

fn minor_collection_handles_old_weak_cells(heap: &mut Heap) {
    let old_key = heap.alloc(Cell::Object(plain_object()));
    let old_value = heap.alloc(Cell::Object(plain_object()));
    let mut entries = WeakMapEntries::default();
    entries.insert((old_key, old_value));
    let map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::new(entries),
    });
    let weak_ref = heap.alloc(Cell::WeakRef {
        object: Box::new(plain_object()),
        target: None,
    });
    heap.collect([map, old_key, weak_ref]);

    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::Object(plain_object()));
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
        entries.insert((key, value));
    }
    let referent = heap.alloc(Cell::Object(plain_object()));
    let target = heap.weak_handle(referent).unwrap();
    if let Some(Cell::WeakRef { target: slot, .. }) = heap.get_mut(weak_ref) {
        *slot = Some(target);
    }

    collect_minor(heap, [map, key, weak_ref]);
    assert!(heap.is_live(value));
    assert!(matches!(
        heap.get(weak_ref),
        Some(Cell::WeakRef { target: None, .. })
    ));
    assert!(!heap.is_live(referent));

    heap.collect([map, weak_ref]);
    assert!(!heap.is_live(key));
    assert!(!heap.is_live(value));
    assert!(!heap.is_live(old_key));
    assert!(!heap.is_live(old_value));
}

#[test]
fn minor_collection_keeps_records_written_into_old_records_and_frees_young_garbage() {
    for_each_promotion_age(minor_collection_keeps_records_written_into_old_records);
}

fn minor_collection_keeps_records_written_into_old_records(heap: &mut Heap, age: PromotionAge) {
    let owner = heap.alloc(Cell::Object(plain_object()));
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), vec![Value::UNDEFINED]),
    });
    assert!(owner.direct_object_index().is_some());
    assert!(array.direct_array_index().is_some());
    heap.collect([owner, array]);
    assert!(heap.is_old(owner) && heap.is_old(array));

    // Young records reachable only through stores into old records.
    let child = heap.alloc(Cell::Object(plain_object()));
    heap.object_mut(owner).unwrap().set_inline_property(0, child);
    let element = heap.alloc(Cell::Object(plain_object()));
    heap.array_elements_mut(array).unwrap().make_mut().set(0, element);
    let garbage = heap.alloc(Cell::Object(plain_object()));
    // The barrier logged both owners, so they are young until the minor traces them.
    assert!(!heap.is_old(owner) && !heap.is_old(array));

    collect_minor(heap, [owner, array]);
    assert!(heap.is_live(child) && heap.is_old(child) != age.ages());
    assert!(heap.is_live(element) && heap.is_old(element) != age.ages());
    assert!(!heap.is_live(garbage));
    // Owners stay old; those of what may stay young are carried to the next minor collection.
    assert!(heap.is_old(owner) && heap.is_old(array));
    assert_eq!(heap.carried_owners.len(), if age.ages() { 2 } else { 0 });
    if age.ages() {
        collect_minor(heap, [owner, array]);
        assert!(heap.is_old(child) && heap.is_old(element));
        assert!(heap.is_old(owner) && heap.is_old(array));
    }

    // An old record that loses its last reference survives minors and dies at a full one.
    heap.object_mut(owner).unwrap().set_inline_property(0, Value::UNDEFINED);
    collect_minor(heap, [owner, array]);
    assert!(heap.is_live(child));
    heap.collect([owner, array]);
    assert!(!heap.is_live(child));
    assert!(heap.is_live(element));
}

#[test]
fn records_written_twice_are_logged_once_per_cycle() {
    for_each_promotion_age(|heap, _| records_written_twice_are_logged_once(heap));
}

fn records_written_twice_are_logged_once(heap: &mut Heap) {
    let owner = heap.alloc(Cell::Object(plain_object()));
    heap.collect([owner]);
    for value in 0..4 {
        heap.object_mut(owner)
            .unwrap()
            .set_inline_property(0, Value::number(f64::from(value)));
    }
    assert_eq!(heap.object_records.remembered_count(), 1);
    collect_minor(heap, [owner]);
    assert_eq!(heap.object_records.remembered_count(), 0);
    assert!(heap.is_old(owner));
}


#[cfg(feature = "profile-gc-time")]
#[test]
fn gc_time_profile_records_collection_phases_without_aggregate_counters() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::Error("kept".into()));
    heap.collect([kept]);

    let profile = heap.gc_profile();
    assert!(profile.mark_nanos + profile.sweep_nanos + profile.mark_clear_nanos > 0);
    assert_eq!(heap.stats().1, 1);
}


#[test]
fn scope_slot_owners_survive_collection_and_release() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::Error("kept".into()));
    let owner = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        function: 0,
        slots: vec![Value::number(1.0), kept].into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: Vec::new().into(),
            with_objects: Box::default(),
        }),
    });
    let slots = heap.clone_environment_slots(owner, &[0], crate::bytecode::Resets::listed(&[])).unwrap();
    let view = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        function: 0,
        slots,
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: Vec::new().into(),
            with_objects: Box::default(),
        }),
    });
    *heap.environment_slot_mut(view, 0).unwrap() = Value::number(2.0);
    let reference = heap.alloc(Cell::BindingReference {
        environment: view,
        slot: 1,
        kind: crate::bytecode::LexicalBindingKind::Mutable,
    });
    assert_eq!(heap.environment_slot(owner, 0), Some(Value::number(1.0)));
    assert_eq!(heap.environment_slot_owner(view, 1), Some(owner));
    heap.collect([reference]);
    assert_eq!(heap.environment_slot(view, 1), Some(kept));
    assert!(heap.is_live(kept));
    *heap.environment_slot_mut(view, 1).unwrap() = Value::FALSE;
    assert_eq!(heap.environment_slot(owner, 1), Some(Value::FALSE));
    heap.collect([reference]);
    assert!(!heap.is_live(kept));
    heap.collect([]);
    assert!(!heap.is_live(owner));
    assert!(!heap.is_live(view));
    assert!(!heap.is_live(reference));
}

#[test]
fn ordinary_object_payloads_live_in_compact_records() {
    // The header words live once per layout (`heap::layouts`), so a record is its layout id
    // plus one payload word of narrow inline values.
    assert_eq!(size_of::<Object>(), size_of::<u32>() + size_of::<u64>());
    assert_eq!(size_of::<Slot>(), size_of::<Option<Cell>>());
}

#[test]
fn strong_root_keeps_cell_alive_until_release() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::Error("kept".into()));
    let dead = heap.alloc(Cell::Error("dead".into()));
    let root = heap.root(kept);

    heap.collect([]);
    assert!(heap.is_live(kept));
    assert!(!heap.is_live(dead));

    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(!heap.is_live(kept));
}

#[test]
fn forged_heap_indices_are_rejected_at_the_access_boundary() {
    let mut heap = Heap::new();
    assert!(!heap.is_live(Value::heap(99_999)));
    assert!(heap.get_mut(Value::heap(99_999)).is_none());
}

#[test]
fn array_buffer_backing_is_accounted_until_owner_collection() {
    let mut heap = Heap::new();
    let buffer = heap.alloc(Cell::ArrayBuffer {
        object: Box::new(plain_object()),
        bytes: vec![0; 16].into(),
        shared: false,
        detached: false,
        max_byte_length: 16,
        resizable: false,
        immutable: false,
    });
    assert_eq!(heap.stats().5, 16);
    heap.collect([buffer]);
    assert_eq!(heap.stats().5, 16);
    heap.collect([]);
    assert_eq!(heap.stats().5, 0);
}

#[test]
fn finalization_jobs_are_created_for_unmarked_targets() {
    let mut heap = Heap::new();
    let target = heap.alloc(Cell::Object(plain_object()));
    let target = heap.weak_handle(target).unwrap();
    let registry = heap.alloc(Cell::FinalizationRegistry {
        object: Box::new(plain_object()),
        callback: Value::number(1.0),
        entries: Box::new(FinalizationEntries(vec![FinalizationEntry {
            target,
            held: Value::number(2.0),
            token: None,
        }])),
    });
    let root = heap.root(registry);
    let jobs = heap.collect([]);
    assert_eq!(jobs, [(Value::number(1.0), Value::number(2.0))]);
    assert!(heap.release_root(root));
}

#[test]
fn unreachable_finalization_registry_does_not_enqueue_jobs() {
    let mut heap = Heap::new();
    let target = heap.alloc(Cell::Object(plain_object()));
    let target = heap.weak_handle(target).unwrap();
    heap.alloc(Cell::FinalizationRegistry {
        object: Box::new(plain_object()),
        callback: Value::number(1.0),
        entries: Box::new(FinalizationEntries(vec![FinalizationEntry {
            target,
            held: Value::number(2.0),
            token: None,
        }])),
    });

    assert!(heap.collect([]).is_empty());
}

#[test]
fn weak_map_values_follow_ephemeron_key_reachability() {
    let mut heap = Heap::new();
    let weak_map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::default(),
    });
    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::Error("value".into()));
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(weak_map) {
        entries.insert((key, value));
    }
    let map_root = heap.root(weak_map);
    let key_root = heap.root(key);
    heap.collect([]);
    assert!(heap.is_live(key));
    assert!(heap.is_live(value));
    assert!(heap.release_root(key_root));
    heap.collect([]);
    assert!(!heap.is_live(key));
    assert!(!heap.is_live(value));
    assert!(heap.release_root(map_root));
}

#[test]
fn weak_map_array_keys_follow_direct_record_liveness() {
    let mut heap = Heap::new();
    let weak_map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::default(),
    });
    let key = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
    let value = heap.alloc(Cell::Error("value".into()));
    assert!(key.direct_array_index().is_some());
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(weak_map) {
        entries.insert((key, value));
    }

    let map_root = heap.root(weak_map);
    let key_root = heap.root(key);
    heap.collect([]);
    assert!(heap.is_live(key));
    assert!(heap.is_live(value));

    assert!(heap.release_root(key_root));
    heap.collect([]);
    assert!(!heap.is_live(key));
    assert!(!heap.is_live(value));
    assert!(matches!(
        heap.get(weak_map),
        Some(Cell::WeakMap { entries, .. }) if entries.iter().next().is_none()
    ));
    assert!(heap.release_root(map_root));
}

#[test]
fn object_space_identity_does_not_alias_legacy_slot_identity() {
    let mut heap = Heap::new();
    let map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::default(),
    });
    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::Object(plain_object()));

    assert_eq!(map.legacy_heap_index(), Some(0));
    assert_eq!(key.direct_object_index(), Some(0));
    assert_ne!(map, key);

    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
        entries.insert((key, value));
    }
    heap.collect([map, key]);
    assert!(heap.is_live(map));
    assert!(heap.is_live(key));
    assert!(heap.is_live(value));

    heap.collect([map]);
    assert!(heap.is_live(map));
    assert!(!heap.is_live(key));
    assert!(!heap.is_live(value));
    assert!(
        matches!(heap.get(map), Some(Cell::WeakMap { entries, .. }) if entries.iter().next().is_none())
    );
}
