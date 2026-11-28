//! The stencil layout numbers (`layout_offsets`, shared with the stencils by `#[path]`) checked against the
//! runtime's own layouts, beside them: each is a `const` check against the runtime's `offset_of!`, `size_of` or
//! layout const, so the tiers need not see the layouts to check them.

use super::layout_offsets as L;
use std::mem::offset_of;

// The stencils' stand-in words for the call context's head and the function record (`layout_offsets::CallContextWords`,
// `FunctionRecordWords`) have their runtime fields' sizes and alignments, so their `offset_of!` values are the runtime's.
const _: () = assert!(std::mem::size_of::<crate::Value>() == std::mem::size_of::<u64>() && std::mem::align_of::<crate::Value>() == std::mem::align_of::<u64>());
const _: () = assert!(std::mem::size_of::<crate::vm::ProgramId>() == std::mem::size_of::<u32>() && std::mem::align_of::<crate::vm::ProgramId>() == std::mem::align_of::<u32>());
const _: () = assert!(std::mem::size_of::<crate::heap::Object>() == std::mem::size_of::<crate::staged::record_heads::ObjectHead>() && std::mem::align_of::<crate::heap::Object>() == std::mem::align_of::<crate::staged::record_heads::ObjectHead>());
const _: () = assert!(std::mem::size_of::<crate::heap::FunctionKind>() == std::mem::size_of::<[u32; 3]>() && std::mem::align_of::<crate::heap::FunctionKind>() == std::mem::align_of::<[u32; 3]>());
// The layout structs: every field of each is the runtime's.
// NATIVE_RECORD_CURSOR_LAYOUT
const _: () = assert!(crate::heap::NATIVE_RECORD_CURSOR_LAYOUT.next_offset == L::NATIVE_RECORD_CURSOR_LAYOUT.next_offset);
const _: () = assert!(crate::heap::NATIVE_RECORD_CURSOR_LAYOUT.limit_offset == L::NATIVE_RECORD_CURSOR_LAYOUT.limit_offset);
// NATIVE_OBJECT_LAYOUT
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset == L::NATIVE_OBJECT_LAYOUT.layout_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.meta_offset == L::NATIVE_OBJECT_LAYOUT.meta_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.auxiliary_offset == L::NATIVE_OBJECT_LAYOUT.auxiliary_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.payload_offset == L::NATIVE_OBJECT_LAYOUT.payload_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_arena_start_offset == L::NATIVE_OBJECT_LAYOUT.extras_arena_start_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_inline_values_offset == L::NATIVE_OBJECT_LAYOUT.extras_inline_values_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_storage_mask == L::NATIVE_OBJECT_LAYOUT.inline_storage_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.array_mask == L::NATIVE_OBJECT_LAYOUT.array_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.storage_start_mask == L::NATIVE_OBJECT_LAYOUT.storage_start_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.dictionary_mask == L::NATIVE_OBJECT_LAYOUT.dictionary_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_form == L::NATIVE_OBJECT_LAYOUT.inline_form);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_form == L::NATIVE_OBJECT_LAYOUT.extras_form);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.narrow_form == L::NATIVE_OBJECT_LAYOUT.narrow_form);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.narrow_run_tag == L::NATIVE_OBJECT_LAYOUT.narrow_run_tag);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.integrity_mask == L::NATIVE_OBJECT_LAYOUT.integrity_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count == L::NATIVE_OBJECT_LAYOUT.inline_property_count);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_tag == L::NATIVE_OBJECT_LAYOUT.extras_tag);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.extras_tag_mask == L::NATIVE_OBJECT_LAYOUT.extras_tag_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_mask == L::NATIVE_OBJECT_LAYOUT.proto_mask);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_record_base == L::NATIVE_OBJECT_LAYOUT.proto_record_base);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_record_end == L::NATIVE_OBJECT_LAYOUT.proto_record_end);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_records_per_block == L::NATIVE_OBJECT_LAYOUT.mark_records_per_block);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_word_bits == L::NATIVE_OBJECT_LAYOUT.mark_word_bits);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_block_size == L::NATIVE_OBJECT_LAYOUT.mark_block_size);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.mark_offset == L::NATIVE_OBJECT_LAYOUT.mark_offset);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size == L::NATIVE_OBJECT_LAYOUT.property_value_size);
const _: () = assert!(crate::heap::NATIVE_OBJECT_LAYOUT.property_value_align == L::NATIVE_OBJECT_LAYOUT.property_value_align);
// NATIVE_ARRAY_LAYOUT
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_capacity == L::NATIVE_ARRAY_LAYOUT.record_capacity);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_size == L::NATIVE_ARRAY_LAYOUT.record_size);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.record_align == L::NATIVE_ARRAY_LAYOUT.record_align);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.elements_offset == L::NATIVE_ARRAY_LAYOUT.elements_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.extras_elements_offset == L::NATIVE_ARRAY_LAYOUT.extras_elements_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.elements_header_offset == L::NATIVE_ARRAY_LAYOUT.elements_header_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_length_offset == L::NATIVE_ARRAY_LAYOUT.header_length_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_values_offset == L::NATIVE_ARRAY_LAYOUT.header_values_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_owners_offset == L::NATIVE_ARRAY_LAYOUT.header_owners_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.header_capacity_offset == L::NATIVE_ARRAY_LAYOUT.header_capacity_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.capacity_mask == L::NATIVE_ARRAY_LAYOUT.capacity_mask);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.object_offset == L::NATIVE_ARRAY_LAYOUT.object_offset);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.frozen_mask == L::NATIVE_ARRAY_LAYOUT.frozen_mask);
const _: () = assert!(crate::heap::NATIVE_ARRAY_LAYOUT.integrity_mask == L::NATIVE_ARRAY_LAYOUT.integrity_mask);
// NATIVE_TYPED_ARRAY_LAYOUT
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.entry_size == L::NATIVE_TYPED_ARRAY_LAYOUT.entry_size);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.page_shift == L::NATIVE_TYPED_ARRAY_LAYOUT.page_shift);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.page_mask == L::NATIVE_TYPED_ARRAY_LAYOUT.page_mask);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.kind_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.kind_offset);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.detached_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.detached_offset);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.unshared_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.unshared_offset);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.data_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.data_offset);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.length_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.length_offset);
const _: () = assert!(crate::heap::NATIVE_TYPED_ARRAY_LAYOUT.stamp_offset == L::NATIVE_TYPED_ARRAY_LAYOUT.stamp_offset);
// NATIVE_ENVIRONMENT_LAYOUT
const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.header_pointer_offset == L::NATIVE_ENVIRONMENT_LAYOUT.header_pointer_offset);
const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.sparse_mark == L::NATIVE_ENVIRONMENT_LAYOUT.sparse_mark);
const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.slots_offset == L::NATIVE_ENVIRONMENT_LAYOUT.slots_offset);
const _: () = assert!(crate::heap::NATIVE_ENVIRONMENT_LAYOUT.slot_size == L::NATIVE_ENVIRONMENT_LAYOUT.slot_size);
// NATIVE_FIELD_CACHE_LAYOUT
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size == L::NATIVE_FIELD_CACHE_LAYOUT.entry_size);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.receiver_offset == L::NATIVE_FIELD_CACHE_LAYOUT.receiver_offset);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.holder_offset == L::NATIVE_FIELD_CACHE_LAYOUT.holder_offset);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.slot_offset == L::NATIVE_FIELD_CACHE_LAYOUT.slot_offset);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.holder_cell_offset == L::NATIVE_FIELD_CACHE_LAYOUT.holder_cell_offset);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.receiver_complement == L::NATIVE_FIELD_CACHE_LAYOUT.receiver_complement);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.holder_complement == L::NATIVE_FIELD_CACHE_LAYOUT.holder_complement);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.slot_complement == L::NATIVE_FIELD_CACHE_LAYOUT.slot_complement);
const _: () = assert!(crate::vm::NATIVE_FIELD_CACHE_LAYOUT.own_holder == L::NATIVE_FIELD_CACHE_LAYOUT.own_holder);
// NATIVE_WASM_GC_LAYOUT: the record's head is its first field, so its words are the record's, and the
// payload words are the head's (`WasmGcRecord` is `repr(C)` with the head first).
const _: () = assert!(L::NATIVE_WASM_GC_LAYOUT.record_capacity == crate::staged::object_encoding::WASM_GC_RECORD_CAPACITY);
const _: () = assert!(L::NATIVE_WASM_GC_LAYOUT.payload_offset == offset_of!(crate::staged::record_heads::WasmGcHead, payload));
const _: () = assert!(L::NATIVE_WASM_GC_LAYOUT.length_offset == offset_of!(crate::staged::record_heads::WasmGcHead, length));
const _: () = assert!(L::NATIVE_WASM_GC_LAYOUT.type_offset == offset_of!(crate::staged::record_heads::WasmGcHead, ty));
