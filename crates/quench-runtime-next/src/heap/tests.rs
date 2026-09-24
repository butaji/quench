use super::*;
use crate::value_vec::ValueVec;
use std::rc::Rc;

#[test]
fn compact_object_header_reduces_gc_slot() {
    assert_eq!(size_of::<Object>(), 16);
    assert_eq!(size_of::<Cell>(), 48);
    assert_eq!(size_of::<Slot>(), 48);
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
        object: Object {
            proto: Value::NULL,
            properties: ValueVec::new(),
        },
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
    let target = heap.alloc(Cell::Object(Object {
        proto: Value::NULL,
        properties: ValueVec::new(),
    }));
    let target = heap.weak_handle(target).unwrap();
    let registry = heap.alloc(Cell::FinalizationRegistry {
        object: Object {
            proto: Value::NULL,
            properties: ValueVec::new(),
        },
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
        object: Object {
            proto: Value::NULL,
            properties: ValueVec::new(),
        },
        entries: Vec::new(),
    });
    let key = heap.alloc(Cell::Object(Object {
        proto: Value::NULL,
        properties: ValueVec::new(),
    }));
    let value = heap.alloc(Cell::String("value".into()));
    if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(weak_map) {
        entries.push((key, value));
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
    let target = heap.alloc(Cell::Object(Object {
        proto: Value::NULL,
        properties: ValueVec::new(),
    }));
    let target_handle = heap.weak_handle(target).unwrap();
    let reference = heap.alloc(Cell::WeakRef {
        object: Object {
            proto: Value::NULL,
            properties: ValueVec::new(),
        },
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
    let object = heap.alloc(Cell::Object(Object {
        proto: Value::NULL,
        properties: ValueVec::new(),
    }));
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
