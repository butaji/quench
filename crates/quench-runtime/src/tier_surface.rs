//! What the tier crates may name of the heap (S1.4, private internals). The heap module itself is the
//! runtime's own; a tier names only the items listed here, reads cells through `Vm::heap_reads` and
//! writes them through the `Vm` mutations of `vm::tier_queries`. Each group says why a tier needs it.

/// The layouts emitted code addresses records by: offsets, record sizes and arena tables, checked
/// against the records themselves (`staged::layout_offset_checks`).
pub use crate::heap::{
    layouts, DOUBLE_LEN, ENCODED_LEN, IN_PLACE_SLOTS_WORDS, LENGTH_FROM_VALUES, NATIVE_ARRAY_LAYOUT,
    NATIVE_COLLECTION_LAYOUT, NATIVE_ENVIRONMENT_LAYOUT, NATIVE_FUNCTION_LAYOUT, NATIVE_OBJECT_LAYOUT, NATIVE_RECORD_CURSOR_LAYOUT,
    NATIVE_TYPED_ARRAY_LAYOUT, OTHERS_FROM_VALUES, OTHERS_UNKNOWN, SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK,
    TYPED_ARRAYS, armed_proofs_address, buffer_relocations_address, index_free_chain_link_address,
};

/// The records a tier reads through `HeapReads` and the kinds it matches on.
pub use crate::heap::{
    ArrayObject, AwaitKind, CallSiteLocation, CallSiteName, CallSiteRecord, Cell, CellKind, CollectionStorage,
    FunctionKind, FunctionRecord, GcTraceKind, InlineObjectHeader, IteratorConsumer, IteratorHelper, IteratorKind,
    IteratorRecord, Native, Object, PlainPropertyStorage, PrivateBrand, PromiseState, ProtoLink, ProxyKind,
    RootId, StackData, TableTarget, TableWords, TypedArrayKind, WasmGcRecord, WeakHandle,
};

/// The class cell region of `fields` fields (`heap::cell_region_for`), which a class's id names (`ProtoLink::class`):
/// past the largest region it is the largest, and the cell spills the rest, so every field count has one.
pub fn cell_region_for(fields: usize) -> u32 {
    crate::heap::cell_region_for(fields)
}

/// Environment storage, which a lane addresses in place.
pub use crate::heap::{
    absent_environment_storage, EnvironmentRecord, EnvironmentScope, EnvironmentScopeRef,
    EnvironmentSlots,
};

/// Proofs and facts: what compiled code holds, the subscriptions that invalidate it, and the stores
/// that fire them.
pub use crate::heap::{
    global_value_fact, hold_prototype_values, note_global_write, retire_array_storage_proofs,
    subscribe_global_layout, subscribe_global_value, subscribe_prototype_values, subscribe_typed_views, typed_views_stable, ArrayStorageProofs, GlobalCell,
    GlobalFact, GlobalLayoutFact, INHERITED_FIELD_CACHE_MAX_DEPTH, InheritedFieldAnswer, InheritedFieldCache,
    LaneProofScope, PropertyStoreLayout,
};
