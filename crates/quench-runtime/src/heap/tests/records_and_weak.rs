use super::*;

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
        declarations,
        ty: 0,
        fields: WasmGcFields::try_new(1, Some(descriptor), |_| Some(external)).unwrap(),
    });
    let tag = heap.alloc(Cell::WasmTag { object: Box::new(plain_object()), declarations: crate::WasmTypes::default(), ty: 0 });
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
    let left = heap.alloc(Cell::Object(plain_object()));
    let right = heap.alloc(Cell::Object(plain_object()));
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
        let outer_key = heap.alloc(Cell::Object(plain_object()));
        let inner_key = heap.alloc(Cell::Object(plain_object()));
        let value = heap.alloc(Cell::Object(plain_object()));
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
    let key = heap.alloc(Cell::Object(plain_object()));
    let mut maps = Vec::new();
    let mut values = Vec::new();
    for _ in 0..3 {
        let value = heap.alloc(Cell::Object(plain_object()));
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
