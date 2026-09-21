use super::{Cell, Heap};
use crate::value::Value;
use std::mem::size_of;

const KINDS: usize = 14;
const BUCKETS: usize = 8;

#[derive(Default)]
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
    allocated_bytes: [u64; KINDS],
    size_buckets: [[u64; BUCKETS]; KINDS],
    freed_counts: [u64; KINDS],
    freed_bytes: [u64; KINDS],
    lifetime_buckets: [[u64; BUCKETS]; KINDS],
    first_orders: [u64; KINDS],
    last_orders: [u64; KINDS],
}

impl MemoryProfile {
    pub(super) fn allocated(&mut self, index: usize, cell: &Cell) {
        self.clock += 1;
        let kind = Heap::cell_kind(cell);
        let bytes = Heap::cell_payload_bytes(cell);
        self.ensure_slot(index);
        self.births[index] = self.clock;
        self.birth_bytes[index] = bytes;
        self.kinds[index] = kind as u8;
        self.live_counts[kind] += 1;
        self.live_birth_bytes[kind] += bytes as u64;
        self.allocated_counts[kind] += 1;
        self.allocated_bytes[kind] += bytes as u64;
        self.size_buckets[kind][Heap::size_bucket(bytes)] += 1;
        if self.first_orders[kind] == 0 {
            self.first_orders[kind] = self.clock;
        }
        self.last_orders[kind] = self.clock;
        if self.live_counts.iter().sum::<u64>() > self.peak_counts.iter().sum() {
            self.peak_counts = self.live_counts;
            self.peak_birth_bytes = self.live_birth_bytes;
        }
    }

    pub(super) fn freed(&mut self, index: usize, cell: &Cell) {
        let kind = self.kinds[index] as usize;
        debug_assert_eq!(kind, Heap::cell_kind(cell));
        let bytes = self.birth_bytes[index];
        self.live_counts[kind] -= 1;
        self.live_birth_bytes[kind] -= bytes as u64;
        self.freed_counts[kind] += 1;
        self.freed_bytes[kind] += bytes as u64;
        let lifetime = self.clock - self.births[index];
        self.lifetime_buckets[kind][Heap::size_bucket(lifetime as usize)] += 1;
    }

    pub(crate) fn report(
        &self,
        phase: &str,
        live_counts: [usize; KINDS],
        live_bytes: [usize; KINDS],
    ) {
        eprintln!(
            "{{\"kind\":\"rqj-allocation-census\",\"phase\":\"{phase}\",\"kind_names\":[\"object\",\"array\",\"map\",\"set\",\"iterator\",\"weak_map\",\"weak_set\",\"function\",\"environment\",\"string\",\"bigint\",\"symbol\",\"date\",\"error\"],\"bucket_max\":[0,7,15,31,63,127,255,null],\"allocation_clock\":{},\"allocated_counts\":{:?},\"allocated_birth_bytes\":{:?},\"allocation_size_buckets\":{:?},\"first_allocation_order\":{:?},\"last_allocation_order\":{:?},\"freed_counts\":{:?},\"freed_birth_bytes\":{:?},\"lifetime_allocation_buckets\":{:?},\"live_counts\":{:?},\"live_current_bytes\":{:?},\"peak_live_counts\":{:?},\"peak_birth_bytes\":{:?}}}",
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
    }

    fn ensure_slot(&mut self, index: usize) {
        let len = index + 1;
        self.births.resize(len.max(self.births.len()), 0);
        self.birth_bytes.resize(len.max(self.birth_bytes.len()), 0);
        self.kinds.resize(len.max(self.kinds.len()), 0);
    }
}

impl Heap {
    pub(crate) fn cell_counts(&self) -> [usize; KINDS] {
        let mut counts = [0; KINDS];
        for cell in self.slots.iter().filter_map(|slot| slot.cell.as_ref()) {
            counts[Self::cell_kind(cell)] += 1;
        }
        counts
    }

    pub(crate) fn memory_profile(&self) -> &MemoryProfile {
        &self.memory_profile
    }

    pub(crate) fn live_payload_bytes(&self) -> [usize; KINDS] {
        let mut bytes = [0; KINDS];
        for cell in self.slots.iter().filter_map(|slot| slot.cell.as_ref()) {
            bytes[Self::cell_kind(cell)] += Self::cell_payload_bytes(cell);
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
                Cell::Array { elements, .. } => Some((elements.len(), elements.capacity())),
                _ => None,
            })
            .fold((0, 0), |(len, capacity), value| {
                (len + value.0, capacity + value.1)
            })
    }
}

fn cell_bytes(cell: &Cell) -> usize {
    match cell {
        Cell::Object(_) | Cell::Function { .. } => 0,
        Cell::Array { elements, .. } => elements.capacity() * size_of::<Value>(),
        Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
        Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
        Cell::WeakMap { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
        Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
        Cell::Iterator { .. } => 0,
        Cell::Environment { slots, .. } => slots.len() * size_of::<Value>(),
        Cell::String(value) | Cell::BigInt(value) | Cell::Error(value) => value.capacity(),
        Cell::Symbol(value) => value.as_ref().map_or(0, String::capacity),
        Cell::Date(_) => 0,
    }
}
