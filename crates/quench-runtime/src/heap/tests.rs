use super::*;
use crate::value_vec::ValueVec;
use std::rc::Rc;

fn plain_object() -> Object {
    Object::new(0)
}

#[test]
fn gc_headroom_factors_scale_live_cells_without_overflow() {
    let live = 781_054usize;
    assert_eq!(
        GcHeadroomFactor {
            numerator: 1,
            denominator: 2,
        }
        .allocation_headroom(live),
        390_527
    );
    assert_eq!(
        GcHeadroomFactor {
            numerator: 3,
            denominator: 4,
        }
        .allocation_headroom(live),
        585_790
    );
    assert_eq!(
        GcHeadroomFactor {
            numerator: 1,
            denominator: 1,
        }
        .allocation_headroom(live),
        live
    );
    assert_eq!(
        gc_allocation_headroom(LARGE_HEAP_MINIMUM_LIVE_CELLS - 1),
        32_767
    );
    assert_eq!(
        gc_allocation_headroom(LARGE_HEAP_MINIMUM_LIVE_CELLS),
        65_536
    );
}

#[cfg(feature = "profile-gc-time")]
#[test]
fn gc_time_profile_records_collection_phases_without_aggregate_counters() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::String("kept".into()));
    heap.collect([kept]);

    let profile = heap.gc_profile();
    assert!(profile.mark_nanos + profile.sweep_nanos + profile.mark_clear_nanos > 0);
    assert_eq!(heap.stats().1, 1);
}

#[test]
fn scope_slot_owners_survive_collection_and_release() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::String("kept".into()));
    let owner = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        function: 0,
        slots: vec![Value::number(1.0), kept].into_boxed_slice().into(),
        scope: Box::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: Vec::new().into(),
            with_objects: Box::default(),
        }),
    });
    let slots = heap.clone_environment_slots(owner, &[0]).unwrap();
    let view = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        function: 0,
        slots,
        scope: Box::new(crate::heap::EnvironmentScope {
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
    assert_eq!(
        size_of::<Object>(),
        size_of::<ValueVec>() + size_of::<[Value; INLINE_PROPERTY_COUNT]>()
    );
    assert_eq!(size_of::<Slot>(), size_of::<Option<Cell>>());
}

#[test]
fn strong_root_keeps_cell_alive_until_release() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::String("kept".into()));
    let dead = heap.alloc(Cell::String("dead".into()));
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
        bytes: Rc::new(vec![0; 16]),
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
fn weak_map_values_follow_ephemeron_key_reachability() {
    let mut heap = Heap::new();
    let weak_map = heap.alloc(Cell::WeakMap {
        object: Box::new(plain_object()),
        entries: Box::default(),
    });
    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::String("value".into()));
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
        object: plain_object(),
        elements: Rc::new(Vec::new()),
    });
    let value = heap.alloc(Cell::String("value".into()));
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
    let target = heap.alloc(Cell::String("target".into()));
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

    let replacement = heap.alloc(Cell::String("replacement".into()));
    assert_eq!(replacement.heap_index(), Some(target_slot));
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef { target: None, .. })
    ));
    assert!(heap.release_root(reference_root));
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
fn sparse_array_metadata_is_removed_before_a_slot_is_reused() {
    let mut heap = Heap::new();
    let array = heap.alloc(Cell::Array {
        object: plain_object(),
        elements: Rc::new(Vec::new()),
    });
    let slot = array.heap_index().unwrap();
    let index = MAX_DENSE_ARRAY_HOLE_LENGTH + 1;
    heap.sparse_set(array, index, Value::number(7.0));
    assert_eq!(heap.sparse_get(array, index), Some(Value::number(7.0)));

    heap.collect([]);

    let reused = heap.alloc(Cell::Array {
        object: plain_object(),
        elements: Rc::new(Vec::new()),
    });
    assert_eq!(reused.heap_index(), Some(slot));
    assert_eq!(heap.sparse_get(reused, index), None);
    assert_eq!(heap.sparse_length(reused), None);
}

#[test]
fn direct_array_records_trace_values_and_release_shared_backing() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(plain_object()));
    let first_child = heap.alloc(Cell::String("first".into()));
    let second_child = heap.alloc(Cell::String("second".into()));
    let shared = Rc::new(vec![first_child, Value::number(1.0)]);
    let first = heap.alloc(Cell::Array {
        object: plain_object(),
        elements: Rc::clone(&shared),
    });
    let second = heap.alloc(Cell::Array {
        object: plain_object(),
        elements: Rc::clone(&shared),
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
    assert_eq!(Rc::strong_count(heap.array(first).unwrap().1), 2);
    let first_root = heap.root(first);
    {
        let (_, elements) = heap.array_mut(first).unwrap();
        Rc::make_mut(elements)[1] = second_child;
    }
    assert_eq!(heap.collect([]), Vec::new());
    assert!(heap.is_live(first));
    assert!(!heap.is_live(second));
    assert!(heap.is_live(first_child));
    assert!(heap.is_live(second_child));
    let (_, elements) = heap.array(first).unwrap();
    assert_eq!(elements[1], second_child);
    assert_eq!(Rc::strong_count(elements), 1);
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
        object: plain_object(),
        elements: Rc::new(Vec::new()),
    });
    let child = heap.alloc(Cell::String("sparse child".into()));
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
        .map(|_| heap.alloc(Cell::String("garbage".into())))
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
    heap.register_property_shape(1, 1);
    heap.register_property_shape(2, 2);
    heap.register_property_shape(3, 3);
    let first = heap.alloc(Cell::String("first".into()));
    let replaced = heap.alloc(Cell::String("replaced".into()));
    let replacement = heap.alloc(Cell::String("replacement".into()));
    let third = heap.alloc(Cell::String("third".into()));
    let object = heap.alloc(Cell::Object(plain_object()));
    assert!(heap.object(object).unwrap().inline_properties().is_some());
    heap.object_record_mut(object)
        .unwrap()
        .set_extensible(false);
    heap.property_push(object, first);
    heap.object_record_mut(object).unwrap().set_shape(1);
    heap.property_push(object, replaced);
    heap.object_record_mut(object).unwrap().set_shape(2);
    assert!(heap.object(object).unwrap().inline_properties().is_some());
    heap.property_set(object, 1, replacement);
    heap.property_push(object, third);
    heap.object_record_mut(object).unwrap().set_shape(3);

    heap.collect([object]);
    let data = heap.object(object).unwrap();
    assert_eq!(data.shape(), 3);
    assert!(data.inline_properties().is_none());
    assert!(!data.is_extensible());
    assert_eq!(heap.property_get(data, 0), Some(first));
    assert_eq!(heap.property_get(data, 1), Some(replacement));
    assert_eq!(heap.property_get(data, 2), Some(third));
    assert!(heap.is_live(first));
    assert!(!heap.is_live(replaced));
    assert!(heap.is_live(replacement));
    assert!(heap.is_live(third));

    heap.collect([]);
    assert!(!heap.is_live(object));
    assert!(!heap.is_live(first));
    assert!(!heap.is_live(replacement));
    assert!(!heap.is_live(third));
}

#[test]
fn inline_property_values_are_gc_roots_until_deleted_or_the_owner_dies() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, INLINE_PROPERTY_COUNT);
    let kept = heap.alloc(Cell::String("kept".into()));
    let deleted = heap.alloc(Cell::String("deleted".into()));
    let object = heap.alloc_object_pair(1, kept, deleted);
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
    let data = heap.object_mut(object).unwrap();
    data.set_arguments_object();
    data.set_arguments_map(vec![0; 32]);
    data.set_module_namespace();
    data.set_module_bindings(vec![(0, crate::vm::program_store::ProgramId::MAIN, 0)]);
    data.set_deferred_module(Some(crate::ModuleSource {
        name: "module.js".into(),
        source: "export const value = 1;".into(),
        bytes: vec![1, 2, 3],
    }));
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
        fields: Box::new(vec![external]),
        descriptor: Some(descriptor),
    });
    let tag = heap.alloc(Cell::WasmTag {
        declarations: crate::WasmTypes::default(),
        ty: 0,
    });
    let exception = heap.alloc(Cell::WasmException {
        tag,
        payload: vec![owner],
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
            meta: Box::new(crate::heap::RegExpMeta {
                source: "a".into(),
                flags: String::new(),
                legacy_constructor: owner(constructor),
            }),
            matcher: Rc::new(quench_regexp::Regex::with_flags("a", Default::default()).unwrap()),
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
    let value = heap.alloc(Cell::String("kept".into()));
    let environment = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        function: 0,
        slots: vec![value].into_boxed_slice().into(),
        scope: Box::new(crate::heap::EnvironmentScope {
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
