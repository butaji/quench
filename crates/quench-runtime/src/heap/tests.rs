use super::*;
use std::rc::Rc;

mod parallel;

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

    let array = heap.alloc_array(ArrayObject::new(plain_object(), vec![Value::integer(7)]));
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
    let shared = heap.alloc_array(ArrayObject::new(plain_object(), shared_elements));
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
fn symbol_records_live_while_named_and_free_their_descriptions() {
    for_each_promotion_age(|heap, _age| {
        let kept = heap.alloc_symbol(SymbolData::public(Some("kept".to_owned())));
        let dead = heap.alloc_symbol(SymbolData::public(Some("dead".to_owned())));
        assert!(heap.is_symbol(kept) && heap.is_symbol(dead));
        assert!(!heap.is_symbol(Value::heap(0)), "a legacy cell is never a symbol record");
        collect_minor(heap, [kept]);
        assert!(heap.is_live(kept) && !heap.is_live(dead));
        heap.collect([kept]);
        assert!(heap.is_live(kept) && !heap.is_live(dead));
        assert_eq!(heap.symbol_data(kept).and_then(|symbol| symbol.description.as_deref()), Some("kept"));
        assert!(heap.symbol_data(dead).is_none());
    });
}

#[test]
fn minor_collection_traces_remembered_object_and_sparse_array_edges() {
    for_each_promotion_age(|heap, _| minor_collection_traces_remembered_edges(heap));
}

fn minor_collection_traces_remembered_edges(heap: &mut Heap) {
    let owner = heap.alloc_array(ArrayObject::new(plain_object(), vec![Value::UNDEFINED]));
    let sparse = heap.alloc_array(ArrayObject::new(plain_object(), Vec::new()));
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
    let old_key = heap.alloc_object(plain_object());
    let old_value = heap.alloc_object(plain_object());
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

    let key = heap.alloc_object(plain_object());
    let value = heap.alloc_object(plain_object());
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
        entries.insert((key, value));
    }
    let referent = heap.alloc_object(plain_object());
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
    let owner = heap.alloc_object(plain_object());
    let array = heap.alloc_array(ArrayObject::new(plain_object(), vec![Value::UNDEFINED]));
    assert!(owner.direct_object_index().is_some());
    assert!(array.direct_array_index().is_some());
    heap.collect([owner, array]);
    assert!(heap.is_old(owner) && heap.is_old(array));

    // Young records reachable only through stores into old records.
    let child = heap.alloc_object(plain_object());
    heap.object_record_mut(owner).unwrap().set_inline_property(0, child);
    let element = heap.alloc_object(plain_object());
    heap.array_elements_mut(array).unwrap().make_mut().set(0, element);
    let garbage = heap.alloc_object(plain_object());
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
    heap.object_record_mut(owner).unwrap().set_inline_property(0, Value::UNDEFINED);
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
    let owner = heap.alloc_object(plain_object());
    heap.collect([owner]);
    for value in 0..4 {
        heap.object_record_mut(owner)
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
    let owner = heap.alloc_environment_record(crate::heap::EnvironmentRecord {
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
    let view = heap.alloc_environment_record(crate::heap::EnvironmentRecord {
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
    let target = heap.alloc_object(plain_object());
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
    let target = heap.alloc_object(plain_object());
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
    let key = heap.alloc_object(plain_object());
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
    let key = heap.alloc_array(ArrayObject::new(plain_object(), Vec::new()));
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
    let key = heap.alloc_object(plain_object());
    let value = heap.alloc_object(plain_object());

    assert_eq!(map.legacy_heap_index(), Some(0));
    assert_eq!(key.direct_object_index().and_then(|object| heap.record_id(object)), Some(0));
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

#[test]
fn weak_ref_target_is_cleared_after_collection() {
    let mut heap = Heap::new();
    let target = heap.alloc_object(plain_object());
    let target_handle = heap.weak_handle(target).unwrap();
    let reference = heap.alloc(Cell::WeakRef {
        object: Box::new(plain_object()),
        target: Some(target_handle),
    });
    let reference_root = heap.root(reference);
    heap.collect([]);
    assert!(!heap.is_live(target));
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef { target: None, .. })
    ));
    assert!(heap.release_root(reference_root));
}

#[test]
fn weak_ref_owner_clears_its_handle_before_slot_reuse() {
    let mut heap = Heap::new();
    let target = heap.alloc(Cell::Error("target".into()));
    let target_slot = target.heap_index().unwrap();
    let target_handle = heap.weak_handle(target).unwrap();
    let reference = heap.alloc(Cell::WeakRef {
        object: Box::new(plain_object()),
        target: Some(target_handle),
    });
    let reference_root = heap.root(reference);

    heap.collect([]);
    assert!(heap.weak_value(target_handle).is_none());
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef { target: None, .. })
    ));

    let replacement = heap.alloc(Cell::Error("replacement".into()));
    assert_eq!(replacement.heap_index(), Some(target_slot));
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef { target: None, .. })
    ));
    assert!(heap.release_root(reference_root));
}

#[test]
fn slab_local_free_bitmap_reuses_slots_across_slab_boundaries() {
    let mut heap = Heap::new();
    let slot_count = super::slots::SLOTS_PER_SLAB + 2;
    for _ in 0..slot_count {
        heap.alloc(Cell::Error("temporary".into()));
    }

    assert_eq!(heap.collect([]), []);
    assert_eq!(heap.free_slots, slot_count);

    let first = heap.alloc(Cell::Error("first".into()));
    let second = heap.alloc(Cell::Error("second".into()));
    let third = heap.alloc(Cell::Error("third".into()));
    assert_eq!(first.heap_index(), Some((slot_count - 1) as u32));
    assert_eq!(second.heap_index(), Some((slot_count - 2) as u32));
    assert_eq!(third.heap_index(), Some((slot_count - 3) as u32));
    assert_eq!(heap.free_slots, slot_count - 3);
}

#[test]
fn finalization_registry_clears_dead_tokens_before_slot_reuse() {
    let mut heap = Heap::new();
    let target = heap.alloc_object(plain_object());
    let token = heap.alloc_object(plain_object());
    let token_slot = token.heap_index().unwrap();
    let target_handle = heap.weak_handle(target).unwrap();
    let token_handle = heap.weak_handle(token).unwrap();
    let registry = heap.alloc(Cell::FinalizationRegistry {
        object: Box::new(plain_object()),
        callback: Value::UNDEFINED,
        entries: Box::new(FinalizationEntries(vec![FinalizationEntry {
            target: target_handle,
            held: Value::number(1.0),
            token: Some(token_handle),
        }])),
    });
    let target_root = heap.root(target);
    let registry_root = heap.root(registry);

    assert!(heap.collect([]).is_empty());
    let Some(Cell::FinalizationRegistry { entries, .. }) = heap.get(registry) else {
        panic!("registry remains live");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].token, None);
    assert_eq!(heap.weak_value(target_handle), Some(target));

    let replacement = heap.alloc_object(plain_object());
    assert_eq!(replacement.heap_index(), Some(token_slot));
    let Some(Cell::FinalizationRegistry { entries, .. }) = heap.get(registry) else {
        panic!("registry remains live");
    };
    assert_eq!(entries[0].token, None);
    assert!(heap.release_root(target_root));
    assert!(heap.release_root(registry_root));
}

#[test]
fn far_sparse_writes_over_a_holey_dense_prefix_stay_sparse() {
    let mut heap = Heap::new();
    let mut prefix = vec![Value::DELETED; MAX_DENSE_ARRAY_HOLE_LENGTH];
    prefix[0] = Value::number(1.0);
    let array = heap.alloc_array(ArrayObject::new(plain_object(), prefix));
    let index = 2 * MAX_DENSE_ARRAY_HOLE_LENGTH;
    heap.sparse_set(array, index, Value::number(2.0));
    assert_eq!(heap.sparse_length(array), Some(index + 1));
    assert_eq!(heap.array_elements(array).map(|dense| dense.len()), Some(MAX_DENSE_ARRAY_HOLE_LENGTH));
    assert_eq!(heap.sparse_get(array, index), Some(Value::number(2.0)));
}

#[test]
fn sparse_array_metadata_is_removed_before_a_slot_is_reused() {
    let mut heap = Heap::new();
    let array = heap.alloc_array(ArrayObject::new(plain_object(), Vec::new()));
    let slot = array.heap_index().unwrap();
    let index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(array, index, Value::number(7.0));
    assert_eq!(heap.sparse_get(array, index), Some(Value::number(7.0)));

    heap.collect([]);

    let reused = heap.alloc_array(ArrayObject::new(plain_object(), Vec::new()));
    assert_eq!(reused.heap_index(), Some(slot));
    assert_eq!(heap.sparse_get(reused, index), None);
    assert_eq!(heap.sparse_length(reused), None);
}

#[test]
fn direct_array_records_trace_values_and_release_shared_backing() {
    let mut heap = Heap::new();
    let object = heap.alloc_object(plain_object());
    let first_child = heap.alloc(Cell::Error("first".into()));
    let second_child = heap.alloc(Cell::Error("second".into()));
    let shared = ArrayElements::from(vec![first_child, Value::number(1.0)]);
    let first = heap.alloc_array(ArrayObject::new(plain_object(), shared.clone()));
    let second = heap.alloc_array(ArrayObject::new(plain_object(), shared.clone()));
    drop(shared);

    assert!(first.direct_array_index().is_some());
    assert_eq!(object.direct_object_index().and_then(|object| heap.record_id(object)), Some(0));
    assert_eq!(first.direct_array_index(), Some(0));
    assert_ne!(object, first);
    assert!(heap.array(object).is_none());
    assert!(heap.object(object).is_some());
    assert!(heap.object(first).is_some());
    assert!(heap.get(first).is_none());
    assert_eq!(heap.array_elements(first).unwrap().owner_count(), 2);
    let first_root = heap.root(first);
    {
        let elements = heap.array_elements_mut(first).unwrap();
        elements.make_mut().set(1, second_child);
    }
    assert_eq!(heap.collect([]), Vec::new());
    assert!(heap.is_live(first));
    assert!(!heap.is_live(second));
    assert!(heap.is_live(first_child));
    assert!(heap.is_live(second_child));
    let elements = heap.array_elements(first).unwrap();
    assert_eq!(elements[1], second_child);
    assert_eq!(elements.owner_count(), 1);
    assert!(heap.release_root(first_root));
    heap.collect([]);
    assert!(!heap.is_live(first));
    assert!(!heap.is_live(first_child));
    assert!(!heap.is_live(second_child));
}

#[test]
fn direct_array_records_trace_and_remove_sparse_elements() {
    let mut heap = Heap::new();
    let array = heap.alloc_array(ArrayObject::new(plain_object(), Vec::new()));
    let child = heap.alloc(Cell::Error("sparse child".into()));
    let index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(array, index, child);
    let array_root = heap.root(array);

    heap.collect([]);
    assert!(heap.is_live(array));
    assert!(heap.is_live(child));
    assert_eq!(heap.sparse_get(array, index), Some(child));

    assert!(heap.release_root(array_root));
    heap.collect([]);
    assert!(!heap.is_live(array));
    assert!(!heap.is_live(child));
    assert_eq!(heap.sparse_get(array, index), None);
}

#[test]
fn object_integrity_metadata_survives_collection() {
    let mut heap = Heap::new();
    let object = heap.alloc_object(plain_object());
    if let Some(data) = heap.object_record_mut(object) {
        data.set_extensible(false);
        data.set_frozen(true);
    }
    let garbage = (0..512)
        .map(|_| heap.alloc(Cell::Error("garbage".into())))
        .collect::<Vec<_>>();
    heap.collect([object]);
    assert!(
        heap.object_record(object)
            .is_some_and(|data| !data.is_extensible() && data.is_frozen())
    );
    assert!(garbage.iter().all(|value| !heap.is_live(*value)));
}

#[cfg(feature = "profile-memory")]
#[test]
fn out_of_line_object_metadata_is_included_in_live_memory_totals() {
    let mut heap = Heap::new();
    let object = heap.alloc_object(plain_object());
    let before = heap.memory_stats().3;
    // An object lives in its record family or a legacy cell; `Heap::object_record_mut` finds either.
    let data = heap.object_record_mut(object).unwrap();
    data.set_arguments_object();
    data.set_arguments_map(vec![0; 32]);
    data.set_module_namespace();
    data.set_module_bindings(vec![(0, crate::vm::program_store::ProgramId::MAIN, 0)]);
    data.set_deferred_module(Some(crate::ModuleSource::text("module.js", "export const value = 1;")));

    assert!(heap.memory_stats().3 > before);
    assert!(heap.live_payload_bytes()[CellKind::Object as usize] > 0);
}

fn anyref_storage() -> wasmparser::StorageType {
    wasmparser::StorageType::Val(wasmparser::ValType::Ref(wasmparser::RefType::ANYREF))
}

/// A struct of an i8 then an anyref; `described` gives it a descriptor, which is the declaration
/// itself (module index 0), so the layout ends with a descriptor word.
fn i8_anyref_struct(described: bool) -> wasmparser::SubType {
    wasmparser::SubType {
        is_final: true,
        supertype_idx: None,
        composite_type: wasmparser::CompositeType {
            inner: wasmparser::CompositeInnerType::Struct(wasmparser::StructType {
                fields: Box::new([
                    wasmparser::FieldType { element_type: wasmparser::StorageType::I8, mutable: true },
                    wasmparser::FieldType { element_type: anyref_storage(), mutable: false },
                ]),
            }),
            shared: false,
            descriptor_idx: if described { wasmparser::PackedIndex::from_module_index(0) } else { None },
            describes_idx: None,
        },
    }
}

#[test]
fn wasm_gc_descriptor_is_the_last_traced_reference_word() {
    let mut heap = Heap::new();
    let bits = 0x7ffc_1234_5678_9abc;
    let value = heap.alloc(Cell::WasmBits64(bits));
    let external = heap.alloc(Cell::WasmExtern(value));
    let descriptor = heap.alloc(Cell::WasmBits64(!bits));
    let declarations = crate::WasmTypes::singletons_for_test(vec![i8_anyref_struct(true)]);
    let layout = declarations.layout(0).expect("a described struct layout");
    let mut stored = WasmGcFields::new(layout, 0, Some(descriptor)).unwrap();
    stored.put(layout, 1, crate::wasm::gc::GcField::new(1, anyref_storage()).unwrap(), external.to_bits());
    assert!(matches!(stored.descriptor(), Some(found) if found == descriptor));
    let owner = heap.alloc_wasm_gc(WasmGcRecord::new(declarations, 0, stored));
    let root = heap.root(owner);
    heap.collect([]);
    assert!(heap.is_live(external));
    assert!(heap.is_live(descriptor));
    assert!(matches!(heap.get(value), Some(Cell::WasmBits64(actual)) if *actual == bits));
    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(!heap.is_live(owner));
    assert!(!heap.is_live(external));
    assert!(!heap.is_live(descriptor));
}

#[test]
fn wasm_scalar_and_gc_fields_follow_the_shared_strong_root_lifecycle() {
    let mut heap = Heap::new();
    let bits = 0x7ffc_1234_5678_9abc;
    let value = heap.alloc(Cell::WasmBits64(bits));
    // This allocator test checks edges; module admission owns declaration validity. The struct is
    // a packed i8 then an anyref, so its reference word is the one edge the collector follows.
    let declarations = crate::WasmTypes::singletons_for_test(vec![i8_anyref_struct(false)]);
    let external = heap.alloc(Cell::WasmExtern(value));
    let layout = declarations.layout(0).expect("a struct layout");
    let mut stored = WasmGcFields::new(layout, 0, None).unwrap();
    stored.put(layout, 1, crate::wasm::gc::GcField::new(1, anyref_storage()).unwrap(), external.to_bits());
    let owner = heap.alloc_wasm_gc(WasmGcRecord::new(declarations, 0, stored));
    let tag = heap.alloc(Cell::WasmTag { object: Box::new(plain_object()), declarations: crate::WasmTypes::default(), ty: 0 });
    let exception = heap.alloc(Cell::WasmException {
        tag,
        payload: Box::new(vec![owner]),
    });
    let root = heap.root(exception);
    heap.collect([]);
    assert!(heap.is_live(tag));
    assert!(heap.is_live(owner));
    assert!(matches!(heap.get(value), Some(Cell::WasmBits64(actual)) if *actual == bits));
    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(!heap.is_live(value));
    assert!(!heap.is_live(owner));
    assert!(!heap.is_live(external));
    assert!(!heap.is_live(tag));
    assert!(!heap.is_live(exception));
}

#[test]
fn regexp_legacy_constructor_is_traced_through_live_instances() {
    let owners: [fn(Value) -> RegExpLegacyOwner; 2] =
        [RegExpLegacyOwner::Enabled, RegExpLegacyOwner::Disabled];
    for owner in owners {
        let mut heap = Heap::new();
        let constructor = heap.alloc_object(plain_object());
        let weak_constructor = heap.weak_handle(constructor).unwrap();
        let regexp = heap.alloc(Cell::RegExp {
            object: plain_object(),
            meta: Rc::new(crate::heap::RegExpMeta {
                source: "a".into(),
                flags: crate::vm::RegExpFlags::validated(""),
                legacy_constructor: owner(constructor),
                matcher: crate::heap::RegExpMatcher::compiled(Rc::new(quench_regexp::Regex::with_flags("a", Default::default()).unwrap())),
            }),
        });
        let root = heap.root(regexp);
        heap.collect([]);
        assert_eq!(heap.weak_value(weak_constructor), Some(constructor));
        assert!(heap.release_root(root));
        heap.collect([]);
        assert!(heap.weak_value(weak_constructor).is_none());
        assert!(!heap.is_live(regexp));
    }
}

#[test]
fn resolved_binding_reference_keeps_its_slot_owner_alive() {
    let mut heap = Heap::new();
    let value = heap.alloc(Cell::Error("kept".into()));
    let environment = heap.alloc_environment_record(crate::heap::EnvironmentRecord {
        parent: Value::NULL,
        function: 0,
        slots: vec![value].into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: Vec::new().into(),
            with_objects: Box::default(),
        }),
    });
    let reference = heap.alloc(Cell::BindingReference {
        environment,
        slot: 0,
        kind: crate::bytecode::LexicalBindingKind::Mutable,
    });
    heap.collect([reference]);
    assert!(heap.is_live(environment));
    assert!(heap.is_live(value));
    heap.collect([]);
    assert!(!heap.is_live(reference));
    assert!(!heap.is_live(environment));
    assert!(!heap.is_live(value));
}

#[test]
fn weak_map_with_live_key_does_not_activate_an_unreachable_map() {
    let mut heap = Heap::new();
    let key = heap.alloc_object(plain_object());
    let value = heap.alloc_object(plain_object());
    let mut entries = WeakMapEntries::default();
    entries.insert((key, value));
    let map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::new(entries),
    });
    heap.collect([key]);
    assert!(heap.is_live(key));
    assert!(!heap.is_live(map));
    assert!(!heap.is_live(value));
}

#[test]
fn weak_map_cycles_do_not_bootstrap_unreachable_keys() {
    let mut heap = Heap::new();
    let left = heap.alloc_object(plain_object());
    let right = heap.alloc_object(plain_object());
    let mut entries = WeakMapEntries::default();
    entries.insert((left, right));
    entries.insert((right, left));
    let map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::new(entries),
    });
    heap.collect([map]);
    assert!(heap.is_live(map));
    assert!(!heap.is_live(left));
    assert!(!heap.is_live(right));
    assert!(
        matches!(heap.get(map), Some(Cell::WeakMap { entries, .. }) if entries.iter().next().is_none())
    );
}

#[test]
fn weak_map_activation_observes_keys_marked_before_and_after_the_map() {
    for keys_first in [false, true] {
        let mut heap = Heap::new();
        let outer_key = heap.alloc_object(plain_object());
        let inner_key = heap.alloc_object(plain_object());
        let value = heap.alloc_object(plain_object());
        let mut inner_entries = WeakMapEntries::default();
        inner_entries.insert((inner_key, value));
        let inner = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::new(inner_entries),
        });
        let mut outer_entries = WeakMapEntries::default();
        outer_entries.insert((outer_key, inner));
        let outer = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::new(outer_entries),
        });
        let roots = if keys_first {
            [outer, outer_key, inner_key]
        } else {
            [outer_key, inner_key, outer]
        };
        heap.collect(roots);
        assert!(heap.is_live(inner));
        assert!(heap.is_live(value));
        heap.collect([outer, outer_key]);
        assert!(heap.is_live(inner));
        assert!(!heap.is_live(inner_key));
        assert!(!heap.is_live(value));
        heap.collect([outer]);
        assert!(!heap.is_live(inner));
    }
}

#[test]
fn weak_map_pending_fan_in_releases_every_value_and_resets_between_collections() {
    let mut heap = Heap::new();
    let key = heap.alloc_object(plain_object());
    let mut maps = Vec::new();
    let mut values = Vec::new();
    for _ in 0..3 {
        let value = heap.alloc_object(plain_object());
        let mut entries = WeakMapEntries::default();
        entries.insert((key, value));
        maps.push(heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::new(entries),
        }));
        values.push(value);
    }
    heap.collect(std::iter::once(key).chain(maps.iter().copied()));
    assert!(values.iter().all(|value| heap.is_live(*value)));
    heap.collect(maps.iter().copied());
    assert!(!heap.is_live(key));
    assert!(values.iter().all(|value| !heap.is_live(*value)));
    assert!(maps.iter().all(|map| heap.is_live(*map)));
}

#[test]
fn dense_storage_reads_writes_and_native_views_share_the_owner_checks() {
    let mut slots = EnvironmentSlots::from(vec![Value::UNDEFINED, Value::DELETED, Value::integer(42)]);
    let storage = slots.dense_storage().unwrap();
    // SAFETY: all owners below stay live; operations borrow only for their duration.
    // Writes contain no heap references and no guest or collection can run here.
    unsafe {
        assert_eq!(EnvironmentSlots::read_storage(storage, 0), Some(Value::UNDEFINED));
        assert_eq!(EnvironmentSlots::read_storage(storage, 1), None);
        assert_eq!(EnvironmentSlots::read_storage(storage, slots.len() as u16), None);
        assert_eq!(EnvironmentSlots::read_storage(std::ptr::null(), 0), None);
        assert_eq!(EnvironmentSlots::write_storage(storage, 0, Value::integer(7)), Some(()));
        assert_eq!(EnvironmentSlots::write_storage(storage, 1, Value::integer(7)), None);
        assert_eq!(EnvironmentSlots::write_storage(storage, slots.len(), Value::integer(7)), None);
        assert_eq!(EnvironmentSlots::write_storage(std::ptr::null(), 0, Value::integer(7)), None);
        assert_eq!(slots.owned(0), Some(Value::integer(7)));
        *slots.owned_mut(2).unwrap() = Value::integer(99);
        assert_eq!(EnvironmentSlots::read_storage(storage, 2), Some(Value::integer(99)));
        assert_eq!(EnvironmentSlots::native_storage_view(storage), slots.native_dense_view());
        assert_eq!(EnvironmentSlots::native_storage_view(std::ptr::null()), None);
        let empty = EnvironmentSlots::from(Vec::new());
        let empty_storage = empty.dense_storage().unwrap();
        assert_eq!(EnvironmentSlots::read_storage(empty_storage, 0), None);
        assert_eq!(EnvironmentSlots::write_storage(empty_storage, 0, Value::TRUE), None);
        assert_eq!(EnvironmentSlots::native_storage_view(empty_storage), empty.native_dense_view());
        let mut heap = Heap::new();
        let owner = heap.alloc_environment(Value::NULL, 1, slots.clone(), EnvironmentScopeRef::of_program(0)).0;
        let shared = slots.iteration_copy(owner, &[], crate::bytecode::Resets::listed(&[]), |_, _| unreachable!()).unwrap();
        let shared_storage = shared.dense_storage().unwrap();
        assert_eq!(EnvironmentSlots::read_storage(shared_storage, 0), None);
        assert_eq!(EnvironmentSlots::write_storage(shared_storage, 0, Value::TRUE), None);
        assert!(matches!(shared.state(0), Some(EnvironmentSlotState::Shared(value)) if value == owner));
    }
    // Scoped descriptors did not consume the owner or leave it marked/uninitialized.
    assert_eq!(slots.dense_plain(0), Some(Value::integer(7)));
    assert_eq!(slots.dense_plain(1), None);
    assert_eq!(slots.dense_plain(2), Some(Value::integer(99)));
}

#[test]
fn wide_environment_clones_store_only_the_slots_they_do_not_share() {
    let mut heap = Heap::new();
    let width = crate::heap::SPARSE_CLONE_MIN_SLOTS + 8;
    let kept = heap.alloc(Cell::Error("kept".into()));
    let environment = |heap: &mut Heap, slots: EnvironmentSlots| {
        heap.alloc_environment_record(crate::heap::EnvironmentRecord {
            parent: Value::NULL,
            function: 0,
            slots,
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(0),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: Vec::new().into(),
                with_objects: Box::default(),
            }),
        })
    };
    let mut values = (0..width).map(|slot| Value::number(slot as f64)).collect::<Vec<_>>();
    values[width - 1] = kept;
    let owner = environment(&mut heap, values.into());
    let first = heap.clone_environment_slots(owner, &[3], crate::bytecode::Resets::listed(&[5])).unwrap();
    // A length and owner, then the fresh and reset slots, each with its index.
    assert_eq!((first.len(), first.0.len()), (width, 6));
    assert!(first.dense_storage().is_none());
    let first = environment(&mut heap, first);
    assert_eq!(heap.environment_slot(first, 3), Some(Value::number(3.0)));
    assert_eq!(heap.environment_slot(first, 5), Some(Value::UNDEFINED));
    assert_eq!(heap.environment_slot(first, 7), Some(Value::number(7.0)));
    assert_eq!(heap.environment_slot(first, width), None);
    // Writes reach whichever environment owns the slot.
    *heap.environment_slot_mut(first, 3).unwrap() = Value::number(30.0);
    *heap.environment_slot_mut(first, 7).unwrap() = Value::number(70.0);
    assert_eq!(heap.environment_slot(owner, 3), Some(Value::number(3.0)));
    assert_eq!(heap.environment_slot(owner, 7), Some(Value::number(70.0)));
    // A copy of a copy shares the first copy's own slots and the original's others.
    let second = heap.clone_environment_slots(first, &[], crate::bytecode::Resets::listed(&[])).unwrap();
    assert_eq!(second.0.len(), 6);
    let second = environment(&mut heap, second);
    assert_eq!(heap.environment_slot_owner(second, 3), Some(first));
    assert_eq!(heap.environment_slot_owner(second, 7), Some(owner));
    assert_eq!(heap.environment_slot(second, 3), Some(Value::number(30.0)));
    // The copies keep the original and its values alive.
    heap.collect_with_object_roots([second], |_, _, _, _, _| {}, None, None, true);
    assert_eq!(heap.environment_slot(second, width - 1), Some(kept));
    assert!(matches!(heap.get(kept), Some(Cell::Error(_))));
    // A narrow environment's copy stays dense.
    let narrow = environment(&mut heap, vec![Value::TRUE; 2].into());
    assert_eq!(heap.clone_environment_slots(narrow, &[0], crate::bytecode::Resets::listed(&[])).unwrap().0.len(), 2);
}

/// The owners remembered for the next minor collection, of every family these tests use.
/// The old owners the next minor collection traces: those the barriers logged, and those the
/// last minor collection carried (`Heap::carried_owners`).
fn remembered_owners(heap: &Heap) -> usize {
    heap.remembered.len()
        + heap.object_records.remembered_count()
        + heap.array_records.remembered_count()
        + heap.carried_owners.len()
}

#[test]
fn aged_survivors_die_at_the_next_minor_collection_or_become_old() {
    let mut heap = heap_at(PromotionAge::Second);
    let allocate = |heap: &mut Heap| {
        let cell = heap.alloc(Cell::Error("aged".into()));
        let object = heap.alloc_object(plain_object());
        assert!(cell.legacy_heap_index().is_some() && object.direct_object_index().is_some());
        [cell, object]
    };
    let dying = allocate(&mut heap);
    let surviving = allocate(&mut heap);
    collect_minor(&mut heap, dying.into_iter().chain(surviving));
    for value in dying.into_iter().chain(surviving) {
        assert!(heap.is_live(value) && !heap.is_old(value), "{value:?} survives young");
    }
    collect_minor(&mut heap, surviving);
    for value in dying {
        assert!(!heap.is_live(value), "{value:?} dies young");
    }
    for value in surviving {
        assert!(heap.is_live(value) && heap.is_old(value), "{value:?} survives old");
    }
    // Old, they outlive minor collections.
    collect_minor(&mut heap, []);
    assert!(surviving.iter().all(|value| heap.is_live(*value)));
    heap.collect([]);
    assert!(surviving.iter().all(|value| !heap.is_live(*value)));
}

/// An aged `owner` takes a young `child` past the barrier, which logs only old owners; the
/// minor collection that promotes the owner remembers it, so the next one keeps the child,
/// which only the owner reaches, and promotes it.
fn promoted_owner_remembers_its_young_child(
    owner: impl FnOnce(&mut Heap) -> Value,
    child: impl FnOnce(&mut Heap) -> Value,
    store: impl FnOnce(&mut Heap, Value, Value),
) {
    let mut heap = heap_at(PromotionAge::Second);
    let owner = owner(&mut heap);
    collect_minor(&mut heap, [owner]);
    assert!(heap.is_live(owner) && !heap.is_old(owner));
    let child = child(&mut heap);
    store(&mut heap, owner, child);
    assert_eq!(remembered_owners(&heap), 0, "the barrier logs no young owner");
    collect_minor(&mut heap, [owner]);
    assert!(heap.is_live(child) && !heap.is_old(child));
    assert_eq!(remembered_owners(&heap), 1, "the promoted owner may hold a young child");
    collect_minor(&mut heap, []);
    assert!(heap.is_live(owner) && heap.is_old(owner));
    assert!(heap.is_live(child) && heap.is_old(child));
    // The owner, not written since it was carried, holds nothing young and is dropped; the
    // child just promoted is carried once.
    assert!(!heap.carried_owners.contains(&owner));
    assert_eq!(remembered_owners(&heap), 1);
    collect_minor(&mut heap, []);
    assert_eq!(remembered_owners(&heap), 0, "nothing was written or promoted");
    collect_minor(&mut heap, []);
    assert!(heap.is_live(child) && remembered_owners(&heap) == 0);
    heap.collect([]);
    assert!(!heap.is_live(owner) && !heap.is_live(child));
}

#[test]
fn promoted_cells_remember_their_young_children() {
    promoted_owner_remembers_its_young_child(
        |heap| {
            heap.alloc_set(plain_object(), Default::default())
        },
        |heap| heap.alloc(Cell::Error("young child".into())),
        |heap, owner, child| {
            if let Some(entries) = heap.set_entries_mut(owner) {
                // The set is only traced, never searched, so the key hash is immaterial.
                entries.push(child, 0);
            }
        },
    );
}

#[test]
fn promoted_object_records_remember_their_young_children() {
    promoted_owner_remembers_its_young_child(
        |heap| heap.alloc_object(plain_object()),
        |heap| heap.alloc_object(plain_object()),
        |heap, owner, child| heap.object_record_mut(owner).unwrap().set_inline_property(0, child),
    );
}

#[test]
fn promoted_array_records_remember_their_young_string_elements() {
    promoted_owner_remembers_its_young_child(
        |heap| {
            heap.alloc_array(ArrayObject::new(plain_object(), vec![Value::UNDEFINED]))
        },
        |heap| heap.alloc_string("young element".into()),
        |heap, owner, child| heap.array_elements_mut(owner).unwrap().make_mut().set(0, child),
    );
}

#[test]
fn old_owners_stay_remembered_until_their_aged_children_are_promoted_or_freed() {
    let mut heap = heap_at(PromotionAge::Second);
    let [keeper, dropper] = [(); 2].map(|()| heap.alloc_object(plain_object()));
    heap.collect([keeper, dropper]);
    let [kept, dropped] = [(); 2].map(|()| heap.alloc_object(plain_object()));
    heap.object_record_mut(keeper).unwrap().set_inline_property(0, kept);
    heap.object_record_mut(dropper).unwrap().set_inline_property(0, dropped);
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_live(dropped));
    assert_eq!(remembered_owners(&heap), 2, "both owners hold an aged child");

    heap.object_record_mut(dropper).unwrap().set_inline_property(0, Value::UNDEFINED);
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_old(kept));
    assert!(!heap.is_live(dropped), "an aged child no owner holds dies at a minor collection");
    assert!(heap.is_old(keeper) && heap.is_old(dropper));
    // The keeper, not written since it was carried, is dropped; the written dropper and the
    // promoted child are carried once more, and nothing after a minor with no writes.
    assert!(!heap.carried_owners.contains(&keeper));
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_old(kept));
    assert_eq!(remembered_owners(&heap), 0);
}

#[test]
fn weak_entries_of_aged_keys_and_targets_are_pruned_when_they_die() {
    for old_weak_cells in [true, false] {
        let mut heap = heap_at(PromotionAge::Second);
        let map = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::default(),
        });
        let weak_ref = heap.alloc(Cell::WeakRef {
            object: Box::new(plain_object()),
            target: None,
        });
        if old_weak_cells {
            heap.collect([map, weak_ref]);
        }
        let [key, value, referent] = [(); 3].map(|()| heap.alloc_object(plain_object()));
        if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
            entries.insert((key, value));
        }
        let target = heap.weak_handle(referent).unwrap();
        if let Some(Cell::WeakRef { target: slot, .. }) = heap.get_mut(weak_ref) {
            *slot = Some(target);
        }
        collect_minor(&mut heap, [map, weak_ref, key, referent]);
        assert!([key, value, referent].iter().all(|value| heap.is_live(*value) && !heap.is_old(*value)));

        collect_minor(&mut heap, [map, weak_ref]);
        assert!(![key, value, referent].iter().any(|value| heap.is_live(*value)));
        assert!(matches!(heap.get(map), Some(Cell::WeakMap { entries, .. }) if entries.iter().next().is_none()));
        assert!(matches!(heap.get(weak_ref), Some(Cell::WeakRef { target: None, .. })));
    }
}

#[test]
fn ephemeron_values_of_young_rooted_keys_survive_until_promoted() {
    for old_map in [true, false] {
        let mut heap = heap_at(PromotionAge::Second);
        let map = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::default(),
        });
        if old_map {
            heap.collect([map]);
        }
        let [key, value] = [(); 2].map(|()| heap.alloc_object(plain_object()));
        if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
            entries.insert((key, value));
        }
        for _ in 0..=PromotionAge::Second.minors() {
            collect_minor(&mut heap, [map, key]);
            assert!(heap.is_live(value), "the key keeps its value");
        }
        assert!(heap.is_old(key) && heap.is_old(value));
        heap.collect([map]);
        assert!(!heap.is_live(key) && !heap.is_live(value));
    }
}

#[test]
fn full_collection_promotes_aged_survivors() {
    let mut heap = heap_at(PromotionAge::Second);
    let cell = heap.alloc(Cell::Error("aged".into()));
    let object = heap.alloc_object(plain_object());
    collect_minor(&mut heap, [cell, object]);
    heap.collect([cell, object]);
    assert!(heap.is_old(cell) && heap.is_old(object));
    assert!(heap.young.count() == 0 && heap.aged.iter().all(|ages| *ages == 0));
    collect_minor(&mut heap, []);
    assert!(heap.is_live(cell) && heap.is_live(object), "old survivors outlive minor collections");
}

#[test]
fn promotion_age_may_change_between_minor_collections() {
    for (first, second) in [(PromotionAge::Second, PromotionAge::First), (PromotionAge::First, PromotionAge::Second)] {
        let mut heap = heap_at(first);
        let [dying, surviving] = [(); 2].map(|()| heap.alloc_object(plain_object()));
        let cell = heap.alloc(Cell::Error("survivor".into()));
        collect_minor(&mut heap, [dying, surviving, cell]);
        heap.set_promotion_age_for_test(second);
        collect_minor(&mut heap, [surviving, cell]);
        assert!(!heap.is_live(dying) || !first.ages());
        assert!(heap.is_live(surviving) && heap.is_live(cell));
        assert!(heap.is_old(surviving) && heap.is_old(cell));
    }
}

#[test]
fn restored_sparse_slot_marks_survive_minor_and_full_collection() {
    for count in [0, 1, u64::BITS as usize, u64::BITS as usize + 1, SLOTS_PER_SLAB + 1] {
        let mut heap = Heap::new();
        let cells: Vec<_> = (0..count).map(|index| heap.alloc(Cell::Error(index.to_string()))).collect();
        // Alternating retained slots exercise both bits at word/slab boundaries.
        let retained: Vec<_> = cells.iter().step_by(2).copied().collect();
        heap.collect(retained.iter().copied());
        let mut writer = crate::snapshot::Writer::new();
        heap.write_snapshot(&mut writer).unwrap();
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let mut restored = Heap::read_snapshot(&mut reader, MemoryPolicy::default()).unwrap();
        reader.finish().unwrap();
        for (index, &cell) in cells.iter().enumerate() {
            assert_eq!(restored.is_live(cell), index % 2 == 0);
            if index % 2 == 0 { assert!(restored.is_old(cell)); }
        }
        collect_minor(&mut restored, []);
        assert!(retained.iter().all(|&cell| restored.is_live(cell)));
        restored.collect([]);
        assert!(retained.iter().all(|&cell| !restored.is_live(cell)));
    }
}

#[test]
fn snapshots_restore_aged_survivors_as_old() {
    let mut heap = heap_at(PromotionAge::Second);
    let cell = heap.alloc(Cell::Error("aged".into()));
    let object = heap.alloc_object(plain_object());
    collect_minor(&mut heap, [cell, object]);
    let mut writer = crate::snapshot::Writer::new();
    heap.write_snapshot(&mut writer).unwrap();
    let bytes = writer.finish();
    let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
    let mut restored = Heap::read_snapshot(&mut reader, MemoryPolicy::default()).unwrap();
    restored.set_promotion_age_for_test(PromotionAge::Second);
    assert!(restored.is_old(cell) && restored.is_old(object));
    collect_minor(&mut restored, []);
    assert!(restored.is_live(cell) && restored.is_live(object));
    // The original's aged survivors still die young.
    collect_minor(&mut heap, []);
    assert!(!heap.is_live(cell) && !heap.is_live(object));
}

#[test]
fn a_rope_flattened_and_freed_between_collections_leaves_the_payload_ledger_balanced() {
    use crate::vm::wtf16::JsString;
    let mut heap = Heap::new();
    let kept = heap.alloc_string(JsString::from_str(&"k".repeat(4096)));
    collect_minor(&mut heap, [kept]);
    let before = heap.payload_bytes;
    assert!(before > 0);
    // Halves longer than a rope leaf join as a rope, which owns no units until read.
    let half = JsString::from_str(&"a".repeat(1024));
    let mut rope = half.clone();
    rope.push_js_string(&half);
    let value = heap.alloc_string(rope.clone());
    assert_eq!(heap.string(value).map(JsString::flat_unit_bytes), Some(0));
    assert_eq!(rope.units().len(), 2048);
    drop(rope);
    // The freed rope releases the units it flattened, which the ledger now holds.
    collect_minor(&mut heap, [kept]);
    assert_eq!(heap.payload_bytes, before);
}

#[test]
fn reclaimable_programs_make_a_full_collection_due_as_old_growth() {
    for policy in crate::MemoryPolicy::ALL {
        let mut heap = Heap::new();
        heap.set_memory_policy(policy);
        collect_minor(&mut heap, []);
        assert!(!heap.should_full_collect());
        let growth = heap.pacing().minimum_program_growth;
        heap.charge_reclaimable_program(growth);
        assert!(heap.should_full_collect());
        heap.collect_with_object_roots([], |_, _, _, _, _| {}, None, None, true);
        assert!(!heap.should_full_collect());
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn memory_census_counts_array_allocations_once_across_storage_and_collection() {
    let mut heap = Heap::new();
    let empty_direct = heap.alloc_array(ArrayObject::new(plain_object(), ArrayElements::new()));
    let empty = heap.memory_composition();
    assert_eq!(empty.dense_array_backing_count, 0);
    assert_eq!(empty.dense_array_elements_bytes, 0);
    assert_eq!(empty.dense_array_headers_bytes, 0);

    let source = ArrayElements::from([Value::integer(1), Value::integer(2), Value::integer(3)]);
    let source_payload_bytes = source.capacity() * size_of::<Value>();
    let header_bytes = source.allocated_bytes() - source_payload_bytes;
    let direct = heap.alloc_array(ArrayObject::new(plain_object(), source.clone()));
    assert!(direct.direct_array_index().is_some());
    let shared = heap.memory_composition();
    assert_eq!(shared.dense_array_backing_count, 1);
    assert_eq!(shared.dense_array_elements_bytes, source_payload_bytes);
    assert_eq!(shared.dense_array_headers_bytes, header_bytes);

    // A double has no narrow form, so the collection below keeps the grown backing wide.
    heap.array_elements_mut(direct).unwrap().make_mut().push(Value::number(0.5));
    let grown = heap.array_elements(direct).unwrap();
    assert!(grown.capacity() > source.capacity());
    let grown_payload_bytes = grown.capacity() * size_of::<Value>();
    assert_eq!(&*source.as_slice(), &[Value::integer(1), Value::integer(2), Value::integer(3)]);
    heap.collect([empty_direct, direct]);
    let detached = heap.memory_composition();
    assert_eq!(detached.dense_array_backing_count, 1);
    assert_eq!(detached.dense_array_elements_bytes, grown_payload_bytes);
    assert_eq!(detached.dense_array_headers_bytes, header_bytes);

    heap.array_elements_mut(direct).unwrap().make_mut().truncate(0);
    assert!(heap.array_elements(direct).unwrap().is_empty());
    let reserved_empty = heap.memory_composition();
    assert_eq!(reserved_empty.dense_array_backing_count, 1);
    assert_eq!(reserved_empty.dense_array_elements_bytes, grown_payload_bytes);
    assert_eq!(reserved_empty.dense_array_headers_bytes, header_bytes);

    heap.collect([]);
    let collected = heap.memory_composition();
    assert_eq!(collected.dense_array_backing_count, 0);
    assert_eq!(collected.dense_array_elements_bytes, 0);
    assert_eq!(collected.dense_array_headers_bytes, 0);
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn memory_census_accounts_for_direct_and_legacy_vectors_across_collection() {
    let mut heap = Heap::new();
    let before = heap.memory_composition();
    let direct = heap.alloc_v128([0x5a; crate::wasm::V128_BYTES]);
    assert!(direct.direct_v128_index().is_some());
    let legacy = heap.alloc_in_slot(Cell::WasmV128([0x66; crate::wasm::V128_BYTES]), false);
    assert!(legacy.direct_v128_index().is_none());
    let census = heap.memory_composition();
    assert_eq!(census.cell_counts[CellKind::WasmV128 as usize], 2);
    assert_eq!(census.direct_records, before.direct_records + 1);
    assert_eq!(census.occupied_slots, before.occupied_slots + 2);
    assert_eq!(census.v128_record_used_bytes, crate::wasm::V128_BYTES);
    assert!(census.v128_record_reserved_bytes >= census.v128_record_used_bytes);
    assert_eq!(census.used_bytes() - before.used_bytes(), crate::wasm::V128_BYTES + census.slot_size_bytes);
    assert_eq!(census.cell_counts.iter().sum::<usize>(), census.occupied_slots);
    let variant = census.cell_variant_names.iter().position(|name| *name == "wasm_v128").unwrap();
    assert_eq!(census.cell_variant_counts[variant], 2);
    heap.collect([direct]);
    let rooted = heap.memory_composition();
    assert_eq!(rooted.cell_counts[CellKind::WasmV128 as usize], 1);
    assert_eq!(rooted.v128_record_used_bytes, crate::wasm::V128_BYTES);
    assert_eq!(heap.v128(direct), Some([0x5a; crate::wasm::V128_BYTES]));
    heap.collect([]);
    let empty = heap.memory_composition();
    assert_eq!(empty.cell_counts[CellKind::WasmV128 as usize], 0);
    assert_eq!(empty.v128_record_used_bytes, 0);
}

#[test]
fn slot_type_admission_reads_live_storage_and_rebuilds_after_remapping() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    let layouts = [(0, None), (1, None), (2, Some((1, 1)))];
    heap.register_shape_layouts(&layouts);
    let link = heap.alloc_object(plain_object());
    let packed = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::NULL]);
    let arena = heap.alloc_object_with_properties(Value::NULL, 2, &[link, Value::number(0.5)]);
    let extras = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::UNDEFINED]);
    heap.object_record_mut(extras).unwrap().set_extensible(false);
    let deleted = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::DELETED]);
    assert!(heap.object(packed).unwrap().record_only().unwrap().inline_properties().is_some());
    assert!(heap.object(arena).unwrap().record_only().unwrap().inline_properties().is_none());
    heap.track_slot_representations(&layouts);
    let nullable = Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH);
    assert_eq!(heap.slot_type(1, 0), nullable);
    assert_eq!(heap.slot_type(2, 0), nullable);
    assert_eq!(heap.slot_type(2, 1), Type::NUMBER);
    assert_eq!(heap.unrepresented_slots(), 0);

    // A fresh admission reflects live contents, not stale observation history.
    for object in [packed, arena, extras, deleted] { heap.property_set(object, 0, link); }
    heap.track_slot_representations(&layouts);
    assert_eq!(heap.slot_type(1, 0), Type::DIRECT_OBJECT_FAMILY);
    // Reassign shape identities and re-establish the parent-owned slot projection.
    let remapped = [(0, None), (0, None), (1, None), (2, Some((2, 1)))];
    heap.remap_live_object_shapes(&[0, 2, 3], &remapped);
    assert_eq!(heap.slot_type(2, 0), Type::DIRECT_OBJECT_FAMILY);
    assert_eq!(heap.slot_type(3, 0), Type::DIRECT_OBJECT_FAMILY);
    assert_eq!(heap.slot_type(3, 1), Type::NUMBER);
    assert_eq!(heap.unrepresented_slots(), 0);
}

#[test]
fn raw_slot_admission_records_write_domain_before_exposing_storage() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    let layouts = [(0, None), (1, None), (3, None)];
    heap.register_shape_layouts(&layouts);
    let packed = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1)]);
    let object = heap.alloc_object_with_properties(Value::NULL, 2,
        &[Value::NULL, Value::integer(1), Value::integer(2)]);
    let child = heap.alloc_object(plain_object());
    heap.track_slot_representations(&layouts);
    // Declined storage does not admit a write capability.
    assert!(unsafe { heap.flat_property_mut_ptr_unchecked(packed, 0, Type::ANY) }.is_none());
    assert_eq!(heap.slot_type(1, 0), Type::INT32);
    let pointer = unsafe { heap.flat_property_mut_ptr_unchecked(object, 0, Type::ANY) }.unwrap();
    assert_eq!(heap.slot_type(2, 0), Type::ANY);
    // A reference-capable region logs its owner before its no-GC raw writes.
    heap.object_store_mut(object, object).unwrap();
    unsafe { pointer.write(child); }
    assert_eq!(heap.unrepresented_slots(), 0);
}

#[test]
fn object_publication_admits_preinitialized_slots_after_census() {
    use crate::typed_ir::Type;
    let publishers: [(&str, fn(&mut Heap, Object) -> Value); 1] = [
        ("object", Heap::alloc_object),
    ];
    for (publisher, publish) in publishers {
        for storage in ["inline", "arena", "extras"] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None)]);
            assert_eq!(heap.slot_type(1, 0), Type::NEVER);
            let value = if storage == "arena" { Value::number(0.5) } else { Value::NULL };
            let (properties, inline) = heap.initial_object_storage(1, &[value], storage != "arena");
            let mut object = Object::with_property_storage(Value::NULL, properties, inline);
            if storage == "extras" { object.set_extensible(false); }
            let published = publish(&mut heap, object);
            assert!(published.direct_object_index().is_some());
            assert_eq!(heap.slot_type(1, 0), Type::immediate(value), "{publisher}/{storage}");
            assert_eq!(heap.unrepresented_slots(), 0, "{publisher}/{storage}");
            assert_eq!(heap.property_get_ref(heap.object(published).unwrap(), 0), Some(value));
        }
    }
}

#[test]
fn unfilled_object_records_only_the_values_that_fill_it() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    heap.track_slot_representations(&[(0, None), (1, None)]);
    let header = crate::heap::InlineObjectHeader::of(Value::NULL, 1);
    let object = heap.alloc_unfilled_object_under_flagged_prototype(header);
    assert_eq!(heap.slot_type(1, 0), Type::NEVER);
    heap.fill_new_object_slot(object, 0, Value::NULL);
    assert_eq!(heap.slot_type(1, 0), Type::immediate(Value::NULL));
    assert_eq!(heap.unrepresented_slots(), 0);
    assert_eq!(heap.property_get_ref(heap.object(object).unwrap(), 0), Some(Value::NULL));
}

#[test]
fn shape_departures_retire_readers_through_each_storage_path() {
    use crate::{code_dependencies::{DependentCode, SlotDependency}, typed_ir::Type};
    struct Reader(std::cell::Cell<bool>);
    impl DependentCode for Reader {
        fn is_published(&self) -> bool { self.0.get() }
        fn retire(&self) { self.0.set(false); }
    }
    #[derive(Clone, Copy, Debug)]
    enum Change { Shape, Replace, Copy, Append, Remap }
    for change in [Change::Shape, Change::Replace, Change::Copy, Change::Append, Change::Remap] {
        for inline in [false, true] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None), (1, None), (2, Some((1, 1)))]);
            let (storage, values) = heap.initial_object_storage(1, &[Value::integer(1)], inline);
            let object = heap.alloc_object(Object::with_property_storage(Value::NULL, storage, values));
            let reader: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.subscribe_slot(SlotDependency { shape: 1, slot: 0, ty: Type::INT32 }, &reader));
            let unrelated: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.properties.subscribe_shape(2, &unrelated));
            // No departure: a repeated shape and storage descriptor preserve the
            // shape assumption (physical storage needs its own proof).
            heap.set_object_shape(object, 1);
            let (data, arena) = heap.object_and_arena_mut(object).unwrap();
            let properties = data.properties();
            data.set_properties(properties, arena);
            assert!(reader.is_published());
            match change {
                Change::Shape => heap.set_object_shape(object, 2),
                Change::Replace => heap.initialize_object_properties(object, 2, &[Value::integer(3)]),
                Change::Copy => {
                    let source = heap.alloc_object_with_properties(Value::NULL, 2, &[Value::integer(3)]);
                    let source = heap.object(source).unwrap().record_only().unwrap().clone();
                    assert!(heap.copy_property_storage(&source, object));
                }
                Change::Append => heap.property_add(object, Value::integer(3), 3),
                Change::Remap => heap.remap_live_object_shapes(&[0, 2, 1, 3],
                    &[(0, None), (1, None), (1, None), (2, Some((2, 1)))]),
            }
            assert!(!reader.is_published(), "{change:?}, inline={inline}");
            assert_eq!(unrelated.is_published(), !matches!(change, Change::Remap), "{change:?}");
            assert_eq!(heap.object(object).unwrap().shape(), if matches!(change, Change::Append) { 3 } else { 2 });
        }
    }
}

#[test]
fn named_property_publication_retires_readers_for_every_record_family() {
    use crate::{code_dependencies::{DependentCode, SlotDependency}, typed_ir::Type};
    struct Reader(std::cell::Cell<bool>);
    impl DependentCode for Reader {
        fn is_published(&self) -> bool { self.0.get() }
        fn retire(&self) { self.0.set(false); }
    }
    #[derive(Clone, Copy, Debug)]
    enum Family { Array, Function, Promise, Iterator, Date }
    for family in [Family::Array, Family::Function, Family::Promise, Family::Iterator, Family::Date] {
        for generic in [false, true] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None)]);
            heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1)]);
            let reader: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.subscribe_slot(SlotDependency { shape: 1, slot: 0, ty: Type::INT32 }, &reader));
            // Construct storage without publishing it: the publication boundary
            // must admit its contents, regardless of the allocator's fast path.
            let value = Value::number(0.5);
            let (properties, inline) = heap.initial_object_storage(1, &[value], false);
            let object = Object::with_property_storage(Value::NULL, properties, inline);
            // Arrays, iterators and functions have no cell: their records are allocated directly, both ways.
            let published = if let Family::Array = family {
                heap.alloc_array(ArrayObject::new(object, Vec::<Value>::new()))
            } else if let Family::Iterator = family {
                heap.alloc_iterator(IteratorRecord {
                    object, source: Value::NULL, kind: IteratorKind::Array, index: 0, done: false,
                    ext: IteratorExt::default(),
                })
            } else if let Family::Function = family {
                heap.alloc_function(FunctionRecord {
                    object, env: Value::NULL,
                    kind: FunctionKind::User(crate::vm::ProgramId::MAIN, 0),
                }.with_realm(Value::NULL))
            } else {
                let cell = match family {
                    Family::Array => unreachable!("an array is allocated as a record"),
                    Family::Promise => Cell::Promise(Box::new(PromiseCell { object, record: PromiseRecord::pending(None.into()) })),
                    Family::Function => unreachable!("a function is allocated as a record"),
                    Family::Iterator => unreachable!("an iterator is allocated as a record"),
                    Family::Date => Cell::Date { object: Box::new(object), milliseconds: 0.0 },
                };
                if generic { heap.alloc_cell(cell) } else {
                    match cell {
                        Cell::Promise(promise) => heap.alloc_promise(*promise),
                        cell => heap.alloc(cell),
                    }
                }
            };
            assert!(!reader.is_published(), "{family:?}, generic={generic}");
            assert_eq!(heap.slot_type(1, 0), Type::NUMBER);
            assert_eq!(heap.property_get_ref(heap.object(published).unwrap(), 0), Some(value));
            assert_eq!(heap.unrepresented_slots(), 0);
            heap.track_slot_representations(&[(0, None), (1, None)]);
            assert_eq!(heap.slot_type(1, 0), Type::NUMBER, "census includes {family:?}");
        }
    }
}

#[test]
fn memory_census_distinguishes_sparse_environment_storage_from_logical_slots() {
    let mut heap = Heap::new();
    let width = super::cell::SPARSE_CLONE_MIN_SLOTS;
    let slots = EnvironmentSlots::from_values(std::iter::repeat_n(Value::UNDEFINED, width));
    let (owner, _) = heap.alloc_environment(Value::UNDEFINED, 0, slots, EnvironmentScopeRef::of_program(0));
    let Some(crate::heap::EnvironmentRecord { slots, .. }) = heap.environment(owner) else { unreachable!() };
    let sparse = slots.iteration_copy(owner, &[], crate::bytecode::Resets::listed(&[]), |_, _| None).unwrap();
    assert!(sparse.dense().is_none());
    let sparse_bytes = sparse.0.allocated_bytes();
    let (copy, _) = heap.alloc_environment(owner, 0, sparse, EnvironmentScopeRef::of_program(0));
    let census = heap.memory_composition();
    assert_eq!(census.environment_logical_slots, 2 * width);
    assert_eq!(census.environment_sparse_count, 1);
    assert_eq!(census.environment_storage_bytes,
        (1 + super::cell::ENVIRONMENT_PREFIX_WORDS) * size_of::<usize>() + width * size_of::<EnvironmentSlot>() + sparse_bytes);
    assert!(sparse_bytes < width * size_of::<EnvironmentSlot>());
    heap.collect([copy]);
    assert_eq!(heap.memory_composition().environment_storage_bytes, census.environment_storage_bytes);
    heap.collect([]);
    assert_eq!(heap.memory_composition().environment_storage_bytes, 0);
}

#[test]
fn a_full_collection_sweeping_strings_on_a_worker_matches_the_serial_sweep() {
    use crate::vm::wtf16::{JsString, Parts};
    let run = |on_worker: bool| {
        let mut heap = Heap::new();
        if on_worker {
            heap.force_parallel_mark_for_test();
        }
        // Text shared with this frame: every dead heap copy and slice drops a clone of its `Rc`.
        let shared = JsString::from_str(&"s".repeat(200));
        let wide = JsString::from_units(&[0x3b1, 0x3b2, 0x3b3, 0x3b4]);
        let (mut kept, mut dead) = (Vec::new(), Vec::new());
        for i in 0..300_usize {
            let mut rope = JsString::from_str(&"r".repeat(1024));
            rope.push_js_string(&shared);
            let strings = [
                JsString::from_str("ab"),
                JsString::from_str(&"x".repeat(40 + i % 7)),
                shared.clone(),
                shared.substring(3..150),
                wide.clone(),
                rope,
            ];
            for (position, string) in strings.into_iter().enumerate() {
                let value = heap.alloc_string(string);
                if (i + position) % 2 == 0 { kept.push(value) } else { dead.push(value) }
            }
            // Parts are ASCII strings (`Vm::string_of_parts`).
            let mut parts = Parts::EMPTY;
            let (first, second) = (heap.alloc_string(JsString::from_str("part one")), heap.alloc_string(JsString::from_str(&"p".repeat(64))));
            kept.extend([first, second]);
            let Some(HeapReference::StringRecord(first)) = first.heap_reference() else { unreachable!() };
            let Some(HeapReference::StringRecord(second)) = second.heap_reference() else { unreachable!() };
            assert!(parts.push_text(first) && parts.push_text(second) && parts.push_number(i as f64));
            let value = heap.alloc_parts_string(parts);
            if i % 3 == 0 { kept.push(value) } else { dead.push(value) }
        }
        for (atom, &value) in dead.iter().chain(&kept).enumerate() {
            heap.record_string_atom(value, atom as u32);
        }
        let texts: Vec<String> = kept
            .iter()
            .map(|&value| heap.string(value).map_or_else(String::new, JsString::to_host_owned))
            .collect();
        heap.collect_with_object_roots(kept.iter().copied(), |_, _, _, _, _| {}, None, None, true);
        assert!(kept.iter().all(|&value| heap.is_live(value)));
        assert!(dead.iter().all(|&value| !heap.is_live(value)));
        assert!(dead.iter().all(|&value| heap.string_atom(value).is_none()), "a dead string's atom is forgotten");
        assert!(kept.iter().all(|&value| heap.string_atom(value).is_some()));
        for (&value, text) in kept.iter().zip(&texts) {
            assert_eq!(&heap.string(value).map_or_else(String::new, JsString::to_host_owned), text);
        }
        assert_eq!(shared.to_host_owned(), "s".repeat(200));
        (heap.string_records.live_count(), heap.payload_bytes)
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn reference_counts_match_the_heap_references_after_each_collection() {
    use std::collections::{HashMap, HashSet};
    let reachable = |roots: &[Value], fields: &HashMap<Value, Vec<Value>>| {
        let (mut seen, mut stack) = (HashSet::new(), roots.to_vec());
        while let Some(node) = stack.pop() {
            if node.is_heap() && seen.insert(node) {
                stack.extend(fields.get(&node).into_iter().flatten().copied());
            }
        }
        seen
    };
    for seed in 1..=24_u64 {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let mut next = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        let mut heap = Heap::new();
        // Two-slot objects keep their values inline; four-slot ones in an arena run.
        heap.register_property_shape(1, 2, None);
        heap.register_property_shape(2, 4, None);
        heap.set_rc(true);
        let mut fields: HashMap<Value, Vec<Value>> = HashMap::new();
        let make = |heap: &mut Heap, fields: &mut HashMap<Value, Vec<Value>>, wide: bool| {
            let node = if wide {
                heap.alloc_object_with_properties(Value::NULL, 2, &[Value::NULL; 4])
            } else {
                heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL)
            };
            fields.insert(node, vec![Value::NULL; if wide { 4 } else { 2 }]);
            node
        };
        let store = |heap: &mut Heap, fields: &mut HashMap<Value, Vec<Value>>, node: Value, slot: usize, value: Value| {
            heap.property_set(node, slot, value);
            fields.get_mut(&node).unwrap()[slot] = value;
        };
        let roots: Vec<Value> = (0..3).map(|index| make(&mut heap, &mut fields, index == 0)).collect();
        for step in 0..600 {
            let mut now: Vec<Value> = reachable(&roots, &fields).into_iter().collect();
            now.sort_by_key(|value| value.to_bits());
            match next(8) {
                0 | 1 => {
                    let node = make(&mut heap, &mut fields, next(3) == 0);
                    let owner = now[next(now.len())];
                    let slot = next(fields[&owner].len());
                    store(&mut heap, &mut fields, owner, slot, node);
                    store(&mut heap, &mut fields, node, 0, now[next(now.len())]);
                }
                2..=5 => {
                    let owner = now[next(now.len())];
                    let slot = next(fields[&owner].len());
                    let value = if next(3) == 0 { Value::NULL } else { now[next(now.len())] };
                    store(&mut heap, &mut fields, owner, slot, value);
                }
                full => {
                    if full == 7 && next(4) == 0 {
                        heap.collect(roots.iter().copied());
                    } else {
                        collect_minor(&mut heap, roots.iter().copied());
                    }
                    let live = reachable(&roots, &fields);
                    for &node in &live {
                        assert!(heap.is_live(node), "seed {seed} step {step}: a reachable node was freed");
                        let object = heap.object(node).unwrap();
                        for (slot, &value) in fields[&node].iter().enumerate() {
                            assert_eq!(heap.property_get_ref(object, slot), Some(value), "seed {seed} step {step}");
                        }
                    }
                    fields.retain(|node, _| live.contains(node));
                    let mismatches = heap.rc_mismatches();
                    assert!(mismatches.is_empty(), "seed {seed} step {step}: counts differ from references: {mismatches:?}");
                }
            }
        }
    }
}

#[test]
fn a_dropped_acyclic_old_structure_is_freed_by_the_next_minor_collection_under_counts() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 2, None);
    heap.set_rc(true);
    let root = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
    let mut chain = Vec::new();
    let mut owner = root;
    for _ in 0..50 {
        let node = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
        heap.property_set(owner, 0, node);
        chain.push(node);
        owner = node;
    }
    // A second edge into the chain's middle, from a node that stays.
    let keeper = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
    heap.property_set(root, 1, keeper);
    heap.property_set(keeper, 0, chain[40]);
    heap.collect([root]);
    assert!(heap.rc_mismatches().is_empty());
    heap.property_set(root, 0, Value::NULL);
    collect_minor(&mut heap, [root]);
    assert!(heap.rc_mismatches().is_empty());
    for (index, &node) in chain.iter().enumerate() {
        assert_eq!(heap.is_live(node), index >= 40, "node {index}");
    }
    assert!(heap.is_live(root) && heap.is_live(keeper));
    // A zero-count record a root names stays.
    heap.property_set(keeper, 0, Value::NULL);
    collect_minor(&mut heap, [root, chain[45]]);
    assert!(heap.is_live(chain[45]) && heap.is_live(chain[49]) && !heap.is_live(chain[40]));
    assert!(heap.rc_mismatches().is_empty());
    collect_minor(&mut heap, [root]);
    assert!(chain.iter().all(|&node| !heap.is_live(node)));
    assert!(heap.rc_mismatches().is_empty());
}
