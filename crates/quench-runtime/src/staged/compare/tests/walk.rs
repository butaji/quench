//! Prototype walks in the comparison jumps. A walk follows an object record's prototype link to the next object
//! record (the stub hop, `object::proto_object_record_id`). An ordinary object is a class cell with no record, and an
//! array's prototype link is an array-record link, so no live chain is one the object-record walk follows; its
//! unrolled and looped agreement and the `instanceof` word test went with the records they read.

/// The walk ends at the heap's null link, which is the object layout's `proto_mask`.
#[test]
fn the_walk_ends_at_the_heaps_null_link() {
    assert_eq!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_mask, crate::heap::ProtoLink::NULL.bits());
}
