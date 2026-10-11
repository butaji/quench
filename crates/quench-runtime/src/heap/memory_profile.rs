use super::{Cell, CellKind, Heap, Object, Slot};
use crate::value::Value;
use crate::value_vec::INLINE_PROPERTY_COUNT;
use rustc_hash::FxHashMap;
use std::collections::{BTreeMap, HashSet};
use std::mem::size_of;
use std::rc::Rc;

const KINDS: usize = CellKind::NAMES.len();
const BUCKETS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct AllocationSite {
    pub program: u32,
    pub function: u32,
    pub pc: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ObjectLifetime {
    site: u32,
    kind: u8,
    allocated: u32,
    peak: u32,
    death: Option<u32>,
    checkpoint: Option<u32>,
}

pub(crate) struct HeapMemoryComposition {
    pub cell_counts: [usize; KINDS],
    pub cell_variant_names: Vec<&'static str>,
    pub cell_variant_counts: Vec<usize>,
    pub slot_arena_reserved_bytes: usize,
    pub slot_size_bytes: usize,
    pub cell_size_bytes: usize,
    pub object_header_size_bytes: usize,
    pub occupied_slots: usize,
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
    pub cell_payload_bytes: usize,
    pub other_cell_payload_bytes: usize,
    pub profile_instrumentation_bytes: usize,
}

pub(crate) struct MemoryProfile {
    clock: u64,
    births: Vec<u64>,
    birth_bytes: Vec<usize>,
    kinds: Vec<u8>,
    live_counts: [u64; KINDS],
    live_birth_bytes: [u64; KINDS],
    peak_counts: [u64; KINDS],
    peak_birth_bytes: [u64; KINDS],
    allocated_counts: [u64; KINDS],
    allocated_variant_counts: BTreeMap<&'static str, u64>,
    allocated_bytes: [u64; KINDS],
    size_buckets: [[u64; BUCKETS]; KINDS],
    freed_counts: [u64; KINDS],
    freed_bytes: [u64; KINDS],
    lifetime_buckets: [[u64; BUCKETS]; KINDS],
    first_orders: [u64; KINDS],
    last_orders: [u64; KINDS],
    current_site: Option<AllocationSite>,
    sites: Vec<Option<AllocationSite>>,
    site_ids: FxHashMap<AllocationSite, u32>,
    object_sites: Vec<u32>,
    object_allocated_slots: Vec<u32>,
    object_peak_slots: Vec<u32>,
    object_lifetimes: FxHashMap<ObjectLifetime, u64>,
}
impl Default for MemoryProfile {
    fn default() -> Self {
        Self {
            clock: 0,
            births: Vec::new(),
            birth_bytes: Vec::new(),
            kinds: Vec::new(),
            live_counts: [0; KINDS],
            live_birth_bytes: [0; KINDS],
            peak_counts: [0; KINDS],
            peak_birth_bytes: [0; KINDS],
            allocated_counts: [0; KINDS],
            allocated_variant_counts: BTreeMap::new(),
            allocated_bytes: [0; KINDS],
            size_buckets: [[0; BUCKETS]; KINDS],
            freed_counts: [0; KINDS],
            freed_bytes: [0; KINDS],
            lifetime_buckets: [[0; BUCKETS]; KINDS],
            first_orders: [0; KINDS],
            last_orders: [0; KINDS],
            current_site: None,
            sites: vec![None],
            site_ids: FxHashMap::default(),
            object_sites: Vec::new(),
            object_allocated_slots: Vec::new(),
            object_peak_slots: Vec::new(),
            object_lifetimes: FxHashMap::default(),
        }
    }
}

impl MemoryProfile {
    pub(super) fn set_allocation_site(&mut self, site: AllocationSite) {
        self.current_site = Some(site);
    }

    pub(super) fn allocated(&mut self, index: usize, cell: &Cell, object_slots: Option<usize>) {
        self.clock += 1;
        let kind = Heap::cell_kind(cell) as usize;
        let bytes = Heap::cell_payload_bytes(cell);
        self.ensure_slot(index);
        self.births[index] = self.clock;
        self.birth_bytes[index] = bytes;
        self.kinds[index] = kind as u8;
        self.live_counts[kind] += 1;
        self.live_birth_bytes[kind] += bytes as u64;
        self.allocated_counts[kind] += 1;
        *self
            .allocated_variant_counts
            .entry(cell.profile_variant_name())
            .or_default() += 1;
        self.allocated_bytes[kind] += bytes as u64;
        self.size_buckets[kind][Heap::size_bucket(bytes)] += 1;
        if self.first_orders[kind] == 0 {
            self.first_orders[kind] = self.clock;
        }
        self.last_orders[kind] = self.clock;
        self.record_object_birth(index, kind, object_slots);
        if self.live_counts.iter().sum::<u64>() > self.peak_counts.iter().sum() {
            self.peak_counts = self.live_counts;
            self.peak_birth_bytes = self.live_birth_bytes;
        }
    }

    pub(super) fn freed(&mut self, index: usize, cell: &Cell, object_slots: Option<usize>) {
        let kind = self.kinds[index] as usize;
        debug_assert_eq!(kind, Heap::cell_kind(cell) as usize);
        let bytes = self.birth_bytes[index];
        self.live_counts[kind] -= 1;
        self.live_birth_bytes[kind] -= bytes as u64;
        self.freed_counts[kind] += 1;
        self.freed_bytes[kind] += bytes as u64;
        let lifetime = self.clock - self.births[index];
        self.lifetime_buckets[kind][Heap::size_bucket(lifetime as usize)] += 1;
        self.record_object_death(index, kind, object_slots);
    }

    pub(super) fn object_slots_changed(&mut self, index: usize, slots: usize) {
        self.object_peak_slots[index] = self.object_peak_slots[index].max(slots as u32);
    }

    pub(crate) fn report(
        &self,
        phase: &str,
        live_counts: [usize; KINDS],
        live_bytes: [usize; KINDS],
        heap: &Heap,
    ) {
        let variant_names = self
            .allocated_variant_counts
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let variant_counts = self
            .allocated_variant_counts
            .values()
            .copied()
            .collect::<Vec<_>>();
        eprintln!(
            "{{\"kind\":\"quench-allocation-census\",\"phase\":\"{phase}\",\"kind_names\":{:?},\"cell_variant_names\":{:?},\"cell_variant_allocated_counts\":{:?},\"bucket_max\":[0,7,15,31,63,127,255,null],\"allocation_clock\":{},\"allocated_counts\":{:?},\"allocated_birth_bytes\":{:?},\"allocation_size_buckets\":{:?},\"first_allocation_order\":{:?},\"last_allocation_order\":{:?},\"freed_counts\":{:?},\"freed_birth_bytes\":{:?},\"lifetime_allocation_buckets\":{:?},\"live_counts\":{:?},\"live_current_bytes\":{:?},\"peak_live_counts\":{:?},\"peak_birth_bytes\":{:?}}}",
            CellKind::NAMES,
            variant_names,
            variant_counts,
            self.clock,
            self.allocated_counts,
            self.allocated_bytes,
            self.size_buckets,
            self.first_orders,
            self.last_orders,
            self.freed_counts,
            self.freed_bytes,
            self.lifetime_buckets,
            live_counts,
            live_bytes,
            self.peak_counts,
            self.peak_birth_bytes,
        );
        if phase == "complete" {
            self.report_object_lifetimes(phase, heap);
        }
    }

    fn ensure_slot(&mut self, index: usize) {
        let len = index + 1;
        self.births.resize(len.max(self.births.len()), 0);
        self.birth_bytes.resize(len.max(self.birth_bytes.len()), 0);
        self.kinds.resize(len.max(self.kinds.len()), 0);
        self.object_sites
            .resize(len.max(self.object_sites.len()), 0);
        self.object_allocated_slots
            .resize(len.max(self.object_allocated_slots.len()), 0);
        self.object_peak_slots
            .resize(len.max(self.object_peak_slots.len()), 0);
    }

    fn record_object_birth(&mut self, index: usize, kind: usize, slots: Option<usize>) {
        let Some(slots) = slots else {
            return;
        };
        self.object_sites[index] = self.site_id(self.current_site);
        self.object_allocated_slots[index] = slots as u32;
        self.object_peak_slots[index] = slots as u32;
        debug_assert!(kind < KINDS);
    }

    fn record_object_death(&mut self, index: usize, kind: usize, slots: Option<usize>) {
        let Some(slots) = slots else {
            return;
        };
        let lifetime = ObjectLifetime {
            site: self.object_sites[index],
            kind: kind as u8,
            allocated: self.object_allocated_slots[index],
            peak: self.object_peak_slots[index],
            death: Some(slots as u32),
            checkpoint: None,
        };
        *self.object_lifetimes.entry(lifetime).or_default() += 1;
    }

    fn site_id(&mut self, site: Option<AllocationSite>) -> u32 {
        let Some(site) = site else {
            return 0;
        };
        if let Some(id) = self.site_ids.get(&site) {
            return *id;
        }
        let id = u32::try_from(self.sites.len()).expect("memory profile site table exhausted");
        self.sites.push(Some(site));
        self.site_ids.insert(site, id);
        id
    }

    fn report_object_lifetimes(&self, phase: &str, heap: &Heap) {
        let mut rows = self.object_lifetimes.clone();
        for (index, slot) in heap.slots.iter().enumerate() {
            let Some(cell) = slot.cell.as_ref() else {
                continue;
            };
            let Some(object) = cell.object() else {
                continue;
            };
            let lifetime = ObjectLifetime {
                site: self.object_sites[index],
                kind: Heap::cell_kind(cell) as u8,
                allocated: self.object_allocated_slots[index],
                peak: self.object_peak_slots[index],
                death: None,
                checkpoint: Some(heap.properties.len(object.properties) as u32),
            };
            *rows.entry(lifetime).or_default() += 1;
        }
        let mut rows = rows.into_iter().collect::<Vec<_>>();
        rows.sort_unstable_by_key(|(row, _)| {
            (
                row.site,
                row.kind,
                row.allocated,
                row.peak,
                row.death.unwrap_or(u32::MAX),
                row.checkpoint.unwrap_or(u32::MAX),
            )
        });
        for (row, count) in rows {
            let site = self.sites[row.site as usize];
            let (program, function, pc) = site.map_or((None, None, None), |site| {
                (Some(site.program), Some(site.function), Some(site.pc))
            });
            eprintln!(
                "{{\"kind\":\"quench-object-property-lifetime\",\"phase\":\"{phase}\",\"site\":{},\"program\":{},\"function\":{},\"pc\":{},\"cell_kind\":\"{}\",\"slots_at_allocation\":{},\"peak_slots\":{},\"slots_at_death\":{},\"slots_at_checkpoint\":{},\"lifecycle\":\"{}\",\"count\":{}}}",
                row.site,
                json_number(program),
                json_number(function),
                json_number(pc),
                CellKind::NAMES[row.kind as usize],
                row.allocated,
                row.peak,
                json_number(row.death),
                json_number(row.checkpoint),
                if row.death.is_some() { "freed" } else { "live" },
                count,
            );
        }
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + self.births.capacity() * size_of::<u64>()
            + self.birth_bytes.capacity() * size_of::<usize>()
            + self.kinds.capacity() * size_of::<u8>()
            + self.sites.capacity() * size_of::<Option<AllocationSite>>()
            + self.site_ids.capacity() * size_of::<(AllocationSite, u32)>()
            + self.object_sites.capacity() * size_of::<u32>()
            + self.object_allocated_slots.capacity() * size_of::<u32>()
            + self.object_peak_slots.capacity() * size_of::<u32>()
            + self.object_lifetimes.capacity() * size_of::<(ObjectLifetime, u64)>()
    }
}

fn json_number(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

impl Heap {
    pub(crate) fn collection_count(&self) -> u64 {
        self.collections
    }

    pub(crate) fn memory_composition(&self) -> HeapMemoryComposition {
        let mut occupied_slots = 0;
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
        for cell in self.slots.iter().filter_map(|slot| slot.cell.as_ref()) {
            occupied_slots += 1;
            let kind = Self::cell_kind(cell) as usize;
            cell_counts[kind] += 1;
            *cell_variant_counts
                .entry(cell.profile_variant_name())
                .or_insert(0usize) += 1;
            if let Some(object) = cell.object() {
                object_headers += 1;
                let property_width = self.properties.len(object.properties);
                object_property_counts_by_width.resize(
                    object_property_counts_by_width
                        .len()
                        .max(property_width + 1),
                    0,
                );
                object_property_counts_by_width[property_width] += 1;
                if matches!(cell, Cell::Function { .. }) {
                    boxed_object_headers += 1;
                } else {
                    embedded_object_headers += 1;
                }
                if !object.properties.has_inline_property_storage() {
                    let (used, capacity) = self.properties.vector_stats(object.properties);
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
            let payload_bytes = Self::cell_payload_bytes(cell);
            match cell {
                cell @ Cell::Array { .. } => {
                    let elements = cell.array_elements();
                    let element_bytes = elements.capacity() * size_of::<Value>();
                    other_cell_payload_bytes += payload_bytes.saturating_sub(element_bytes);
                    let identity = Rc::as_ptr(elements) as usize;
                    if array_identities.insert(identity) {
                        const RC_HEADER_WORDS: usize = 2;
                        dense_array_elements_bytes += element_bytes;
                        dense_array_headers_bytes +=
                            size_of::<Vec<Value>>() + RC_HEADER_WORDS * size_of::<usize>();
                        dense_array_backing_count += 1;
                    }
                }
                Cell::String(value) => {
                    other_cell_payload_bytes += payload_bytes.saturating_sub(value.capacity());
                    let (units, host) = value.memory_parts();
                    if string_identities.insert(units.0) {
                        string_buffers_bytes += units.1;
                        string_buffer_count += 1;
                    }
                    if let Some(host) = host {
                        if string_identities.insert(host.0) {
                            string_buffers_bytes += host.1;
                            string_buffer_count += 1;
                        }
                    }
                }
                _ => other_cell_payload_bytes += payload_bytes,
            }
        }
        let cell_payload_bytes = other_cell_payload_bytes
            + dense_array_elements_bytes
            + dense_array_headers_bytes
            + string_buffers_bytes;
        let (property_arena_values_bytes, property_arena_metadata_bytes) =
            self.properties.memory_breakdown();
        let (property_arena_used_values, property_arena_capacity_values, _) =
            self.properties.stats();
        let sparse_array_sidecar_bytes = self.sparse_arrays.as_ref().map_or(0, |arrays| {
            arrays.capacity() * size_of::<(u32, super::SparseElements)>()
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
            slot_size_bytes: size_of::<Slot>(),
            cell_size_bytes: size_of::<Cell>(),
            object_header_size_bytes: size_of::<Object>(),
            occupied_slots,
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
            cell_payload_bytes,
            other_cell_payload_bytes,
            profile_instrumentation_bytes: self.memory_profile.memory_bytes(),
        }
    }

    pub(crate) fn cell_counts(&self) -> [usize; KINDS] {
        let mut counts = [0; KINDS];
        for cell in self.slots.iter().filter_map(|slot| slot.cell.as_ref()) {
            counts[Self::cell_kind(cell) as usize] += 1;
        }
        counts
    }

    pub(crate) fn memory_profile(&self) -> &MemoryProfile {
        &self.memory_profile
    }

    pub(crate) fn live_payload_bytes(&self) -> [usize; KINDS] {
        let mut bytes = [0; KINDS];
        for cell in self.slots.iter().filter_map(|slot| slot.cell.as_ref()) {
            bytes[Self::cell_kind(cell) as usize] += Self::cell_payload_bytes(cell);
        }
        bytes
    }

    pub fn memory_stats(&self) -> (usize, usize, usize, usize, usize, usize, usize) {
        let dynamic = self.properties.memory_bytes()
            + self
                .slots
                .iter()
                .filter_map(|slot| slot.cell.as_ref())
                .map(cell_bytes)
                .sum::<usize>()
            + self
                .sparse_arrays
                .iter()
                .flat_map(|arrays| arrays.values())
                .map(|elements| {
                    elements.values.capacity() * (size_of::<usize>() + size_of::<Value>())
                })
                .sum::<usize>();
        let property = self.properties.stats();
        (
            self.slots.len(),
            self.slots.capacity() * size_of::<super::Slot>(),
            self.free.capacity() * size_of::<u32>(),
            dynamic,
            property.0,
            property.1,
            property.2,
        )
    }

    pub(crate) fn live_property_stats(&self) -> (usize, usize) {
        self.slots
            .iter()
            .filter_map(|slot| slot.cell.as_ref()?.object())
            .fold((0, 0), |(len, capacity), object| {
                let stats = self.properties.vector_stats(object.properties);
                (len + stats.0, capacity + stats.1)
            })
    }

    pub(crate) fn array_element_stats(&self) -> (usize, usize) {
        self.slots
            .iter()
            .filter_map(|slot| match slot.cell.as_ref()? {
                cell @ Cell::Array { .. } => {
                    let elements = cell.array_elements();
                    Some((elements.len(), elements.capacity()))
                }
                _ => None,
            })
            .fold((0, 0), |(len, capacity), value| {
                (len + value.0, capacity + value.1)
            })
    }
}

fn cell_bytes(cell: &Cell) -> usize {
    let object_extra_bytes = cell
        .object()
        .map_or(0, |object| object.allocated_extra_bytes());
    object_extra_bytes
        + match cell {
            Cell::Object(_)
            | Cell::Function { .. }
            | Cell::ShadowRealm { .. }
            | Cell::PromiseResolvingState { .. }
            | Cell::TemporalDuration { .. }
            | Cell::TemporalInstant { .. }
            | Cell::TypedArray { .. }
            | Cell::DataView { .. }
            | Cell::WeakRef { .. }
            | Cell::FinalizationRegistry { .. }
            | Cell::Iterator { .. }
            | Cell::Proxy { .. }
            | Cell::ArrayFromAsyncState(_)
            | Cell::WasmBits64(_)
            | Cell::WasmV128(_)
            | Cell::WasmExtern(_)
            | Cell::WasmTag { .. }
            | Cell::WasmGlobal { .. }
            | Cell::BindingReference { .. } => 0,
            Cell::WasmHostFunction { .. } | Cell::WasmException { .. } => {
                Heap::cell_payload_bytes(cell)
            }
            Cell::TemporalZonedDateTime {
                time_zone,
                calendar,
                ..
            } => time_zone.capacity() + calendar.capacity(),
            Cell::TemporalPlainDate { calendar, .. } => calendar.capacity(),
            Cell::TemporalPlainDateTime { calendar, .. } => calendar.capacity(),
            Cell::TemporalPlainMonthDay { calendar, .. }
            | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
            Cell::WasmElements(elements) => elements.capacity() * size_of::<Value>(),
            Cell::WasmTable { elements, .. }
            | Cell::WasmGc {
                fields: elements, ..
            } => elements.capacity() * size_of::<Value>(),
            cell @ Cell::Array { .. } => {
                cell.array_elements().capacity() * size_of::<Value>()
            }
            Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
            Cell::WasmMemory { bytes, .. } => bytes.capacity(),
            Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
            Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
            Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
            Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
            Cell::Environment { slots, scope, .. } => {
                let with_objects = &scope.with_objects;
                slots.len() * size_of::<super::EnvironmentSlot>()
                    + with_objects.len() * size_of::<Value>()
            }
            Cell::String(value) => value.capacity(),
            Cell::BigInt(value) | Cell::Error(value) => value.capacity(),
            Cell::Symbol(value) => value.as_ref().map_or(0, String::capacity),
            Cell::Date { .. } => 0,
            Cell::RegExp { meta, .. } => meta.source.capacity() + meta.flags.capacity(),
        }
}
