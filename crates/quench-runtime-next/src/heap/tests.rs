use super::*;
use crate::value_vec::ValueVec;

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
    let reference = heap.alloc(Cell::WeakRef {
        object: Object {
            proto: Value::NULL,
            properties: ValueVec::new(),
        },
        target,
    });
    let reference_root = heap.root(reference);
    heap.collect([]);
    assert!(heap.get(target).is_none());
    assert!(matches!(
        heap.get(reference),
        Some(Cell::WeakRef {
            target: Value::UNDEFINED,
            ..
        })
    ));
    assert!(heap.release_root(reference_root));
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
