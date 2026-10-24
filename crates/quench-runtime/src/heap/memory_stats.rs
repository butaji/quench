//! A live census of the heap's actual record spaces and owned payloads.
//! Shared array/string storage is charged once; collecting a census adds no
//! allocation-path instrumentation and does not execute guest code.
use super::{Cell, CellKind, FreeSlotBitmap, Heap, Object, Slot};
use crate::value::Value;
use crate::value_vec::INLINE_PROPERTY_COUNT;
use std::collections::{BTreeMap, HashSet};
use std::mem::size_of;
const KINDS: usize = CellKind::NAMES.len();

/// Allocated heap records, arenas and unique backing storage at one instant.
pub struct HeapMemoryComposition {
    pub cell_counts: [usize; KINDS],
    pub cell_variant_names: Vec<&'static str>,
    pub cell_variant_counts: Vec<usize>,
    pub slot_arena_reserved_bytes: usize,
    pub object_record_reserved_bytes: usize,
    pub object_record_reusable_ids: usize,
    pub object_record_allocation_scan_cursor: usize,
    pub array_record_reserved_bytes: usize,
    pub array_record_reusable_ids: usize,
    pub array_record_allocation_scan_cursor: usize,
    pub function_record_reserved_bytes: usize,
    pub promise_record_reserved_bytes: usize,
    pub await_record_reserved_bytes: usize,
    pub iterator_record_reserved_bytes: usize,
    pub string_record_reserved_bytes: usize,
    pub legacy_cell_record_reserved_bytes: usize,
    pub free_slot_metadata_bytes: usize,
    pub slot_size_bytes: usize,
    pub cell_size_bytes: usize,
    pub object_header_size_bytes: usize,
    pub occupied_slots: usize,
    /// Live values held in object or array records rather than the slot arena.
    pub direct_records: usize,
    pub object_headers: usize,
    pub embedded_object_headers: usize,
    pub boxed_object_headers: usize,
    pub boxed_object_header_bytes: usize,
    pub object_property_counts_by_width: Vec<usize>,
    pub inline_property_attribution_bytes: usize,
    pub property_arena_used_values: usize,
    pub property_arena_capacity_values: usize,
    pub live_property_used_values: usize,
    pub live_property_capacity_values: usize,
    pub live_property_used_values_by_width: Vec<usize>,
    pub live_property_capacity_values_by_width: Vec<usize>,
    pub property_arena_values_bytes: usize,
    pub property_arena_metadata_bytes: usize,
    pub dense_array_elements_bytes: usize,
    pub dense_array_headers_bytes: usize,
    pub dense_array_backing_count: usize,
    pub sparse_array_sidecar_bytes: usize,
    pub string_buffers_bytes: usize,
    pub string_buffer_count: usize,
    /// Byte lengths of the unique ArrayBuffer backing stores: memory outside the heap
    /// proper, as V8 counts it (`external`, `arrayBuffers`).
    pub array_buffer_bytes: usize,
    /// The heap allocations those backing stores own.
    pub array_buffer_store_bytes: usize,
    pub cell_payload_bytes: usize,
    pub other_cell_payload_bytes: usize,
    pub profile_instrumentation_bytes: usize,
}

impl Heap {
    pub(crate) fn memory_composition(&self) -> HeapMemoryComposition {
        let mut occupied_slots = 0;
        let mut direct_records = 0;
        let mut object_headers = 0;
        let mut embedded_object_headers = 0;
        let mut boxed_object_headers = 0;
        let mut object_property_counts_by_width = Vec::new();
        let mut cell_counts = [0; KINDS];
        let mut cell_variant_counts = BTreeMap::new();
        let mut live_property_used_values = 0;
        let mut live_property_capacity_values = 0;
        let mut live_property_used_values_by_width = Vec::new();
        let mut live_property_capacity_values_by_width = Vec::new();
        let mut dense_array_elements_bytes = 0;
        let mut dense_array_headers_bytes = 0;
        let mut dense_array_backing_count = 0;
        let mut string_buffers_bytes = 0;
        let mut string_buffer_count = 0;
        let mut other_cell_payload_bytes = 0;
        let mut array_identities = HashSet::new();
        let mut string_identities = HashSet::new();
        let mut array_buffer_bytes = 0;
        let mut array_buffer_store_bytes = 0;
        let mut buffer_identities = HashSet::new();
        self.for_each_live_value(|identity| {
            occupied_slots += 1;
            let object_record = self.object_record(identity);
            let direct_array = identity.direct_array_index().is_some();
            let cell = self.get(identity);
            let direct_object = identity.direct_object_index().is_some();
            let direct_function = identity.direct_function_index().is_some();
            let direct_promise = identity.direct_promise_index().is_some();
            let direct_await = identity.direct_await_index().is_some();
            let direct_iterator = identity.direct_iterator_index().is_some();
            let direct_string = identity.direct_string_index().is_some();
            direct_records += usize::from(
                direct_object
                    || direct_array
                    || direct_function
                    || direct_promise
                    || direct_await
                    || direct_iterator
                    || direct_string,
            );
            let kind = if direct_string {
                CellKind::String as usize
            } else if direct_iterator {
                CellKind::Iterator as usize
            } else if direct_await {
                CellKind::AwaitState as usize
            } else if direct_object || direct_promise {
                CellKind::Object as usize
            } else if direct_array {
                CellKind::Array as usize
            } else if direct_function {
                CellKind::Function as usize
            } else {
                Self::cell_kind(cell.expect("live legacy value has a Cell")) as usize
            };
            cell_counts[kind] += 1;
            let variant_name = if direct_object {
                "object"
            } else if direct_array {
                "array"
            } else if direct_function {
                "function"
            } else if direct_promise {
                "promise"
            } else if direct_await {
                "await_state"
            } else if direct_iterator {
                "iterator"
            } else if direct_string {
                "string"
            } else {
                cell.expect("live legacy value has a Cell")
                    .profile_variant_name()
            };
            *cell_variant_counts.entry(variant_name).or_insert(0usize) += 1;
            let object = object_record.or_else(|| self.object(identity));
            if let Some(object) = object {
                object_headers += 1;
                let property_width = self.properties.len(object.properties());
                object_property_counts_by_width.resize(
                    object_property_counts_by_width
                        .len()
                        .max(property_width + 1),
                    0,
                );
                object_property_counts_by_width[property_width] += 1;
                if cell.is_some_and(|cell| matches!(cell, Cell::Function(_)))
                {
                    boxed_object_headers += 1;
                } else {
                    embedded_object_headers += 1;
                }
                if !object.properties().has_inline_property_storage() {
                    let (used, capacity) = self.properties.vector_stats(object.properties());
                    live_property_used_values += used;
                    live_property_capacity_values += capacity;
                    live_property_used_values_by_width.resize(
                        live_property_used_values_by_width
                            .len()
                            .max(property_width + 1),
                        0,
                    );
                    live_property_capacity_values_by_width.resize(
                        live_property_capacity_values_by_width
                            .len()
                            .max(property_width + 1),
                        0,
                    );
                    live_property_used_values_by_width[property_width] += used;
                    live_property_capacity_values_by_width[property_width] += capacity;
                }
            }
            let payload_bytes = if direct_object {
                object_record.map_or(0, Object::allocated_extra_bytes)
            } else if let Some(array) = self.array(identity)
                && direct_array
            {
                array.allocated_extra_bytes() + array.elements().capacity() * size_of::<Value>()
            } else if direct_function {
                self.function(identity)
                    .map_or(0, |function| function.object.allocated_extra_bytes())
            } else if direct_promise {
                self.promise(identity)
                    .map_or(0, |promise| promise.object.allocated_extra_bytes())
            } else if direct_iterator {
                self.iterator(identity)
                    .map_or(0, |iterator| iterator.object.allocated_extra_bytes())
            } else if let Some(string) = self.string(identity) {
                string.capacity()
            } else {
                cell.map_or(0, Self::cell_payload_bytes)
            };
            if let Some(value) = self.string(identity) {
                other_cell_payload_bytes += payload_bytes.saturating_sub(value.capacity());
                let (units, host) = value.memory_parts();
                // Inline text (`Short`) owns no buffer of its own.
                if units.1 > 0 && string_identities.insert(units.0) {
                    string_buffers_bytes += units.1;
                    string_buffer_count += 1;
                }
                if let Some(host) = host
                    && string_identities.insert(host.0)
                {
                    string_buffers_bytes += host.1;
                    string_buffer_count += 1;
                }
            } else if direct_array {
                let elements = self
                    .array_elements(identity)
                    .expect("live direct array record");
                let element_bytes = elements.capacity() * size_of::<Value>();
                other_cell_payload_bytes += payload_bytes.saturating_sub(element_bytes);
                let identity = elements.as_ptr() as usize;
                if array_identities.insert(identity) {
                    const RC_HEADER_WORDS: usize = 2;
                    dense_array_elements_bytes += element_bytes;
                    dense_array_headers_bytes +=
                        size_of::<Vec<Value>>() + RC_HEADER_WORDS * size_of::<usize>();
                    dense_array_backing_count += 1;
                }
            } else {
                match cell {
                    Some(Cell::Array { object }) => {
                        let elements = object.elements();
                        let element_bytes = elements.capacity() * size_of::<Value>();
                        other_cell_payload_bytes += payload_bytes.saturating_sub(element_bytes);
                        let identity = elements.as_ptr() as usize;
                        if array_identities.insert(identity) {
                            const RC_HEADER_WORDS: usize = 2;
                            dense_array_elements_bytes += element_bytes;
                            dense_array_headers_bytes +=
                                size_of::<Vec<Value>>() + RC_HEADER_WORDS * size_of::<usize>();
                            dense_array_backing_count += 1;
                        }
                    }
                    Some(Cell::ArrayBuffer { bytes, .. }) => {
                        other_cell_payload_bytes += payload_bytes.saturating_sub(bytes.capacity());
                        if buffer_identities.insert(bytes.as_ptr() as usize) {
                            array_buffer_bytes += bytes.len();
                            array_buffer_store_bytes += bytes.capacity();
                        }
                    }
                    _ => other_cell_payload_bytes += payload_bytes,
                }
            }
        });
        let cell_payload_bytes = other_cell_payload_bytes
            + dense_array_elements_bytes
            + dense_array_headers_bytes
            + string_buffers_bytes;
        let (property_arena_values_bytes, property_arena_metadata_bytes) =
            self.properties.memory_breakdown();
        let (property_arena_used_values, property_arena_capacity_values, _) =
            self.properties.stats();
        let sparse_array_sidecar_bytes = self.sparse_arrays.as_ref().map_or(0, |arrays| {
            arrays.capacity() * size_of::<(Value, super::SparseElements)>()
                + arrays
                    .values()
                    .map(|elements| elements.values.capacity() * size_of::<(usize, Value)>())
                    .sum::<usize>()
        });
        HeapMemoryComposition {
            cell_counts,
            cell_variant_names: cell_variant_counts.keys().copied().collect(),
            cell_variant_counts: cell_variant_counts.values().copied().collect(),
            slot_arena_reserved_bytes: self.slots.capacity() * size_of::<Slot>(),
            object_record_reserved_bytes: self.object_records.allocated_bytes(),
            object_record_reusable_ids: self.object_records.reusable_ids(),
            object_record_allocation_scan_cursor: self.object_records.allocation_scan_cursor(),
            array_record_reserved_bytes: self.array_records.allocated_bytes(),
            array_record_reusable_ids: self.array_records.reusable_ids(),
            array_record_allocation_scan_cursor: self.array_records.allocation_scan_cursor(),
            function_record_reserved_bytes: self.function_records.allocated_bytes(),
            promise_record_reserved_bytes: self.promise_records.allocated_bytes(),
            await_record_reserved_bytes: self.await_records.allocated_bytes(),
            iterator_record_reserved_bytes: self.iterator_records.allocated_bytes(),
            string_record_reserved_bytes: self.string_records.allocated_bytes(),
            legacy_cell_record_reserved_bytes: 0,
            free_slot_metadata_bytes: self.free_by_slab.capacity() * size_of::<FreeSlotBitmap>()
                + self.free_slabs.capacity() * size_of::<u32>(),
            slot_size_bytes: size_of::<Slot>(),
            cell_size_bytes: size_of::<Cell>(),
            object_header_size_bytes: size_of::<Object>(),
            occupied_slots,
            direct_records,
            object_headers,
            embedded_object_headers,
            boxed_object_headers,
            boxed_object_header_bytes: boxed_object_headers * size_of::<Object>(),
            object_property_counts_by_width,
            inline_property_attribution_bytes: embedded_object_headers
                * INLINE_PROPERTY_COUNT
                * size_of::<Value>(),
            property_arena_used_values,
            property_arena_capacity_values,
            live_property_used_values,
            live_property_capacity_values,
            live_property_used_values_by_width,
            live_property_capacity_values_by_width,
            property_arena_values_bytes,
            property_arena_metadata_bytes,
            dense_array_elements_bytes,
            dense_array_headers_bytes,
            dense_array_backing_count,
            sparse_array_sidecar_bytes,
            string_buffers_bytes,
            string_buffer_count,
            array_buffer_bytes,
            array_buffer_store_bytes,
            cell_payload_bytes,
            other_cell_payload_bytes,
            profile_instrumentation_bytes: {
                #[cfg(feature = "profile-memory")]
                { self.memory_profile.memory_bytes() }
                #[cfg(not(feature = "profile-memory"))]
                { 0 }
            },
        }
    }

}

impl HeapMemoryComposition {
    /// Bytes the heap holds for live values: their slots, object headers, used property
    /// values and payloads. ArrayBuffer backing stores are external memory, outside it.
    pub fn used_bytes(&self) -> usize {
        (self.occupied_slots - self.direct_records) * self.slot_size_bytes
            + self.embedded_object_headers * self.object_header_size_bytes
            + self.boxed_object_header_bytes
            + self.live_property_used_values * size_of::<crate::Value>()
            + self.cell_payload_bytes
            + self.sparse_array_sidecar_bytes
    }

    /// Bytes the heap has reserved: arenas and records at capacity, plus payloads; like
    /// `used_bytes`, without ArrayBuffer backing stores.
    pub fn reserved_bytes(&self) -> usize {
        self.slot_arena_reserved_bytes
            + self.object_record_reserved_bytes
            + self.array_record_reserved_bytes
            + self.function_record_reserved_bytes
            + self.promise_record_reserved_bytes
            + self.await_record_reserved_bytes
            + self.iterator_record_reserved_bytes
            + self.string_record_reserved_bytes
            + self.legacy_cell_record_reserved_bytes
            + self.free_slot_metadata_bytes
            + self.boxed_object_header_bytes
            + self.property_arena_values_bytes
            + self.property_arena_metadata_bytes
            + self.cell_payload_bytes
            + self.sparse_array_sidecar_bytes
    }
}
