use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
#[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
use crate::vm::wtf16::JsString;
use rustc_hash::FxHashMap;
use std::rc::Rc;
mod access;
mod cell;
mod cell_access;
mod external;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod root;
mod slots;
mod weak;
pub(crate) use cell::*;
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::SlotArena;

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 256 KiB covers the measured 16,900-slot NavierStokes grids while
// still routing genuinely large `new Array(length)` allocations to sparse
// storage. Derive the slot count from the runtime Value representation.
const DENSE_ARRAY_HOLE_BUDGET_BYTES: usize = 256 * 1024;
const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
const LARGE_HEAP_MINIMUM_LIVE_CELLS: usize = 1 << 16;
const MINIMUM_GC_ALLOCATION_HEADROOM: usize = 384;

#[derive(Clone, Copy)]
struct GcHeadroomFactor {
    numerator: usize,
    denominator: usize,
}

impl GcHeadroomFactor {
    fn allocation_headroom(self, live_cells: usize) -> usize {
        debug_assert!(self.denominator > 0);
        debug_assert!(self.numerator <= self.denominator);
        let whole = live_cells / self.denominator;
        let remainder = live_cells % self.denominator;
        whole * self.numerator + remainder * self.numerator / self.denominator
    }
}

const SMALL_HEAP_GC_HEADROOM: GcHeadroomFactor = GcHeadroomFactor {
    numerator: 1,
    denominator: 2,
};
// This is the sweep variable for large-heap RSS/Score measurements. A factor
// of 1/1 allows one live set's worth of new cells before collection (2x total
// occupied high-water); 3/4 targets 1.75x and 1/2 targets 1.5x.
const LARGE_HEAP_GC_HEADROOM: GcHeadroomFactor = GcHeadroomFactor {
    numerator: 1,
    denominator: 1,
};

fn gc_allocation_headroom(live_cells: usize) -> usize {
    let factor = if live_cells >= LARGE_HEAP_MINIMUM_LIVE_CELLS {
        LARGE_HEAP_GC_HEADROOM
    } else {
        SMALL_HEAP_GC_HEADROOM
    };
    factor
        .allocation_headroom(live_cells)
        .max(MINIMUM_GC_ALLOCATION_HEADROOM)
}

pub(super) struct Slot {
    cell: Option<Cell>,
}
#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    marks: Vec<u64>,
    free: Vec<u32>,
    retired_slots: usize,
    generations: Vec<u32>,
    external_bytes: usize,
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
#[derive(Clone, Copy)]
pub(crate) struct GcProfile {
    pub allocated_kinds: [u64; CellKind::COUNT],
    pub allocated_payload_bytes: [u64; CellKind::COUNT],
    pub allocated_size_buckets: [[u64; 8]; CellKind::COUNT],
    pub roots: u64,
    pub work_items: u64,
    pub max_worklist: u64,
    pub marked: u64,
    pub freed: u64,
    pub sweep_slots: u64,
    pub mark_nanos: u64,
    pub sweep_nanos: u64,
    pub mark_clear_nanos: u64,
    pub allocations_between_collections_total: u64,
    pub allocations_between_collections_min: u64,
    pub allocations_between_collections_max: u64,
    pub survivors_total: u64,
    pub survivors_min: u64,
    pub survivors_max: u64,
    pub marked_kinds: [u64; CellKind::COUNT],
}
#[cfg(feature = "profile-aggregate")]
impl Default for GcProfile {
    fn default() -> Self {
        Self {
            allocated_kinds: [0; CellKind::COUNT],
            allocated_payload_bytes: [0; CellKind::COUNT],
            allocated_size_buckets: [[0; 8]; CellKind::COUNT],
            roots: 0,
            work_items: 0,
            max_worklist: 0,
            marked: 0,
            freed: 0,
            sweep_slots: 0,
            mark_nanos: 0,
            sweep_nanos: 0,
            mark_clear_nanos: 0,
            allocations_between_collections_total: 0,
            allocations_between_collections_min: 0,
            allocations_between_collections_max: 0,
            survivors_total: 0,
            survivors_min: 0,
            survivors_max: 0,
            marked_kinds: [0; CellKind::COUNT],
        }
    }
}

#[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
#[repr(usize)]
#[derive(Clone, Copy)]
pub(crate) enum CellKind {
    Object,
    Array,
    Map,
    Set,
    Iterator,
    WeakMap,
    WeakSet,
    WeakRef,
    Function,
    Environment,
    String,
    BigInt,
    Symbol,
    Date,
    Error,
    RegExp,
    ArrayFromAsync,
    TemporalDuration,
    TemporalPlainDate,
    TemporalPlainDateTime,
    TemporalPlainMonthDay,
    TemporalPlainYearMonth,
    TemporalZonedDateTime,
    TemporalInstant,
    WasmGlobal,
    WasmMemory,
    WasmTable,
    WasmElements,
    WasmBits64,
    WasmHostFunction,
    WasmV128,
    WasmGc,
    WasmExtern,
    WasmTag,
    WasmException,
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
impl CellKind {
    pub(crate) const COUNT: usize = Self::WasmException as usize + 1;
    #[cfg(feature = "profile-memory")]
    pub(crate) const NAMES: [&'static str; Self::COUNT] = [
        "object",
        "array",
        "map",
        "set",
        "iterator",
        "weak_map",
        "weak_set",
        "weak_ref",
        "function",
        "environment",
        "string",
        "bigint",
        "symbol",
        "date",
        "error",
        "regexp",
        "array_from_async",
        "temporal_duration",
        "temporal_plain_date",
        "temporal_plain_date_time",
        "temporal_plain_month_day",
        "temporal_plain_year_month",
        "temporal_zoned_date_time",
        "temporal_instant",
        "wasm_global",
        "wasm_memory",
        "wasm_table",
        "wasm_elements",
        "wasm_bits64",
        "wasm_host_function",
        "wasm_v128",
        "wasm_gc",
        "wasm_extern",
        "wasm_tag",
        "wasm_exception",
    ];
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
            threshold: MINIMUM_GC_ALLOCATION_HEADROOM,
            max_threshold: MINIMUM_GC_ALLOCATION_HEADROOM,
            ..Self::default()
        }
    }
    /// Keep the complete allocation census available to ownership tests.
    /// Explicit collection remains available; only threshold collection stops.
    #[cfg(test)]
    pub(crate) fn retain_allocations_for_test(&mut self) {
        self.threshold = usize::MAX;
    }

    #[cfg(test)]
    pub(crate) fn environment_count_for_test(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot.cell, Some(Cell::Environment { .. })))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn occupied_cell_count_for_test(&self) -> usize {
        self.slots.iter().filter(|slot| slot.cell.is_some()).count()
    }

    #[cfg(test)]
    pub(crate) fn root_count_for_test(&self) -> usize {
        self.roots.values().count()
    }

    pub fn alloc(&mut self, cell: Cell) -> Value {
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell) as usize;
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.allocations += 1;
        self.total_allocations += 1;
        self.external_bytes += cell.external_bytes();
        while let Some(index) = self.free.pop() {
            let Some(generation) = self.generations[index as usize].checked_add(1) else {
                self.retired_slots += 1;
                continue;
            };
            self.generations[index as usize] = generation;
            self.slots.get_mut(index as usize).unwrap().cell = Some(cell);
            #[cfg(feature = "profile-memory")]
            self.profile_allocation(index as usize);
            self.peak_live = self
                .peak_live
                .max(self.slots.len() - self.free.len() - self.retired_slots);
            return Value::heap(index);
        }
        let index = self.slots.len();
        self.slots.push(Slot { cell: Some(cell) });
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(index);
        if index / 64 == self.marks.len() {
            self.marks.push(0);
        }
        self.generations.push(1);
        self.peak_live = self
            .peak_live
            .max(self.slots.len() - self.free.len() - self.retired_slots);
        Value::heap(index as u32)
    }
    pub(crate) fn alloc_object_pair(
        &mut self,
        proto: Value,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        let values = [first, second];
        self.alloc_object_with_properties(proto, shape, &values)
    }
    pub(crate) fn alloc_object_with_properties(
        &mut self,
        proto: Value,
        shape: u32,
        values: &[Value],
    ) -> Value {
        let (properties, inline_properties) = self.initial_object_storage(shape, values);
        self.alloc(Cell::Object(Object::with_property_storage(
            proto,
            properties,
            inline_properties,
        )))
    }
    pub(crate) fn initialize_object_properties(
        &mut self,
        owner: Value,
        shape: u32,
        values: &[Value],
    ) {
        let (properties, inline_properties) = self.initial_object_storage(shape, values);
        let previous = {
            let object = self
                .get_mut(owner)
                .and_then(Cell::object_mut)
                .expect("property owner is an object");
            object.replace_property_storage(properties, inline_properties)
        };
        self.properties.release(previous);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(owner, values.len());
    }

    fn initial_object_storage(
        &mut self,
        shape: u32,
        values: &[Value],
    ) -> (ValueVec, [Value; INLINE_PROPERTY_COUNT]) {
        self.properties.assert_shape_length(shape, values.len());
        if values.len() <= INLINE_PROPERTY_COUNT {
            let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
            inline[..values.len()].copy_from_slice(values);
            (ValueVec::inline_property_storage(shape), inline)
        } else {
            (
                self.properties.with_values(shape, values),
                [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
            )
        }
    }
    pub(crate) fn register_property_shape(&mut self, shape: u32, length: usize) {
        self.properties.register_shape(shape, length);
    }
    pub(crate) fn remap_live_object_shapes(&mut self, mapping: &[u32], lengths: &[usize]) {
        for slot in self.slots.iter_mut() {
            let Some(object) = slot.cell.as_mut().and_then(Cell::object_mut) else {
                continue;
            };
            let old_shape = object.shape() as usize;
            object.set_shape(mapping[old_shape]);
        }
        self.properties.reset_shapes();
        for (shape, length) in lengths.iter().copied().enumerate() {
            self.properties.register_shape(shape as u32, length);
        }
    }
    pub(crate) fn compact_property_arena(&mut self) {
        if !self.properties.has_released_ranges() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        let mut objects = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let object = slot.cell.as_ref()?.object()?;
                self.properties
                    .has_compact_range(object.properties)
                    .then_some(())?;
                Some((object.properties.start_offset(), index))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable();
        let mut target = 0;
        for (_, index) in objects {
            let slot = self
                .slots
                .get_mut(index)
                .expect("property owner slot exists");
            let object = slot
                .cell
                .as_mut()
                .and_then(Cell::object_mut)
                .expect("property owner remains live during compaction");
            target += self
                .properties
                .compact_vector(&mut object.properties, target);
        }
        self.properties.finish_compaction(target);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            let after = self.properties.stats();
            eprintln!(
                "{{\"kind\":\"quench-property-compaction\",\"values_before\":{},\"values_after\":{},\"capacity_before\":{},\"capacity_after\":{},\"free_ranges_before\":{},\"free_ranges_after\":{}}}",
                before.0, after.0, before.1, after.1, before.2, after.2
            );
        }
    }
    pub(crate) fn reset(&mut self) {
        self.slots.clear();
        self.marks.clear();
        self.free.clear();
        self.retired_slots = 0;
        self.generations.clear();
        self.external_bytes = 0;
        self.allocations = 0;
        self.threshold = MINIMUM_GC_ALLOCATION_HEADROOM;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = MINIMUM_GC_ALLOCATION_HEADROOM;
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
    #[cfg(test)]
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        self.collect_with_object_roots(roots, |_, _, _| {})
    }
    pub(crate) fn collect_with_object_roots(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut object_roots: impl FnMut(Value, u32, &mut Vec<Value>),
    ) -> Vec<(Value, Value)> {
        self.collections += 1;
        #[cfg(feature = "profile-aggregate")]
        let allocations_between_collections = self.allocations as u64;
        #[cfg(feature = "profile-aggregate")]
        let mark_started = std::time::Instant::now();
        let mut work: Vec<Value> = roots.into_iter().chain(self.roots.values()).collect();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        {
            let mut ephemerons = weak::EphemeronWork::default();
            self.mark_work(&mut work, &mut object_roots, &mut ephemerons);
        }
        let finalization_jobs = self.prune_weak_entries();
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
                self.external_bytes = self.external_bytes.saturating_sub(cell.external_bytes());
                #[cfg(feature = "profile-memory")]
                {
                    let object_slots = cell
                        .object()
                        .map(|object| properties.len(object.properties));
                    self.memory_profile.freed(index, &cell, object_slots);
                }
                if let Some(object) = cell.object() {
                    properties.release(object.properties);
                }
                if let Some(arrays) = &mut self.sparse_arrays {
                    arrays.remove(&(index as u32));
                }
                // Reused slots enter `free` only after their side-table entries are gone.
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
        #[cfg(feature = "profile-aggregate")]
        let mark_clear_started = std::time::Instant::now();
        self.marks.fill(0);
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.mark_clear_nanos += mark_clear_started.elapsed().as_nanos() as u64;
            self.gc_profile.allocations_between_collections_total +=
                allocations_between_collections;
            self.gc_profile.survivors_total += live as u64;
            if self.collections == 1 {
                self.gc_profile.allocations_between_collections_min =
                    allocations_between_collections;
                self.gc_profile.allocations_between_collections_max =
                    allocations_between_collections;
                self.gc_profile.survivors_min = live as u64;
                self.gc_profile.survivors_max = live as u64;
            } else {
                self.gc_profile.allocations_between_collections_min = self
                    .gc_profile
                    .allocations_between_collections_min
                    .min(allocations_between_collections);
                self.gc_profile.allocations_between_collections_max = self
                    .gc_profile
                    .allocations_between_collections_max
                    .max(allocations_between_collections);
                self.gc_profile.survivors_min = self.gc_profile.survivors_min.min(live as u64);
                self.gc_profile.survivors_max = self.gc_profile.survivors_max.max(live as u64);
            }
        }
        self.allocations = 0;
        // `threshold` counts allocations after collection. The selected growth
        // factor is added to the live set to describe the occupied high-water.
        self.threshold = gc_allocation_headroom(live);
        self.peak_survivors = self.peak_survivors.max(live);
        self.max_threshold = self.max_threshold.max(self.threshold);
        finalization_jobs
    }
    fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
        ephemerons: &mut weak::EphemeronWork,
    ) {
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
                self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
            }
            Self::children(value, cell, &self.properties, work, object_roots);
            ephemerons.newly_marked(index as u32, cell, &self.marks, work);
            if let Some(arrays) = self.sparse_arrays.as_ref()
                && matches!(cell, Cell::Array { .. })
                && let Some(elements) = arrays.get(&(index as u32))
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
        let length = elements.length;
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH {
            self.sparse_set_length(array, length);
        }
    }
    pub(crate) fn sparse_set_length(&mut self, array: Value, length: usize) {
        let Some(Cell::Array { elements, .. }) = self.get(array) else {
            return;
        };
        let dense_length = elements.len();
        let sparse_length = self.sparse_length(array);
        if sparse_length.is_none() && length == dense_length {
            return;
        }
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH || length <= dense_length {
            self.materialize_sparse_array(array, length);
            return;
        }
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array.heap_index().unwrap()).or_default();
        if length < elements.length {
            elements.values.retain(|index, _| *index < length);
        }
        elements.length = length;
    }
    fn materialize_sparse_array(&mut self, array: Value, length: usize) {
        let Some(index) = array.heap_index() else {
            return;
        };
        let sparse = self
            .sparse_arrays
            .as_mut()
            .and_then(|arrays| arrays.remove(&index));
        if let Some(arrays) = &mut self.sparse_arrays
            && arrays.is_empty()
        {
            self.sparse_arrays = None;
        }
        let Some(Cell::Array { elements, .. }) = self.get_mut(array) else {
            return;
        };
        let dense_length = elements.len();
        let elements = Rc::make_mut(elements);
        elements.resize(length, Value::DELETED);
        for (index, value) in sparse.into_iter().flat_map(|elements| elements.values) {
            if index < length && (index >= dense_length || elements[index].is_deleted()) {
                elements[index] = value;
            }
        }
    }
    pub(crate) fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
        let value = match object.inline_properties() {
            Some(values) => values.get(slot).copied(),
            None => self.properties.get(object.properties, slot),
        }?;
        (!value.is_deleted()).then_some(value)
    }
    pub(crate) unsafe fn property_get_unchecked(&self, object: &Object, slot: usize) -> Value {
        if let Some(values) = object.inline_properties() {
            debug_assert!(slot < INLINE_PROPERTY_COUNT);
            // SAFETY: the caller proves that the property slot exists.
            unsafe { *values.get_unchecked(slot) }
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_unchecked(object.properties, slot) }
        }
    }
    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        let vector = {
            let data = self.get_mut(object).unwrap().object_mut().unwrap();
            if data.inline_properties().is_some() {
                data.set_inline_property(slot, value);
                return;
            }
            data.properties
        };
        self.properties.set(vector, slot, value);
    }
    pub(crate) unsafe fn property_set_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        value: Value,
    ) {
        let vector = {
            let data = self.get_mut(object).unwrap().object_mut().unwrap();
            if data.inline_properties().is_some() {
                debug_assert!(slot < INLINE_PROPERTY_COUNT);
                data.set_inline_property(slot, value);
                return;
            }
            data.properties
        };
        // SAFETY: the caller proves that the property slot exists.
        unsafe { self.properties.set_unchecked(vector, slot, value) };
    }
    pub(crate) fn property_push(&mut self, object: Value, value: Value) {
        let (vector, inline, slot) = {
            let data = self.get(object).unwrap().object().unwrap();
            (
                data.properties,
                data.inline_properties().copied(),
                self.properties.len(data.properties),
            )
        };
        if let Some(inline) = inline {
            if slot < INLINE_PROPERTY_COUNT {
                self.get_mut(object)
                    .unwrap()
                    .object_mut()
                    .unwrap()
                    .set_inline_property(slot, value);
                #[cfg(feature = "profile-memory")]
                self.note_object_slots(object, slot + 1);
                return;
            }
            debug_assert_eq!(slot, INLINE_PROPERTY_COUNT);
            let mut vector = self.properties.with_values(vector.auxiliary(), &inline);
            self.properties.push(&mut vector, value);
            self.get_mut(object)
                .unwrap()
                .object_mut()
                .unwrap()
                .replace_property_storage(vector, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
            #[cfg(feature = "profile-memory")]
            self.note_object_slots(object, slot + 1);
            return;
        }
        let mut vector = vector;
        self.properties.push(&mut vector, value);
        let data = self.get_mut(object).unwrap().object_mut().unwrap();
        data.properties = vector;
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, slot + 1);
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn set_memory_allocation_site(&mut self, program: u32, function: u32, pc: usize) {
        self.memory_profile
            .set_allocation_site(memory_profile::AllocationSite {
                program,
                function,
                pc,
            });
    }

    #[cfg(feature = "profile-memory")]
    fn profile_allocation(&mut self, index: usize) {
        let cell = self.slots.get(index).unwrap().cell.as_ref().unwrap();
        let object_slots = cell
            .object()
            .map(|object| self.properties.len(object.properties));
        self.memory_profile.allocated(index, cell, object_slots);
    }

    #[cfg(feature = "profile-memory")]
    fn note_object_slots(&mut self, object: Value, slots: usize) {
        if let Some(index) = object.heap_index() {
            self.memory_profile
                .object_slots_changed(index as usize, slots);
        }
    }
    fn children(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
    ) {
        let mut object = |object: &Object| {
            work.push(object.proto);
            object_roots(owner, object.shape(), work);
            work.extend(object.private_names().iter().map(|brand| brand.home));
            if let Some(values) = object.inline_properties() {
                work.extend(values.iter().copied().filter(|value| !value.is_deleted()));
            } else {
                properties.append_live_values(object.properties, work);
            }
            object.visit_stack_data_roots(|value| work.push(value));
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
            Cell::RegExp {
                object: value,
                legacy_constructor,
                ..
            } => {
                object(value);
                work.push(legacy_constructor.constructor());
            }
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
            Cell::ShadowRealm {
                object: value,
                caller_global,
                realm_global,
            } => {
                object(value);
                work.extend([*caller_global, *realm_global]);
            }
            Cell::WeakMap { object: value, .. } | Cell::WeakSet { object: value, .. } => {
                object(value);
            }
            Cell::WeakRef { object: value, .. } => object(value),
            Cell::FinalizationRegistry {
                object: value,
                callback,
                entries,
            } => {
                object(value);
                work.push(*callback);
                work.extend(entries.iter().map(|entry| entry.held));
            }
            Cell::Iterator {
                object: value,
                source,
                next_method,
                helper,
                generator,
                ..
            } => {
                object(value);
                work.push(*source);
                if !next_method.is_deleted() {
                    work.push(*next_method);
                }
                if let Some(helper) = helper {
                    match helper.as_ref() {
                        IteratorHelper::Map { callback, .. }
                        | IteratorHelper::Filter { callback, .. }
                        | IteratorHelper::FlatMap { callback, .. } => work.push(*callback),
                        IteratorHelper::Take { .. }
                        | IteratorHelper::Drop { .. }
                        | IteratorHelper::RegExpStringMatchAll { .. } => {}
                        IteratorHelper::Concat {
                            items,
                            methods,
                            opened,
                            active,
                            ..
                        } => {
                            work.extend(items.iter().copied());
                            work.extend(methods.iter().copied());
                            work.extend(opened.iter().flatten().copied());
                            work.extend(*active);
                        }
                        IteratorHelper::Zip {
                            iterators,
                            padding,
                            keys,
                            ..
                        } => {
                            work.extend(iterators.iter().copied());
                            work.extend(padding.iter().copied());
                            work.extend(keys.iter().flatten().copied());
                        }
                    }
                    match helper.as_ref() {
                        IteratorHelper::FlatMap {
                            inner: Some(inner), ..
                        } => work.push(*inner),
                        IteratorHelper::FlatMap { inner: None, .. } => {}
                        IteratorHelper::Zip { .. }
                        | IteratorHelper::Map { .. }
                        | IteratorHelper::Filter { .. }
                        | IteratorHelper::Take { .. }
                        | IteratorHelper::Drop { .. }
                        | IteratorHelper::Concat { .. }
                        | IteratorHelper::RegExpStringMatchAll { .. } => {}
                    }
                }
                if let Some(record) = generator {
                    work.extend(record.roots());
                }
            }
            Cell::ArrayFromAsyncState(state) => {
                work.extend([state.output, state.result, state.this_arg]);
                work.extend(state.iterator);
                work.extend(state.mapper);
                work.extend(state.array_like.map(|(source, _)| source));
            }
            Cell::Proxy {
                object: value,
                target,
                handler,
                ..
            } => {
                object(value);
                work.extend([*target, *handler]);
            }
            Cell::Function {
                object: value,
                env,
                realm,
                ..
            } => {
                object(value);
                work.extend([*env, *realm]);
            }
            Cell::WasmGlobal { value, .. } => work.push(*value),
            Cell::BindingReference { environment, .. } => work.push(*environment),
            Cell::Environment {
                parent,
                slots,
                dynamic_bindings,
                with_objects,
                ..
            } => {
                work.push(*parent);
                work.extend(slots.roots());
                match dynamic_bindings.as_ref() {
                    EnvironmentBindings::Owned(bindings) => {
                        work.extend(bindings.iter().map(|(_, value)| *value));
                    }
                    EnvironmentBindings::Shared(owner) => work.push(*owner),
                }
                work.extend(with_objects.iter().copied());
            }
            Cell::WasmElements(elements) | Cell::WasmTable { elements, .. } => {
                work.extend(elements.iter().copied())
            }
            Cell::WasmGc {
                fields, descriptor, ..
            } => {
                work.extend(fields.iter().copied());
                work.extend(descriptor.iter().copied());
            }
            Cell::WasmExtern(value) => work.push(*value),
            Cell::WasmException { tag, payload } => {
                work.push(*tag);
                work.extend(payload.iter().copied());
            }
            Cell::PromiseResolvingState { promise, .. } => work.push(*promise),
            Cell::Date { object: value, .. }
            | Cell::TemporalDuration { object: value, .. }
            | Cell::TemporalPlainDate { object: value, .. }
            | Cell::TemporalPlainDateTime { object: value, .. }
            | Cell::TemporalPlainMonthDay { object: value, .. }
            | Cell::TemporalPlainYearMonth { object: value, .. }
            | Cell::TemporalZonedDateTime { object: value, .. }
            | Cell::TemporalInstant { object: value, .. } => object(value),
            Cell::String(_)
            | Cell::BigInt(_)
            | Cell::Symbol(_)
            | Cell::Error(_)
            | Cell::WasmTag { .. }
            | Cell::WasmHostFunction { .. }
            | Cell::WasmBits64(_)
            | Cell::WasmV128(_)
            | Cell::WasmMemory { .. } => {}
            _ => unreachable!("typed array backing handled above"),
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_kind(cell: &Cell) -> CellKind {
        match cell {
            Cell::Object(_)
            | Cell::ArrayBuffer { .. }
            | Cell::TypedArray { .. }
            | Cell::DataView { .. }
            | Cell::ShadowRealm { .. }
            | Cell::Proxy { .. }
            | Cell::PromiseResolvingState { .. } => CellKind::Object,
            Cell::Array { .. } => CellKind::Array,
            Cell::Map { .. } => CellKind::Map,
            Cell::Set { .. } => CellKind::Set,
            Cell::Iterator { .. } => CellKind::Iterator,
            Cell::WeakMap { .. } => CellKind::WeakMap,
            Cell::WeakSet { .. } => CellKind::WeakSet,
            Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. } => CellKind::WeakRef,
            Cell::Function { .. } => CellKind::Function,
            Cell::Environment { .. } | Cell::BindingReference { .. } => CellKind::Environment,
            Cell::String(_) => CellKind::String,
            Cell::BigInt(_) => CellKind::BigInt,
            Cell::WasmElements(_) => CellKind::WasmElements,
            Cell::WasmGlobal { .. } => CellKind::WasmGlobal,
            Cell::WasmMemory { .. } => CellKind::WasmMemory,
            Cell::WasmTable { .. } => CellKind::WasmTable,
            Cell::WasmBits64(_) => CellKind::WasmBits64,
            Cell::WasmV128(_) => CellKind::WasmV128,
            Cell::WasmExtern(_) => CellKind::WasmExtern,
            Cell::WasmTag { .. } => CellKind::WasmTag,
            Cell::WasmException { .. } => CellKind::WasmException,
            Cell::WasmGc { .. } => CellKind::WasmGc,
            Cell::WasmHostFunction { .. } => CellKind::WasmHostFunction,
            Cell::Symbol(_) => CellKind::Symbol,
            Cell::Date { .. } => CellKind::Date,
            Cell::Error(_) => CellKind::Error,
            Cell::RegExp { .. } => CellKind::RegExp,
            Cell::ArrayFromAsyncState(_) => CellKind::ArrayFromAsync,
            Cell::TemporalDuration { .. } => CellKind::TemporalDuration,
            Cell::TemporalPlainDate { .. } => CellKind::TemporalPlainDate,
            Cell::TemporalPlainDateTime { .. } => CellKind::TemporalPlainDateTime,
            Cell::TemporalPlainMonthDay { .. } => CellKind::TemporalPlainMonthDay,
            Cell::TemporalPlainYearMonth { .. } => CellKind::TemporalPlainYearMonth,
            Cell::TemporalZonedDateTime { .. } => CellKind::TemporalZonedDateTime,
            Cell::TemporalInstant { .. } => CellKind::TemporalInstant,
        }
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        let object_extra_bytes = cell
            .object()
            .map_or(0, |object| object.allocated_extra_bytes());
        object_extra_bytes
            + match cell {
                Cell::Object(_)
                | Cell::ShadowRealm { .. }
                | Cell::Iterator { .. }
                | Cell::Proxy { .. }
                | Cell::Date { .. }
                | Cell::PromiseResolvingState { .. }
                | Cell::BindingReference { .. }
                | Cell::TemporalInstant { .. }
                | Cell::TypedArray { .. }
                | Cell::DataView { .. }
                | Cell::WeakRef { .. }
                | Cell::FinalizationRegistry { .. }
                | Cell::WasmGlobal { .. }
                | Cell::WasmBits64(_)
                | Cell::WasmV128(_)
                | Cell::WasmExtern(_)
                | Cell::WasmTag { .. } => 0,
                Cell::WasmException { payload, .. } => payload.capacity() * size_of::<Value>(),
                Cell::ArrayFromAsyncState(_) => size_of::<ArrayFromAsyncState>(),
                Cell::TemporalDuration { .. } => size_of::<[f64; 10]>(),
                Cell::WasmHostFunction { signature, .. } => {
                    size_of::<crate::WasmSignature>()
                        + (signature.params.capacity() + signature.results.capacity())
                            * size_of::<crate::WasmType>()
                }
                Cell::WasmElements(elements)
                | Cell::WasmTable { elements, .. }
                | Cell::WasmGc {
                    fields: elements, ..
                } => elements.capacity() * size_of::<Value>(),
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
                Cell::TemporalZonedDateTime {
                    time_zone,
                    calendar,
                    ..
                } => size_of::<String>() + time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { source, flags, .. } => {
                    size_of::<JsString>() + source.capacity() + flags.capacity()
                }
                Cell::Array { elements, .. } => elements.capacity() * size_of::<Value>(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::WasmMemory { bytes, .. } => bytes.capacity(),
                Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
                Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::WeakMap { entries, .. } => {
                    size_of::<WeakMapEntries>() + entries.allocated_bytes()
                }
                Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::Function { .. } => size_of::<Object>(),
                Cell::Environment {
                    slots,
                    with_objects,
                    ..
                } => {
                    size_of::<EnvironmentBindings>()
                        + slots.len() * size_of::<EnvironmentSlot>()
                        + with_objects.len() * size_of::<Value>()
                }
                Cell::String(value) => value.capacity(),
                Cell::BigInt(value) | Cell::Error(value) => value.capacity(),
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
