use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
mod access;
mod cell;
mod cell_access;
mod external;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod parallel_mark;
mod parallel_sweep;
mod root;
mod slots;
mod thin_slice;
mod weak;
mod workers;
pub(crate) use cell::*;
use parallel_mark::MarkBits;
#[cfg(test)]
pub(crate) use parallel_mark::NoObjectRoots;
pub(crate) use parallel_mark::ObjectRootTracer;
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::SlotArena;

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 256 KiB covers the measured 16,900-slot NavierStokes grids while
// still sending genuinely large `new Array(length)` allocations to the sparse
// representation. The slot limit is derived from Value's representation.
const DENSE_ARRAY_HOLE_BUDGET_BYTES: usize = 256 * 1024;
const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
const MINIMUM_GC_ALLOCATION_HEADROOM: usize = 384;
/// The share of the property arena its free buckets must hold before a collection compacts
/// it. Released ranges are reused by vectors of their size class, so a smaller share would
/// move every live vector to reclaim storage the next allocations reuse anyway.
const PROPERTY_ARENA_COMPACTION_SHARE: (usize, usize) = (1, 2);

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

// The allocation headroom after each collection, as a fraction of its survivors. A factor
// of 1/1 allows one live set's worth of new cells before collection (2x total occupied
// high-water); 3/4 targets 1.75x and 1/2 targets 1.5x. With prefetched parallel marking,
// 1/2 measured Splay 4066 vs 4129 for 1/1 (within run noise) at 55.3 vs 69.1 MB peak RSS
// (x86_64, 4 vCPU, 9 interleaved rounds); smaller heaps already used 1/2.
const GC_HEADROOM: GcHeadroomFactor = GcHeadroomFactor {
    numerator: 1,
    denominator: 2,
};

fn gc_allocation_headroom(live_cells: usize) -> usize {
    GC_HEADROOM
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
    free: slots::FreeSlots,
    external_bytes: usize,
    allocations: usize,
    threshold: usize,
    total_allocations: u64,
    collections: u64,
    peak_live: usize,
    peak_survivors: usize,
    /// Cells the latest collection kept, which sizes the next marking.
    last_survivors: usize,
    max_threshold: usize,
    properties: ValueArena,
    roots: RootTable,
    sparse_arrays: Option<Box<FxHashMap<u32, SparseElements>>>,
    /// Helper threads for parallel marking and sweeping, started by the first large collection.
    gc_workers: Option<workers::GcWorkers>,
    /// Slots of the live cells that hold weak references (`Cell::holds_weak_references`):
    /// collection resolves ephemerons and prunes weak entries through these alone.
    weak_holders: FxHashSet<u32>,
    /// Arena owners the current collection has marked, keyed by `pack_offset_owner`: marking
    /// already reads each live cell, so the sweep need not revisit survivors to find them.
    marked_property_owners: Vec<u64>,
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
            free: slots::FreeSlots::default(),
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

    /// Slots that currently hold a cell.
    pub(crate) fn live_slots(&self) -> usize {
        self.slots.len() - self.free.len()
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
        let weak = cell.holds_weak_references();
        if let Some(index) = self.free.pop().map(|index| index as u32) {
            if weak {
                self.weak_holders.insert(index);
            }
            debug_assert!((index as usize) < self.slots.len());
            // SAFETY: free bits cover only slots below the arena length (`FreeSlots`).
            let slot = unsafe { &mut self.slots.get_unchecked_mut(index as usize).cell };
            debug_assert!(slot.is_none(), "free-list slots hold no cell");
            // Collection clears heap weak owners before adding a dead slot here;
            // the VM clears its weak closure cache before control can allocate.
            // SAFETY: `slot` is a valid, exclusively borrowed place. Sweeping empties a slot
            // before freeing it, so overwriting it without dropping discards nothing.
            unsafe { std::ptr::write(slot, Some(cell)) };
            #[cfg(feature = "profile-memory")]
            self.profile_allocation(index as usize);
            return Value::heap(index);
        }
        let index = self.slots.len();
        assert!(index < MAX_HEAP_CELLS, "heap cell limit reached");
        if weak {
            self.weak_holders.insert(index as u32);
        }
        self.slots.push(Slot { cell: Some(cell) });
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(index);
        if index / 64 == self.marks.len() {
            self.marks.push(0);
        }
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
        // Array objects keep named properties in the arena (see `ArrayObject`).
        let (properties, inline_properties) = if matches!(self.get(owner), Some(Cell::Array { .. }))
        {
            (
                self.properties.with_values(shape, values),
                [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
            )
        } else {
            self.initial_object_storage(shape, values)
        };
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
    /// Slide every live arena range down over released ones, in offset order.
    /// `objects` holds each live arena owner packed with its range offset.
    /// Packs live arena vectors to the front once released ranges hold
    /// `PROPERTY_ARENA_COMPACTION_SHARE` of the arena; until then new vectors reuse them.
    fn compact_property_arena(&mut self, mut objects: Vec<u64>) {
        if !self
            .properties
            .released_share_reaches(PROPERTY_ARENA_COMPACTION_SHARE)
        {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        sort_by_unique_offset(&mut objects);
        let mut target = 0;
        for index in objects.into_iter().map(owner_of_packed) {
            let slot = self
                .slots
                .get_mut(index)
                .expect("property owner slot exists");
            let object = slot
                .cell
                .as_mut()
                .and_then(Cell::object_mut)
                .expect("property owner remains live during compaction");
            let mut properties = object.properties();
            target += self.properties.compact_vector(&mut properties, target);
            object.set_properties(properties);
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
        self.external_bytes = 0;
        self.weak_holders.clear();
        self.allocations = 0;
        self.threshold = MINIMUM_GC_ALLOCATION_HEADROOM;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.last_survivors = 0;
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
        self.collect_with_object_roots(roots, &NoObjectRoots).0
    }
    /// Collects garbage; returns the finalization jobs of dead registered targets and each
    /// marking thread's tracer state.
    pub(crate) fn collect_with_object_roots<T: ObjectRootTracer>(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        tracer: &T,
    ) -> (Vec<(Value, Value)>, Vec<T::Local>) {
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
        let mut tracer_locals = if self.parallel_marking() {
            let (locals, owners) = self.mark_parallel(work, tracer);
            self.marked_property_owners
                .extend(owners.into_iter().flatten());
            locals
        } else {
            let mut local = tracer.local();
            self.mark_work(
                &mut work,
                &mut |owner, shape, roots| tracer.visit(&mut local, owner, shape, roots),
                None,
            );
            vec![local]
        };
        if let Some(local) = self.mark_ephemerons(tracer) {
            tracer_locals.push(local);
        }
        let finalization_jobs = self.prune_weak_entries();
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.mark_nanos += mark_started.elapsed().as_nanos() as u64;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let mut live = 0;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += self.slots.len() as u64;
        }
        // Live arena owners, keyed for the offset-ordered compaction below.
        let property_owners = std::mem::take(&mut self.marked_property_owners);
        if self.parallel_marking() && !cfg!(feature = "profile-memory") {
            live = self.sweep_parallel();
        } else {
            for (base, slab) in self.slots.slabs_mut() {
                for (offset, slot) in slab.iter_mut().enumerate() {
                    let index = base + offset;
                    if Self::marked(&self.marks, index) {
                        live += 1;
                    } else if let Some(cell) = slot.cell.take() {
                        Self::free_cell_parts(
                            &mut self.properties,
                            &mut self.external_bytes,
                            &mut self.weak_holders,
                            &mut self.sparse_arrays,
                            &mut self.free,
                            index,
                            cell,
                            #[cfg(feature = "profile-memory")]
                            &mut self.memory_profile,
                        );
                        #[cfg(feature = "profile-aggregate")]
                        {
                            self.gc_profile.freed += 1;
                        }
                    }
                }
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
        self.last_survivors = live;
        self.max_threshold = self.max_threshold.max(self.threshold);
        (finalization_jobs, tracer_locals)
    }
    fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
        mut ephemerons: Option<&mut weak::EphemeronWork>,
    ) {
        let mut last_shared_elements: *const Vec<Value> = std::ptr::null();
        while let Some(value) = work.pop() {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.work_items += 1;
            }
            let Some(index) = value.heap_index().map(|value| value as usize) else {
                continue;
            };
            if index >= self.slots.len() {
                continue;
            }
            // SAFETY: bounds-checked above.
            let slot = unsafe { self.slots.get_unchecked_mut(index) };
            if Self::marked(&self.marks, index) || slot.cell.is_none() {
                continue;
            }
            Self::mark(&mut self.marks, index);
            let cell = slot.cell.as_ref().unwrap();
            self.marked_property_owners
                .extend(property_owner_key(&self.properties, index, cell));
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.marked += 1;
                self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
            }
            Self::children(
                value,
                cell,
                &self.properties,
                self.marks.as_slice(),
                &mut last_shared_elements,
                work,
                object_roots,
            );
            if let Some(ephemerons) = ephemerons.as_deref_mut() {
                ephemerons.newly_marked(index as u32, cell, &self.marks, work);
            }
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
    /// Frees the dead `cell` taken from slot `index`: its external and side-table accounting,
    /// its property storage, then the cell itself, and the slot joins the free list.
    fn free_cell(&mut self, index: usize, cell: Cell) {
        Self::free_cell_parts(
            &mut self.properties,
            &mut self.external_bytes,
            &mut self.weak_holders,
            &mut self.sparse_arrays,
            &mut self.free,
            index,
            cell,
            #[cfg(feature = "profile-memory")]
            &mut self.memory_profile,
        );
    }

    /// `free_cell` over the heap fields it touches, so the sweep can call it while it holds
    /// the slot arena.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn free_cell_parts(
        properties: &mut ValueArena,
        external_bytes: &mut usize,
        weak_holders: &mut FxHashSet<u32>,
        sparse_arrays: &mut Option<Box<FxHashMap<u32, SparseElements>>>,
        free: &mut slots::FreeSlots,
        index: usize,
        cell: Cell,
        #[cfg(feature = "profile-memory")] memory_profile: &mut memory_profile::MemoryProfile,
    ) {
        *external_bytes = external_bytes.saturating_sub(cell.external_bytes());
        if cell.holds_weak_references() {
            weak_holders.remove(&(index as u32));
        }
        #[cfg(feature = "profile-memory")]
        {
            let object_slots = cell
                .object()
                .map(|object| properties.len(object.properties()));
            memory_profile.freed(index, &cell, object_slots);
        }
        if let Some(object) = cell.object() {
            properties.release(object.properties());
        }
        if let Some(arrays) = sparse_arrays {
            arrays.remove(&(index as u32));
        }
        // Reused slots enter `free` only after their side-table entries are gone.
        free.insert(index);
        // The commonest dead cells, plain objects and arrays without extras, own nothing
        // beyond an array's element backing.
        match cell {
            Cell::Object(object) if object.owns_no_allocation() => std::mem::forget(object),
            // The array's own drop releases only its backing.
            Cell::Array { object } if object.owns_no_allocation() => drop(object),
            cell => drop(cell),
        }
    }

    /// Marks what live WeakMaps keep alive once the strong graph is marked: values whose keys
    /// are marked, repeatedly as newly marked cells mark further keys. Returns the tracer
    /// state of that marking, if any was needed.
    fn mark_ephemerons<T: ObjectRootTracer>(&mut self, tracer: &T) -> Option<T::Local> {
        let mut ephemerons = weak::EphemeronWork::default();
        let mut work = Vec::new();
        for &index in &self.weak_holders {
            let index = index as usize;
            if Self::marked(&self.marks, index)
                && let Some(cell) = self.slots.get(index).and_then(|slot| slot.cell.as_ref())
            {
                ephemerons.newly_marked(index as u32, cell, &self.marks, &mut work);
            }
        }
        if work.is_empty() {
            return None;
        }
        let mut local = tracer.local();
        self.mark_work(
            &mut work,
            &mut |owner, shape, roots| tracer.visit(&mut local, owner, shape, roots),
            Some(&mut ephemerons),
        );
        Some(local)
    }
    #[inline(always)]
    fn marked(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }
    #[inline(always)]
    fn mark(marks: &mut [u64], index: usize) {
        marks[index / 64] |= 1 << (index % 64);
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
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
        let Some(Cell::Array {
            object: array_object,
        }) = self.get(array)
        else {
            return;
        };
        let elements = array_object.elements();
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
        let Some(Cell::Array {
            object: array_object,
        }) = self.get_mut(array)
        else {
            return;
        };
        let elements = array_object.elements_mut();
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
            None => self.properties.get(object.properties(), slot),
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
            unsafe { self.properties.get_unchecked(object.properties(), slot) }
        }
    }
    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        let vector = {
            let data = self.get_mut(object).unwrap().object_mut().unwrap();
            if data.inline_properties().is_some() {
                data.set_inline_property(slot, value);
                return;
            }
            data.properties()
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
            let data = self.get_mut(object).unwrap().object_mut().unwrap();
            if data.inline_properties().is_some() {
                debug_assert!(slot < INLINE_PROPERTY_COUNT);
                data.set_inline_property(slot, value);
                return;
            }
            data.properties()
        };
        // SAFETY: the caller proves that the property slot exists.
        unsafe { self.properties.set_unchecked(vector, slot, value) };
    }
    /// One cached property add: appends `value` at `slot`, the object's current storage
    /// length, and moves the object to `shape`, whose storage length is `slot + 1`.
    #[inline(always)]
    /// Appends `value` as the next property of `object`, which moves to `shape`: inline while
    /// the values fit, otherwise in the arena, into which the add that outgrows the inline
    /// values moves them. One cell lookup; the arena borrow is disjoint from the slots.
    pub(crate) fn property_add(&mut self, object: Value, value: Value, shape: u32) {
        let index = object.heap_index().expect("property owner is a heap value") as usize;
        assert!(index < self.slots.len(), "property owner is a live cell");
        let Self {
            slots, properties, ..
        } = self;
        // SAFETY: bounds-checked above.
        let data = unsafe { slots.get_unchecked_mut(index) }
            .cell
            .as_mut()
            .and_then(Cell::object_mut)
            .expect("property owner is an object");
        let vector = data.properties();
        let slot = properties.len(vector);
        match data.inline_properties().copied() {
            Some(_) if slot < INLINE_PROPERTY_COUNT => data.set_inline_property(slot, value),
            Some(inline) => {
                debug_assert_eq!(slot, INLINE_PROPERTY_COUNT);
                let mut moved = properties.with_values(vector.auxiliary(), &inline);
                properties.push(&mut moved, value);
                data.replace_property_storage(moved, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
            }
            None => {
                let mut vector = vector;
                properties.push(&mut vector, value);
                data.set_properties(vector);
            }
        }
        data.set_shape(shape);
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
            .map(|object| self.properties.len(object.properties()));
        self.memory_profile.allocated(index, cell, object_slots);
    }

    #[cfg(feature = "profile-memory")]
    fn note_object_slots(&mut self, object: Value, slots: usize) {
        if let Some(index) = object.heap_index() {
            self.memory_profile
                .object_slots_changed(index as usize, slots);
        }
    }
    /// Pushes the references `cell` traces. Its prototype, inline properties and array
    /// elements, the bulk of a heap's edges, skip referents already marked, which would only
    /// be popped and discarded. A copy-on-write element backing shared by many arrays is
    /// traced by the first of them: `last_shared_elements` holds the latest such backing.
    fn children<M: MarkBits + ?Sized>(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        marks: &M,
        last_shared_elements: &mut *const Vec<Value>,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(Value, u32, &mut Vec<Value>),
    ) {
        // A reference beyond the mark bitmap names no slot and is dropped, as popping it would.
        let unmarked_heap = |value: &Value| {
            value
                .heap_index()
                .is_some_and(|index| marks.unmarked(index as usize))
        };
        let mut object = |object: &Object| {
            if unmarked_heap(&object.proto()) {
                work.push(object.proto());
            }
            object_roots(owner, object.shape(), work);
            if !object.owns_no_allocation() {
                work.extend(object.private_names().iter().map(|brand| brand.home));
                object.visit_stack_data_roots(|value| work.push(value));
            }
            if let Some(values) = object.inline_properties() {
                work.extend(values.iter().copied().filter(unmarked_heap));
            } else {
                properties.append_heap_references(object.properties(), work);
            }
        };
        if let Some((value, buffer)) = cell.typed_array_backing() {
            object(value);
            work.push(buffer);
            return;
        }
        match cell {
            Cell::Object(value) => object(value),
            Cell::Array { object: value } => {
                object(value);
                let elements = value.elements();
                // Shared backings are immutable until copied, so one trace per cycle suffices.
                let backing: *const Vec<Value> = &**elements;
                if std::rc::Rc::strong_count(elements) > 1 {
                    if *last_shared_elements == backing {
                        return;
                    }
                    *last_shared_elements = backing;
                }
                work.extend(elements.iter().copied().filter(unmarked_heap));
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
                object: value, env, ..
            } => {
                object(value);
                work.extend([*env, value.realm]);
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
            Cell::WasmGc { fields, .. } => {
                work.extend(fields.iter().copied());
                work.extend(fields.descriptor);
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
                Cell::WasmTable { elements, .. } => elements.capacity() * size_of::<Value>(),
                Cell::WasmGc { fields, .. } => fields.capacity() * size_of::<Value>(),
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
                Cell::TemporalZonedDateTime {
                    instant, calendar, ..
                } => instant.time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { meta, .. } => meta.source.capacity() + meta.flags.capacity(),
                Cell::Array { object } => object.elements().capacity() * size_of::<Value>(),
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

/// Bits per radix pass when ordering property owners by arena offset: the per-pass count
/// table (2^bits entries) stays a small stack array, so a collection with few owners pays
/// for its keys, not for the table.
const OFFSET_RADIX_BITS: u32 = 8;
/// An owner key keeps the arena offset in the high half and the slot index in the low half.
const OWNER_KEY_HALF_BITS: u32 = u32::BITS;

/// The compaction key of the cell in slot `index` when its properties occupy an arena range.
#[inline(always)]
fn property_owner_key(properties: &ValueArena, index: usize, cell: &Cell) -> Option<u64> {
    let vector = cell.object()?.properties();
    properties
        .has_compact_range(vector)
        .then(|| pack_offset_owner(vector.start_offset(), index))
}

fn pack_offset_owner(offset: usize, owner: usize) -> u64 {
    debug_assert!(offset <= u32::MAX as usize && owner <= u32::MAX as usize);
    ((offset as u64) << OWNER_KEY_HALF_BITS) | owner as u64
}

fn owner_of_packed(key: u64) -> usize {
    (key & u64::from(u32::MAX)) as usize
}

/// Orders packed owner keys by arena offset in linear time. Offsets are distinct arena
/// positions, so stable radix passes over the offset digits the largest offset uses give a
/// full ordering.
fn sort_by_unique_offset(keys: &mut Vec<u64>) {
    const BUCKETS: usize = 1 << OFFSET_RADIX_BITS;
    const MASK: u64 = (BUCKETS - 1) as u64;
    let largest = keys
        .iter()
        .map(|key| key >> OWNER_KEY_HALF_BITS)
        .max()
        .unwrap_or(0);
    let passes = (u64::BITS - largest.leading_zeros()).div_ceil(OFFSET_RADIX_BITS);
    let mut scratch = vec![0; keys.len()];
    for pass in 0..passes {
        let shift = OWNER_KEY_HALF_BITS + pass * OFFSET_RADIX_BITS;
        let mut counts = [0usize; BUCKETS + 1];
        for key in keys.iter() {
            counts[((key >> shift) & MASK) as usize + 1] += 1;
        }
        for bucket in 0..BUCKETS {
            counts[bucket + 1] += counts[bucket];
        }
        for key in keys.iter() {
            let bucket = ((key >> shift) & MASK) as usize;
            scratch[counts[bucket]] = *key;
            counts[bucket] += 1;
        }
        std::mem::swap(keys, &mut scratch);
    }
}

#[cfg(test)]
mod offset_order_tests {
    #[test]
    fn radix_offset_order_matches_comparison_sort() {
        let mut state = 0x2545_f491_u64;
        let mut offsets: Vec<usize> = (0..5000)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (state >> 33) as usize
            })
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        let mut keys: Vec<u64> = offsets
            .iter()
            .rev()
            .enumerate()
            .map(|(slot, offset)| super::pack_offset_owner(*offset, slot))
            .collect();
        let mut expected = keys.clone();
        expected.sort_unstable();
        super::sort_by_unique_offset(&mut keys);
        assert_eq!(keys, expected);
    }
}
