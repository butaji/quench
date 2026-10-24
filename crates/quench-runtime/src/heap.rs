use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use rayon::prelude::*;
use rayon::{Scope, ThreadPool, ThreadPoolBuilder};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
mod access;
mod buffer_bytes;
mod cell;
mod cell_access;
mod collection_entries;
mod external;
mod memory_stats;
pub use memory_stats::HeapMemoryComposition;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod records;
mod root;
mod slots;
mod thin_slice;
mod weak;
pub(crate) use buffer_bytes::{BufferBytes, BufferRead};
pub(crate) use cell::*;
pub(crate) use collection_entries::CollectionEntries;
use records::{ArrayRecordSpace, ObjectRecordSpace, RecordSweep};
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::{FreeSlotBitmap, SLOTS_PER_SLAB, SlotArena};

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 256 KiB covers the measured 16,900-slot NavierStokes grids while
// still sending genuinely large `new Array(length)` allocations to the sparse
// representation. The slot limit is derived from Value's representation.
const DENSE_ARRAY_HOLE_BUDGET_BYTES: usize = 256 * 1024;
const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
const LARGE_HEAP_MINIMUM_LIVE_CELLS: usize = 1 << 16;
/// Allocations a collection allows at least, whatever survived. The trunk owns this
/// headroom with the factors below (decision 21), and it stays at 384 cells until a change
/// wins one bar without losing the other (decision 5). The Score lane measured 2^16 cells
/// (2 MB of 32-byte cells): RayTrace +20% Score, the others even or better, 2^17 and 2^18 no
/// further gain (Linux x86_64, 4 vCPU, paired non-PGO rounds); at that minimum, fixed-work
/// max RSS rose 22% to 36% on DeltaBlue, EarleyBoyer and RayTrace.
const MINIMUM_GC_ALLOCATION_HEADROOM: usize = 384;
/// Payload bytes beside a cell (string units, array elements) that weigh as one more
/// allocation toward the next collection.
const PAYLOAD_BYTES_PER_ALLOCATION: usize = 64;
const PARALLEL_MARK_MINIMUM_SLOTS: usize = 50_000;
const PARALLEL_MARK_MAX_THREADS: usize = 4;
// Paired Splay screens on the 4-vCPU Linux host favored the collector thread;
// larger hosts start with four total markers, with an explicit env override.
const PARALLEL_MARK_SMALL_HOST_MAX_CPUS: usize = 4;
const PARALLEL_MARK_SMALL_HOST_DEFAULT_THREADS: usize = 1;
const PARALLEL_MARK_LARGE_HOST_DEFAULT_THREADS: usize = PARALLEL_MARK_MAX_THREADS;
const PARALLEL_MARK_SPLIT_THRESHOLD: usize = 256;
const PARALLEL_MARK_STACK_SIZE_BYTES: usize = 256 * 1024;
const PARALLEL_SWEEP_SLAB_BATCH_SIZE: usize = 8;
const RC_DROP_LOCK_STRIPES: usize = 64;

#[inline(always)]
fn mark_word_if_new(word: &AtomicU64, mask: u64) -> bool {
    if word.load(Ordering::Relaxed) & mask != 0 {
        return false;
    }
    word.fetch_or(mask, Ordering::Relaxed) & mask == 0
}

#[derive(Clone, Copy)]
struct GcHeadroomFactor {
    numerator: usize,
    denominator: usize,
}

impl GcHeadroomFactor {
    fn allocation_headroom(self, live_cells: usize) -> usize {
        debug_assert!(self.denominator > 0);
        let whole = live_cells / self.denominator;
        let remainder = live_cells % self.denominator;
        whole * self.numerator + remainder * self.numerator / self.denominator
    }
}

// Half a live set of new cells before the next collection on small heaps. The Score lane
// measured 1/1 with the 2^16 minimum at RayTrace +20% Score, for more max RSS (above).
const SMALL_HEAP_GC_HEADROOM: GcHeadroomFactor = GcHeadroomFactor {
    numerator: 1,
    denominator: 2,
};
// This is the sweep variable for large-heap RSS/Score measurements. A factor
// of 1/1 allows one live set's worth of new cells before collection (2x total
// occupied high-water); 3/4 targets 1.75x and 1/2 targets 1.5x. The trunk
// owns this factor (decision 21) and it stays 1/1 until a change wins one bar
// without losing the other (decision 5). Lane measurements on Linux x86_64:
// RSS lane 1/2, stock Splay max RSS 66.3 -> 54.5 MB for -14% Score (one run);
// Score lane, Splay paired non-PGO rounds: 1/1 3542, 2/1 3638, 4/1 4023, 8/1
// below 4/1, at a higher max RSS.
const LARGE_HEAP_GC_HEADROOM: GcHeadroomFactor = GcHeadroomFactor {
    numerator: 1,
    denominator: 1,
};

#[derive(Clone, Copy)]
struct FullCollectionGrowthFactor {
    numerator: usize,
    denominator: usize,
}

impl FullCollectionGrowthFactor {
    fn allocation_limit(self, live_cells: usize) -> usize {
        debug_assert!(self.denominator > 0);
        let whole = live_cells / self.denominator;
        let remainder = live_cells % self.denominator;
        whole
            .saturating_mul(self.numerator)
            .saturating_add(remainder.saturating_mul(self.numerator) / self.denominator)
    }
}

// Bound total allocations between full collections. Minor collections may
// run earlier to sweep the young cells.
const FULL_COLLECTION_GROWTH: FullCollectionGrowthFactor = FullCollectionGrowthFactor {
    numerator: 1,
    denominator: 1,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CollectionState {
    #[default]
    Idle,
    MinorMarking,
    FullMarking,
    Sweeping,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GcTraceKind {
    Collection,
    VerifyFull,
}

pub(crate) type ParallelObjectRootTrace<'a> =
    dyn Fn(GcTraceKind, Value, u32, bool, &mut Vec<Value>) + Sync + 'a;

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
}

struct ParallelSweepSlab<'slots> {
    base: usize,
    slots: *mut Slot,
    len: usize,
    _borrow: std::marker::PhantomData<&'slots mut [Slot]>,
}

// SAFETY: these wrappers are built from the disjoint mutable slices returned
// by SlotArena::slabs_mut. Each wrapper is consumed by exactly one Rayon task,
// and all tasks join before any slot is read or mutated on the collector
// thread. The collection has stopped the mutator, and the task only reads live
// cells and takes dead cells from its unique slice.
unsafe impl Send for ParallelSweepSlab<'_> {}

impl<'slots> ParallelSweepSlab<'slots> {
    fn new(base: usize, slots: &'slots mut [Slot]) -> Self {
        Self {
            base,
            slots: slots.as_mut_ptr(),
            len: slots.len(),
            _borrow: std::marker::PhantomData,
        }
    }

    fn sweep(&mut self, marks: &[AtomicU64]) -> ParallelSweepResult {
        let mut result = ParallelSweepResult {
            slab: (self.base / SLOTS_PER_SLAB) as u32,
            ..ParallelSweepResult::default()
        };
        // SAFETY: the wrapper exclusively owns this disjoint slab until the
        // joined Rayon task returns. No other code accesses its cells during
        // the stop-the-world sweep.
        let slots = unsafe { std::slice::from_raw_parts_mut(self.slots, self.len) };
        result.swept_slots = slots.len();
        for (offset, slot) in slots.iter_mut().enumerate() {
            let Some(cell) = slot.cell.as_ref() else {
                continue;
            };
            let index = self.base + offset;
            if Heap::marked(marks, index) {
                result.live += 1;
                if let Some(object) = cell.object() {
                    if object.properties().has_compact_range() {
                        result
                            .property_owners
                            .push(pack_offset_owner(object.properties().start_offset(), index));
                    }
                }
                continue;
            }

            let cell = slot.cell.take().expect("occupied dead sweep slot");
            result.external_bytes = result.external_bytes.saturating_add(cell.external_bytes());
            if let Some(object) = cell.object() {
                result.released_properties.push(object.properties());
            }
            if matches!(cell, Cell::Array { .. }) {
                result.sparse_arrays.push(index as u32);
            }
            result.free_slots.insert(offset);
            let dead = StoppedCellDrop(Some(cell));
            if cell_rc_drop_lock(dead.0.as_ref().expect("dead sweep cell")).is_some() {
                result.rc_cells.push(dead);
            } else {
                drop(dead);
            }
        }
        result
    }
}

#[derive(Default)]
struct ParallelSweepResult {
    slab: u32,
    live: usize,
    property_owners: Vec<u64>,
    free_slots: FreeSlotBitmap,
    released_properties: Vec<ValueVec>,
    sparse_arrays: Vec<u32>,
    rc_cells: Vec<StoppedCellDrop>,
    external_bytes: usize,
    swept_slots: usize,
}

#[derive(Default)]
struct SweepOutput {
    live: usize,
    property_owners: Vec<u64>,
    freed_cells: usize,
    swept_slots: usize,
}

static SHARED_RC_DROP_LOCK: Mutex<()> = Mutex::new(());
static RC_DROP_LOCKS: [Mutex<()>; RC_DROP_LOCK_STRIPES] =
    [const { Mutex::new(()) }; RC_DROP_LOCK_STRIPES];

/// A dead cell moved from a sweep worker to the collector thread. Rc-bearing
/// cells are released by that owner after the sweep batch joins; the
/// destructor's lock is a safe fallback if a transfer cannot be completed.
struct StoppedCellDrop(Option<Cell>);

// SAFETY: a sweep worker only moves the detached Cell into this wrapper and
// reads an Rc allocation pointer to classify it. It does not clone, inspect or
// drop any Rc payload. The collector thread drops transferred cells after the
// sweep batch joins; the fallback Drop path serializes worker-side destruction.
unsafe impl Send for StoppedCellDrop {}

impl StoppedCellDrop {
    fn drop_on_owner(mut self) {
        if let Some(cell) = self.0.take() {
            drop(cell);
        }
    }
}

impl Drop for StoppedCellDrop {
    fn drop(&mut self) {
        let Some(cell) = self.0.take() else {
            return;
        };
        let Some(lock) = cell_rc_drop_lock(&cell) else {
            drop(cell);
            return;
        };

        let guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cell_has_shared_rc_payload(&cell) {
            drop(cell);
            drop(guard);
        } else {
            drop(guard);
            drop(cell);
        }
    }
}

fn cell_rc_drop_lock(cell: &Cell) -> Option<&'static Mutex<()>> {
    match cell {
        Cell::Array { object } => {
            Some(&RC_DROP_LOCKS[rc_drop_lock_index(Rc::as_ptr(object.elements()) as usize)])
        }
        Cell::ArrayBuffer { bytes, .. } => Some(if bytes.holds_external_rc() {
            &SHARED_RC_DROP_LOCK
        } else {
            &RC_DROP_LOCKS[rc_drop_lock_index(bytes.as_ptr() as usize)]
        }),
        Cell::WasmHostFunction { signature, .. } => {
            Some(&RC_DROP_LOCKS[rc_drop_lock_index(Rc::as_ptr(signature) as usize)])
        }
        Cell::Iterator { .. }
        | Cell::String(_)
        | Cell::WasmGc { .. }
        | Cell::WasmGlobal { .. }
        | Cell::WasmTable { .. }
        | Cell::WasmTag { .. }
        | Cell::RegExp { .. } => Some(&SHARED_RC_DROP_LOCK),
        _ => None,
    }
}

fn cell_has_shared_rc_payload(cell: &Cell) -> bool {
    match cell {
        Cell::Array { object } => Rc::strong_count(object.elements()) > 1,
        Cell::ArrayBuffer { bytes, .. } => bytes.is_shared(),
        Cell::WasmHostFunction { signature, .. } => Rc::strong_count(signature) > 1,
        Cell::Iterator { .. }
        | Cell::String(_)
        | Cell::WasmGc { .. }
        | Cell::WasmGlobal { .. }
        | Cell::WasmTable { .. }
        | Cell::WasmTag { .. }
        | Cell::RegExp { .. } => true,
        _ => false,
    }
}

fn rc_drop_lock_index(address: usize) -> usize {
    (address >> 4) & (RC_DROP_LOCK_STRIPES - 1)
}
#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    object_records: ObjectRecordSpace,
    array_records: ArrayRecordSpace,
    marks: Vec<AtomicU64>,
    remembered_marks: Vec<u64>,
    remembered: Vec<u32>,
    young: Vec<u32>,
    free_by_slab: Vec<FreeSlotBitmap>,
    free_slabs: Vec<u32>,
    free_slots: usize,
    external_bytes: usize,
    allocations: usize,
    allocations_since_full: usize,
    /// Payload bytes grown since the last charge, below one allocation's weight.
    uncharged_payload_bytes: usize,
    full_collection_growth_limit: usize,
    old_live: usize,
    threshold: usize,
    total_allocations: u64,
    collections: u64,
    peak_live: usize,
    peak_survivors: usize,
    max_threshold: usize,
    collection_state: CollectionState,
    properties: ValueArena,
    roots: RootTable,
    sparse_arrays: Option<Box<FxHashMap<Value, SparseElements>>>,
    #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
    gc_profile: GcProfile,
    #[cfg(feature = "profile-memory")]
    memory_profile: memory_profile::MemoryProfile,
}

/// Immutable view used only while the mutator is stopped during parallel
/// marking. `Cell` contains `Rc` and other `!Sync` payloads, so workers must
/// only borrow them and walk immutable edges: they may not clone/drop an `Rc`,
/// mutate a cell, or retain any reference past the joined Rayon scope.
#[derive(Clone, Copy)]
struct ReadOnlyHeapView<'heap> {
    slots: *const SlotArena,
    properties: *const ValueArena,
    sparse_arrays: *const Option<Box<FxHashMap<Value, SparseElements>>>,
    marks: *const [AtomicU64],
    slot_len: usize,
    _heap: std::marker::PhantomData<&'heap Heap>,
}

// SAFETY: the view is created from an immutable borrow of Heap and remains
// scoped to the stop-the-world marking call. SlotArena, ValueArena, sparse
// arrays and Cells are not mutated until all Rayon work joins. The only
// shared writes are the atomic mark bits. Cell tracing borrows Rc payloads
// without changing their reference counts.
unsafe impl Send for ReadOnlyHeapView<'_> {}
// SAFETY: same stopped-mutator and read-only-payload invariant as above.
unsafe impl Sync for ReadOnlyHeapView<'_> {}

impl<'heap> ReadOnlyHeapView<'heap> {
    fn new(heap: &'heap Heap) -> Self {
        Self {
            slots: &heap.slots,
            properties: &heap.properties,
            sparse_arrays: &heap.sparse_arrays,
            marks: heap.marks.as_slice(),
            slot_len: heap.slots.len(),
            _heap: std::marker::PhantomData,
        }
    }

    fn cell(&self, index: usize) -> Option<&'heap Cell> {
        if index >= self.slot_len {
            return None;
        }
        // SAFETY: the view borrows this immutable SlotArena for 'heap; the
        // bound proves the flat slot index is initialized and no worker can
        // mutate or reclaim it until the scope has joined.
        unsafe {
            let slots: &'heap SlotArena = &*self.slots;
            slots.get_unchecked(index).cell.as_ref()
        }
    }

    fn properties(&self) -> &'heap ValueArena {
        // SAFETY: protected by the view's immutable heap borrow and scope.
        unsafe { &*self.properties }
    }

    fn sparse_arrays(&self) -> Option<&'heap FxHashMap<Value, SparseElements>> {
        // SAFETY: protected by the view's immutable heap borrow and scope.
        unsafe { (&*self.sparse_arrays).as_deref() }
    }

    fn mark_if_new(&self, index: usize) -> bool {
        // SAFETY: the mark slice has one atomic word for every allocated slot.
        let marks = unsafe { &*self.marks };
        let word = &marks[index / u64::BITS as usize];
        let mask = 1_u64 << (index % u64::BITS as usize);
        mark_word_if_new(word, mask)
    }
}

struct ParallelMarkBatch {
    weak_maps: Vec<u32>,
    object_roots: Vec<(Value, u32, bool)>,
    work_items: u64,
    max_worklist: usize,
    marked_count: usize,
    #[cfg(feature = "profile-aggregate")]
    marked_kinds: [u64; CellKind::COUNT],
}

impl Default for ParallelMarkBatch {
    // Arrays longer than 32 have no derived `Default`.
    fn default() -> Self {
        Self {
            weak_maps: Vec::new(),
            object_roots: Vec::new(),
            work_items: 0,
            max_worklist: 0,
            marked_count: 0,
            #[cfg(feature = "profile-aggregate")]
            marked_kinds: [0; CellKind::COUNT],
        }
    }
}

fn parallel_mark_pool() -> Option<&'static ThreadPool> {
    static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let available = std::thread::available_parallelism().map_or(1, |count| count.get());
        let default_threads = if available <= PARALLEL_MARK_SMALL_HOST_MAX_CPUS {
            PARALLEL_MARK_SMALL_HOST_DEFAULT_THREADS
        } else {
            PARALLEL_MARK_LARGE_HOST_DEFAULT_THREADS
        };
        let requested = std::env::var("QUENCH_GC_MARK_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default_threads)
            .clamp(1, PARALLEL_MARK_MAX_THREADS);
        let total_threads = available.min(requested);
        let worker_threads = total_threads.saturating_sub(1);
        (worker_threads > 0).then(|| {
            ThreadPoolBuilder::new()
                .num_threads(worker_threads)
                .stack_size(PARALLEL_MARK_STACK_SIZE_BYTES)
                .build()
                .expect("parallel GC mark pool construction failed")
        })
    })
    .as_ref()
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
    pub full_mark_nanos: u64,
    pub minor_mark_nanos: u64,
    pub sweep_nanos: u64,
    pub mark_clear_nanos: u64,
    #[cfg(feature = "profile-aggregate")]
    pub full_collections: u64,
    #[cfg(feature = "profile-aggregate")]
    pub minor_collections: u64,
    #[cfg(feature = "profile-aggregate")]
    pub remembered_owner_scans: u64,
    #[cfg(feature = "profile-aggregate")]
    pub young_cells_swept: u64,
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
            full_mark_nanos: 0,
            minor_mark_nanos: 0,
            sweep_nanos: 0,
            mark_clear_nanos: 0,
            #[cfg(feature = "profile-aggregate")]
            full_collections: 0,
            #[cfg(feature = "profile-aggregate")]
            minor_collections: 0,
            #[cfg(feature = "profile-aggregate")]
            remembered_owner_scans: 0,
            #[cfg(feature = "profile-aggregate")]
            young_cells_swept: 0,
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
impl CellKind {
    pub(crate) const COUNT: usize = Self::WasmException as usize + 1;
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
            remembered_marks: Vec::with_capacity(12),
            remembered: Vec::with_capacity(384),
            young: Vec::with_capacity(384),
            free_by_slab: Vec::with_capacity(12),
            free_slabs: Vec::with_capacity(12),
            free_slots: 0,
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

    /// Direct object or array records exist. They are traced and swept only on the serial
    /// full-collection path until they carry a generation and parallel sweep support.
    fn has_direct_records(&self) -> bool {
        self.object_records.live_count() > 0 || self.array_records.live_count() > 0
    }

    /// Live legacy Cells and direct object-family records.
    /// Visits every cell in the slot heap.
    pub(crate) fn for_each_cell(&mut self, mut visit: impl FnMut(&Cell)) {
        for (_, slab) in self.slots.slabs_mut() {
            for slot in slab.iter() {
                if let Some(cell) = &slot.cell {
                    visit(cell);
                }
            }
        }
    }

    pub(crate) fn live_slots(&self) -> usize {
        self.slots.len() - self.free_slots
            + self.object_records.live_count()
            + self.array_records.live_count()
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn slot_slab_census(
        &self,
        page_size: Option<usize>,
    ) -> (usize, usize, usize, Option<usize>) {
        let complete_slabs = self.slots.len() / SLOTS_PER_SLAB;
        let fully_free_slabs = self
            .free_by_slab
            .iter()
            .take(complete_slabs)
            .filter(|free| free.len() == SLOTS_PER_SLAB)
            .count();
        let partial_slabs = self
            .free_by_slab
            .iter()
            .take(complete_slabs)
            .filter(|free| !free.is_empty() && free.len() < SLOTS_PER_SLAB)
            .count();
        let fully_empty_pages =
            page_size.map(|page_size| self.slots.fully_empty_page_count(page_size));
        (
            complete_slabs,
            fully_free_slabs,
            partial_slabs,
            fully_empty_pages,
        )
    }

    pub(crate) fn mark_prototype(&mut self, prototype: Value) {
        if let Some(object) = self.object_mut(prototype) {
            object.mark_as_prototype();
        }
    }

    pub fn alloc(&mut self, cell: Cell) -> Value {
        if let Some(object) = cell.object() {
            self.mark_prototype(object.proto());
        }
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell) as usize;
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        let weight = 1 + match &cell {
            Cell::String(text) => text.flat_unit_bytes() / PAYLOAD_BYTES_PER_ALLOCATION,
            Cell::Array { object } => {
                object.elements().capacity() * size_of::<Value>() / PAYLOAD_BYTES_PER_ALLOCATION
            }
            _ => 0,
        };
        self.allocations += weight;
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
        let track_young = self.tracks_young_cells();
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
            Cell::Array { object } => match self.array_records.try_insert(object) {
                Ok(record_id) => {
                    let value = Value::direct_array(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(object) => Cell::Array { object },
            },
            cell => cell,
        };
        while let Some(slab) = self.free_slabs.last().copied() {
            let slab_index = slab as usize;
            let Some(offset) = self.free_by_slab[slab_index].pop() else {
                self.free_slabs.pop();
                continue;
            };
            self.free_slots -= 1;
            if self.free_by_slab[slab_index].is_empty() {
                self.free_slabs.pop();
            }
            let slot = slab_index * SLOTS_PER_SLAB + offset as usize;
            let index = slot as u32;
            self.slots.get_mut(slot).unwrap().cell = Some(cell);
            if track_young {
                self.young.push(index);
            }
            #[cfg(feature = "profile-memory")]
            self.profile_allocation(Value::heap(index));
            return Value::heap(index);
        }
        let index = self.slots.len();
        self.slots.push(Slot { cell: Some(cell) });
        let slab = index / SLOTS_PER_SLAB;
        if self.free_by_slab.len() == slab {
            self.free_by_slab.push(FreeSlotBitmap::default());
        }
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(Value::heap(index as u32));
        if index / 64 == self.marks.len() {
            self.marks.push(AtomicU64::new(0));
        }
        if track_young {
            self.young.push(index as u32);
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
        let (properties, inline_properties) = self.initial_object_storage(shape, values, true);
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
        // An array's payload holds its element backing, so its properties live in the arena.
        let inline = !self
            .object(owner)
            .expect("property owner is an object")
            .is_array();
        let (properties, inline_properties) = self.initial_object_storage(shape, values, inline);
        let previous = {
            let object = self.object_mut(owner).expect("property owner is an object");
            object.replace_property_storage(properties, inline_properties)
        };
        self.properties.release(previous);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(owner, values.len());
    }

    /// Storage for an object's first property `values`: inline where they fit and `inline`
    /// allows it, else the arena.
    fn initial_object_storage(
        &mut self,
        shape: u32,
        values: &[Value],
        inline: bool,
    ) -> (ValueVec, [Value; INLINE_PROPERTY_COUNT]) {
        self.properties.assert_shape_length(shape, values.len());
        if inline && values.len() <= INLINE_PROPERTY_COUNT {
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
            let old_shape = array.shape() as usize;
            array.set_shape(mapping[old_shape]);
        });
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
    fn compact_property_arena(
        &mut self,
        slot_owners: Vec<u64>,
        direct_owners: Vec<(usize, Value)>,
    ) {
        if !self.properties.has_released_ranges() {
            return;
        }
        #[cfg(feature = "profile-memory")]
        let before = self.properties.stats();
        let mut owners = Vec::with_capacity(slot_owners.len() + direct_owners.len());
        owners.extend(slot_owners.into_iter().map(|key| {
            let offset = (key >> u32::BITS) as usize;
            let owner = owner_of_packed(key) as u32;
            (offset, Value::heap(owner))
        }));
        owners.extend(direct_owners);
        owners.sort_unstable_by_key(|(offset, _)| *offset);
        let mut target = 0;
        let Self {
            object_records,
            array_records,
            slots,
            properties,
            ..
        } = self;
        for (_, owner) in owners {
            let object = if let Some(record_id) = owner.direct_object_index() {
                // SAFETY: direct owners were collected from live marked records.
                Some(unsafe { object_records.get_live_mut(record_id) })
            } else if let Some(record_id) = owner.direct_array_index() {
                // SAFETY: array owners were collected from live array records.
                Some(&mut **unsafe { array_records.get_live_mut(record_id) })
            } else {
                owner
                    .legacy_heap_index()
                    .and_then(|index| slots.get_mut(index as usize))
                    .and_then(|slot| slot.cell.as_mut())
                    .and_then(Cell::object_mut)
            }
            .expect("property owner remains live during compaction");
            let mut vector = object.properties();
            target += properties.compact_vector(&mut vector, target);
            object.set_properties(vector);
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
        self.free_by_slab.clear();
        self.free_slabs.clear();
        self.free_slots = 0;
        self.external_bytes = 0;
        self.allocations = 0;
        self.allocations_since_full = 0;
        self.full_collection_growth_limit = 0;
        self.old_live = 0;
        self.threshold = MINIMUM_GC_ALLOCATION_HEADROOM;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = MINIMUM_GC_ALLOCATION_HEADROOM;
        self.properties.reset();
        self.remembered_marks.clear();
        self.remembered.clear();
        self.young.clear();
        self.roots.clear();
        self.sparse_arrays = None;
        self.collection_state = CollectionState::Idle;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile = GcProfile::default();
        }
        #[cfg(feature = "profile-memory")]
        {
            self.memory_profile = memory_profile::MemoryProfile::default();
        }
    }
    /// Counts collections; a slot index names the same cell for as long as this is unchanged.
    #[inline(always)]
    pub(crate) fn collection_epoch(&self) -> u64 {
        self.collections
    }
    /// Charges payload a live cell grew outside `alloc` (array elements) toward the next
    /// collection, at the weight `alloc` gives a cell's own payload.
    pub(crate) fn charge_payload_growth(&mut self, bytes: usize) {
        let pending = self.uncharged_payload_bytes.saturating_add(bytes);
        let weight = pending / PAYLOAD_BYTES_PER_ALLOCATION;
        self.uncharged_payload_bytes = pending % PAYLOAD_BYTES_PER_ALLOCATION;
        self.allocations += weight;
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
    }
    pub fn should_collect(&self) -> bool {
        self.allocations >= self.threshold
    }
    pub(crate) fn should_full_collect(&self) -> bool {
        self.full_collection_growth_limit == 0
            || self.allocations_since_full >= self.full_collection_growth_limit
    }
    fn minor_collection_scheduled(&self) -> bool {
        let allocations_to_threshold = self.threshold.saturating_sub(self.allocations);
        self.allocations_since_full
            .saturating_add(allocations_to_threshold)
            < self.full_collection_growth_limit
    }
    fn tracks_young_cells(&self) -> bool {
        cfg!(test) || self.minor_collection_scheduled()
    }
    pub(crate) fn is_old(&self, value: Value) -> bool {
        value
            .legacy_heap_index()
            .map(|index| index as usize)
            .is_some_and(|index| self.get(value).is_some() && Self::marked(&self.marks, index))
    }
    /// Make a collection due at the next safepoint.
    #[cfg(test)]
    pub(crate) fn make_collection_due_for_test(&mut self) {
        self.threshold = self.allocations;
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
        self.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, true)
    }
    /// Whether a collection requested as `full` traces and sweeps the whole heap.
    /// Direct records do not yet carry a generation. A full trace is the
    /// exact fallback until their write barrier and remembered set exist.
    pub(crate) fn collection_is_full(&self, full: bool) -> bool {
        full || self.has_direct_records()
    }

    pub(crate) fn collect_with_object_roots(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut object_roots: impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        full: bool,
    ) -> Vec<(Value, Value)> {
        let full = self.collection_is_full(full);
        debug_assert_eq!(self.collection_state, CollectionState::Idle);
        self.collection_state = if full {
            CollectionState::FullMarking
        } else {
            CollectionState::MinorMarking
        };
        // Live slots grow only by allocation and shrink only by sweeping, so the peak is
        // observed here and when it is reported rather than on every allocation.
        self.peak_live = self.peak_live.max(self.live_slots());
        self.collections += 1;
        #[cfg(feature = "profile-aggregate")]
        let allocations_between_collections = self.allocations as u64;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let mark_started = std::time::Instant::now();
        #[cfg(feature = "profile-aggregate")]
        let mark_clear_started = std::time::Instant::now();
        #[cfg(feature = "profile-aggregate")]
        if full {
            self.gc_profile.full_collections += 1;
        } else {
            self.gc_profile.minor_collections += 1;
            self.gc_profile.remembered_owner_scans += self.remembered.len() as u64;
            self.gc_profile.young_cells_swept += self.young.len() as u64;
        }
        if full {
            Self::clear_marks(&self.marks);
            self.remembered.clear();
            self.remembered_marks.fill(0);
        }
        #[cfg(feature = "profile-aggregate")]
        if full {
            self.gc_profile.mark_clear_nanos += mark_clear_started.elapsed().as_nanos() as u64;
        }
        let mut work: Vec<Value> = roots.into_iter().chain(self.roots.values()).collect();
        #[cfg(debug_assertions)]
        let verify_roots = work.clone();
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        {
            let mut ephemerons = weak::EphemeronWork::default();
            if !full {
                let properties = &self.properties;
                let remembered = &self.remembered;
                let mut last_proto = Value::UNDEFINED;
                for index in remembered.iter().copied() {
                    let value = Value::heap(index);
                    if let Some(cell) = self.get(value) {
                        Self::children(
                            value,
                            cell,
                            properties,
                            &mut work,
                            &mut object_roots,
                            GcTraceKind::Collection,
                            &mut last_proto,
                        );
                        if let Some(arrays) = &self.sparse_arrays
                            && let Some(elements) = arrays.get(&Value::heap(index))
                        {
                            work.extend(elements.values.values().copied());
                        }
                        let object_records = &self.object_records;
                        let array_records = &self.array_records;
                        let marks = &self.marks;
                        ephemerons.remembered(
                            value,
                            cell,
                            |target| {
                                Self::is_marked_in(object_records, array_records, marks, target)
                            },
                            &mut work,
                        );
                    }
                }
            }
            self.mark_work_parallel_or_serial(
                &mut work,
                &mut object_roots,
                parallel_object_roots,
                &mut ephemerons,
                GcTraceKind::Collection,
            );
        }
        #[cfg(debug_assertions)]
        {
            if full {
                self.debug_verify_full(&verify_roots, &mut object_roots);
            } else {
                self.debug_verify_minor(&verify_roots, &mut object_roots);
            }
        }
        let finalization_jobs = self.prune_weak_entries(full);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            let mark_nanos = mark_started.elapsed().as_nanos() as u64;
            self.gc_profile.mark_nanos += mark_nanos;
            if full {
                self.gc_profile.full_mark_nanos += mark_nanos;
            } else {
                self.gc_profile.minor_mark_nanos += mark_nanos;
            }
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let sweep_pool = if full
            && !self.has_direct_records()
            && self.slots.len() >= PARALLEL_MARK_MINIMUM_SLOTS
            && !cfg!(feature = "profile-memory")
        {
            parallel_mark_pool()
        } else {
            None
        };
        let SweepOutput {
            mut live,
            property_owners,
            freed_cells,
            swept_slots,
        } = if let Some(pool) = sweep_pool {
            self.sweep_full_parallel(pool)
        } else {
            self.sweep_serial(full)
        };
        let mut direct_property_owners = Vec::new();
        if full {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.sweep_slots +=
                    (self.object_records.high_water() + self.array_records.high_water()) as u64;
            }
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            let properties = &mut self.properties;
            let (object_live, object_freed) = self.object_records.sweep(|record| match record {
                RecordSweep::Keep(record_id, object) => {
                    if object.properties().has_compact_range() {
                        direct_property_owners.push((
                            object.properties().start_offset(),
                            Value::direct_object(record_id),
                        ));
                    }
                }
                RecordSweep::Dead(_record_id, object) => {
                    #[cfg(feature = "profile-memory")]
                    let slots = properties.len(object.properties());
                    properties.release(object.properties());
                    #[cfg(feature = "profile-memory")]
                    memory_profile.freed_object(Value::direct_object(_record_id), slots);
                }
            });
            let mut dead_arrays = Vec::new();
            let (array_live, array_freed) = self.array_records.sweep(|record| match record {
                RecordSweep::Keep(record_id, array) => {
                    if array.properties().has_compact_range() {
                        direct_property_owners.push((
                            array.properties().start_offset(),
                            Value::direct_array(record_id),
                        ));
                    }
                }
                RecordSweep::Dead(record_id, array) => {
                    #[cfg(feature = "profile-memory")]
                    let slots = properties.len(array.properties());
                    properties.release(array.properties());
                    #[cfg(feature = "profile-memory")]
                    memory_profile.freed_array(Value::direct_array(record_id), slots, &array);
                    dead_arrays.push(Value::direct_array(record_id));
                }
            });
            if let Some(arrays) = &mut self.sparse_arrays {
                for array in dead_arrays {
                    arrays.remove(&array);
                }
                if arrays.is_empty() {
                    self.sparse_arrays = None;
                }
            }
            live += object_live + array_live;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.freed += (object_freed + array_freed) as u64;
            }
            #[cfg(not(feature = "profile-aggregate"))]
            let _ = (object_freed, array_freed);
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += swept_slots as u64;
            self.gc_profile.freed += freed_cells as u64;
        }
        let _ = (freed_cells, swept_slots);
        if full {
            self.compact_property_arena(property_owners, direct_property_owners);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        self.allocations = 0;
        if full {
            self.allocations_since_full = 0;
            self.old_live = live;
            self.full_collection_growth_limit = FULL_COLLECTION_GROWTH
                .allocation_limit(live)
                .max(MINIMUM_GC_ALLOCATION_HEADROOM);
            self.young.clear();
            self.remembered.clear();
            self.remembered_marks.fill(0);
        }
        let allocation_headroom = gc_allocation_headroom(live);
        self.threshold = if full {
            allocation_headroom
        } else {
            let remaining_full_growth = self
                .full_collection_growth_limit
                .saturating_sub(self.allocations_since_full)
                .max(1);
            allocation_headroom.min(remaining_full_growth)
        };
        #[cfg(feature = "profile-aggregate")]
        {
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
        self.peak_survivors = self.peak_survivors.max(live);
        self.max_threshold = self.max_threshold.max(self.threshold);
        self.collection_state = CollectionState::Idle;
        #[cfg(feature = "profile-memory")]
        self.memory_profile.collection_finished();
        finalization_jobs
    }
    fn sweep_full_parallel(&mut self, pool: &ThreadPool) -> SweepOutput {
        self.collection_state = CollectionState::Sweeping;
        let slot_len = self.slots.len();
        let mut slabs = self
            .slots
            .slabs_mut()
            .map(|(base, slots)| ParallelSweepSlab::new(base, slots))
            .collect::<Vec<_>>();
        let mut output = SweepOutput::default();

        for batch in slabs.chunks_mut(PARALLEL_SWEEP_SLAB_BATCH_SIZE) {
            let results = {
                let marks = &self.marks;
                pool.install(|| {
                    batch
                        .par_iter_mut()
                        .map(|slab| slab.sweep(marks))
                        .collect::<Vec<_>>()
                })
            };

            for result in results {
                let ParallelSweepResult {
                    slab,
                    live,
                    property_owners,
                    free_slots,
                    released_properties,
                    sparse_arrays,
                    rc_cells,
                    external_bytes,
                    swept_slots,
                } = result;
                let freed_cells = free_slots.len();
                if freed_cells > 0 {
                    let slab_free = &mut self.free_by_slab[slab as usize];
                    if slab_free.is_empty() {
                        self.free_slabs.push(slab);
                    }
                    slab_free.merge(&free_slots);
                    self.free_slots += freed_cells;
                    for properties in released_properties {
                        self.properties.release(properties);
                    }
                    if let Some(arrays) = &mut self.sparse_arrays {
                        for index in sparse_arrays {
                            arrays.remove(&Value::heap(index));
                        }
                    }
                    self.external_bytes = self.external_bytes.saturating_sub(external_bytes);
                }
                for cell in rc_cells {
                    cell.drop_on_owner();
                }
                output.live += live;
                output.property_owners.extend(property_owners);
                output.freed_cells += freed_cells;
                output.swept_slots += swept_slots;
            }
        }
        debug_assert_eq!(output.swept_slots, slot_len);
        output
    }

    fn sweep_serial(&mut self, full: bool) -> SweepOutput {
        let mut live = 0;
        let mut freed_cells = 0;
        let mut swept_slots = 0;
        let properties = &mut self.properties;
        let mut property_owners_for_sweep = Vec::new();
        {
            let external_bytes = &mut self.external_bytes;
            let free_by_slab = &mut self.free_by_slab;
            let free_slabs = &mut self.free_slabs;
            let free_slot_count = &mut self.free_slots;
            let sparse_arrays = &mut self.sparse_arrays;
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            let mut reclaim = |properties: &mut ValueArena, index: usize, cell: Cell| {
                *external_bytes = external_bytes.saturating_sub(cell.external_bytes());
                #[cfg(feature = "profile-memory")]
                {
                    let object_slots = cell
                        .object()
                        .map(|object| properties.len(object.properties()));
                    memory_profile.freed(Value::heap(index as u32), &cell, object_slots);
                }
                if let Some(object) = cell.object() {
                    properties.release(object.properties());
                }
                if let Some(arrays) = sparse_arrays {
                    arrays.remove(&Value::heap(index as u32));
                }
                let slab = index / SLOTS_PER_SLAB;
                let slab_free = &mut free_by_slab[slab];
                if slab_free.is_empty() {
                    free_slabs.push(slab as u32);
                }
                slab_free.insert(index % SLOTS_PER_SLAB);
                *free_slot_count += 1;
                freed_cells += 1;
                drop(cell);
            };
            self.collection_state = CollectionState::Sweeping;
            if full {
                for (base, slab) in self.slots.slabs_mut() {
                    for (offset, slot) in slab.iter_mut().enumerate() {
                        let index = base + offset;
                        swept_slots += 1;
                        if Self::marked(&self.marks, index) {
                            live += 1;
                            if let Some(object) = slot.cell.as_ref().and_then(Cell::object) {
                                if properties.has_compact_range(object.properties()) {
                                    property_owners_for_sweep.push(pack_offset_owner(
                                        object.properties().start_offset(),
                                        index,
                                    ));
                                }
                            }
                        } else if let Some(cell) = slot.cell.take() {
                            reclaim(properties, index, cell);
                        }
                    }
                }
            } else {
                let mut promoted = 0;
                for index in self.young.iter().copied() {
                    swept_slots += 1;
                    let index_usize = index as usize;
                    let Some(slot) = self.slots.get_mut(index_usize) else {
                        continue;
                    };
                    if Self::marked(&self.marks, index_usize) {
                        promoted += 1;
                    } else if let Some(cell) = slot.cell.take() {
                        reclaim(properties, index_usize, cell);
                        self.marks[index_usize / 64]
                            .fetch_and(!(1 << (index_usize % 64)), Ordering::Relaxed);
                    }
                }
                self.young.clear();
                for index in self.remembered.iter().copied() {
                    let index = index as usize;
                    self.remembered_marks[index / 64] &= !(1 << (index % 64));
                }
                self.remembered.clear();
                self.old_live = self.old_live.saturating_add(promoted);
                live = self.old_live;
            }
        }
        SweepOutput {
            live,
            property_owners: property_owners_for_sweep,
            freed_cells,
            swept_slots,
        }
    }

    fn mark_work_parallel_or_serial(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
    ) {
        if self.has_direct_records() {
            self.mark_work(work, object_roots, ephemerons, trace_kind);
            return;
        }
        if self.slots.len() < PARALLEL_MARK_MINIMUM_SLOTS {
            self.mark_work(work, object_roots, ephemerons, trace_kind);
            return;
        }
        let Some(pool) = parallel_mark_pool() else {
            self.mark_work(work, object_roots, ephemerons, trace_kind);
            return;
        };

        while !work.is_empty() {
            let initial_work = std::mem::take(work);
            let batches = {
                let view = ReadOnlyHeapView::new(self);
                let batches = Mutex::new(Vec::new());
                pool.scope(|scope| {
                    Self::visit_parallel_work(
                        scope,
                        initial_work,
                        view,
                        &batches,
                        parallel_object_roots,
                        trace_kind,
                    );
                });
                batches
                    .into_inner()
                    .expect("parallel GC mark output mutex poisoned")
            };
            for batch in batches {
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.work_items += batch.work_items;
                    self.gc_profile.max_worklist =
                        self.gc_profile.max_worklist.max(batch.max_worklist as u64);
                    self.gc_profile.marked += batch.marked_count as u64;
                    for (kind, count) in batch.marked_kinds.iter().copied().enumerate() {
                        self.gc_profile.marked_kinds[kind] += count;
                    }
                }
                for (owner, shape, prototype) in batch.object_roots {
                    object_roots(trace_kind, owner, shape, prototype, work);
                }
                for index in batch.weak_maps {
                    let owner = Value::heap(index);
                    if let Some(cell) = self.get(owner) {
                        let object_records = &self.object_records;
                        let array_records = &self.array_records;
                        let marks = &self.marks;
                        ephemerons.newly_marked(
                            owner,
                            Some(cell),
                            |target| {
                                Self::is_marked_in(object_records, array_records, marks, target)
                            },
                            work,
                        );
                    }
                }
            }
        }
    }

    fn visit_parallel_work<'scope>(
        scope: &Scope<'scope>,
        mut work: Vec<Value>,
        view: ReadOnlyHeapView<'scope>,
        batches: &'scope Mutex<Vec<ParallelMarkBatch>>,
        parallel_object_roots: Option<&'scope ParallelObjectRootTrace<'scope>>,
        trace_kind: GcTraceKind,
    ) {
        let mut batch = ParallelMarkBatch::default();
        batch.max_worklist = work.len();
        let mut last_proto = Value::UNDEFINED;
        while let Some(value) = work.pop() {
            batch.work_items += 1;
            let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
                continue;
            };
            let Some(cell) = view.cell(index) else {
                continue;
            };
            if !view.mark_if_new(index) {
                continue;
            }

            let mut trace_object_root =
                |kind: GcTraceKind,
                 owner: Value,
                 shape: u32,
                 prototype: bool,
                 roots: &mut Vec<Value>| {
                    if let Some(parallel_object_roots) = parallel_object_roots {
                        parallel_object_roots(kind, owner, shape, prototype, roots);
                    } else {
                        batch.object_roots.push((owner, shape, prototype));
                    }
                };
            Self::children(
                value,
                cell,
                view.properties(),
                &mut work,
                &mut trace_object_root,
                trace_kind,
                &mut last_proto,
            );
            if matches!(cell, Cell::Array { .. })
                && let Some(elements) = view
                    .sparse_arrays()
                    .and_then(|arrays| arrays.get(&Value::heap(index as u32)))
            {
                work.extend(elements.values.values().copied());
            }
            batch.marked_count += 1;
            if matches!(cell, Cell::WeakMap { .. }) {
                batch.weak_maps.push(index as u32);
            }
            #[cfg(feature = "profile-aggregate")]
            {
                batch.marked_kinds[Self::cell_kind(cell) as usize] += 1;
            }
            batch.max_worklist = batch.max_worklist.max(work.len());

            if work.len() > PARALLEL_MARK_SPLIT_THRESHOLD {
                let spawned = work.split_off(work.len() / 2);
                scope.spawn(move |scope| {
                    Self::visit_parallel_work(
                        scope,
                        spawned,
                        view,
                        batches,
                        parallel_object_roots,
                        trace_kind,
                    );
                });
            }
        }
        batches
            .lock()
            .expect("parallel GC mark output mutex poisoned")
            .push(batch);
    }

    #[cfg(debug_assertions)]
    fn debug_verify_full(
        &mut self,
        roots: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    ) {
        let parallel_marks = Self::snapshot_marks(&self.marks);
        let parallel_object_marks = self.object_records.snapshot_marks();
        let parallel_array_marks = self.array_records.snapshot_marks();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        self.object_records.clear_marks();
        self.array_records.clear_marks();
        let mut work = roots.to_vec();
        let mut ephemerons = weak::EphemeronWork::default();
        self.mark_work(
            &mut work,
            object_roots,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
        );
        let sequential_marks = Self::snapshot_marks(&self.marks);
        let sequential_object_marks = self.object_records.snapshot_marks();
        let sequential_array_marks = self.array_records.snapshot_marks();
        assert_eq!(
            (
                sequential_marks.as_slice(),
                sequential_object_marks.as_slice(),
                sequential_array_marks.as_slice(),
            ),
            (
                parallel_marks.as_slice(),
                parallel_object_marks.as_slice(),
                parallel_array_marks.as_slice(),
            ),
            "full mark differs from the single-threaded verifier"
        );
        Self::restore_marks(&self.marks, &parallel_marks);
        self.object_records.restore_marks(&parallel_object_marks);
        self.array_records.restore_marks(&parallel_array_marks);
        self.collection_state = CollectionState::FullMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }

    #[cfg(debug_assertions)]
    fn debug_verify_minor(
        &mut self,
        roots: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    ) {
        let minor_marks = Self::snapshot_marks(&self.marks);
        let young = self.young.clone();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        let mut young_slots = vec![false; self.slots.len()];
        for index in young.iter().copied() {
            young_slots[index as usize] = true;
        }
        let mut work = roots.to_vec();
        work.extend(self.slots.iter().enumerate().filter_map(|(index, slot)| {
            (slot.cell.is_some() && !young_slots[index] && Self::marked_words(&minor_marks, index))
                .then_some(Value::heap(index as u32))
        }));
        let mut ephemerons = weak::EphemeronWork::default();
        self.collection_state = CollectionState::FullMarking;
        self.mark_work(
            &mut work,
            object_roots,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
        );
        for index in young {
            let index = index as usize;
            if self.get(Value::heap(index as u32)).is_some() {
                assert_eq!(
                    Self::marked(&self.marks, index),
                    Self::marked_words(&minor_marks, index),
                    "minor collection's young live set differs from a full mark with old cells pinned: {index}"
                );
            }
        }
        Self::restore_marks(&self.marks, &minor_marks);
        self.collection_state = CollectionState::MinorMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }
    fn mark_work(
        &mut self,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
    ) {
        let mut last_proto = Value::UNDEFINED;
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
                // SAFETY: the direct handle is live and its mark bit was newly set.
                let object = unsafe { self.object_records.get_live(record_id) };
                Self::object_children(
                    value,
                    object,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                let (object_records, array_records, marks) =
                    (&self.object_records, &self.array_records, &self.marks);
                ephemerons.newly_marked(
                    value,
                    None,
                    |target| Self::is_marked_in(object_records, array_records, marks, target),
                    work,
                );
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
                Self::array_children(
                    value,
                    array,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                if let Some(arrays) = self.sparse_arrays.as_ref()
                    && let Some(elements) = arrays.get(&value)
                {
                    work.extend(elements.values.values().copied());
                }
                let (object_records, array_records, marks) =
                    (&self.object_records, &self.array_records, &self.marks);
                ephemerons.newly_marked(
                    value,
                    None,
                    |target| Self::is_marked_in(object_records, array_records, marks, target),
                    work,
                );
            } else {
                let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
                    continue;
                };
                let Some(slot) = self.slots.get_mut(index) else {
                    continue;
                };
                if slot.cell.is_none() || !Self::mark_serial(&self.marks, index) {
                    continue;
                }
                let cell = slot.cell.as_ref().unwrap();
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[Self::cell_kind(cell) as usize] += 1;
                }
                Self::children(
                    value,
                    cell,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                let object_records = &self.object_records;
                let array_records = &self.array_records;
                let marks = &self.marks;
                ephemerons.newly_marked(
                    value,
                    Some(cell),
                    |target| Self::is_marked_in(object_records, array_records, marks, target),
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

    fn is_marked_in(
        object_records: &ObjectRecordSpace,
        array_records: &ArrayRecordSpace,
        marks: &[AtomicU64],
        value: Value,
    ) -> bool {
        if let Some(record_id) = value.direct_object_index() {
            return object_records.is_marked(record_id);
        }
        if let Some(record_id) = value.direct_array_index() {
            return array_records.is_marked(record_id);
        }
        let Some(index) = value.legacy_heap_index().map(|index| index as usize) else {
            return false;
        };
        marks.get(index / u64::BITS as usize).is_some_and(|word| {
            word.load(Ordering::Relaxed) & (1 << (index % u64::BITS as usize)) != 0
        })
    }

    #[inline(always)]
    fn marked(marks: &[AtomicU64], index: usize) -> bool {
        marks[index / 64].load(Ordering::Relaxed) & (1 << (index % 64)) != 0
    }
    #[inline(always)]
    fn mark_serial(marks: &[AtomicU64], index: usize) -> bool {
        let mask = 1_u64 << (index % u64::BITS as usize);
        mark_word_if_new(&marks[index / u64::BITS as usize], mask)
    }
    #[cfg(debug_assertions)]
    fn marked_words(marks: &[u64], index: usize) -> bool {
        marks[index / 64] & (1 << (index % 64)) != 0
    }
    #[cfg(debug_assertions)]
    fn snapshot_marks(marks: &[AtomicU64]) -> Vec<u64> {
        marks
            .iter()
            .map(|word| word.load(Ordering::Relaxed))
            .collect()
    }
    fn clear_marks(marks: &[AtomicU64]) {
        for word in marks {
            word.store(0, Ordering::Relaxed);
        }
    }
    #[cfg(debug_assertions)]
    fn restore_marks(marks: &[AtomicU64], words: &[u64]) {
        debug_assert_eq!(marks.len(), words.len());
        for (mark, value) in marks.iter().zip(words) {
            mark.store(*value, Ordering::Relaxed);
        }
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
        if let Some(owner) = array.legacy_heap_index() {
            self.remember(owner as usize);
        }
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
        let Some(elements) = self.array_elements(array) else {
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
        let Some(elements) = self.array_elements_mut(array) else {
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
            None => self.properties.get(object.properties(), slot),
        }?;
        (!value.is_deleted()).then_some(value)
    }
    /// `property_get_unchecked` that leaves a dictionary's storage to the caller's slower path
    /// (`ValueArena::get_shaped`), so a cache hit makes no call.
    ///
    /// # Safety
    /// As for `property_get_unchecked`.
    #[inline(always)]
    pub(crate) unsafe fn property_get_shaped(&self, object: &Object, slot: usize) -> Option<Value> {
        if let Some(values) = object.inline_properties() {
            debug_assert!(slot < INLINE_PROPERTY_COUNT);
            // SAFETY: the caller proves that the property slot exists.
            Some(unsafe { *values.get_unchecked(slot) })
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_shaped(object.properties(), slot) }
        }
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
            let data = self
                .object_mut(object)
                .expect("property owner is an object");
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
            let data = self
                .object_mut(object)
                .expect("property owner is an object");
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
    /// `property_set_unchecked` for the direct object `record_id` names, decoded once, where
    /// its values are inline or a flat arena run. `false` leaves a dictionary's store, which
    /// makes a call, to the caller.
    ///
    /// # Safety
    /// `record_id` names a live object record holding a property at `slot`.
    #[inline(always)]
    pub(crate) unsafe fn record_property_set_flat(
        &mut self,
        record_id: u32,
        slot: usize,
        value: Value,
    ) -> bool {
        // SAFETY: the caller proves the record live; `&mut self` is exclusive.
        let data = unsafe { self.object_records.get_live_mut(record_id) };
        if data.inline_properties().is_some() {
            debug_assert!(slot < INLINE_PROPERTY_COUNT);
            data.set_inline_property(slot, value);
            return true;
        }
        let vector = data.properties();
        if vector.is_dictionary() {
            return false;
        }
        // SAFETY: the caller proves that the property slot exists; the vector is flat.
        unsafe { self.properties.set_flat_unchecked(vector, slot, value) };
        true
    }
    /// One cached property add: appends `value` at `slot`, the object's current storage
    /// length, and moves the object to `shape`, whose storage length is `slot + 1`.
    #[inline(always)]
    /// Appends `value` as the next property of `object`, which moves to `shape`: inline while
    /// the values fit, otherwise in the arena, into which the add that outgrows the inline
    /// values moves them. One owner lookup; the arena borrow is disjoint from the owner.
    pub(crate) fn property_add(&mut self, object: Value, value: Value, shape: u32) {
        let (data, properties) = self
            .object_and_arena_mut(object)
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
            // SAFETY: called immediately after insertion into the direct space.
            let object = unsafe { object_records.get_live(record_id) };
            memory_profile.allocated_object(identity, object, properties.len(object.properties()));
            return;
        }
        if let Some(record_id) = identity.direct_array_index() {
            // SAFETY: profile runs immediately after inserting this array.
            let array = unsafe { array_records.get_live(record_id) };
            memory_profile.allocated_array(identity, array, properties.len(array.properties()));
            return;
        }
        let index = identity
            .legacy_heap_index()
            .expect("legacy allocation has a slot index") as usize;
        let cell = slots
            .get_mut(index)
            .and_then(|slot| slot.cell.as_ref())
            .expect("allocated slot contains its Cell");
        let object_slots = cell
            .object()
            .map(|object| properties.len(object.properties()));
        memory_profile.allocated(identity, cell, object_slots);
    }

    #[cfg(feature = "profile-memory")]
    fn note_object_slots(&mut self, object: Value, slots: usize) {
        self.memory_profile.object_slots_changed(object, slots);
    }
    /// Push the values `object` keeps alive. Objects that share a prototype push it once:
    /// `last_proto`, the prototype pushed last in this walk, is already on the work list or
    /// marked, so a second push would only be popped again.
    #[inline(always)]
    fn object_children(
        owner: Value,
        object: &Object,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut Value,
    ) {
        let proto = object.proto();
        if proto != *last_proto {
            *last_proto = proto;
            if proto.is_heap() {
                work.push(proto);
            }
        }
        object_roots(
            trace_kind,
            owner,
            object.shape(),
            object.was_used_as_prototype(),
            work,
        );
        object.visit_extras_roots(|value| work.push(value));
        if let Some(values) = object.inline_properties() {
            for &value in values {
                if value.is_heap() && !value.is_deleted() {
                    work.push(value);
                }
            }
        } else {
            properties.append_heap_references(object.properties(), work);
        }
    }

    fn children(
        owner: Value,
        cell: &Cell,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut Value,
    ) {
        let mut object = |object: &Object| {
            Self::object_children(
                owner,
                object,
                properties,
                work,
                object_roots,
                trace_kind,
                last_proto,
            );
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
                work.extend(
                    value
                        .elements()
                        .iter()
                        .copied()
                        .filter(|value| value.is_heap()),
                );
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
                work.extend(entries.iter().flat_map(|(key, value)| [key, value]));
            }
            Cell::Set {
                object: value,
                entries,
            } => {
                object(value);
                work.extend(entries.iter());
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
            Cell::WasmGlobal {
                header,
                value: global,
                ..
            } => {
                object(&header.object);
                work.push(*global);
            }
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
            Cell::WasmTable {
                object: value,
                elements,
                ..
            } => {
                object(value);
                work.extend(elements.iter().copied());
            }
            Cell::WasmGc { fields, .. } => {
                work.extend(fields.values.iter().copied());
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
            | Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Host(_),
                ..
            }
            | Cell::WasmBits64(_)
            | Cell::WasmV128(_) => {}
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::Js(target),
                ..
            } => work.push(*target),
            Cell::WasmObject {
                object: value,
                slot,
            } => {
                object(value);
                if let crate::heap::WasmObjectSlot::Instance { exports } = slot {
                    work.push(*exports);
                }
            }
            Cell::WasmMemory { header, buffer, .. } => {
                object(&header.object);
                work.push(*buffer);
            }
            _ => unreachable!("typed array backing handled above"),
        }
    }

    /// A direct Array record traces exactly as a legacy `Cell::Array`: its object part, then
    /// its heap-valued elements.
    fn array_children(
        owner: Value,
        array: &ArrayObject,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut Value,
    ) {
        Self::object_children(
            owner,
            array,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        work.extend(
            array
                .elements()
                .iter()
                .copied()
                .filter(|value| value.is_heap()),
        );
    }
    pub(super) fn cell_kind(cell: &Cell) -> CellKind {
        match cell {
            Cell::Object(_)
            | Cell::WasmObject { .. }
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
    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        let object_extra_bytes = cell
            .object()
            .map_or(0, |object| object.allocated_extra_bytes());
        object_extra_bytes
            + match cell {
                Cell::Object(_)
                | Cell::WasmObject { .. }
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
                Cell::Symbol(value) => value.as_ref().map_or(0, |description| description.len()),
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

/// Bits per radix pass when ordering property owners by arena offset. Each
/// pass counts into a table of `2^bits` buckets, which every compacting
/// collection allocates; a byte keeps it small.
#[cfg(test)]
const OFFSET_RADIX_BITS: u32 = u8::BITS;
/// An owner key keeps the arena offset in the high half and the slot index in the low half.
const OWNER_KEY_HALF_BITS: u32 = u32::BITS;
/// Radix passes that order the whole offset half.
#[cfg(test)]
const OFFSET_RADIX_PASSES: u32 = OWNER_KEY_HALF_BITS / OFFSET_RADIX_BITS;

fn pack_offset_owner(offset: usize, owner: usize) -> u64 {
    debug_assert!(offset <= u32::MAX as usize && owner <= u32::MAX as usize);
    ((offset as u64) << OWNER_KEY_HALF_BITS) | owner as u64
}

fn owner_of_packed(key: u64) -> usize {
    (key & u64::from(u32::MAX)) as usize
}

/// Orders packed owner keys by arena offset in linear time. Offsets are distinct arena
/// positions, so stable passes over the offset half give a full ordering.
#[cfg(test)]
fn sort_by_unique_offset(keys: &mut Vec<u64>) {
    let buckets = 1usize << OFFSET_RADIX_BITS;
    let mask = (buckets - 1) as u64;
    let mut scratch = vec![0; keys.len()];
    let mut counts = vec![0usize; buckets + 1];
    for pass in 0..OFFSET_RADIX_PASSES {
        let shift = OWNER_KEY_HALF_BITS + pass * OFFSET_RADIX_BITS;
        counts.fill(0);
        for key in keys.iter() {
            counts[((key >> shift) & mask) as usize + 1] += 1;
        }
        for bucket in 0..buckets {
            counts[bucket + 1] += counts[bucket];
        }
        for key in keys.iter() {
            let bucket = ((key >> shift) & mask) as usize;
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
