use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use rustc_hash::FxHashMap;
use std::rc::Rc;
mod access;
mod cell;
mod cell_access;
mod external;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod records;
mod root;
mod slots;
mod weak;
pub(crate) use cell::*;
use records::{ArrayRecordSpace, ObjectRecordSpace, RecordSweep};
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::SlotArena;

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 256 KiB covers the measured 16,900-slot NavierStokes grids while
// still sending genuinely large `new Array(length)` allocations to the sparse
// representation. The slot limit is derived from Value's representation.
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

impl Slot {
    fn is_live(&self) -> bool {
        self.cell.is_some()
    }

    fn clear(&mut self) -> Cell {
        self.cell.take().expect("only live slots are cleared")
    }
}

#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    object_records: ObjectRecordSpace,
    array_records: ArrayRecordSpace,
    marks: Vec<u64>,
    free: Vec<u32>,
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
    sparse_arrays: Option<Box<FxHashMap<Value, SparseElements>>>,
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    gc_profile: GcProfile,
    #[cfg(feature = "profile-memory")]
    memory_profile: memory_profile::MemoryProfile,
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
#[derive(Clone, Copy)]
pub(crate) struct GcProfile {
    #[cfg(feature = "profile-aggregate")]
    pub allocated_kinds: [u64; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub allocated_payload_bytes: [u64; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub allocated_size_buckets: [[u64; 8]; CellKind::COUNT],
    #[cfg(feature = "profile-aggregate")]
    pub roots: u64,
    #[cfg(feature = "profile-aggregate")]
    pub work_items: u64,
    #[cfg(feature = "profile-aggregate")]
    pub max_worklist: u64,
    #[cfg(feature = "profile-aggregate")]
    pub marked: u64,
    #[cfg(feature = "profile-aggregate")]
    pub freed: u64,
    #[cfg(feature = "profile-aggregate")]
    pub sweep_slots: u64,
    pub mark_nanos: u64,
    pub sweep_nanos: u64,
    pub mark_clear_nanos: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_total: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_min: u64,
    #[cfg(feature = "profile-aggregate")]
    pub allocations_between_collections_max: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_total: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_min: u64,
    #[cfg(feature = "profile-aggregate")]
    pub survivors_max: u64,
    #[cfg(feature = "profile-aggregate")]
    pub marked_kinds: [u64; CellKind::COUNT],
}
#[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
impl Default for GcProfile {
    fn default() -> Self {
        Self {
            #[cfg(feature = "profile-aggregate")]
            allocated_kinds: [0; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            allocated_payload_bytes: [0; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            allocated_size_buckets: [[0; 8]; CellKind::COUNT],
            #[cfg(feature = "profile-aggregate")]
            roots: 0,
            #[cfg(feature = "profile-aggregate")]
            work_items: 0,
            #[cfg(feature = "profile-aggregate")]
            max_worklist: 0,
            #[cfg(feature = "profile-aggregate")]
            marked: 0,
            #[cfg(feature = "profile-aggregate")]
            freed: 0,
            #[cfg(feature = "profile-aggregate")]
            sweep_slots: 0,
            mark_nanos: 0,
            sweep_nanos: 0,
            mark_clear_nanos: 0,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_total: 0,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_min: u64::MAX,
            #[cfg(feature = "profile-aggregate")]
            allocations_between_collections_max: 0,
            #[cfg(feature = "profile-aggregate")]
            survivors_total: 0,
            #[cfg(feature = "profile-aggregate")]
            survivors_min: u64::MAX,
            #[cfg(feature = "profile-aggregate")]
            survivors_max: 0,
            #[cfg(feature = "profile-aggregate")]
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
        (0..self.slots.len())
            .filter(|index| matches!(self.cell_at_index(*index), Some(Cell::Environment { .. })))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn occupied_cell_count_for_test(&self) -> usize {
        self.live_slots()
    }

    #[cfg(test)]
    pub(crate) fn root_count_for_test(&self) -> usize {
        self.roots.values().count()
    }

    /// Live legacy Cells and direct object-family records.
    pub(crate) fn live_slots(&self) -> usize {
        self.slots.len() - self.free.len()
            + self.object_records.live_count()
            + self.array_records.live_count()
    }
    pub fn alloc(&mut self, cell: Cell) -> Value {
        match &cell {
            Cell::Object(object) | Cell::Array { object, .. } => {
                self.properties.ensure_empty_shape(object.shape());
            }
            _ => {}
        }
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
        let cell = match cell {
            Cell::Object(object) => match self.object_records.try_insert(object) {
                Ok(record_id) => {
                    let value = Value::direct_object(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(object) => Cell::Object(object),
            },
            Cell::Array { object, elements } => match self
                .array_records
                .try_insert(ArrayRecord { object, elements })
            {
                Ok(record_id) => {
                    let value = Value::direct_array(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(array) => Cell::Array {
                    object: array.object,
                    elements: array.elements,
                },
            },
            cell => cell,
        };
        let reused_index = self.free.last().copied();
        let slot_index = reused_index.unwrap_or_else(|| {
            u32::try_from(self.slots.len()).expect("heap exhausted its 32-bit Value index space")
        });
        if let Some(index) = self.free.pop() {
            debug_assert_eq!(Some(index), reused_index);
            let slot = self.slots.get_mut(index as usize).unwrap();
            debug_assert!(!slot.is_live(), "free-list slots hold no payload");
            // Collection clears heap weak owners before adding a dead slot here;
            // the VM clears its weak closure cache before control can allocate.
            slot.cell = Some(cell);
            #[cfg(feature = "profile-memory")]
            self.profile_allocation(Value::heap(index));
            return Value::heap(index);
        }
        let index = self.slots.len();
        debug_assert_eq!(index as u32, slot_index);
        self.slots.push(Slot { cell: Some(cell) });
        if index / 64 == self.marks.len() {
            self.marks.push(0);
        }
        let value = Value::heap(index as u32);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }
    pub(crate) fn alloc_object_pair(
        &mut self,
        shape: u32,
        first: Value,
        second: Value,
    ) -> Value {
        let values = [first, second];
        self.alloc_object_with_properties(shape, &values)
    }
    pub(crate) fn alloc_object_with_properties(
        &mut self,
        shape: u32,
        values: &[Value],
    ) -> Value {
        let (properties, inline_properties) = self.initial_object_storage(shape, values);
        self.alloc(Cell::Object(Object::with_property_storage(
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
            let object = self.object_mut(owner).expect("property owner is an object");
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
        self.object_records.for_each_live_mut(|_, object| {
            let old_shape = object.shape() as usize;
            object.set_shape(mapping[old_shape]);
        });
        self.array_records.for_each_live_mut(|_, array| {
            let old_shape = array.object.shape() as usize;
            array.object.set_shape(mapping[old_shape]);
        });
        for index in 0..self.slots.len() {
            let Some(object) = self.object_at_index_mut(index) else {
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
    /// Slide every live arena range down over released ones, in offset order.
    /// `objects` holds each live arena owner packed with its range offset.
    fn compact_property_arena(&mut self, mut objects: Vec<(usize, Value)>) {
        if !self.properties.has_released_ranges() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        let mut target = 0;
        let Self {
            object_records,
            array_records,
            slots,
            properties,
            ..
        } = self;
        for (_, owner) in objects {
            let object = if let Some(record_id) = owner.direct_object_index() {
                // SAFETY: direct owners were collected from live object records.
                Some(unsafe { object_records.get_live_mut(record_id) })
            } else if let Some(record_id) = owner.direct_array_index() {
                // SAFETY: array owners were collected from live array records.
                Some(&mut unsafe { array_records.get_live_mut(record_id) }.object)
            } else {
                owner
                    .legacy_heap_index()
                    .and_then(|index| slots.get_mut(index as usize))
                    .and_then(|slot| slot.cell.as_mut())
                    .and_then(Cell::object_mut)
            }
            .expect("property owner remains live during compaction");
            target += properties.compact_vector(&mut object.properties, target);
        }
        properties.finish_compaction(target);
        #[cfg(feature = "profile-memory")]
        if std::env::var_os("QUENCH_MEMORY").is_some() {
            let after = properties.stats();
            eprintln!(
                "{{\"kind\":\"quench-property-compaction\",\"values_before\":{},\"values_after\":{},\"capacity_before\":{},\"capacity_after\":{},\"free_ranges_before\":{},\"free_ranges_after\":{}}}",
                before.0, after.0, before.1, after.1, before.2, after.2
            );
        }
    }
    pub(crate) fn reset(&mut self) {
        self.slots.clear();
        self.object_records = ObjectRecordSpace::default();
        self.array_records = ArrayRecordSpace::default();
        self.marks.clear();
        self.free.clear();
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
        // Live slots grow only by allocation and shrink only by sweeping, so the peak is
        // observed here and when it is reported rather than on every allocation.
        self.peak_live = self.peak_live.max(self.live_slots());
        self.collections += 1;
        #[cfg(feature = "profile-aggregate")]
        {
            let allocations = self.allocations as u64;
            let profile = &mut self.gc_profile;
            profile.allocations_between_collections_total += allocations;
            profile.allocations_between_collections_min =
                profile.allocations_between_collections_min.min(allocations);
            profile.allocations_between_collections_max =
                profile.allocations_between_collections_max.max(allocations);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
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
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.mark_nanos += mark_started.elapsed().as_nanos() as u64;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let mut live = 0;
        let mut property_owners = Vec::new();
        let properties = &mut self.properties;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += (self.slots.len()
                + self.object_records.high_water()
                + self.array_records.high_water())
                as u64;
        }
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut self.memory_profile;
        let (object_live, object_freed) = self.object_records.sweep(|record| match record {
            RecordSweep::Keep(record_id, object) => {
                let vector = object.properties;
                if properties.has_compact_range(vector) {
                    property_owners.push((vector.start_offset(), Value::direct_object(record_id)));
                }
            }
            RecordSweep::Dead(_record_id, object) => {
                #[cfg(feature = "profile-memory")]
                let slots = properties.len(object.properties);
                properties.release(object.properties);
                #[cfg(feature = "profile-memory")]
                memory_profile.freed_object(Value::direct_object(_record_id), slots);
            }
        });
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = object_freed;
        live += object_live;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.freed += object_freed as u64;
        }
        let mut dead_array_ids = Vec::new();
        let (array_live, array_freed) = self.array_records.sweep(|record| match record {
            RecordSweep::Keep(record_id, array) => {
                let vector = array.object.properties;
                if properties.has_compact_range(vector) {
                    property_owners.push((vector.start_offset(), Value::direct_array(record_id)));
                }
            }
            RecordSweep::Dead(_record_id, array) => {
                #[cfg(feature = "profile-memory")]
                let slots = properties.len(array.object.properties);
                properties.release(array.object.properties);
                #[cfg(feature = "profile-memory")]
                memory_profile.freed_array(Value::direct_array(_record_id), slots, &array);
                dead_array_ids.push(_record_id);
            }
        });
        let clear_sparse_arrays = if let Some(sparse_arrays) = &mut self.sparse_arrays {
            for record_id in dead_array_ids {
                sparse_arrays.remove(&Value::direct_array(record_id));
            }
            sparse_arrays.is_empty()
        } else {
            false
        };
        if clear_sparse_arrays {
            self.sparse_arrays = None;
        }
        live += array_live;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.freed += array_freed as u64;
        }
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = array_freed;
        for index in 0..self.slots.len() {
            let slot = self.slots.get(index).expect("slot index is in range");
            if !slot.is_live() {
                continue;
            }
            if Self::marked(&self.marks, index) {
                live += 1;
                let property_vector = slot
                    .cell
                    .as_ref()
                    .and_then(Cell::object)
                    .map(|object| object.properties);
                if let Some(property_vector) = property_vector
                    && properties.has_compact_range(property_vector)
                {
                    property_owners
                        .push((property_vector.start_offset(), Value::heap(index as u32)));
                }
                continue;
            }

            let cell = slot.cell.as_ref().expect("live slot contains a Cell");
            self.external_bytes = self.external_bytes.saturating_sub(cell.external_bytes());
            #[cfg(feature = "profile-memory")]
            {
                let object_slots = cell
                    .object()
                    .map(|object| properties.len(object.properties));
                memory_profile.freed(Value::heap(index as u32), cell, object_slots);
            }
            if let Some(object) = cell.object() {
                properties.release(object.properties);
            }
            if let Some(arrays) = &mut self.sparse_arrays {
                arrays.remove(&Value::heap(index as u32));
            }
            let cell = self
                .slots
                .get_mut(index)
                .expect("slot index is in range")
                .clear();
            drop(cell);
            // Reused slots enter `free` only after their side-table entries and
            // backing Cell are gone.
            self.free.push(index as u32);
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.freed += 1;
            }
        }
        self.compact_property_arena(property_owners);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let clear_started = std::time::Instant::now();
        self.marks.fill(0);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.mark_clear_nanos += clear_started.elapsed().as_nanos() as u64;
        }
        #[cfg(feature = "profile-aggregate")]
        {
            let survivors = live as u64;
            let profile = &mut self.gc_profile;
            profile.survivors_total += survivors;
            profile.survivors_min = profile.survivors_min.min(survivors);
            profile.survivors_max = profile.survivors_max.max(survivors);
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
            if let Some(record_id) = value.direct_object_index() {
                if !self.object_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Object as usize] += 1;
                }
                // SAFETY: mark succeeded only for an occupied dense object ID.
                let object = unsafe { self.object_records.get_live(record_id) };
                Self::object_children(value, object, &self.properties, work, object_roots);
                ephemerons.newly_marked(value, None, |target| self.is_marked_value(target), work);
            } else if let Some(record_id) = value.direct_array_index() {
                if !self.array_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Array as usize] += 1;
                }
                // SAFETY: a direct Array Value names an initialized record.
                let array = unsafe { self.array_records.get_live(record_id) };
                Self::array_children(value, array, &self.properties, work, object_roots);
                if let Some(arrays) = self.sparse_arrays.as_ref()
                    && let Some(elements) = arrays.get(&value)
                {
                    work.extend(elements.values.values().copied());
                }
                ephemerons.newly_marked(value, None, |target| self.is_marked_value(target), work);
            } else {
                let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
                    continue;
                };
                let Some(slot) = self.slots.get(index) else {
                    continue;
                };
                if Self::marked(&self.marks, index) || !slot.is_live() {
                    continue;
                }
                Self::mark(&mut self.marks, index);
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                }
                let cell = slot.cell.as_ref().expect("live slot contains a Cell");
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
                }
                Self::children(value, cell, &self.properties, work, object_roots);
                ephemerons.newly_marked(
                    value,
                    Some(cell),
                    |target| self.is_marked_value(target),
                    work,
                );
                if let Some(arrays) = self.sparse_arrays.as_ref()
                    && matches!(cell, Cell::Array { .. })
                    && let Some(elements) = arrays.get(&Value::heap(index as u32))
                {
                    work.extend(elements.values.values().copied());
                }
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
    fn is_marked_value(&self, value: Value) -> bool {
        if let Some(record_id) = value.direct_object_index() {
            return self.object_records.is_marked(record_id);
        }
        if let Some(record_id) = value.direct_array_index() {
            return self.array_records.is_marked(record_id);
        }
        let Some(index) = value.legacy_heap_index().map(|index| index as usize) else {
            return false;
        };
        self.slots
            .get(index)
            .is_some_and(|slot| slot.is_live() && Self::marked(&self.marks, index))
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    pub(crate) const fn gc_profile(&self) -> GcProfile {
        self.gc_profile
    }
    pub(crate) fn sparse_get(&self, array: Value, index: usize) -> Option<Value> {
        self.sparse_arrays
            .as_ref()?
            .get(&array)?
            .values
            .get(&index)
            .copied()
    }
    pub(crate) fn sparse_length(&self, array: Value) -> Option<usize> {
        self.sparse_arrays
            .as_ref()?
            .get(&array)
            .map(|elements| elements.length)
    }
    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array).or_default();
        elements.values.insert(index, value);
        elements.length = elements.length.max(index.saturating_add(1));
        let length = elements.length;
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH {
            self.sparse_set_length(array, length);
        }
    }
    pub(crate) fn sparse_set_length(&mut self, array: Value, length: usize) {
        let Some((_, elements)) = self.array(array) else {
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
        let elements = arrays.entry(array).or_default();
        if length < elements.length {
            elements.values.retain(|index, _| *index < length);
        }
        elements.length = length;
    }
    fn materialize_sparse_array(&mut self, array: Value, length: usize) {
        let sparse = self
            .sparse_arrays
            .as_mut()
            .and_then(|arrays| arrays.remove(&array));
        if let Some(arrays) = &mut self.sparse_arrays
            && arrays.is_empty()
        {
            self.sparse_arrays = None;
        }
        let Some((_, elements)) = self.array_mut(array) else {
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
    #[inline(always)]
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
            let data = self.object_mut(object).unwrap();
            if data.inline_properties().is_some() {
                data.set_inline_property(slot, value);
                return;
            }
            data.properties
        };
        self.properties.set(vector, slot, value);
    }
    #[inline(always)]
    pub(crate) unsafe fn property_set_unchecked(
        &mut self,
        object: Value,
        slot: usize,
        value: Value,
    ) {
        let vector = {
            let data = self.object_mut(object).unwrap();
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
    /// One cached property add: appends `value` at `slot`, the object's current storage
    /// length, and moves the object to `shape`, whose storage length is `slot + 1`.
    #[inline(always)]
    pub(crate) fn property_add(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        let data = self.object_mut(object).unwrap();
        if slot < INLINE_PROPERTY_COUNT && data.inline_properties().is_some() {
            data.set_inline_property(slot, value);
            data.set_shape(shape);
            #[cfg(feature = "profile-memory")]
            self.note_object_slots(object, slot + 1);
            return;
        }
        self.property_push(object, value);
        self.object_mut(object).unwrap().set_shape(shape);
    }
    pub(crate) fn property_push(&mut self, object: Value, value: Value) {
        let (vector, inline, slot) = {
            let data = self.object(object).unwrap();
            (
                data.properties,
                data.inline_properties().copied(),
                self.properties.len(data.properties),
            )
        };
        if let Some(inline) = inline {
            if slot < INLINE_PROPERTY_COUNT {
                self.object_mut(object)
                    .unwrap()
                    .set_inline_property(slot, value);
                #[cfg(feature = "profile-memory")]
                self.note_object_slots(object, slot + 1);
                return;
            }
            debug_assert_eq!(slot, INLINE_PROPERTY_COUNT);
            let mut vector = self.properties.with_values(vector.auxiliary(), &inline);
            self.properties.push(&mut vector, value);
            self.object_mut(object)
                .unwrap()
                .replace_property_storage(vector, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
            #[cfg(feature = "profile-memory")]
            self.note_object_slots(object, slot + 1);
            return;
        }
        let mut vector = vector;
        self.properties.push(&mut vector, value);
        let data = self.object_mut(object).unwrap();
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
    fn profile_allocation(&mut self, identity: Value) {
        let Self {
            object_records,
            array_records,
            slots,
            properties,
            memory_profile,
            ..
        } = self;
        if let Some(record_id) = identity.direct_object_index() {
            // SAFETY: this function is called immediately after insertion.
            let object = unsafe { object_records.get_live(record_id) };
            memory_profile.allocated_object(identity, object, properties.len(object.properties));
        } else if let Some(record_id) = identity.direct_array_index() {
            // SAFETY: profile runs immediately after inserting this array.
            let array = unsafe { array_records.get_live(record_id) };
            memory_profile.allocated_array(
                identity,
                array,
                properties.len(array.object.properties),
            );
        } else {
            let index = identity
                .legacy_heap_index()
                .expect("legacy allocation has a slot index") as usize;
            let cell = slots
                .get(index)
                .and_then(|slot| slot.cell.as_ref())
                .expect("allocated slot contains its Cell");
            let object_slots = cell
                .object()
                .map(|object| properties.len(object.properties));
            memory_profile.allocated(identity, cell, object_slots);
        }
    }

    #[cfg(feature = "profile-memory")]
    fn note_object_slots(&mut self, object: Value, slots: usize) {
        self.memory_profile.object_slots_changed(object, slots);
    }
    fn children(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
    ) {
        let mut object =
            |object: &Object| Self::object_children(owner, object, properties, work, object_roots);
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
                work.extend(elements.iter().copied().filter(|value| value.is_heap()));
            }
            Cell::ArrayBuffer { object: value, .. } => object(value),
            Cell::RegExp {
                object: value,
                meta,
                ..
            } => {
                object(value);
                work.push(meta.legacy_constructor.constructor());
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
                ext,
                ..
            } => {
                let IteratorExt {
                    next_method,
                    helper,
                    generator,
                    ..
                } = &**ext;
                object(value);
                work.push(*source);
                work.extend(*next_method);
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
                scope,
                ..
            } => {
                work.push(*parent);
                work.extend(slots.roots());
                let crate::heap::EnvironmentScope {
                    dynamic_bindings,
                    with_objects,
                    ..
                } = &**scope;
                match dynamic_bindings {
                    EnvironmentBindings::Owned(bindings) => {
                        work.extend(bindings.iter().map(|(_, value)| *value));
                    }
                    EnvironmentBindings::Shared(owner) => work.push(*owner),
                }
                work.extend(with_objects.iter().copied());
            }
            Cell::WasmElements(elements) => work.extend(elements.iter().copied()),
            Cell::WasmTable { elements, .. } => work.extend(elements.iter().copied()),
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

    fn array_children(
        owner: Value,
        array: &ArrayRecord,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
    ) {
        Self::object_children(owner, &array.object, properties, work, object_roots);
        work.extend(
            array
                .elements
                .iter()
                .copied()
                .filter(|value| value.is_heap()),
        );
    }

    fn object_children(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
    ) {
        object_roots(owner, object.shape(), work);
        work.extend(object.private_names().iter().map(|brand| brand.home));
        if let Some(values) = object.inline_properties() {
            work.extend(
                values
                    .iter()
                    .copied()
                    .filter(|value| value.is_heap() && !value.is_deleted()),
            );
        } else {
            properties.append_heap_references(object.properties, work);
        }
        object.visit_stack_data_roots(|value| work.push(value));
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
                | Cell::ArrayFromAsyncState(_)
                | Cell::Proxy { .. }
                | Cell::Date { .. }
                | Cell::PromiseResolvingState { .. }
                | Cell::BindingReference { .. }
                | Cell::TemporalDuration { .. }
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
                Cell::WasmHostFunction { signature, .. } => {
                    size_of::<crate::WasmSignature>()
                        + (signature.params.capacity() + signature.results.capacity())
                            * size_of::<crate::WasmType>()
                }
                Cell::WasmElements(elements) => elements.capacity() * size_of::<Value>(),
                Cell::WasmTable { elements, .. }
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
                } => time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { meta, .. } => meta.source.capacity() + meta.flags.capacity(),
                Cell::Array { elements, .. } => elements.capacity() * size_of::<Value>(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::WasmMemory { bytes, .. } => bytes.capacity(),
                Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
                Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
                Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::Function { .. } => size_of::<Object>(),
                Cell::Environment { slots, scope, .. } => {
                    let with_objects = &scope.with_objects;
                    slots.len() * size_of::<EnvironmentSlot>()
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
