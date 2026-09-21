use super::*;

#[test]
fn compact_object_header_reduces_gc_slot() {
    assert_eq!(size_of::<Object>(), 16);
    assert_eq!(size_of::<Cell>(), 32);
    assert_eq!(size_of::<Slot>(), 32);
}
