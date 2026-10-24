use crate::value::{HeapReference, Value};
use crate::vm::wtf16::JsString;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueArena, ValueVec};
use rayon::prelude::*;
use rayon::{Scope, ThreadPool, ThreadPoolBuilder};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
mod access;
mod array_elements;
pub(crate) use array_elements::{ArrayElements, UniqueElements};
mod cell;
mod cell_access;
mod buffer_bytes;
mod collection_entries;
mod environment_trace;
pub(crate) use environment_trace::{ClosureReadSource, EnvironmentTrace};
mod external;
mod memory_stats;
pub use memory_stats::HeapMemoryComposition;
#[cfg(feature = "profile-memory")]
mod memory_profile;
mod pacing;
mod promise_record;
pub(crate) use promise_record::{AbandonedHostContexts, PromiseCell, PromiseReaction, PromiseRecord, PromiseRejectionState, PromiseState};
mod records;
mod root;
mod slots;
mod snapshot;
mod thin_slice;
pub(crate) use thin_slice::ThinBoxSlice;
mod weak;
pub(crate) use cell::*;
pub(crate) use buffer_bytes::{BufferBytes,BufferRead};
pub(crate) use collection_entries::{CollectionEntries, CollectionEntry};
use external::array_payload_bytes;
use records::{
    ArrayRecordSpace, AwaitRecordSpace, FunctionRecordSpace, IteratorRecordSpace, V128RecordSpace,
    ObjectRecordSpace, PromiseRecordSpace, RecordFamily, StringRecordSpace,
    RecordId, RecordSweep,
};
pub use root::RootId;
pub(crate) use root::{RootTable, WeakHandle};
use slots::{FreeSlotBitmap, SLOTS_PER_SLAB, SlotArena};

// Keep implicit holes in dense storage when their one-time allocation is
// bounded. 1 MiB covers the measured 16,900-slot NavierStokes grids and the
// 131,072-bucket tables hash-map's rehash allocates with `new Array(capacity)`,
// which at its load factor never fill enough to turn dense, while still sending
// genuinely large `new Array(length)` allocations to the sparse representation.
// The slot limit is derived from Value's representation.
const DENSE_ARRAY_HOLE_BUDGET_BYTES: usize = 1024 * 1024;
const MAX_DENSE_ARRAY_HOLE_LENGTH: usize = DENSE_ARRAY_HOLE_BUDGET_BYTES / size_of::<Value>();
// A sparse element is an index and a value in a hash table, at least this many dense slots;
// a sparse array whose elements fill as many slots as its length turns dense.
const SPARSE_ENTRY_SLOTS: usize = size_of::<(usize, Value)>() / size_of::<Value>();

/// Whether `present` elements of an array of `length` fill as many dense slots as its
/// length, so dense storage takes no more words than sparse entries would.
fn dense_storage_fits(present: usize, length: usize) -> bool {
    present.saturating_mul(SPARSE_ENTRY_SLOTS) >= length
}
pub use pacing::MemoryPolicy;
/// The prototypes `Heap::mark_prototype` remembers: an ordinary object's and an array's,
/// whose allocations alternate (array literals and objects in one request handler), so one
/// remembered prototype would look the other up on nearly every allocation.
const MARKED_PROTOTYPE_WAYS: usize = 2;
#[inline(always)]
fn payload_unit() -> usize {
    #[cfg(feature = "gc-tuning")]
    return *PAYLOAD_BYTES_PER_ALLOCATION_TUNED;
    #[cfg(not(feature = "gc-tuning"))]
    PAYLOAD_BYTES_PER_ALLOCATION
}
/// The fewest allocations after which a collection is full on a small heap: the first
/// collection runs after the young minimum, and is full once the allocation cap has passed.
#[cfg(test)]
pub(crate) fn full_collection_allocation_bound(policy: MemoryPolicy) -> usize {
    let pacing = policy.pacing();
    pacing.minimum_young.max(pacing.minimum_allocation_cap)
}
/// Payload bytes beside a cell (string units, array elements) that weigh as one more
/// allocation toward the next collection.
#[cfg(not(feature = "gc-tuning"))]
const PAYLOAD_BYTES_PER_ALLOCATION: usize = 64;
/// Backing-store bytes (ArrayBuffer contents) allocated since the last collection that
/// make the next one due, at least; the budget grows to the live external bytes a
/// collection leaves. Buffers die only with their cells, so streaming code that drops
/// one buffer per chunk retains every chunk until a collection. Like V8's external
/// memory limit this bounds that garbage without collecting at every chunk: on the
/// SaltyAom video route (64 KiB chunks, Linux x86_64, 4 vCPU) one collection per
/// 64 KiB buffer cut peak RSS 112 -> 47 MB and throughput by two thirds.
const MINIMUM_EXTERNAL_COLLECTION_BUDGET: usize = 8 << 20;
/// Diagnostic builds read the unit from `QUENCH_GC_PAYLOAD_UNIT` to measure it.
#[cfg(feature = "gc-tuning")]
static PAYLOAD_BYTES_PER_ALLOCATION_TUNED: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("QUENCH_GC_PAYLOAD_UNIT").ok().and_then(|v| v.parse().ok()).unwrap_or(64)
});
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

    fn sweep(&mut self, marks: &[AtomicU64], compacts: bool) -> ParallelSweepResult {
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
                result.live_payload_bytes += cell.payload_bytes();
                if let Some(object) = cell.object() {
                    if compacts && object.properties().has_compact_range() {
                        result
                            .property_owners
                            .push(pack_offset_owner(object.properties().start_offset(), index));
                    }
                }
                continue;
            }

            let cell = slot.cell.take().expect("occupied dead sweep slot");
            result.external_bytes = result.external_bytes.saturating_add(cell.external_bytes());
            if let Cell::Promise(promise) = &cell {
                result.abandoned_contexts.abandon(&promise.record);
            }
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
    live_payload_bytes: usize,
    abandoned_contexts: AbandonedHostContexts,
    swept_slots: usize,
}

#[derive(Default)]
struct SweepOutput {
    live: usize,
    /// Payload of the surviving cells, summed by full sweeps only.
    live_payload_bytes: usize,
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
            Some(&RC_DROP_LOCKS[rc_drop_lock_index(object.elements().as_ptr() as usize)])
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
        Cell::Array { object } => object.elements().owner_count() > 1,
        Cell::ArrayBuffer { bytes, .. } => bytes.is_shared(),
        Cell::WasmHostFunction { signature, .. } => Rc::strong_count(signature) > 1,
        Cell::Iterator { .. }
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
/// The record families `Heap::record_families` lists.
const RECORD_FAMILIES: usize = 8;

/// Every family's mark bits, borrowed apart from the slot a trace holds (`mark_bits!`).
#[derive(Clone, Copy)]
pub(super) struct MarkBits<'a> {
    object_records: &'a ObjectRecordSpace,
    array_records: &'a ArrayRecordSpace,
    function_records: &'a FunctionRecordSpace,
    promise_records: &'a PromiseRecordSpace,
    await_records: &'a AwaitRecordSpace,
    iterator_records: &'a IteratorRecordSpace,
    string_records: &'a StringRecordSpace,
    v128_records: &'a V128RecordSpace,
    marks: &'a [AtomicU64],
}

impl MarkBits<'_> {
    /// Whether `value` is a heap value marked so far.
    pub(super) fn is_marked(self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::ObjectRecord(id)) => self.object_records.is_marked(id),
            Some(HeapReference::ArrayRecord(id)) => self.array_records.is_marked(id),
            Some(HeapReference::FunctionRecord(id)) => self.function_records.is_marked(id),
            Some(HeapReference::PromiseRecord(id)) => self.promise_records.is_marked(id),
            Some(HeapReference::AwaitRecord(id)) => self.await_records.is_marked(id),
            Some(HeapReference::IteratorRecord(id)) => self.iterator_records.is_marked(id),
            Some(HeapReference::StringRecord(id)) => self.string_records.is_marked(id),
            Some(HeapReference::V128Record(id)) => self.v128_records.is_marked(id),
            Some(HeapReference::LegacySlot(index)) => {
                let index = index as usize;
                self.marks.get(index / u64::BITS as usize).is_some_and(|word| {
                    word.load(Ordering::Relaxed) & (1 << (index % u64::BITS as usize)) != 0
                })
            }
            None => false,
        }
    }
}

/// `MarkBits` of a heap, borrowing only its mark fields.
macro_rules! mark_bits {
    ($heap:expr) => {
        MarkBits {
            object_records: &$heap.object_records,
            array_records: &$heap.array_records,
            function_records: &$heap.function_records,
            promise_records: &$heap.promise_records,
            await_records: &$heap.await_records,
            iterator_records: &$heap.iterator_records,
            string_records: &$heap.string_records,
            v128_records: &$heap.v128_records,
            marks: &$heap.marks,
        }
    };
}

#[derive(Default)]
pub(crate) struct Heap {
    slots: SlotArena,
    object_records: ObjectRecordSpace,
    array_records: ArrayRecordSpace,
    function_records: FunctionRecordSpace,
    promise_records: PromiseRecordSpace,
    await_records: AwaitRecordSpace,
    iterator_records: IteratorRecordSpace,
    /// Strings: immutable leaves, never prototypes, with no legacy-slot form.
    string_records: StringRecordSpace,
    /// Wasm vectors: immutable leaves (`Heap::alloc_v128`), in a slot when this is full.
    v128_records: V128RecordSpace,
    /// Allocations per cell variant (`Cell::profile_variant_name`), finer than `CellKind`.
    #[cfg(feature = "profile-aggregate")]
    allocated_variants: std::collections::BTreeMap<&'static str, u64>,
    marks: Vec<AtomicU64>,
    remembered_marks: Vec<u64>,
    remembered: Vec<u32>,
    young: Vec<u32>,
    /// The young cells with weak entries (weak maps and sets, weak refs, finalization
    /// registries): a minor collection prunes only these and the remembered cells.
    young_weak: Vec<u32>,
    free_by_slab: Vec<FreeSlotBitmap>,
    free_slabs: Vec<u32>,
    free_slots: usize,
    external_bytes: usize,
    allocations: usize,
    allocations_since_full: usize,
    /// Payload the live and not yet swept cells and records own (`Cell::payload_bytes`):
    /// allocation and growth add it, reclamation subtracts it. Pacing measures live
    /// weight with it, in the units allocation is charged in.
    payload_bytes: usize,
    /// Payload bytes grown since the last charge, below one allocation's weight.
    uncharged_payload_bytes: usize,
    /// What the last full collection kept, which the generational pacing grows from.
    live_after_full: usize,
    /// What the full collection that last traced closure environments through their reads
    /// kept (`EnvironmentTrace`); `None` before one has. Slots it cleared stay cleared, so a
    /// later one finds only what was captured since: it runs again once the live set has
    /// grown by `ENVIRONMENT_TRIM_GROWTH` of that.
    live_at_environment_trim: Option<usize>,
    /// The next collection traces the whole heap.
    full_due: bool,
    /// Selects the pacing table (`MemoryPolicy::pacing`); reset keeps it.
    memory_policy: MemoryPolicy,
    pacing_state: pacing::PacingState,
    /// Host contexts of promises the sweep freed, for the host to release.
    abandoned_contexts: AbandonedHostContexts,
    /// Live weight after the previous collection.
    live_weight_after_previous: usize,
    live_weight_after_full: usize,
    /// Backing-store bytes allocated since the last collection, against its budget.
    external_since_collection: usize,
    external_collection_budget: usize,
    /// The slot and record property owners the last full sweep listed, which size the next
    /// sweep's lists so they do not regrow from empty (and reallocate) every collection.
    property_owner_counts: (usize, usize),
    old_live: usize,
    threshold: usize,
    total_allocations: u64,
    collections: u64,
    /// The prototypes `mark_prototype` marked last, most recent first, which keep their
    /// mark for life: an allocation with one of them needs no lookup. A collection may free
    /// one and reuse its handle, so each one forgets them.
    marked_prototypes: [Option<Value>; MARKED_PROTOTYPE_WAYS],
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
    /// The sweep's phases: slot cells, direct records, and property-arena compaction.
    pub sweep_phase_nanos: [u64; 3],
    pub mark_clear_nanos: u64,
    /// VM-side collection phases: owner edges, root gathering, shape-trace setup, and
    /// the work after the heap collection.
    pub vm_phase_nanos: [u64; 4],
    pub full_collections: u64,
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
            sweep_phase_nanos: [0; 3],
            mark_clear_nanos: 0,
            vm_phase_nanos: [0; 4],
            full_collections: 0,
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
    /// String records; only the allocation profile counts them by kind.
    #[cfg_attr(not(feature = "profile-aggregate"), allow(dead_code))]
    String,
    BigInt,
    Symbol,
    Date,
    Error,
    RegExp,
    ArrayFromAsync,
    AwaitState,
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
        "await_state",
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
/// How much the live set grows past what the last environment-trimming full collection kept
/// before another trims (`Heap::live_at_environment_trim`): one live set, so what closures
/// over-capture since can at most double what a trimmed heap holds.
const ENVIRONMENT_TRIM_GROWTH: pacing::Ratio = pacing::Ratio::new(1, 1);

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
            threshold: MemoryPolicy::default().pacing().minimum_young,
            max_threshold: MemoryPolicy::default().pacing().minimum_young,
            external_collection_budget: MINIMUM_EXTERNAL_COLLECTION_BUDGET,
            ..Self::default()
        }
    }
    /// Selects the memory policy at startup, before the first collection: the next
    /// collection runs after the policy's young minimum, and pacing follows its table.
    pub(crate) fn set_memory_policy(&mut self, policy: MemoryPolicy) {
        debug_assert_eq!(self.collections, 0, "the memory policy is chosen at startup");
        self.memory_policy = policy;
        self.threshold = self.pacing().minimum_young;
        self.max_threshold = self.threshold;
    }
    pub(crate) fn memory_policy(&self) -> MemoryPolicy {
        self.memory_policy
    }
    fn pacing(&self) -> pacing::GenerationalPacing {
        self.memory_policy.pacing()
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
        self.record_families()
            .iter()
            .any(|(family, _)| family.live_count() > 0)
    }

    /// Every record family, with the `Value` constructor naming its records.
    fn record_families(&self) -> [(&dyn RecordFamily, fn(RecordId) -> Value); RECORD_FAMILIES] {
        [
            (&self.object_records, Value::direct_object),
            (&self.array_records, Value::direct_array),
            (&self.function_records, Value::direct_function),
            (&self.promise_records, Value::direct_promise),
            (&self.await_records, Value::direct_await),
            (&self.iterator_records, Value::direct_iterator),
            (&self.string_records, Value::direct_string),
            (&self.v128_records, Value::direct_v128),
        ]
    }

    /// `record_families`, mutably.
    fn record_families_mut(
        &mut self,
    ) -> [(&mut dyn RecordFamily, fn(RecordId) -> Value); RECORD_FAMILIES] {
        [
            (&mut self.object_records, Value::direct_object),
            (&mut self.array_records, Value::direct_array),
            (&mut self.function_records, Value::direct_function),
            (&mut self.promise_records, Value::direct_promise),
            (&mut self.await_records, Value::direct_await),
            (&mut self.iterator_records, Value::direct_iterator),
            (&mut self.string_records, Value::direct_string),
            (&mut self.v128_records, Value::direct_v128),
        ]
    }

    /// Each program a live cell or function can still run or resolve names in
    /// (`Cell::program_references`).
    pub(crate) fn for_each_program_reference(&mut self, mut visit: impl FnMut(crate::vm::ProgramId)) {
        self.function_records
            .for_each_live(|_, function| function.program().into_iter().for_each(&mut visit));
        self.await_records
            .for_each_live(|_, state| state.program_references(&mut visit));
        self.iterator_records
            .for_each_live(|_, iterator| iterator.program_references(&mut visit));
        self.for_each_cell(|cell| cell.program_references(&mut visit));
    }

    /// Every live await state, in its record or in a slot.
    #[cfg(test)]
    pub(crate) fn for_each_await_state(&mut self, mut visit: impl FnMut(&AwaitState)) {
        self.await_records.for_each_live(|_, state| visit(state));
        self.for_each_cell(|cell| {
            if let Cell::AwaitState(state) = cell {
                visit(state);
            }
        });
    }

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

    /// Every live legacy cell, without needing `&mut` (census and diagnostics).
    #[cfg(feature = "profile-gc-time")]
    pub(crate) fn for_each_live_cell(&self, mut visit: impl FnMut(&Cell)) {
        for slot in self.slots.iter() {
            if let Some(cell) = &slot.cell {
                visit(cell);
            }
        }
    }

    pub(crate) fn live_slots(&self) -> usize {
        self.slots.len() - self.free_slots
            + self
                .record_families()
                .iter()
                .map(|(family, _)| family.live_count())
                .sum::<usize>()
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

    #[inline(always)]
    pub(crate) fn mark_prototype(&mut self, prototype: Value) {
        if !self.marked_prototypes.contains(&Some(prototype)) {
            self.mark_new_prototype(prototype);
        }
    }

    /// `mark_prototype` of a prototype other than the one it marked last.
    #[inline(never)]
    fn mark_new_prototype(&mut self, prototype: Value) {
        // A flag, not an edge: no write barrier for record prototypes.
        let object = match prototype.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            Some(record_id) => Some(unsafe { self.object_records.get_live_mut_unbarriered(record_id) }),
            None => self.object_mut(prototype),
        };
        if let Some(object) = object {
            object.mark_as_prototype();
            self.marked_prototypes.rotate_right(1);
            self.marked_prototypes[0] = Some(prototype);
        }
    }

    /// Allocates `cell`. An array, whose kind a call site's literal cell fixes, takes its
    /// record directly (`alloc_array`) without the dispatch on the cell's kind.
    #[inline(always)]
    pub fn alloc(&mut self, cell: Cell) -> Value {
        match cell {
            Cell::Array { object } if !cfg!(feature = "profile-aggregate") => self.alloc_array(object),
            cell => self.alloc_cell(cell),
        }
    }

    /// `alloc(Cell::Array { object })`: an array record while one is free, charged as any
    /// allocation (`charge_allocation`) for its element storage.
    fn alloc_array(&mut self, object: ArrayObject) -> Value {
        self.mark_prototype(object.proto());
        self.charge_allocation(array_payload_bytes(&object), 0);
        match self.array_records.try_insert(object) {
            Ok(record_id) => {
                let value = Value::direct_array(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(object) => self.alloc_in_slot(Cell::Array { object }, self.tracks_young_cells()),
        }
    }

    /// The allocation ledger's charge for one cell owning `payload` bytes beyond its slot, of
    /// which `external` are external buffers.
    #[inline(always)]
    fn charge_allocation(&mut self, payload: usize, external: usize) {
        self.charge_materialized_text();
        let weight = 1 + payload / payload_unit();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocations += weight;
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
        self.total_allocations += 1;
        self.charge_external_growth(external);
    }

    fn alloc_cell(&mut self, cell: Cell) -> Value {
        if let Some(object) = cell.object() { self.mark_prototype(object.proto()); }
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = Self::cell_kind(&cell) as usize;
            let bytes = Self::cell_payload_bytes(&cell);
            self.gc_profile.allocated_kinds[kind] += 1;
            *self
                .allocated_variants
                .entry(cell.profile_variant_name())
                .or_default() += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.charge_allocation(cell.payload_bytes(), cell.external_bytes());
        let track_young = self.tracks_young_cells();
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
            Cell::Promise(promise) => match self.promise_records.try_insert(*promise) {
                Ok(record_id) => {
                    let value = Value::direct_promise(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(promise) => Cell::Promise(Box::new(promise)),
            },
            Cell::Iterator(iterator) => match self.iterator_records.try_insert(*iterator) {
                Ok(record_id) => {
                    let value = Value::direct_iterator(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(iterator) => Cell::Iterator(Box::new(iterator)),
            },
            Cell::AwaitState(state) => match self.await_records.try_insert(*state) {
                Ok(record_id) => return Value::direct_await(record_id),
                Err(state) => Cell::AwaitState(Box::new(state)),
            },
            Cell::Function(function) => match self.function_records.try_insert(*function) {
                Ok(record_id) => {
                    let value = Value::direct_function(record_id);
                    #[cfg(feature = "profile-memory")]
                    self.profile_allocation(value);
                    return value;
                }
                Err(function) => Cell::Function(Box::new(function)),
            },
            cell => cell,
        };
        self.alloc_in_slot(cell, track_young)
    }

    /// `alloc(Cell::Environment { .. })` without the dispatch on the cell's kind, with the
    /// address of the environment's slot storage, which stays put while it lives. An
    /// environment's weight is one allocation: it has no prototype, payload or external bytes.
    pub(crate) fn alloc_environment(
        &mut self,
        parent: Value,
        function: u32,
        slots: EnvironmentSlots,
        scope: EnvironmentScopeRef,
    ) -> (Value, *const EnvironmentSlots) {
        let cell = Cell::Environment { parent, function, slots, scope };
        let environment = if cfg!(feature = "profile-aggregate") {
            self.alloc(cell)
        } else {
            self.allocations += 1;
            self.allocations_since_full = self.allocations_since_full.saturating_add(1);
            self.total_allocations += 1;
            self.alloc_in_slot(cell, self.tracks_young_cells())
        };
        let storage = match self.get(environment) {
            Some(Cell::Environment { slots, .. }) => slots as *const EnvironmentSlots,
            _ => unreachable!("an environment is allocated in a slot"),
        };
        (environment, storage)
    }

    /// `alloc(Cell::Object(object))` without the dispatch on the cell's kind, which an
    /// ordinary object's construction knows: a direct record while one is free.
    #[inline(always)]
    pub(crate) fn alloc_object(&mut self, object: Object) -> Value {
        let prototype = object.proto();
        self.alloc_object_of(object, prototype)
    }

    /// `alloc_object_with_properties` of a planned object: `header`, derived once for
    /// `prototype` and an inline shape (`InlineObjectHeader::of`), and its `inline` values.
    #[inline(always)]
    pub(crate) fn alloc_object_from_header(
        &mut self,
        header: InlineObjectHeader,
        prototype: Value,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Value {
        let Some(object) = Object::from_inline_header(header, inline) else {
            return self.alloc_object_with_properties_of_header(header, prototype, inline);
        };
        debug_assert!(object.proto() == prototype);
        self.alloc_object_of(object, prototype)
    }

    /// `alloc_object_from_header` of values the payload word cannot hold: arena storage.
    #[cold]
    fn alloc_object_with_properties_of_header(
        &mut self,
        header: InlineObjectHeader,
        prototype: Value,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Value {
        let shape = header.shape();
        let length = self.properties.len(ValueVec::inline_property_storage(shape));
        self.alloc_object_with_properties(prototype, shape, &inline[..length])
    }

    /// `alloc_object` of `object`, whose prototype is `prototype`.
    #[inline(always)]
    fn alloc_object_of(&mut self, object: Object, prototype: Value) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Object(object));
        }
        self.mark_prototype(prototype);
        self.insert_object_record(object)
    }

    /// `alloc_object` of `object` whose prototype is already flagged as one
    /// (`Object::was_used_as_prototype`), as a recorded closure's are (`Vm::closure_of_template`):
    /// the flag is never cleared, so it needs no marking.
    pub(crate) fn alloc_object_under_flagged_prototype(&mut self, object: Object) -> Value {
        debug_assert!(self.prototype_flagged(object.proto()));
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Object(object));
        }
        self.insert_object_record(object)
    }

    /// Whether `prototype` names no object or one flagged as a prototype, as
    /// `mark_new_prototype` leaves it.
    fn prototype_flagged(&self, prototype: Value) -> bool {
        self.object(prototype).is_none_or(Object::was_used_as_prototype)
    }

    /// An object record of `object`, else a slot: one allocation, with no external bytes.
    #[inline(always)]
    fn insert_object_record(&mut self, object: Object) -> Value {
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.object_records.try_insert(object) {
            Ok(record_id) => {
                let value = Value::direct_object(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(object) => self.alloc_in_slot(Cell::Object(object), self.tracks_young_cells()),
        }
    }

    /// `alloc(Cell::Function(..))` without boxing the function first: a function record
    /// while one is free.
    pub(crate) fn alloc_function(&mut self, function: FunctionRecord) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Function(Box::new(function)));
        }
        self.mark_prototype(function.object.proto());
        self.insert_function_record(function)
    }

    /// `alloc_function` of `function` whose prototype is already flagged as one, as
    /// `alloc_object_under_flagged_prototype` allocates an object.
    pub(crate) fn alloc_function_under_flagged_prototype(&mut self, function: FunctionRecord) -> Value {
        debug_assert!(self.prototype_flagged(function.object.proto()));
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Function(Box::new(function)));
        }
        self.insert_function_record(function)
    }

    /// A function record of `function`, else a slot: one allocation, with no payload or
    /// external bytes.
    #[inline(always)]
    fn insert_function_record(&mut self, function: FunctionRecord) -> Value {
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.function_records.try_insert(function) {
            Ok(record_id) => {
                let value = Value::direct_function(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(function) => self.alloc_in_slot(
                Cell::Function(Box::new(function)),
                self.tracks_young_cells(),
            ),
        }
    }

    /// `alloc(Cell::Promise(..))` without boxing the promise first: a promise record while
    /// one is free.
    /// `alloc(Cell::AwaitState(..))` without boxing the state first: an await record while
    /// one is free. An await state is internal: no prototype, payload or external bytes.
    pub(crate) fn alloc_await_state(&mut self, state: AwaitState) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::AwaitState(Box::new(state)));
        }
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.await_records.try_insert(state) {
            Ok(record_id) => Value::direct_await(record_id),
            Err(state) => {
                self.alloc_in_slot(Cell::AwaitState(Box::new(state)), self.tracks_young_cells())
            }
        }
    }

    /// A Wasm vector of `bytes`: a vector record while one is free, else a slot. A vector is
    /// an immutable leaf with no prototype, payload or external bytes.
    #[inline]
    pub(crate) fn alloc_v128(&mut self, bytes: [u8; crate::wasm::V128_BYTES]) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::WasmV128(bytes));
        }
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.v128_records.try_insert(bytes) {
            Ok(record_id) => Value::direct_v128(record_id),
            Err(bytes) => self.alloc_in_slot(Cell::WasmV128(bytes), self.tracks_young_cells()),
        }
    }

    /// A Wasm vector's bytes, in its record or in a slot.
    #[inline]
    pub(crate) fn v128(&self, value: Value) -> Option<[u8; crate::wasm::V128_BYTES]> {
        if let Some(record_id) = value.direct_v128_index() {
            // SAFETY: a direct-v128 Value names an initialized record.
            return Some(*unsafe { self.v128_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::WasmV128(bytes) => Some(*bytes),
            _ => None,
        }
    }

    /// `alloc(Cell::Iterator(..))` without boxing the iterator first: an iterator record
    /// while one is free.
    /// Allocates a string record. Strings have no legacy-slot form, so their space holds as
    /// many records as the legacy slots do (`STRING_RECORD_CAPACITY`); exhausting it is out
    /// of memory, as exhausting the slots is.
    pub(crate) fn alloc_string(&mut self, string: JsString) -> Value {
        #[cfg(feature = "profile-aggregate")]
        {
            let kind = CellKind::String as usize;
            // Do not materialize ropes in the profiler: production charges the flat payload
            // only when it already exists. `capacity()` flattens a rope and perturbs the
            // allocation shape the profile is meant to observe.
            let bytes = string.flat_unit_bytes();
            self.gc_profile.allocated_kinds[kind] += 1;
            *self.allocated_variants.entry("string").or_default() += 1;
            self.gc_profile.allocated_payload_bytes[kind] += bytes as u64;
            self.gc_profile.allocated_size_buckets[kind][Self::size_bucket(bytes)] += 1;
        }
        self.charge_materialized_text();
        let payload = string.flat_unit_bytes();
        let weight = 1 + payload / payload_unit();
        self.payload_bytes = self.payload_bytes.saturating_add(payload);
        self.allocations += weight;
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
        self.total_allocations += 1;
        match self.string_records.try_insert(string) {
            Ok(record_id) => Value::direct_string(record_id),
            Err(_) => panic!("string record space exhausted"),
        }
    }

    pub(crate) fn alloc_iterator(&mut self, iterator: IteratorRecord) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Iterator(Box::new(iterator)));
        }
        self.mark_prototype(iterator.object.proto());
        // An iterator's weight is one allocation, and it holds no payload or external bytes.
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.iterator_records.try_insert(iterator) {
            Ok(record_id) => {
                let value = Value::direct_iterator(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(iterator) => {
                self.alloc_in_slot(Cell::Iterator(Box::new(iterator)), self.tracks_young_cells())
            }
        }
    }

    /// Allocations per cell variant.
    #[cfg(feature = "profile-aggregate")]
    pub(crate) fn allocated_variants(&self) -> &std::collections::BTreeMap<&'static str, u64> {
        &self.allocated_variants
    }

    pub(crate) fn alloc_promise(&mut self, promise: PromiseCell) -> Value {
        if cfg!(feature = "profile-aggregate") {
            return self.alloc(Cell::Promise(Box::new(promise)));
        }
        self.mark_prototype(promise.object.proto());
        // A promise's weight is one allocation, and it holds no payload or external bytes.
        self.allocations += 1;
        self.allocations_since_full = self.allocations_since_full.saturating_add(1);
        self.total_allocations += 1;
        match self.promise_records.try_insert(promise) {
            Ok(record_id) => {
                let value = Value::direct_promise(record_id);
                #[cfg(feature = "profile-memory")]
                self.profile_allocation(value);
                value
            }
            Err(promise) => {
                self.alloc_in_slot(Cell::Promise(Box::new(promise)), self.tracks_young_cells())
            }
        }
    }

    /// The slot part of `alloc`, for a cell no record space takes.
    fn alloc_in_slot(&mut self, cell: Cell, track_young: bool) -> Value {
        let weak = cell.has_weak_entries();
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
            if weak {
                self.young_weak.push(index);
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
        if weak {
            self.young_weak.push(index as u32);
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
        self.properties.assert_shape_length(shape, values.len());
        if values.len() <= INLINE_PROPERTY_COUNT
            && let Some(object) = Object::with_inline_values(proto, shape, values)
        {
            return self.alloc_object(object);
        }
        let properties = self.properties.with_values(shape, values);
        self.alloc_object(Object::with_property_storage(
            proto,
            properties,
            [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
        ))
    }
    /// `alloc(Cell::Array { object: array })` of a fresh array, given its first named
    /// properties `values` under `shape` first: the storage steps
    /// `initialize_object_properties` takes on the published cell (arena storage, as an
    /// array keeps its element backing in the payload; the replaced storage's integrity
    /// carried over and its arena words released), taken before the array is published. No
    /// collection runs in between, so the arena holds the values for their owner exactly as
    /// after the two-step path.
    pub(crate) fn alloc_array_with_properties(
        &mut self,
        mut array: ArrayObject,
        shape: u32,
        values: &[Value],
    ) -> Value {
        let (properties, inline_properties) = self.initial_object_storage(shape, values, false);
        let previous = array.replace_property_storage(properties, inline_properties);
        self.properties.release(previous);
        let value = self.alloc(Cell::Array { object: array });
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(value, values.len());
        value
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
    pub(crate) fn initial_object_storage(
        &mut self,
        shape: u32,
        values: &[Value],
        inline: bool,
    ) -> (ValueVec, [Value; INLINE_PROPERTY_COUNT]) {
        self.properties.assert_shape_length(shape, values.len());
        if inline && values.len() <= INLINE_PROPERTY_COUNT && Object::inline_values_narrow(values) {
            // At most `INLINE_PROPERTY_COUNT` values: a loop over the fixed bound unrolls to
            // element moves, where one over `values` becomes a `memcpy` call.
            let inline = std::array::from_fn(|slot| values.get(slot).copied().unwrap_or(Value::UNDEFINED));
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
        self.function_records.for_each_live_mut(|_, function| {
            let old_shape = function.object.shape() as usize;
            function.object.set_shape(mapping[old_shape]);
        });
        self.promise_records.for_each_live_mut(|_, promise| {
            let old_shape = promise.object.shape() as usize;
            promise.object.set_shape(mapping[old_shape]);
        });
        self.iterator_records.for_each_live_mut(|_, iterator| {
            let old_shape = iterator.object.shape() as usize;
            iterator.object.set_shape(mapping[old_shape]);
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
        if !self.properties.compaction_due() {
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
            function_records,
            promise_records,
            iterator_records,
            slots,
            properties,
            ..
        } = self;
        for (_, owner) in owners {
            let object = if let Some(record_id) = owner.direct_object_index() {
                // SAFETY: direct owners were collected from live marked records.
                Some(unsafe { object_records.get_live_mut_unbarriered(record_id) })
            } else if let Some(record_id) = owner.direct_array_index() {
                // SAFETY: array owners were collected from live array records.
                Some(&mut **unsafe { array_records.get_live_mut_unbarriered(record_id) })
            } else if let Some(record_id) = owner.direct_function_index() {
                // SAFETY: function owners were collected from live function records.
                Some(&mut unsafe { function_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_promise_index() {
                // SAFETY: promise owners were collected from live promise records.
                Some(&mut unsafe { promise_records.get_live_mut_unbarriered(record_id) }.object)
            } else if let Some(record_id) = owner.direct_iterator_index() {
                // SAFETY: iterator owners were collected from live iterator records.
                Some(&mut unsafe { iterator_records.get_live_mut_unbarriered(record_id) }.object)
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
        self.function_records = FunctionRecordSpace::default();
        self.promise_records = PromiseRecordSpace::default();
        self.await_records = AwaitRecordSpace::default();
        self.iterator_records = IteratorRecordSpace::default();
        self.string_records = StringRecordSpace::default();
        self.v128_records = V128RecordSpace::default();
        self.marks.clear();
        self.free_by_slab.clear();
        self.free_slabs.clear();
        self.free_slots = 0;
        self.external_bytes = 0;
        self.external_since_collection = 0;
        self.external_collection_budget = MINIMUM_EXTERNAL_COLLECTION_BUDGET;
        self.payload_bytes = 0;
        self.allocations = 0;
        self.allocations_since_full = 0;
        self.live_after_full = 0;
        self.live_at_environment_trim = None;
        self.full_due = false;
        self.pacing_state = pacing::PacingState::default();
        self.live_weight_after_previous = 0;
        self.live_weight_after_full = 0;
        self.old_live = 0;
        self.threshold = self.pacing().minimum_young;
        self.total_allocations = 0;
        self.collections = 0;
        self.peak_live = 0;
        self.peak_survivors = 0;
        self.max_threshold = self.pacing().minimum_young;
        self.properties.reset();
        self.remembered_marks.clear();
        self.remembered.clear();
        self.young.clear();
        self.young_weak.clear();
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
        self.payload_bytes = self.payload_bytes.saturating_add(bytes);
        let pending = self.uncharged_payload_bytes.saturating_add(bytes);
        let weight = pending / payload_unit();
        self.uncharged_payload_bytes = pending % payload_unit();
        self.allocations += weight;
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
    }
    /// Accounts new backing-store bytes; past the external budget a collection is due.
    pub(crate) fn charge_external_growth(&mut self, bytes: usize) {
        self.external_bytes += bytes;
        self.external_since_collection = self.external_since_collection.saturating_add(bytes);
        if self.external_since_collection >= self.external_collection_budget {
            self.threshold = self.threshold.min(self.allocations);
        }
    }
    /// Charges text made on demand (`wtf16::take_materialized_bytes`) as payload allocation.
    /// The string operations that make it allocate or return through the places that call
    /// this (generic allocation, native and host calls, each collection); safepoints, which
    /// run far more often, never pay for it.
    pub(crate) fn charge_materialized_text(&mut self) {
        let bytes = crate::vm::wtf16::take_materialized_bytes();
        if bytes == 0 {
            return;
        }
        // Allocation weight only: the payload ledger (`payload_bytes`) follows the strings
        // themselves, whose sweep releases what they own.
        let weight = bytes / payload_unit();
        self.allocations = self.allocations.saturating_add(weight);
        self.allocations_since_full = self.allocations_since_full.saturating_add(weight);
    }

    pub fn should_collect(&self) -> bool {
        self.allocations >= self.threshold
    }
    /// The host contexts of promises freed since the last call.
    pub(crate) fn take_abandoned_contexts(&mut self) -> AbandonedHostContexts {
        std::mem::take(&mut self.abandoned_contexts)
    }
    pub(crate) fn should_full_collect(&self) -> bool {
        self.full_due
            || self.pacing().allocation_cap_reached(
                self.live_after_full,
                self.allocations_since_full,
                self.pacing_state.cap_backoff,
            )
    }
    /// Every collection may be minor, so the legacy young list and barrier are always kept.
    fn tracks_young_cells(&self) -> bool {
        true
    }
    pub(crate) fn is_old(&self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::LegacySlot(index)) => {
                self.get(value).is_some() && Self::marked(&self.marks, index as usize)
            }
            Some(_) => mark_bits!(self).is_marked(value),
            None => false,
        }
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
    /// A root that lasts until its native scope ends (`RootTable::insert_scoped`).
    pub(crate) fn root_scoped(&mut self, value: Value) -> RootId {
        self.roots.insert_scoped(value)
    }
    pub(crate) fn scoped_root_height(&self) -> usize {
        self.roots.scoped_height()
    }
    pub(crate) fn release_root_scope(&mut self, height: usize) {
        self.roots.release_scope(height);
    }
    pub(crate) fn release_root_scope_keeping(&mut self, height: usize, keep: &[RootId]) {
        self.roots.release_scope_keeping(height, keep);
    }
    #[cfg(test)]
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        self.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, true)
    }
    /// Whether a collection requested as `full` traces and sweeps the whole heap. Records
    /// and legacy cells share one generation rule: survivors stay marked (old), the write
    /// barriers log old owners they write, and a minor traces roots and that log only.
    /// Whether this full collection traces closure environments through their reads
    /// (`live_at_environment_trim`).
    fn environment_trim_due(&self) -> bool {
        self.live_at_environment_trim.is_none_or(|trimmed| {
            self.live_after_full >= trimmed.saturating_add(ENVIRONMENT_TRIM_GROWTH.of(trimmed))
        })
    }
    pub(crate) fn collection_is_full(&self, full: bool) -> bool {
        full
    }

    pub(crate) fn collect_with_object_roots(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut object_roots: impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        closure_reads: Option<&dyn ClosureReadSource>,
        full: bool,
    ) -> Vec<(Value, Value)> {
        let full = self.collection_is_full(full);
        self.charge_materialized_text();
        // The sweep may free a prototype whose record a later object reuses.
        advance_prototype_layout_epoch();
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
        self.marked_prototypes = [None; MARKED_PROTOTYPE_WAYS];
        #[cfg(feature = "profile-aggregate")]
        let allocations_between_collections = self.allocations as u64;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let mark_started = std::time::Instant::now();
        #[cfg(feature = "profile-aggregate")]
        let mark_clear_started = std::time::Instant::now();
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        if full {
            self.gc_profile.full_collections += 1;
        } else {
            self.gc_profile.minor_collections += 1;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.remembered_owner_scans += self.remembered.len() as u64;
                self.gc_profile.young_cells_swept += self.young.len() as u64;
            }
        }
        if full {
            Self::clear_marks(&self.marks);
            self.remembered.clear();
            self.remembered_marks.fill(0);
            for (family, _) in self.record_families_mut() {
                family.clear_marks();
            }
        }
        #[cfg(feature = "profile-aggregate")]
        if full {
            self.gc_profile.mark_clear_nanos += mark_clear_started.elapsed().as_nanos() as u64;
        }
        // The roots become the mark stack: collecting a vector's own iterator keeps its
        // buffer, so a collection holds one root-sized buffer, not a copy beside it.
        let mut work: Vec<Value> = roots.into_iter().collect();
        work.reserve(self.roots.slot_count());
        work.extend(self.roots.values());
        #[cfg(debug_assertions)]
        let verify_roots = work.clone();
        #[cfg(debug_assertions)]
        let verify_old_records = (!full).then(|| {
            let mut old = Vec::new();
            for (family, handle) in self.record_families() {
                family.for_each_marked(&mut |id| old.push(handle(id)));
            }
            old
        });
        if !full {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.remembered_owner_scans += self
                    .record_families()
                    .iter()
                    .map(|(family, _)| family.remembered_count())
                    .sum::<usize>() as u64;
            }
            for (family, handle) in self.record_families_mut() {
                work.extend(family.take_remembered().into_iter().map(handle));
            }
        }
        #[cfg(debug_assertions)]
        let verify_remembered_records: Vec<Value> = if full {
            Vec::new()
        } else {
            work[verify_roots.len()..].to_vec()
        };
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        // A full collection traces closure environments through what closures can read, once
        // the live set has grown since the last that did: environments over-captured since
        // then are what grew it, and slots cleared before stay cleared. Between them the
        // trace's per-environment state would only add to each full collection's peak
        // (V8-v7 max RSS +50 to +156 KiB on most fixtures when every full collection traced).
        let trims_environments = full && self.environment_trim_due();
        let mut environments = EnvironmentTrace::new(closure_reads.filter(|_| trims_environments));
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
                        let mark_bits = mark_bits!(self);
                        ephemerons.remembered(
                            value,
                            cell,
                            |target| mark_bits.is_marked(target),
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
                &mut environments,
            );
        }
        #[cfg(debug_assertions)]
        {
            if full {
                self.debug_verify_full(&verify_roots, &mut object_roots, closure_reads.filter(|_| trims_environments));
            } else {
                let mut pinned = verify_old_records.unwrap_or_default();
                pinned.extend(verify_remembered_records);
                self.debug_verify_minor(&verify_roots, &pinned, &mut object_roots);
            }
        }
        self.clear_unread_environment_slots(environments);
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
        // Property-arena owners are listed only for a full collection that compacts: the
        // lists hold an entry per live owner at the heap's peak. A compaction that this
        // sweep's releases make due waits for the next full collection.
        let compacts = full && self.properties.compaction_due();
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
            mut live_payload_bytes,
            property_owners,
            freed_cells,
            swept_slots,
        } = if let Some(pool) = sweep_pool {
            self.sweep_full_parallel(pool, compacts)
        } else {
            self.sweep_serial(full, compacts)
        };
        let mut direct_property_owners = Vec::with_capacity(if compacts {
            self.property_owner_counts.1
        } else {
            0
        });
        let legacy_live = live;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let records_started = std::time::Instant::now();
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[0] +=
                (records_started - sweep_started).as_nanos() as u64;
        }
        {
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.sweep_slots +=
                    self.record_families()
                        .iter()
                        .map(|(family, _)| family.high_water())
                        .sum::<usize>() as u64;
            }
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            let properties = &mut self.properties;
            let payload_bytes = &mut self.payload_bytes;
            let (object_live, object_freed) = self.object_records.sweep(full, full, |record| match record {
                RecordSweep::Keep(record_id, object) => {
                    if compacts && object.properties().has_compact_range() {
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
            let (function_live, function_freed) =
                self.function_records.sweep(full, full, |record| match record {
                    RecordSweep::Keep(record_id, function) => {
                        if compacts && function.object.properties().has_compact_range() {
                            direct_property_owners.push((
                                function.object.properties().start_offset(),
                                Value::direct_function(record_id),
                            ));
                        }
                    }
                    RecordSweep::Dead(_record_id, function) => {
                        #[cfg(feature = "profile-memory")]
                        let slots = properties.len(function.object.properties());
                        properties.release(function.object.properties());
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed_function(Value::direct_function(_record_id), slots);
                    }
                });
            let abandoned_contexts = &mut self.abandoned_contexts;
            let (promise_live, promise_freed) =
                self.promise_records.sweep(full, full, |record| match record {
                    RecordSweep::Keep(record_id, promise) => {
                        if compacts && promise.object.properties().has_compact_range() {
                            direct_property_owners.push((
                                promise.object.properties().start_offset(),
                                Value::direct_promise(record_id),
                            ));
                        }
                    }
                    RecordSweep::Dead(_record_id, promise) => {
                        abandoned_contexts.abandon(&promise.record);
                        #[cfg(feature = "profile-memory")]
                        let slots = properties.len(promise.object.properties());
                        properties.release(promise.object.properties());
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed_promise(Value::direct_promise(_record_id), slots);
                    }
                });
            // An await state owns no arena storage; dropping a dead one frees its unresumed
            // continuation.
            let (await_live, await_freed) = self.await_records.sweep(full, false, |_| {});
            // A vector owns nothing beyond its record.
            let (v128_live, v128_freed) = self.v128_records.sweep(full, false, |_| {});
            let (iterator_live, iterator_freed) =
                self.iterator_records.sweep(full, full, |record| match record {
                    RecordSweep::Keep(record_id, iterator) => {
                        if compacts && iterator.object.properties().has_compact_range() {
                            direct_property_owners.push((
                                iterator.object.properties().start_offset(),
                                Value::direct_iterator(record_id),
                            ));
                        }
                    }
                    RecordSweep::Dead(_record_id, iterator) => {
                        #[cfg(feature = "profile-memory")]
                        let slots = properties.len(iterator.object.properties());
                        properties.release(iterator.object.properties());
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed_iterator(Value::direct_iterator(_record_id), slots);
                    }
                });
            let (string_live, string_freed) = self.string_records.sweep(full, full, |record| match record {
                RecordSweep::Keep(_, string) => {
                    if full {
                        live_payload_bytes += string.flat_unit_bytes();
                    }
                }
                RecordSweep::Dead(_, string) => {
                    *payload_bytes = payload_bytes.saturating_sub(string.flat_unit_bytes());
                }
            });
            let mut dead_arrays = Vec::new();
            let (array_live, array_freed) = self.array_records.sweep(full, full, |record| match record {
                RecordSweep::Keep(record_id, array) => {
                    if full {
                        live_payload_bytes += array_payload_bytes(array);
                    }
                    if compacts && array.properties().has_compact_range() {
                        direct_property_owners.push((
                            array.properties().start_offset(),
                            Value::direct_array(record_id),
                        ));
                    }
                }
                RecordSweep::Dead(record_id, array) => {
                    *payload_bytes = payload_bytes.saturating_sub(array_payload_bytes(&array));
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
            live += object_live
                + array_live
                + function_live
                + promise_live
                + await_live
                + v128_live
                + iterator_live
                + string_live;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.freed += (object_freed
                    + array_freed
                    + function_freed
                    + promise_freed
                    + await_freed
                    + v128_freed
                    + iterator_freed
                    + string_freed) as u64;
            }
            #[cfg(not(feature = "profile-aggregate"))]
            let _ = (
                object_freed,
                array_freed,
                function_freed,
                promise_freed,
                await_freed,
                v128_freed,
                iterator_freed,
                string_freed,
            );
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += swept_slots as u64;
            self.gc_profile.freed += freed_cells as u64;
        }
        let _ = (freed_cells, swept_slots);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[1] += records_started.elapsed().as_nanos() as u64;
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let compaction_started = std::time::Instant::now();
        if full {
            // A full sweep recounts the ledger from the survivors: shrinking a live array
            // (truncation, `pop`, `splice`) is not charged back between full collections.
            self.payload_bytes = live_payload_bytes;
        }
        if compacts {
            self.property_owner_counts = (property_owners.len(), direct_property_owners.len());
            self.compact_property_arena(property_owners, direct_property_owners);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[2] += compaction_started.elapsed().as_nanos() as u64;
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        // Live weight in the units allocation is charged in: one per cell or record plus
        // its payload.
        let live_weight = live + self.payload_bytes / payload_unit();
        let allocated = std::mem::take(&mut self.allocations);
        self.external_since_collection = 0;
        self.external_collection_budget = self.external_bytes.max(MINIMUM_EXTERNAL_COLLECTION_BUDGET);
        if full {
            self.allocations_since_full = 0;
            self.old_live = legacy_live;
            self.young.clear();
            self.remembered.clear();
            self.remembered_marks.fill(0);
        }
        if full {
            self.live_after_full = live;
            self.live_weight_after_full = live_weight;
        }
        if trims_environments && closure_reads.is_some() {
            self.live_at_environment_trim = Some(live);
        }
        let next = self.pacing().plan(
            &mut self.pacing_state,
            pacing::CollectionOutcome {
                full,
                live,
                live_weight,
                previous_live_weight: self.live_weight_after_previous,
                live_after_full: self.live_after_full,
                live_weight_after_full: self.live_weight_after_full,
                allocated,
                since_full: self.allocations_since_full,
            },
        );
        #[cfg(all(feature = "gc-tuning", feature = "profile-gc-time"))]
        if full && std::env::var_os("QUENCH_GC_TRACE").is_some() {
            // Diagnostic recount of the payload ledger after a full sweep.
            let mut exact = 0;
            self.for_each_live_cell(|cell| exact += cell.payload_bytes());
            self.array_records.for_each_live(|_, array| exact += array_payload_bytes(array));
            eprintln!("gc payload ledger={} exact={exact}", self.payload_bytes);
        }
        #[cfg(feature = "gc-tuning")]
        if std::env::var_os("QUENCH_GC_TRACE").is_some() {
            eprintln!("gc full={full} live={live} prev={} after_full={} alloc={} since_full={} next={next:?} state={:?}", self.live_weight_after_previous, self.live_after_full, allocated, self.allocations_since_full, self.pacing_state);
        }
        self.live_weight_after_previous = live_weight;
        self.threshold = next.threshold;
        self.full_due = next.full;
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
    fn sweep_full_parallel(&mut self, pool: &ThreadPool, compacts: bool) -> SweepOutput {
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
                        .map(|slab| slab.sweep(marks, compacts))
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
                    live_payload_bytes,
                    abandoned_contexts,
                    swept_slots,
                } = result;
                self.abandoned_contexts.append(abandoned_contexts);
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
                output.live_payload_bytes += live_payload_bytes;
                output.property_owners.extend(property_owners);
                output.freed_cells += freed_cells;
                output.swept_slots += swept_slots;
            }
        }
        debug_assert_eq!(output.swept_slots, slot_len);
        output
    }

    fn sweep_serial(&mut self, full: bool, compacts: bool) -> SweepOutput {
        let mut live = 0;
        let mut live_payload_bytes = 0;
        let mut freed_cells = 0;
        let mut swept_slots = 0;
        let properties = &mut self.properties;
        let mut property_owners_for_sweep = Vec::with_capacity(if compacts {
            self.property_owner_counts.0
        } else {
            0
        });
        {
            let external_bytes = &mut self.external_bytes;
            let payload_bytes = &mut self.payload_bytes;
            let abandoned_contexts = &mut self.abandoned_contexts;
            let free_by_slab = &mut self.free_by_slab;
            let free_slabs = &mut self.free_slabs;
            let free_slot_count = &mut self.free_slots;
            let sparse_arrays = &mut self.sparse_arrays;
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            let mut reclaim = |properties: &mut ValueArena, index: usize, cell: Cell| {
                *external_bytes = external_bytes.saturating_sub(cell.external_bytes());
                *payload_bytes = payload_bytes.saturating_sub(cell.payload_bytes());
                if let Cell::Promise(promise) = &cell {
                    abandoned_contexts.abandon(&promise.record);
                }
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
                            live_payload_bytes += slot.cell.as_ref().map_or(0, Cell::payload_bytes);
                            if let Some(object) = slot.cell.as_ref().and_then(Cell::object) {
                                if compacts && properties.has_compact_range(object.properties()) {
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
                        // Its mark bit is already clear: the test above found it so.
                        reclaim(properties, index_usize, cell);
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
            live_payload_bytes,
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
        environments: &mut EnvironmentTrace<'_>,
    ) {
        if self.has_direct_records() {
            self.mark_work(work, object_roots, ephemerons, trace_kind, environments);
            return;
        }
        if self.slots.len() < PARALLEL_MARK_MINIMUM_SLOTS {
            self.mark_work(work, object_roots, ephemerons, trace_kind, environments);
            return;
        }
        // Without function records, closures are boxed cells (`Cell::Function`), whose
        // environments the parallel trace marks whole.
        let Some(pool) = parallel_mark_pool() else {
            self.mark_work(work, object_roots, ephemerons, trace_kind, environments);
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
                        let mark_bits = mark_bits!(self);
                        ephemerons.newly_marked(
                            owner,
                            Some(cell),
                            |target| mark_bits.is_marked(target),
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
                |kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>| {
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
    fn snapshot_record_marks(&self) -> Vec<Vec<u64>> {
        self.record_families()
            .iter()
            .map(|(family, _)| family.snapshot_marks())
            .collect()
    }

    #[cfg(debug_assertions)]
    fn restore_record_marks(&mut self, marks: &[Vec<u64>]) {
        for ((family, _), marks) in self.record_families_mut().into_iter().zip(marks) {
            family.restore_marks(marks);
        }
    }

    #[cfg(debug_assertions)]
    fn debug_verify_full(
        &mut self,
        roots: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        closure_reads: Option<&dyn ClosureReadSource>,
    ) {
        let parallel_marks = Self::snapshot_marks(&self.marks);
        let parallel_record_marks = self.snapshot_record_marks();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
        }
        let mut work = roots.to_vec();
        let mut ephemerons = weak::EphemeronWork::default();
        self.mark_work(
            &mut work,
            object_roots,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(closure_reads),
        );
        let sequential_marks = Self::snapshot_marks(&self.marks);
        let sequential_record_marks = self.snapshot_record_marks();
        assert_eq!(
            (sequential_marks.as_slice(), &sequential_record_marks),
            (parallel_marks.as_slice(), &parallel_record_marks),
            "full mark differs from the single-threaded verifier"
        );
        Self::restore_marks(&self.marks, &parallel_marks);
        self.restore_record_marks(&parallel_record_marks);
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
        old_records: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    ) {
        let minor_marks = Self::snapshot_marks(&self.marks);
        let minor_record_marks = self.snapshot_record_marks();
        let young = self.young.clone();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
        }
        let pinned_records: rustc_hash::FxHashSet<Value> = old_records.iter().copied().collect();
        let mut young_slots = vec![false; self.slots.len()];
        for index in young.iter().copied() {
            young_slots[index as usize] = true;
        }
        let mut work = roots.to_vec();
        work.extend(self.slots.iter().enumerate().filter_map(|(index, slot)| {
            (slot.cell.is_some() && !young_slots[index] && Self::marked_words(&minor_marks, index))
                .then_some(Value::heap(index as u32))
        }));
        work.extend(old_records.iter().copied());
        let mut ephemerons = weak::EphemeronWork::default();
        self.collection_state = CollectionState::FullMarking;
        self.mark_work(
            &mut work,
            object_roots,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(None),
        );
        let verified_record_marks = self.snapshot_record_marks();
        let marked = |marks: &[u64], id: RecordId| {
            marks[id as usize / u64::BITS as usize] & (1_u64 << (id as usize % u64::BITS as usize)) != 0
        };
        for (((family, handle), minor), verified) in self
            .record_families()
            .into_iter()
            .zip(&minor_record_marks)
            .zip(&verified_record_marks)
        {
            family.for_each_live_id(&mut |id| {
                let identity = handle(id);
                assert!(
                    pinned_records.contains(&identity) || marked(minor, id) == marked(verified, id),
                    "minor collection's young records differ from a full mark with old records pinned: {identity:?}"
                );
            });
        }
        self.restore_record_marks(&minor_record_marks);
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
        environments: &mut EnvironmentTrace<'_>,
    ) {
        let mut last_proto = Value::UNDEFINED;
        loop {
            if work.is_empty() {
                if !environments.has_pending() {
                    break;
                }
                self.trace_environment_edges(environments, work);
                continue;
            }
            let value = work.pop().expect("work is not empty");
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
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(
                    value,
                    None,
                    |target| mark_bits.is_marked(target),
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
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(
                    value,
                    None,
                    |target| mark_bits.is_marked(target),
                    work,
                );
            } else if let Some(record_id) = value.direct_function_index() {
                if !self.function_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Function as usize] += 1;
                }
                // SAFETY: a direct function Value names an initialized record.
                let function = unsafe { self.function_records.get_live(record_id) };
                Self::function_children(
                    value,
                    function,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                environments.closure(function.env, &function.kind, work);
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), work);
            } else if let Some(record_id) = value.direct_iterator_index() {
                if !self.iterator_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Iterator as usize] += 1;
                }
                // SAFETY: a direct iterator Value names an initialized record.
                let iterator = unsafe { self.iterator_records.get_live(record_id) };
                Self::object_children(
                    value,
                    &iterator.object,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                Self::iterator_edges(iterator, work);
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), work);
            } else if let Some(record_id) = value.direct_string_index() {
                // A string holds no heap edges: marking it is the whole visit.
                if self.string_records.mark(record_id) {
                    #[cfg(feature = "profile-aggregate")]
                    {
                        self.gc_profile.marked += 1;
                        self.gc_profile.marked_kinds[CellKind::String as usize] += 1;
                    }
                }
            } else if let Some(record_id) = value.direct_v128_index() {
                // A vector holds no heap edges: marking it is the whole visit.
                if self.v128_records.mark(record_id) {
                    #[cfg(feature = "profile-aggregate")]
                    {
                        self.gc_profile.marked += 1;
                        self.gc_profile.marked_kinds[CellKind::WasmV128 as usize] += 1;
                    }
                }
            } else if let Some(record_id) = value.direct_await_index() {
                if !self.await_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::AwaitState as usize] += 1;
                }
                // SAFETY: a direct await Value names an initialized record.
                unsafe { self.await_records.get_live(record_id) }.extend_roots(work);
            } else if let Some(record_id) = value.direct_promise_index() {
                if !self.promise_records.mark(record_id) {
                    continue;
                }
                #[cfg(feature = "profile-aggregate")]
                {
                    self.gc_profile.marked += 1;
                    self.gc_profile.marked_kinds[CellKind::Object as usize] += 1;
                }
                // SAFETY: a direct promise Value names an initialized record.
                let promise = unsafe { self.promise_records.get_live(record_id) };
                Self::object_children(
                    value,
                    &promise.object,
                    &self.properties,
                    work,
                    object_roots,
                    trace_kind,
                    &mut last_proto,
                );
                promise.record.append_roots(work);
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(value, None, |target| mark_bits.is_marked(target), work);
            } else {
                let Some(index) = value.legacy_heap_index().map(|value| value as usize) else {
                    continue;
                };
                let Some(slot) = self.slots.get_mut(index) else {
                    continue;
                };
                if slot.cell.is_none() {
                    continue;
                }
                // A full collection traces environments by what their readers read.
                if let Some(Cell::Environment { function, .. }) = &slot.cell
                    && environments.whole(value, index as u32, *function)
                {
                    continue;
                }
                if !Self::mark_serial(&self.marks, index) {
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
                let mark_bits = mark_bits!(self);
                ephemerons.newly_marked(
                    value,
                    Some(cell),
                    |target| mark_bits.is_marked(target),
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
    pub(crate) fn record_vm_gc_phase(&mut self, phase: usize, started: std::time::Instant) {
        self.gc_profile.vm_phase_nanos[phase] += started.elapsed().as_nanos() as u64;
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
    /// The write barrier for an edge stored outside `owner`'s own storage (sparse elements,
    /// owner-attached VM tables): `owner` is logged as if it had been written.
    pub(crate) fn write_barrier(&mut self, owner: Value) {
        match owner.heap_reference() {
            Some(HeapReference::ObjectRecord(record_id)) => self.object_records.remember(record_id),
            Some(HeapReference::ArrayRecord(record_id)) => self.array_records.remember(record_id),
            Some(HeapReference::FunctionRecord(record_id)) => {
                self.function_records.remember(record_id)
            }
            Some(HeapReference::PromiseRecord(record_id)) => self.promise_records.remember(record_id),
            Some(HeapReference::AwaitRecord(record_id)) => self.await_records.remember(record_id),
            Some(HeapReference::IteratorRecord(record_id)) => {
                self.iterator_records.remember(record_id)
            }
            Some(HeapReference::LegacySlot(index)) => self.remember(index as usize),
            // Strings and vectors are immutable and hold no edges, so nothing is ever stored
            // into them.
            Some(HeapReference::StringRecord(_) | HeapReference::V128Record(_)) | None => {}
        }
    }
    pub(crate) fn sparse_set(&mut self, array: Value, index: usize, value: Value) {
        self.write_barrier(array);
        let arrays = self
            .sparse_arrays
            .get_or_insert_with(|| Box::new(FxHashMap::default()));
        let elements = arrays.entry(array).or_default();
        elements.values.insert(index, value);
        elements.length = elements.length.max(index.saturating_add(1));
        let (length, sparse_count) = (elements.length, elements.values.len());
        // Dense storage of the whole length takes no more words than the elements hold. Holes
        // in the dense prefix are not elements; counting them would promote an array whose
        // writes only grow its length, doubling its storage at every far write.
        let fills = |count: usize| dense_storage_fits(count.saturating_add(sparse_count), length);
        let dense = self.array_elements(array);
        let dense_enough = fills(dense.map_or(0, |dense| dense.len()))
            && fills(dense.map_or(0, |dense| dense.iter().filter(|value| !value.is_deleted()).count()));
        if length <= MAX_DENSE_ARRAY_HOLE_LENGTH || dense_enough {
            self.materialize_sparse_array(array, length);
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
        let elements = elements.make_mut();
        elements.resize(length, Value::DELETED);
        for (index, value) in sparse.into_iter().flat_map(|elements| elements.values) {
            if index < length && (index >= dense_length || elements[index].is_deleted()) {
                elements[index] = value;
            }
        }
    }
    pub(crate) fn property_get(&self, object: &Object, slot: usize) -> Option<Value> {
        let value = match object.properties().has_inline_property_storage() {
            true if slot < INLINE_PROPERTY_COUNT => object.inline_property(slot),
            true => None,
            false => self.properties.get(object.properties(), slot),
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
        if let Some(value) = object.inline_property(slot) {
            Some(value)
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_shaped(object.properties(), slot) }
        }
    }
    /// `property_get_unchecked` of an object outside dictionary storage, as an exact shape
    /// match proves (`Object::exact_shape`).
    ///
    /// # Safety
    /// The slot exists and the object is not in dictionary storage.
    #[inline(always)]
    pub(crate) unsafe fn property_get_flat(&self, object: &Object, slot: usize) -> Value {
        if let Some(value) = object.inline_property(slot) {
            value
        } else {
            // SAFETY: the caller proves the slot exists outside dictionary storage.
            unsafe { self.properties.get_flat(object.properties(), slot) }
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn property_get_unchecked(&self, object: &Object, slot: usize) -> Value {
        if let Some(value) = object.inline_property(slot) {
            value
        } else {
            // SAFETY: the caller proves that the property slot exists.
            unsafe { self.properties.get_unchecked(object.properties(), slot) }
        }
    }
    pub(crate) fn property_set(&mut self, object: Value, slot: usize, value: Value) {
        let vector = {
            let data = self
                .object_store_mut(object, value)
                .expect("property owner is an object");
            let vector = data.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if data.store_inline_narrow(slot, value) {
                return;
            } else {
                None
            }
        };
        match vector {
            Some(vector) => self.properties.set(vector, slot, value),
            None => self.widen_inline_property(object, slot, value),
        }
    }

    /// Stores `value` at inline `slot` of `object`, whose payload word cannot hold it: its
    /// values move to the arena. The caller has applied the write barrier.
    #[cold]
    fn widen_inline_property(&mut self, object: Value, slot: usize, value: Value) {
        let (data, properties) = self
            .object_and_arena_mut(object)
            .expect("property owner is an object");
        let length = properties.len(data.properties());
        widen_inline_storage(data, properties, data.shape(), length, slot, value);
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
                .object_store_mut(object, value)
                .expect("property owner is an object");
            let vector = data.properties();
            if !vector.has_inline_property_storage() {
                Some(vector)
            } else if data.store_inline_narrow(slot, value) {
                return;
            } else {
                None
            }
        };
        match vector {
            // SAFETY: the caller proves that the property slot exists.
            Some(vector) => unsafe { self.properties.set_unchecked(vector, slot, value) },
            None => self.widen_inline_property(object, slot, value),
        }
    }
    /// `property_set_unchecked` for the direct object `record_id` names, decoded once: its
    /// values are inline or a flat arena run. Without `LOGS`, `false` leaves a store the
    /// write barrier logs to the caller.
    ///
    /// # Safety
    /// `record_id` names a live object record outside dictionary storage (its exact shape
    /// matched a recorded one) holding a property at `slot`.
    #[inline(always)]
    pub(crate) unsafe fn record_property_set_flat<const LOGS: bool>(
        &mut self,
        record_id: u32,
        slot: usize,
        value: Value,
    ) -> bool {
        // Without `LOGS`, a store the barrier would log is left to the caller, so this one
        // makes no call; the barrier does nothing for any other.
        if !LOGS && self.object_records.store_logs(record_id, value) {
            return false;
        }
        // SAFETY: the caller proves the record live; `&mut self` is exclusive.
        let data = unsafe {
            if LOGS {
                self.object_records.get_live_mut_storing(record_id, value)
            } else {
                self.object_records.get_live_mut_unbarriered(record_id)
            }
        };
        if data.properties().has_inline_property_storage() {
            if !data.store_inline_narrow(slot, value) {
                let length = self.properties.len(data.properties());
                widen_inline_storage(data, &mut self.properties, data.shape(), length, slot, value);
            }
            return true;
        }
        let vector = data.properties();
        debug_assert!(!vector.is_dictionary(), "the caller matched an exact shape");
        // SAFETY: the caller proves that the property slot exists outside dictionary storage.
        unsafe { self.properties.set_flat_unchecked(vector, slot, value) };
        true
    }
    /// Appends `value` as the next property of `object`, which moves to `shape`, whose
    /// storage length is one more than the object's: inline while the values fit, otherwise
    /// in the arena, into which the add that outgrows the inline values moves them. One owner
    /// lookup, which passes the write barrier only for a stored reference; the arena borrow
    /// is disjoint from the owner.
    #[inline(always)]
    pub(crate) fn property_add(&mut self, object: Value, value: Value, shape: u32) {
        let _slot = if let Some(record_id) = object.direct_object_index() {
            let Self {
                object_records,
                properties,
                ..
            } = &mut *self;
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            let data = unsafe { object_records.get_live_mut_storing(record_id, value) };
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape);
            slot
        } else {
            let (data, properties) = self
                .object_and_arena_mut(object)
                .expect("property owner is an object");
            let slot = properties.len(data.properties());
            append_property(data, properties, slot, value, shape);
            slot
        };
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, _slot + 1);
    }

    /// `property_add` to a direct object's record at `slot`, which the caller proved its
    /// storage length (a cached add's recorded slot), without deriving it again.
    #[inline(always)]
    pub(crate) fn record_property_add_at(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        let Some(record_id) = object.direct_object_index() else {
            return self.property_add(object, value, shape);
        };
        let Self {
            object_records,
            properties,
            ..
        } = &mut *self;
        // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
        let data = unsafe { object_records.get_live_mut_storing(record_id, value) };
        debug_assert_eq!(slot, properties.len(data.properties()));
        append_property(data, properties, slot, value, shape);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(object, slot + 1);
    }

    /// `record_property_add_at` out of line, for an add whose storage moves or whose store
    /// the write barrier logs, so a caller's plain appends keep their code call-free.
    #[inline(never)]
    pub(crate) fn record_property_add_moving(&mut self, object: Value, slot: usize, value: Value, shape: u32) {
        self.record_property_add_at(object, slot, value, shape);
    }

    /// `record_property_add_at` of an add the record admits as a plain append
    /// (`Object::admits_plain_append`): the store into storage that does not move. A store
    /// the write barrier would log is left to the caller (`false`, nothing changed), as for
    /// `record_property_set_flat`, so this one makes no call.
    #[inline(always)]
    pub(crate) fn record_property_append_plain(&mut self, record_id: u32, slot: usize, value: Value, shape: u32) -> bool {
        if self.object_records.store_logs(record_id, value) {
            return false;
        }
        let Self {
            object_records,
            properties,
            ..
        } = &mut *self;
        // SAFETY: the caller read the record from a direct-object Value; `&mut self` is
        // exclusive, and the barrier logs nothing for this store.
        let data = unsafe { object_records.get_live_mut_unbarriered(record_id) };
        data.append_plain(properties, slot, value, shape);
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(Value::direct_object(record_id), slot + 1);
        true
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
            function_records,
            promise_records,
            iterator_records,
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
        if let Some(record_id) = identity.direct_function_index() {
            // SAFETY: profile runs immediately after inserting this function.
            let function = unsafe { function_records.get_live(record_id) };
            let slots = properties.len(function.object.properties());
            memory_profile.allocated_function(identity, function, slots);
            return;
        }
        if let Some(record_id) = identity.direct_promise_index() {
            // SAFETY: profile runs immediately after inserting this promise.
            let promise = unsafe { promise_records.get_live(record_id) };
            let slots = properties.len(promise.object.properties());
            memory_profile.allocated_promise(identity, promise, slots);
            return;
        }
        if let Some(record_id) = identity.direct_iterator_index() {
            // SAFETY: profile runs immediately after inserting this iterator.
            let iterator = unsafe { iterator_records.get_live(record_id) };
            let slots = properties.len(iterator.object.properties());
            memory_profile.allocated_iterator(identity, iterator, slots);
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
        object_roots(trace_kind, owner, object.shape(), object.was_used_as_prototype(), work);
        object.visit_extras_roots(|value| work.push(value));
        if let Some(values) = object.inline_properties() {
            for value in values {
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
            Cell::Iterator(iterator) => {
                object(&iterator.object);
                Self::iterator_edges(iterator, work);
            }
            Cell::AwaitState(state) => state.extend_roots(work),
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
            Cell::Function(function) => {
                object(&function.object);
                work.extend([function.env, function.realm]);
            }
            Cell::WasmGlobal { header, value: global, .. } => {
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
                let (dynamic_bindings, with_objects) = (scope.dynamic_bindings(), scope.with_objects());
                match dynamic_bindings {
                    EnvironmentBindings::Owned(bindings) => {
                        work.extend(bindings.iter().map(|(_, value)| *value));
                    }
                    EnvironmentBindings::Shared(owner) => work.push(*owner),
                }
                work.extend(with_objects.iter().copied());
            }
            Cell::WasmElements(elements) => work.extend(elements.iter().copied()),
            Cell::WasmTable { object: value, elements, .. } => {
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
            Cell::Promise(promise) => {
                object(&promise.object);
                promise.record.append_roots(work);
            }
            Cell::Date { object: value, .. }
            | Cell::TemporalDuration { object: value, .. }
            | Cell::TemporalPlainDate { object: value, .. }
            | Cell::TemporalPlainDateTime { object: value, .. }
            | Cell::TemporalPlainMonthDay { object: value, .. }
            | Cell::TemporalPlainYearMonth { object: value, .. }
            | Cell::TemporalZonedDateTime { object: value, .. }
            | Cell::TemporalInstant { object: value, .. } => object(value),
            Cell::BigInt(_)
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
            Cell::WasmHostFunction {
                target: crate::heap::WasmHostTarget::JsEmscriptenMemcpy(target),
                ..
            } => {
                work.push(target.function);
                work.push(target.view_environment);
            }
            Cell::WasmObject { object: value, slot } => {
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
    /// An iterator's edges besides its object: its source, cached next method, helper
    /// state and suspended generator.
    fn iterator_edges(iterator: &IteratorRecord, work: &mut Vec<Value>) {
        let IteratorExt {
            next_method,
            helper,
            generator,
            ..
        } = &iterator.ext;
        work.push(iterator.source);
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
            record.extend_roots(work);
        }
    }
    /// A function's edges: its object, closure environment and realm.
    fn function_children(
        owner: Value,
        function: &FunctionRecord,
        properties: &ValueArena,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        trace_kind: GcTraceKind,
        last_proto: &mut Value,
    ) {
        Self::object_children(
            owner,
            &function.object,
            properties,
            work,
            object_roots,
            trace_kind,
            last_proto,
        );
        // The environment edge is the caller's (`EnvironmentTrace::closure`).
        work.push(function.realm);
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
            | Cell::PromiseResolvingState { .. }
            | Cell::Promise(_) => CellKind::Object,
            Cell::Array { .. } => CellKind::Array,
            Cell::Map { .. } => CellKind::Map,
            Cell::Set { .. } => CellKind::Set,
            Cell::Iterator { .. } => CellKind::Iterator,
            Cell::WeakMap { .. } => CellKind::WeakMap,
            Cell::WeakSet { .. } => CellKind::WeakSet,
            Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. } => CellKind::WeakRef,
            Cell::Function(_) => CellKind::Function,
            Cell::Environment { .. } | Cell::BindingReference { .. } => CellKind::Environment,
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
            Cell::AwaitState(_) => CellKind::AwaitState,
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
                | Cell::AwaitState(_)
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
                Cell::Promise(promise) => {
                    size_of::<PromiseCell>()
                        + promise.record.reaction_heap_bytes()
                }
                Cell::WasmException { payload, .. } => payload.capacity() * size_of::<Value>(),
                Cell::WasmHostFunction { target, signature } => {
                    size_of::<crate::WasmSignature>()
                        + (signature.params.capacity() + signature.results.capacity())
                            * size_of::<crate::WasmType>()
                        + if matches!(target, crate::heap::WasmHostTarget::JsEmscriptenMemcpy(_)) {
                            size_of::<crate::heap::WasmHostMemcpyTarget>()
                        } else {
                            0
                        }
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
                Cell::RegExp { meta, .. } => meta.source.capacity(),
                Cell::Array { object } => object.elements().capacity() * size_of::<Value>(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::WasmMemory { bytes, .. } => bytes.capacity(),
                Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
                Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
                Cell::WeakSet { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::Function(_) => size_of::<FunctionRecord>(),
                Cell::Environment { slots, scope, .. } => {
                    let with_objects = scope.with_objects();
                    slots.len() * size_of::<EnvironmentSlot>()
                        + with_objects.len() * size_of::<Value>()
                }
                Cell::BigInt(value) => crate::bigint::magnitude_bytes(value),
                Cell::Error(value) => value.capacity(),
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

/// Moves `data`'s inline values into an arena range of `shape`, `length` values long, with
/// `value` at `slot`: the store its payload word cannot hold (`Object::store_inline_narrow`).
fn widen_inline_storage(
    data: &mut Object,
    properties: &mut ValueArena,
    shape: u32,
    length: usize,
    slot: usize,
    value: Value,
) {
    let mut values = data.inline_properties().expect("inline storage");
    values[slot] = value;
    let moved = properties.with_values(shape, &values[..length]);
    let previous = data.replace_property_storage(moved, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
    properties.release(previous);
}

/// `Heap::property_add` on the owner's storage: writes `value` at `slot`, the owner's
/// storage length, and moves the owner to `shape`.
#[inline(always)]
fn append_property(data: &mut Object, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
    let mut vector = data.properties();
    if !vector.has_inline_property_storage() {
        let before = vector.into_parts();
        properties.push(&mut vector, value);
        // An append within its range leaves the storage as it was; `set_shape` below
        // records the grown shape.
        if vector.into_parts() != before {
            data.set_properties(vector);
        }
    } else if slot < INLINE_PROPERTY_COUNT {
        if !data.store_inline_narrow(slot, value) {
            widen_inline_storage(data, properties, shape, slot + 1, slot, value);
        }
    } else {
        debug_assert_eq!(slot, INLINE_PROPERTY_COUNT);
        // One arena range sized for the grown shape, not one for the inline values that the
        // add then outgrows at once.
        let inline = data.inline_properties().expect("inline storage");
        let mut grown = [value; INLINE_PROPERTY_COUNT + 1];
        grown[..INLINE_PROPERTY_COUNT].copy_from_slice(&inline);
        let moved = properties.with_values(shape, &grown);
        data.replace_property_storage(moved, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
    }
    data.set_shape(shape);
}
