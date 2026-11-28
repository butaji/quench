//! Layout-test helpers for the cell records: the native layouts the stencils and the runtime agree on.

use super::*;


/// Every heap slot stores an `Option<Cell>`: a tag word and a payload of at most three
/// words. No variant may grow past that; an array record is smaller.
const MAX_CELL_BYTES: usize = 32;

#[test]
fn cells_stay_compact() {
    assert!(std::mem::size_of::<Option<Cell>>() <= MAX_CELL_BYTES);
    assert!(std::mem::size_of::<ArrayObject>() + std::mem::size_of::<Value>() <= MAX_CELL_BYTES);
}

#[test]
fn native_array_layout_is_derived_from_the_current_storage_types() {
    assert_eq!(NATIVE_ARRAY_RECORD_SIZE, std::mem::size_of::<ArrayObject>());
    assert_eq!(NATIVE_ARRAY_RECORD_ALIGN, std::mem::align_of::<ArrayObject>());
    assert_eq!(
        NATIVE_ARRAY_ELEMENTS_OFFSET,
        std::mem::offset_of!(ArrayObject, object) + std::mem::offset_of!(Object, head) + std::mem::offset_of!(ObjectHead, payload)
    );
    assert_eq!(NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET, std::mem::offset_of!(ObjectExtras, head) + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, elements));
    let layout = super::super::NATIVE_ARRAY_LAYOUT;
    assert_eq!(layout.extras_elements_offset, NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET);
    assert_eq!(layout.record_size, std::mem::size_of::<ArrayObject>());
    assert_eq!(layout.record_align, std::mem::align_of::<ArrayObject>());
    assert_eq!(layout.elements_offset, NATIVE_ARRAY_ELEMENTS_OFFSET);
}

#[test]
fn native_object_layout_is_derived_from_the_current_storage_types() {
    assert_eq!(NATIVE_OBJECT_RECORD_SIZE, std::mem::size_of::<Object>());
    assert_eq!(NATIVE_OBJECT_RECORD_ALIGN, std::mem::align_of::<Object>());
    assert_eq!(
        NATIVE_OBJECT_LAYOUT_OFFSET,
        std::mem::offset_of!(Object, head) + std::mem::offset_of!(ObjectHead, layout)
    );
    assert_eq!(
        NATIVE_OBJECT_PAYLOAD_OFFSET,
        std::mem::offset_of!(Object, head) + std::mem::offset_of!(ObjectHead, payload)
    );
    assert_eq!(
        NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET,
        std::mem::offset_of!(ObjectExtras, head) + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, arena_start)
    );

    let layout = super::super::NATIVE_OBJECT_LAYOUT;
    assert_eq!(layout.extras_inline_values_offset,
        std::mem::offset_of!(ObjectExtras, head)
            + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, inline_properties)
            + std::mem::offset_of!(DisplacedInline, 0));
    assert_eq!(layout.record_size, std::mem::size_of::<Object>());
    assert_eq!(layout.record_align, std::mem::align_of::<Object>());
    assert_eq!(layout.meta_offset, NATIVE_OBJECT_META_OFFSET);
    assert_eq!(layout.auxiliary_offset, NATIVE_OBJECT_AUXILIARY_OFFSET);
    assert_eq!(layout.payload_offset, NATIVE_OBJECT_PAYLOAD_OFFSET);
    assert_eq!(
        layout.extras_arena_start_offset,
        NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET
    );
}
