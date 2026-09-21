use crate::value::Value;
use crate::value_vec::ValueArena;
use rustc_hash::FxHashMap;

mod cell;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod root;
mod slots;
mod weak;
pub(crate) use cell::*;
pub use root::RootId;
pub(crate) use root::RootTable;
use slots::SlotArena;
pub(super) struct Slot {
    cell: Option<Cell>,
}
#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    marks: Vec<u64>,
    free: Vec<u32>,
    allocations: usize,
    threshold: usize,
    total_allocations: u64,
    collections: u64,
    peak_live: usize,
    peak_survivors: usize,
    max_threshold: usize,
    properties: ValueArena,
    roots: RootTable,
    sparse_arrays: Option<Box<FxHashMap<u32, SparseElements>>>,
    #[cfg(feature = "profile-aggregate")]
    gc_profile: GcProfile,
    #[cfg(feature = "profile-memory")]
    memory_profile: memory_profile::MemoryProfile,
}
#[cfg(feature = "profile-aggregate")]
#[derive(Clone, Copy, Default)]
pub(crate) struct GcProfile {
    pub allocated_kinds: [u64; 15],
    pub allocated_payload_bytes: [u64; 15],
    pub allocated_size_buckets: [[u64; 8]; 15],
    pub roots: u64,
    pub work_items: u64,
    pub max_worklist: u64,
    pub marked: u64,
    pub freed: u64,
    pub sweep_slots: u64,
    pub mark_nanos: u64,
    pub sweep_nanos: u64,
    pub marked_kinds: [u64; 15],
}
#[derive(Default)]
struct SparseElements {
    values: FxHashMap<usize, Value>,
    length: usize,
}
impl Heap {
    pub fn new() -> Self {
        Self {
            slots: SlotArena::with_small_capacity(),
            marks: Vec::with_capacity(12),
            free: Vec::with_capacity(384),
            threshold: 384,
            max_threshold: 384,
            ..Self::default()
        }
    }
    pub fn alloc(&mut self, cell: Cell) -> Value {
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell);
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.allocations += 1;
        self.total_allocations += 1;
        if let Some(index) = self.free.pop() {
            if let Some(arrays) = &mut self.sparse_arrays {
                arrays.remove(&index);
            }
            self.slots.get_mut(index as usize).unwrap().cell = Some(cell);
            #[cfg(feature = "profile-memory")]
            self.memory_profile.allocated(
                index as usize,
                self.slots
                    .get(index as usize)
                    .unwrap()
                    .cell
                    .as_ref()
                    .unwrap(),
            );
            self.peak_live = self.peak_live.max(self.slots.len() - self.free.len());
            return Value::heap(index);
        }
        let index = self.slots.len();
        self.slots.push(Slot { cell: Some(cell) });
        #[cfg(feature = "profile-memory")]
        self.memory_profile
            .allocated(index, self.slots.get(index).unwrap().cell.as_ref().unwrap());
        if index / 64 == self.marks.len() {
            self.marks.push(0);
        }
        self.peak_live = self.peak_live.max(self.slots.len() - self.free.len());
        Value::heap(index as u32)
    }
    pub(crate) fn alloc_object_pair(
        &mut self,
        proto: Value,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        let properties = self.properties.pair(shape, first, second);
        self.alloc(Cell::Object(Object { proto, properties }))
    }
    pub(crate) fn register_property_shape(&mut self, shape: u32, length: usize) {
        self.properties.register_shape(shape, length);
    }
    pub(crate) fn reset(&mut self) {
        self.slots.clear();
        self.marks.clear();
        self.free.clear();
        self.allocations = 0;
        self.threshold = 384;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = 384;
        self.properties.reset();
        self.roots.clear();
        self.sparse_arrays = None;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = GcProfile::default();
        }
        #[cfg(feature = "profile-memory")]
        {
            self.memory_profile = memory_profile::MemoryProfile::default();
        }
    }
    pub fn get(&self, value: Value) -> Option<&Cell> {
        let index = value.heap_index()? as usize;
        // SAFETY: heap Values are minted only by `alloc`; tracing keeps every
        // reachable handle live, and raw indices never cross the VM boundary.
        unsafe { self.slots.get_unchecked(index).cell.as_ref() }
    }
    pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
        let index = value.heap_index()? as usize;
        // SAFETY: identical live-handle invariant to `get`; unique mutable
        // access remains confined to this heap edge.
        unsafe { self.slots.get_unchecked_mut(index).cell.as_mut() }
    }
    pub fn should_collect(&self) -> bool {
        self.allocations >= self.threshold
    }
    pub(crate) fn root(&mut self, value: Value) -> RootId {
        self.roots.insert(value)
    }
    pub(crate) fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.roots.update(root, value)
    }
    pub(crate) fn release_root(&mut self, root: RootId) -> bool {
        self.roots.remove(root)
    }
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) {
        self.collections += 1;
        #[cfg(feature = "profile-aggregate")]
        let mark_started = std::time::Instant::now();
        let mut work: Vec<Value> = roots.into_iter().chain(self.roots.values()).collect();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        self.mark_work(&mut work);
        self.mark_ephemerons(&mut work);
        self.prune_weak_entries();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.mark_nanos += mark_started.elapsed().as_nanos() as u64;
        }
        #[cfg(feature = "profile-aggregate")]
        let sweep_started = std::time::Instant::now();
        let mut live = 0;
        let properties = &mut self.properties;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += self.slots.len() as u64;
        }
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if Self::marked(&self.marks, index) {
                live += 1;
            } else if let Some(cell) = slot.cell.take() {
                #[cfg(feature = "profile-memory")]
                self.memory_profile.freed(index, &cell);
                if let Some(object) = cell.object() {
                    properties.release(object.properties);
                }
                if let Some(arrays) = &mut self.sparse_arrays {
                    arrays.remove(&(index as u32));
                }
                self.free.push(index as u32);
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.freed += 1;
                }
            }
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        self.marks.fill(0);
        self.allocations = 0;
        // `threshold` counts allocations *after* this collection. Half the
        // surviving set therefore targets a 1.5x total occupied high-water.
        let headroom = if live >= 1 << 16 { live } else { live / 2 };
        self.threshold = headroom.max(384);
        self.peak_survivors = self.peak_survivors.max(live);
        self.max_threshold = self.max_threshold.max(self.threshold);
    }
    pub(super) fn mark_work(&mut self, work: &mut Vec<Value>) {
        while let Some(value) = work.pop() {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.work_items += 1;
            }
            let Some(index) = value.heap_index().map(|value| value as usize) else {
                continue;
            };
            let Some(slot) = self.slots.get_mut(index) else {
                continue;
            };
            if Self::marked(&self.marks, index) || slot.cell.is_none() {
                continue;
            }
            Self::mark(&mut self.marks, index);
            let cell = slot.cell.as_ref().unwrap();
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[Self::cell_kind(cell)] += 1;
            }
            Self::children(cell, &self.properties, work);
            if let Some(elements) = self
                .sparse_arrays
                .as_ref()
                .and_then(|arrays| arrays.get(&(index as u32)))
            {
                work.extend(elements.values.values().copied());
            }
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
            }
        }
    }
    #[inline(always)]
    fn marked(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }
    #[inline(always)]
    fn mark(marks: &mut [u64], index: usize) {
        marks[index / 64] |= 1 << (index % 64);
    }
    #[allow(dead_code)]
    pub fn stats(&self) -> (u64, u64, usize, usize, usize) {
        (
            self.total_allocations,
            self.collections,
            self.peak_live,
            self.peak_survivors,
            self.max_threshold,
        )
    }
    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn gc_profile(&self) -> GcProfile {
        self.gc_profile
    }
    pub(crate) fn sparse_get(&self, array: Value, index: usize) -> Option<Value> {
        self.sparse_arrays
            .as_ref()?
            .get(&array.heap_index()?)?
            .values
            .get(&index)
            .copied()
    }

    pub(crate) fn sparse_length(&self, array: Value) -> Option<usize> {
        self.sparse_arrays
            .as_ref()?
            .get(&array.heap_index()?)
            .map(|elements| elements.length)
    }

    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array.heap_index().unwrap()).or_default();
        elements.values.insert(index, value);
        elements.length = elements.length.max(index.saturating_add(1));
    }

    pub(crate) fn sparse_pop(&mut self, array: Value, dense_len: usize) -> Value {
        let index = array.heap_index().unwrap();
        let arrays = self.sparse_arrays.as_mut().unwrap();
        let elements = arrays.get_mut(&index).unwrap();
        debug_assert!(elements.length > dense_len);
        elements.length -= 1;
        let value = elements
            .values
            .remove(&elements.length)
            .unwrap_or(Value::UNDEFINED);
        if elements.length == dense_len {
            arrays.remove(&index);
        }
        value
    }
    pub(crate) fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
        self.properties.get(object.properties, slot)
    }
    pub(crate) unsafe fn property_get_unchecked(&self, object: &Object, slot: usize) -> Value {
        // SAFETY: forwarded immutable-shape slot invariant.
        unsafe { self.properties.get_unchecked(object.properties, slot) }
    }
    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        let vector = self.get(object).unwrap().object().unwrap().properties;
        self.properties.set(vector, slot, value);
    }
    pub(crate) unsafe fn property_set_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        value: Value,
    ) {
        let vector = self.get(object).unwrap().object().unwrap().properties;
        // SAFETY: forwarded immutable-shape slot invariant.
        unsafe { self.properties.set_unchecked(vector, slot, value) };
    }
    pub(crate) fn property_push(&mut self, object: Value, value: Value) {
        let mut vector = self.get(object).unwrap().object().unwrap().properties;
        self.properties.push(&mut vector, value);
        self.get_mut(object)
            .unwrap()
            .object_mut()
            .unwrap()
            .properties = vector;
    }
    fn children(cell: &Cell, properties: &ValueArena, work: &mut Vec<Value>) {
        let mut object = |object: &Object| {
            work.push(object.proto);
            work.extend(properties.values(object.properties).iter().copied());
        };
        if let Some((value, buffer)) = cell.typed_array_backing() {
            object(value);
            work.push(buffer);
            return;
        }
        match cell {
            Cell::Object(value) => object(value),
            Cell::Array {
                object: value,
                elements,
            } => {
                object(value);
                work.extend(elements.iter().copied());
            }
            Cell::ArrayBuffer { object: value, .. } => object(value),
            Cell::DataView {
                object: value,
                buffer,
                ..
            } => {
                object(value);
                work.push(*buffer);
            }
            Cell::Map {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter().flat_map(|(key, value)| [*key, *value]));
            }
            Cell::Set {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter().copied());
            }
            Cell::WeakMap { object: value, .. } | Cell::WeakSet { object: value, .. } => {
                object(value);
            }
            Cell::WeakRef { object: value, .. } => object(value),
            Cell::Iterator {
                object: value,
                source,
                ..
            } => {
                object(value);
                work.push(*source);
            }
            Cell::Function {
                object: value, env, ..
            } => {
                object(value);
                work.push(*env);
            }
            Cell::Environment { parent, slots, .. } => {
                work.push(*parent);
                work.extend(slots.iter().copied());
            }
            Cell::String(_)
            | Cell::BigInt(_)
            | Cell::Symbol(_)
            | Cell::Date(_)
            | Cell::Error(_) => {}
            _ => unreachable!("typed array backing handled above"),
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_kind(cell: &Cell) -> usize {
        match cell {
            Cell::Object(_) => 0,
            Cell::Array { .. } => 1,
            Cell::ArrayBuffer { .. } => 0,
            Cell::Uint8Array { .. }
            | Cell::Uint8ClampedArray { .. }
            | Cell::Uint16Array { .. }
            | Cell::Uint32Array { .. }
            | Cell::Int8Array { .. }
            | Cell::Int16Array { .. }
            | Cell::Int32Array { .. }
            | Cell::Float32Array { .. }
            | Cell::Float64Array { .. } => 0,
            Cell::DataView { .. } => 0,
            Cell::Map { .. } => 2,
            Cell::Set { .. } => 3,
            Cell::Iterator { .. } => 4,
            Cell::WeakMap { .. } => 5,
            Cell::WeakSet { .. } => 6,
            Cell::WeakRef { .. } => 7,
            Cell::Function { .. } => 8,
            Cell::Environment { .. } => 9,
            Cell::String(_) => 10,
            Cell::BigInt(_) => 11,
            Cell::Symbol(_) => 12,
            Cell::Date(_) => 13,
            Cell::Error(_) => 14,
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        match cell {
            Cell::Object(_) | Cell::Iterator { .. } | Cell::Date(_) => 0,
            Cell::Array { elements, .. } => elements.capacity() * size_of::<Value>(),
            Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
            Cell::Uint8Array { .. }
            | Cell::Uint8ClampedArray { .. }
            | Cell::Uint16Array { .. }
            | Cell::Uint32Array { .. }
            | Cell::Int8Array { .. }
            | Cell::Int16Array { .. }
            | Cell::Int32Array { .. }
            | Cell::Float32Array { .. }
            | Cell::Float64Array { .. } => 0,
            Cell::DataView { .. } => 0,
            Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
            Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
            Cell::WeakMap { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
            Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
            Cell::WeakRef { .. } => 0,
            Cell::Function { .. } => size_of::<Object>(),
            Cell::Environment { slots, .. } => slots.len() * size_of::<Value>(),
            Cell::String(value) | Cell::BigInt(value) | Cell::Error(value) => value.capacity(),
            Cell::Symbol(value) => value.as_ref().map_or(0, String::capacity),
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn size_bucket(bytes: usize) -> usize {
        match bytes {
            0 => 0,
            1..=7 => 1,
            8..=15 => 2,
            16..=31 => 3,
            32..=63 => 4,
            64..=127 => 5,
            128..=255 => 6,
            _ => 7,
        }
    }
}
#[cfg(test)]
mod tests;
