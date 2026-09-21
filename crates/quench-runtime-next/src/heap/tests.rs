use super::*;

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
