use super::*;
use crate::value_vec::ValueVec;
use std::rc::Rc;

fn plain_object() -> Object {
    Object::new(Value::NULL, ValueVec::new())
}

#[test]
fn resolved_binding_reference_keeps_its_slot_owner_alive() {
    let mut heap = Heap::new();
    let value = heap.alloc(Cell::String("kept".into()));
    let environment = heap.alloc(Cell::Environment {
        parent: Value::NULL,
        program: None,
        root_eval_scope: false,
        function: 0,
        slots: vec![value].into_boxed_slice(),
        dynamic_bindings: Vec::new(),
        with_objects: Vec::new(),
    });
    let reference = heap.alloc(Cell::BindingReference {
        environment,
        slot: 0,
        kind: crate::bytecode::LexicalBindingKind::Mutable,
    });
    heap.collect([reference]);
    assert!(heap.get(environment).is_some());
    assert!(heap.get(value).is_some());
    heap.collect([]);
    assert!(heap.get(reference).is_none());
    assert!(heap.get(environment).is_none());
    assert!(heap.get(value).is_none());
}

#[test]
fn object_side_metadata_is_out_of_line() {
    assert_eq!(
        size_of::<Object>(),
        size_of::<Value>() + size_of::<ValueVec>() + size_of::<Option<Box<()>>>()
    );
    assert_eq!(size_of::<Cell>(), size_of::<Slot>());
}

#[test]
fn strong_root_keeps_cell_alive_until_release() {
    let mut heap = Heap::new();
    let kept = heap.alloc(Cell::String("kept".into()));
    let dead = heap.alloc(Cell::String("dead".into()));
    let root = heap.root(kept);

    heap.collect([]);
    assert!(heap.get(kept).is_some());
    assert!(heap.get(dead).is_none());

    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(heap.get(kept).is_none());
}

#[test]
fn forged_heap_indices_are_rejected_at_the_access_boundary() {
    let mut heap = Heap::new();
    assert!(heap.get(Value::heap(99_999)).is_none());
    assert!(heap.get_mut(Value::heap(99_999)).is_none());
}

#[test]
fn array_buffer_backing_is_accounted_until_owner_collection() {
    let mut heap = Heap::new();
    let buffer = heap.alloc(Cell::ArrayBuffer {
        object: plain_object(),
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
        object: plain_object(),
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
        object: plain_object(),
        entries: WeakMapEntries::default(),
    });
    let key = heap.alloc(Cell::Object(plain_object()));
    let value = heap.alloc(Cell::String("value".into()));
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(weak_map) {
        entries.insert(key, value);
    }
    let map_root = heap.root(weak_map);
    let key_root = heap.root(key);
    heap.collect([]);
    assert!(heap.get(key).is_some());
    assert!(heap.get(value).is_some());
    assert!(heap.release_root(key_root));
    heap.collect([]);
    assert!(heap.get(key).is_none());
    assert!(heap.get(value).is_none());
    assert!(heap.release_root(map_root));
}

#[test]
fn weak_ref_target_is_cleared_after_collection() {
    let mut heap = Heap::new();
    let target = heap.alloc(Cell::Object(plain_object()));
    let target_handle = heap.weak_handle(target).unwrap();
    let reference = heap.alloc(Cell::WeakRef {
        object: plain_object(),
        target: Some(target_handle),
    });
    let reference_root = heap.root(reference);
    heap.collect([]);
    assert!(heap.get(target).is_none());
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef { target: None, .. })
    ));
    assert!(heap.release_root(reference_root));
}

#[test]
fn exhausted_heap_generations_retire_slots_without_weak_handle_aliasing() {
    let mut heap = Heap::new();
    let target = heap.alloc(Cell::String("old".into()));
    let slot = target.heap_index().unwrap() as usize;
    heap.generations[slot] = u32::MAX;
    let stale = heap.weak_handle(target).unwrap();

    heap.collect([]);
    assert!(heap.get(target).is_none());
    assert_eq!(heap.weak_value(stale), None);

    let replacement = heap.alloc(Cell::String("new".into()));
    assert_ne!(replacement.heap_index(), Some(slot as u32));
    assert_eq!(heap.weak_value(stale), None);
    assert_eq!(heap.retired_slots, 1);

    let next = heap.alloc(Cell::String("next".into()));
    assert_ne!(next.heap_index(), Some(slot as u32));
    assert_eq!(heap.retired_slots, 1);
}

#[test]
fn stale_weak_handles_cannot_resolve_reused_slots() {
    let mut heap = Heap::new();
    let first = heap.alloc(Cell::String("first".into()));
    let stale = heap.weak_handle(first).unwrap();
    heap.collect([]);
    let replacement = heap.alloc(Cell::String("replacement".into()));
    assert_eq!(first.heap_index(), replacement.heap_index());
    assert!(heap.weak_value(stale).is_none());
    let current = heap.weak_handle(replacement).unwrap();
    assert_eq!(heap.weak_value(current), Some(replacement));
}

#[test]
fn object_integrity_metadata_survives_collection() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(plain_object()));
    if let Some(Cell::Object(data)) = heap.get_mut(object) {
        data.set_extensible(false);
        data.set_frozen(true);
    }
    let garbage = (0..512)
        .map(|_| heap.alloc(Cell::String("garbage".into())))
        .collect::<Vec<_>>();
    heap.collect([object]);
    assert!(
        matches!(heap.get(object), Some(Cell::Object(data)) if !data.is_extensible() && data.is_frozen())
    );
    assert!(garbage.iter().all(|value| heap.get(*value).is_none()));
}

#[test]
fn out_of_line_private_brand_keeps_its_home_alive() {
    let mut heap = Heap::new();
    let home = heap.alloc(Cell::Object(plain_object()));
    let instance = heap.alloc(Cell::Object(plain_object()));
    heap.get_mut(instance)
        .and_then(Cell::object_mut)
        .unwrap()
        .add_private_name(PrivateBrand { home, name: 0 });

    heap.collect([instance]);

    assert!(heap.get(instance).is_some());
    assert!(heap.get(home).is_some());

    heap.collect([]);

    assert!(heap.get(instance).is_none());
    assert!(heap.get(home).is_none());
}

#[test]
fn object_property_storage_migration_preserves_writes_and_gc_roots() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 2);
    let first = heap.alloc(Cell::String("first".into()));
    let replaced = heap.alloc(Cell::String("replaced".into()));
    let replacement = heap.alloc(Cell::String("replacement".into()));
    let object = heap.alloc_object_pair(Value::NULL, 1, first, replaced);
    heap.get_mut(object)
        .and_then(Cell::object_mut)
        .unwrap()
        .set_extensible(false);

    let mut properties = heap.get(object).unwrap().object().unwrap().properties;
    heap.properties
        .migrate_to_dictionary_for_test(&mut properties);
    heap.get_mut(object)
        .and_then(Cell::object_mut)
        .unwrap()
        .properties = properties;
    heap.property_set(object, 1, replacement);

    heap.collect([object]);
    let data = heap.get(object).unwrap().object().unwrap();
    assert_eq!(data.shape(), 1);
    assert!(!data.is_extensible());
    assert_eq!(heap.property_get(data, 0), Some(first));
    assert_eq!(heap.property_get(data, 1), Some(replacement));
    assert!(heap.get(first).is_some());
    assert!(heap.get(replaced).is_none());
    assert!(heap.get(replacement).is_some());

    heap.collect([]);
    assert!(heap.get(object).is_none());
    assert!(heap.get(first).is_none());
    assert!(heap.get(replacement).is_none());
}

#[cfg(feature = "profile-memory")]
#[test]
fn out_of_line_object_metadata_is_included_in_live_memory_totals() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(plain_object()));
    let before = heap.memory_stats().3;
    let data = heap.get_mut(object).unwrap().object_mut().unwrap();
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
fn wasm_bits64_follow_the_shared_strong_root_lifecycle() {
    let mut heap = Heap::new();
    let bits = 0x7ffc_1234_5678_9abc;
    let value = heap.alloc(Cell::WasmBits64(bits));
    let root = heap.root(value);
    heap.collect([]);
    assert!(matches!(heap.get(value), Some(Cell::WasmBits64(actual)) if *actual == bits));
    assert!(heap.release_root(root));
    heap.collect([]);
    assert!(heap.get(value).is_none());
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
            source: "a".into(),
            flags: String::new(),
            matcher: Rc::new(quench_regexp::Regex::with_flags("a", Default::default()).unwrap()),
            legacy_constructor: owner(constructor),
        });
        let root = heap.root(regexp);
        heap.collect([]);
        assert_eq!(heap.weak_value(weak_constructor), Some(constructor));
        assert!(heap.release_root(root));
        heap.collect([]);
        assert!(heap.weak_value(weak_constructor).is_none());
        assert!(heap.get(regexp).is_none());
    }
}
