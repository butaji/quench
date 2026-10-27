use super::*;
use crate::value_vec::ValueVec;
use std::rc::Rc;

fn plain_object() -> Object {
    Object::new(Value::NULL)
}

fn collect_minor(heap: &mut Heap, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
    heap.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, false)
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
    let mut heap = Heap::new();
    let owner = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), vec![Value::UNDEFINED]),
    });
    let sparse = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
    heap.collect([owner, sparse]);

    let child = heap.alloc(Cell::Error("dense child".into()));
    if let Some(elements) = heap.array_elements_mut(owner) {
        elements.make_mut()[0] = child;
    }
    let sparse_child = heap.alloc(Cell::Error("sparse child".into()));
    let sparse_index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(sparse, sparse_index, sparse_child);
    let garbage = heap.alloc(Cell::Error("unreachable young".into()));

    collect_minor(&mut heap, [owner, sparse]);
    assert!(heap.get(child).is_some());
    assert_eq!(heap.sparse_get(sparse, sparse_index), Some(sparse_child));
    assert!(heap.get(sparse_child).is_some());
    assert!(heap.get(garbage).is_none());

    if let Some(elements) = heap.array_elements_mut(owner) {
        elements.make_mut()[0] = Value::UNDEFINED;
    }
    heap.collect([owner, sparse]);
    assert!(heap.get(child).is_none());
}

#[test]
fn parallel_mark_matches_serial_verifier_for_full_and_minor_collections() {
    let mut heap = Heap::new();
    let children = (0..PARALLEL_MARK_MINIMUM_CELLS)
        .map(|_| heap.alloc(Cell::Error("parallel survivor".into())))
        .collect::<Vec<_>>();
    // Only a heap without records sweeps, and marks a minor collection, in parallel. Keep
    // this root in the legacy space so those paths remain covered.
    let root = heap.alloc(Cell::Set {
        object: Box::new(plain_object()),
        entries: Box::new(children.clone().into()),
    });

    heap.collect([root]);
    assert!(children.iter().all(|child| heap.get(*child).is_some()));

    let young = (0..128)
        .map(|_| heap.alloc(Cell::Error("remembered young survivor".into())))
        .collect::<Vec<_>>();
    if let Some(Cell::Set { entries, .. }) = heap.get_mut(root) {
        // The set is only traced, never searched, so the key hash is immaterial.
        young.iter().for_each(|child| entries.push(*child, 0));
    }
    collect_minor(&mut heap, [root]);
    assert!(young.iter().all(|child| heap.get(*child).is_some()));

    if let Some(Cell::Set { entries, .. }) = heap.get_mut(root) {
        entries.clear();
    }
    heap.collect([root]);
    assert!(children.iter().all(|child| heap.get(*child).is_none()));
    assert!(young.iter().all(|child| heap.get(*child).is_none()));
}

/// The links in `parallel_full_mark_follows_records_and_ephemerons_like_the_serial_mark`'s
/// chain of arrays: enough for many rounds.
const PARALLEL_TEST_CHAIN_LINKS: usize = 64;
/// The strings each array in that chain holds, past `PARALLEL_MARK_SPLIT_THRESHOLD`, so the
/// arrays' strings are marked as leaves while other workers mark the chain.
const PARALLEL_TEST_ARRAY_STRINGS: usize = 2 * PARALLEL_MARK_SPLIT_THRESHOLD;

/// Runs `scenario` on a heap marked serially and on one whose every full mark runs in
/// parallel rounds; each full collection's verifier (`debug_verify_full`) also re-marks the
/// heap serially and compares.
fn for_each_mark_mode(mut scenario: impl FnMut(&mut Heap)) {
    for parallel in [false, true] {
        let mut heap = Heap::new();
        if parallel {
            heap.force_parallel_mark_for_test();
        }
        scenario(&mut heap);
    }
}

#[test]
fn parallel_full_mark_follows_records_and_ephemerons_like_the_serial_mark() {
    // Each mode's mark counts, which must agree: both count each value once, when marked.
    #[cfg(feature = "profile-aggregate")]
    let mut marked_counts = Vec::new();
    for_each_mark_mode(|heap| {
        // Weak map keys reachable only through records: an object and an array record the
        // chain's first array holds, and a key nothing holds.
        let object_key = heap.alloc(Cell::Object(plain_object()));
        let array_key = heap.alloc(Cell::Array {
            object: ArrayObject::new(plain_object(), Vec::new()),
        });
        let lost_key = heap.alloc(Cell::Object(plain_object()));
        let [object_value, array_value, lost_value] =
            [(); 3].map(|()| heap.alloc(Cell::Object(plain_object())));
        // A value that is itself a key, whose own value waits for it in a later round.
        let nested_value = heap.alloc(Cell::Error("nested".into()));
        let mut entries = WeakMapEntries::default();
        entries.insert(object_key, object_value);
        entries.insert(array_key, array_value);
        entries.insert(lost_key, lost_value);
        entries.insert(object_value, nested_value);
        let map = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::new(entries),
        });
        let mut strings = Vec::new();
        let mut link = Value::UNDEFINED;
        for index in 0..PARALLEL_TEST_CHAIN_LINKS {
            let mut elements = (0..PARALLEL_TEST_ARRAY_STRINGS)
                .map(|string| heap.alloc_string(format!("{index}:{string}").into()))
                .collect::<Vec<_>>();
            strings.extend(elements.iter().copied());
            if index == 0 {
                elements.extend([object_key, array_key]);
            }
            // Each array reaches the previous one through an object record.
            let object = heap.alloc(Cell::Object(plain_object()));
            heap.object_mut(object).unwrap().set_inline_property(0, link);
            elements.push(object);
            link = heap.alloc(Cell::Array {
                object: ArrayObject::new(plain_object(), elements),
            });
        }
        let garbage = heap.alloc_string("garbage".into());
        assert!(link.direct_array_index().is_some() && strings[0].direct_string_index().is_some());

        heap.collect([map, link]);
        #[cfg(feature = "profile-aggregate")]
        marked_counts.push((heap.gc_profile().marked, heap.gc_profile().marked_kinds));
        assert!(strings.iter().all(|string| heap.is_live(*string)));
        for live in [object_key, array_key, object_value, array_value, nested_value] {
            assert!(heap.is_live(live), "{live:?} is reachable");
        }
        for dead in [lost_key, lost_value, garbage] {
            assert!(!heap.is_live(dead), "{dead:?} is unreachable");
        }

        heap.collect([map]);
        assert!(strings.iter().all(|string| !heap.is_live(*string)));
        for dead in [object_key, array_key, object_value, array_value, nested_value] {
            assert!(!heap.is_live(dead), "{dead:?} is unreachable");
        }
    });
    #[cfg(feature = "profile-aggregate")]
    assert_eq!(marked_counts[0], marked_counts[1], "parallel and serial marks count alike");
}

#[test]
fn parallel_sweep_releases_shared_array_payloads() {
    let mut heap = Heap::new();
    let elements = ArrayElements::from(vec![Value::UNDEFINED]);
    let arrays = (0..PARALLEL_MARK_MINIMUM_CELLS)
        .map(|_| {
            heap.alloc(Cell::Array {
                object: ArrayObject::new(plain_object(), elements.clone()),
            })
        })
        .collect::<Vec<_>>();
    drop(elements);

    heap.collect([]);

    assert!(arrays.iter().all(|array| heap.get(*array).is_none()));
    assert_eq!(heap.live_slots(), 0);
}

#[test]
fn minor_collection_handles_old_weak_maps_and_weak_refs() {
    let mut heap = Heap::new();
    let old_key = heap.alloc(Cell::Object(plain_object()));
    let old_value = heap.alloc(Cell::Object(plain_object()));
    let mut entries = WeakMapEntries::default();
    entries.insert(old_key, old_value);
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
        entries.insert(key, value);
    }
    let referent = heap.alloc(Cell::Object(plain_object()));
    let target = heap.weak_handle(referent).unwrap();
    if let Some(Cell::WeakRef { target: slot, .. }) = heap.get_mut(weak_ref) {
        *slot = Some(target);
    }

    collect_minor(&mut heap, [map, key, weak_ref]);
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
    let mut heap = Heap::new();
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
    heap.array_elements_mut(array).unwrap().make_mut()[0] = element;
    let garbage = heap.alloc(Cell::Object(plain_object()));
    // The barrier logged both owners, so they are young until the minor traces them.
    assert!(!heap.is_old(owner) && !heap.is_old(array));

    collect_minor(&mut heap, [owner, array]);
    assert!(heap.is_live(child) && heap.is_old(child));
    assert!(heap.is_live(element) && heap.is_old(element));
    assert!(!heap.is_live(garbage));
    assert!(heap.is_old(owner) && heap.is_old(array));

    // An old record that loses its last reference survives minors and dies at a full one.
    heap.object_mut(owner).unwrap().set_inline_property(0, Value::UNDEFINED);
    collect_minor(&mut heap, [owner, array]);
    assert!(heap.is_live(child));
    heap.collect([owner, array]);
    assert!(!heap.is_live(child));
    assert!(heap.is_live(element));
}

#[test]
fn records_written_twice_are_logged_once_per_cycle() {
    let mut heap = Heap::new();
    let owner = heap.alloc(Cell::Object(plain_object()));
    heap.collect([owner]);
    for value in 0..4 {
        heap.object_mut(owner)
            .unwrap()
            .set_inline_property(0, Value::number(f64::from(value)));
    }
    assert_eq!(heap.object_records.remembered_count(), 1);
    collect_minor(&mut heap, [owner]);
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
    let slots = heap.clone_environment_slots(owner, &[0], &[]).unwrap();
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
    // The prototype reference packs beside the shape and integrity bits, so a record is a
    // header the size of a property vector plus one payload word of narrow inline values.
    assert_eq!(size_of::<Object>(), size_of::<ValueVec>() + size_of::<u64>());
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
        entries.insert(key, value);
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
        entries.insert(key, value);
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
        entries.insert(key, value);
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
    let target = heap.alloc(Cell::Object(plain_object()));
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
    let target = heap.alloc(Cell::Object(plain_object()));
    let token = heap.alloc(Cell::Object(plain_object()));
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

    let replacement = heap.alloc(Cell::Object(plain_object()));
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
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), prefix),
    });
    let index = 2 * MAX_DENSE_ARRAY_HOLE_LENGTH;
    heap.sparse_set(array, index, Value::number(2.0));
    assert_eq!(heap.sparse_length(array), Some(index + 1));
    assert_eq!(heap.array_elements(array).map(|dense| dense.len()), Some(MAX_DENSE_ARRAY_HOLE_LENGTH));
    assert_eq!(heap.sparse_get(array, index), Some(Value::number(2.0)));
}

#[test]
fn sparse_array_metadata_is_removed_before_a_slot_is_reused() {
    let mut heap = Heap::new();
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
    let slot = array.heap_index().unwrap();
    let index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(array, index, Value::number(7.0));
    assert_eq!(heap.sparse_get(array, index), Some(Value::number(7.0)));

    heap.collect([]);

    let reused = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
    assert_eq!(reused.heap_index(), Some(slot));
    assert_eq!(heap.sparse_get(reused, index), None);
    assert_eq!(heap.sparse_length(reused), None);
}

#[test]
fn direct_array_records_trace_values_and_release_shared_backing() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(plain_object()));
    let first_child = heap.alloc(Cell::Error("first".into()));
    let second_child = heap.alloc(Cell::Error("second".into()));
    let shared = ArrayElements::from(vec![first_child, Value::number(1.0)]);
    let first = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), shared.clone()),
    });
    let second = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), shared.clone()),
    });
    drop(shared);

    assert!(first.direct_array_index().is_some());
    assert_eq!(object.direct_object_index(), Some(0));
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
        elements.make_mut()[1] = second_child;
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
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), Vec::new()),
    });
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
    let object = heap.alloc(Cell::Object(plain_object()));
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

#[test]
fn out_of_line_private_brand_keeps_its_home_alive() {
    let mut heap = Heap::new();
    let home = heap.alloc(Cell::Object(plain_object()));
    let instance = heap.alloc(Cell::Object(plain_object()));
    heap.object_record_mut(instance)
        .unwrap()
        .add_private_name(PrivateBrand { home, name: 0 });

    heap.collect([instance]);

    assert!(heap.is_live(instance));
    assert!(heap.is_live(home));

    heap.collect([]);

    assert!(!heap.is_live(instance));
    assert!(!heap.is_live(home));
}

#[test]
fn object_property_storage_migration_preserves_writes_and_gc_roots() {
    let mut heap = Heap::new();
    // Shape `n` holds `n` properties; the add past the inline values migrates them.
    let migrated = INLINE_PROPERTY_COUNT + 1;
    for shape in 1..=migrated {
        heap.register_property_shape(shape as u32, shape);
    }
    let first = heap.alloc(Cell::Error("first".into()));
    let replaced = heap.alloc(Cell::Error("replaced".into()));
    let replacement = heap.alloc(Cell::Error("replacement".into()));
    let fillers = (2..INLINE_PROPERTY_COUNT)
        .map(|_| heap.alloc(Cell::Error("filler".into())))
        .collect::<Vec<_>>();
    let last = heap.alloc(Cell::Error("last".into()));
    let object = heap.alloc(Cell::Object(plain_object()));
    assert!(heap.object(object).unwrap().inline_properties().is_some());
    heap.object_record_mut(object)
        .unwrap()
        .set_extensible(false);
    heap.property_add(object, first, 1);
    heap.property_add(object, replaced, 2);
    for (index, filler) in fillers.iter().enumerate() {
        heap.property_add(object, *filler, (3 + index) as u32);
    }
    assert!(heap.object(object).unwrap().inline_properties().is_some());
    heap.property_set(object, 1, replacement);
    heap.property_add(object, last, migrated as u32);

    heap.collect([object]);
    let data = heap.object(object).unwrap();
    assert_eq!(data.shape(), migrated as u32);
    assert!(data.inline_properties().is_none());
    assert!(!data.is_extensible());
    assert_eq!(heap.property_get(data, 0), Some(first));
    assert_eq!(heap.property_get(data, 1), Some(replacement));
    for (index, filler) in fillers.iter().enumerate() {
        assert_eq!(heap.property_get(data, 2 + index), Some(*filler));
    }
    assert_eq!(heap.property_get(data, migrated - 1), Some(last));
    assert!(heap.is_live(first));
    assert!(!heap.is_live(replaced));
    assert!(heap.is_live(replacement));
    assert!(heap.is_live(last));

    heap.collect([]);
    assert!(!heap.is_live(object));
    assert!(!heap.is_live(first));
    assert!(!heap.is_live(replacement));
    assert!(!heap.is_live(last));
}

#[test]
fn inline_property_values_are_gc_roots_until_deleted_or_the_owner_dies() {
    let mut heap = Heap::new();
    // A shape of two properties, the pair below, whatever the inline count.
    heap.register_property_shape(1, 2);
    let kept = heap.alloc(Cell::Error("kept".into()));
    let deleted = heap.alloc(Cell::Error("deleted".into()));
    let object = heap.alloc_object_pair(Value::NULL, 1, kept, deleted);
    heap.property_set(object, 1, Value::DELETED);

    heap.collect([object]);

    assert!(heap.is_live(object));
    assert!(heap.is_live(kept));
    assert!(!heap.is_live(deleted));

    heap.collect([]);

    assert!(!heap.is_live(object));
    assert!(!heap.is_live(kept));
}

#[cfg(feature = "profile-memory")]
#[test]
fn out_of_line_object_metadata_is_included_in_live_memory_totals() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(plain_object()));
    let before = heap.memory_stats().3;
    // An object lives in its record family or a legacy cell; `Heap::object_mut` finds either.
    let data = heap.object_mut(object).unwrap();
    data.set_arguments_object();
    data.set_arguments_map(vec![0; 32]);
    data.set_module_namespace();
    data.set_module_bindings(vec![(0, crate::vm::program_store::ProgramId::MAIN, 0)]);
    data.set_deferred_module(Some(crate::ModuleSource::text("module.js", "export const value = 1;")));
    data.add_private_name(PrivateBrand {
        home: object,
        name: 0,
    });

    assert!(heap.memory_stats().3 > before);
    assert!(heap.live_payload_bytes()[CellKind::Object as usize] > 0);
}

#[test]
fn wasm_scalar_and_gc_fields_follow_the_shared_strong_root_lifecycle() {
    let mut heap = Heap::new();
    let bits = 0x7ffc_1234_5678_9abc;
    let value = heap.alloc(Cell::WasmBits64(bits));
    // This allocator test checks edges; module admission owns declaration validity.
    let declarations = crate::WasmTypes::default();
    let external = heap.alloc(Cell::WasmExtern(value));
    let descriptor = heap.alloc(Cell::WasmBits64(!bits));
    let owner = heap.alloc(Cell::WasmGc {
        declarations: Box::new(declarations),
        ty: 0,
        fields: Box::new(WasmGcFields {
            values: vec![external],
            descriptor: Some(descriptor),
        }),
    });
    let tag = heap.alloc(Cell::WasmTag {
        declarations: crate::WasmTypes::default(),
        ty: 0,
    });
    let exception = heap.alloc(Cell::WasmException {
        tag,
        payload: Box::new(vec![owner]),
    });
    let root = heap.root(exception);
    heap.collect([]);
    assert!(heap.is_live(tag));
    assert!(heap.is_live(owner));
    assert!(heap.is_live(descriptor));
    assert!(matches!(heap.get(value), Some(Cell::WasmBits64(actual)) if *actual == bits));
    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(!heap.is_live(value));
    assert!(!heap.is_live(owner));
    assert!(!heap.is_live(external));
    assert!(!heap.is_live(descriptor));
    assert!(!heap.is_live(tag));
    assert!(!heap.is_live(exception));
}

#[test]
fn regexp_legacy_constructor_is_traced_through_live_instances() {
    let owners: [fn(Value) -> RegExpLegacyOwner; 2] =
        [RegExpLegacyOwner::Enabled, RegExpLegacyOwner::Disabled];
    for owner in owners {
        let mut heap = Heap::new();
        let constructor = heap.alloc(Cell::Object(plain_object()));
        let weak_constructor = heap.weak_handle(constructor).unwrap();
        let regexp = heap.alloc(Cell::RegExp {
            object: Box::new(plain_object()),
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
    let environment = heap.alloc(Cell::Environment {
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
    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::Object(plain_object()));
    let mut entries = WeakMapEntries::default();
    entries.insert(key, value);
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
    let left = heap.alloc(Cell::Object(plain_object()));
    let right = heap.alloc(Cell::Object(plain_object()));
    let mut entries = WeakMapEntries::default();
    entries.insert(left, right);
    entries.insert(right, left);
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
        let outer_key = heap.alloc(Cell::Object(plain_object()));
        let inner_key = heap.alloc(Cell::Object(plain_object()));
        let value = heap.alloc(Cell::Object(plain_object()));
        let mut inner_entries = WeakMapEntries::default();
        inner_entries.insert(inner_key, value);
        let inner = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::new(inner_entries),
        });
        let mut outer_entries = WeakMapEntries::default();
        outer_entries.insert(outer_key, inner);
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
    let key = heap.alloc(Cell::Object(plain_object()));
    let mut maps = Vec::new();
    let mut values = Vec::new();
    for _ in 0..3 {
        let value = heap.alloc(Cell::Object(plain_object()));
        let mut entries = WeakMapEntries::default();
        entries.insert(key, value);
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
fn wide_environment_clones_store_only_the_slots_they_do_not_share() {
    let mut heap = Heap::new();
    let width = crate::heap::SPARSE_CLONE_MIN_SLOTS + 8;
    let kept = heap.alloc(Cell::Error("kept".into()));
    let environment = |heap: &mut Heap, slots: EnvironmentSlots| {
        heap.alloc(Cell::Environment {
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
    let first = heap.clone_environment_slots(owner, &[3], &[5]).unwrap();
    // A length and owner, then the fresh and reset slots, each with its index.
    assert_eq!((first.len(), first.0.len()), (width, 6));
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
    let second = heap.clone_environment_slots(first, &[], &[]).unwrap();
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
    assert_eq!(heap.clone_environment_slots(narrow, &[0], &[]).unwrap().0.len(), 2);
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
