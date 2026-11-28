//! Prototype walks in the comparison jumps. The object-record walk went with the records it followed (an ordinary
//! object is a class cell, and no link names a record now), so only the null link's encoding is checked here.

/// The walk ends at the heap's null link, which is the object layout's `proto_mask`.
#[test]
fn the_walk_ends_at_the_heaps_null_link() {
    assert_eq!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_mask, crate::heap::ProtoLink::NULL.bits());
}
