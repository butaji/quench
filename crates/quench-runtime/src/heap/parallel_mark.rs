//! A collection's mark on the parallel pool (`Heap::mark_work` with a `ParallelMark`).
//!
//! A round hands the mark stack to workers, which mark legacy slots and every record family
//! through atomic views of the mark words the serial mark sets (`ParallelRecordMarks`) and
//! push the values they reach. Workers only read the heap and set mark bits. What needs the
//! collector's mutable state they record in their batch instead, and the collector applies it
//! once the round joins (`Heap::finish_parallel_batch`, and `Heap::mark_work` for
//! environments):
//!
//! - a marked function's closure environment edge, while the trace reads closures
//!   (`EnvironmentTrace::closure`); the trace then follows it serially, as the serial mark
//!   does, so environments keep only the slots their readers read;
//! - an environment reached whole while the trace reads closures, which the serial visit
//!   marks (`Heap::mark_value`, through `EnvironmentTrace::whole`);
//! - a newly marked weak map, and a newly marked key some weak map value waits for
//!   (`weak::EphemeronWork::newly_marked`);
//! - an object's shape roots, unless the VM supplies a shareable trace.
//!
//! The values these push become the next round's work, until no work and no environment edge
//! is left: the serial mark's ending.

use super::records::{ParallelRecordMarks, RecordId};
use super::{
    ArrayObject, AwaitState, Cell, CellKind, EnvironmentTrace, FunctionRecord, GcTraceKind, Heap,
    IteratorRecord, MarkBits, Object, ParallelObjectRootTrace, PromiseCell, SlotArena, SparseElements,
    PARALLEL_MARK_LARGE_HOST_DEFAULT_THREADS, PARALLEL_MARK_MAX_THREADS, PARALLEL_MARK_SMALL_HOST_DEFAULT_THREADS,
    PARALLEL_MARK_SMALL_HOST_MAX_CPUS, PARALLEL_MARK_SPLIT_THRESHOLD, PARALLEL_MARK_STACK_SIZE_BYTES, mark_bits,
    mark_word_if_new, weak,
};
use crate::value::{HeapReference, Value};
use crate::value_vec::ValueArena;
use crate::vm::wtf16::JsString;
use rayon::{Scope, ThreadPool, ThreadPoolBuilder};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// What a collection marks in parallel with (`Heap::parallel_mark`).
#[derive(Clone, Copy)]
pub(super) struct ParallelMark<'a> {
    pub(super) pool: &'static ThreadPool,
    /// The VM's shareable shape-root trace, without which workers defer shape roots to the
    /// collector.
    pub(super) object_roots: Option<&'a ParallelObjectRootTrace<'a>>,
    /// The most work the collector marks itself rather than start a round: work a worker
    /// would not split (`PARALLEL_MARK_SPLIT_THRESHOLD`) would keep the round on one thread,
    /// where the collector marks it without dispatch and with nothing left to apply after.
    pub(super) round_minimum: usize,
}

/// The heap as workers see it while the mutator is stopped for one parallel round. `Cell`
/// and records contain `Rc` and other `!Sync` payloads, so workers only borrow them and walk
/// immutable edges: they may not clone or drop an `Rc`, mutate a cell or record, or retain
/// any reference past the joined Rayon scope. Their only writes are mark bits, through
/// atomics: legacy slots' (`marks`) and each record family's (`ParallelRecordMarks`).
#[derive(Clone, Copy)]
struct ParallelMarkView<'heap> {
    slots: &'heap SlotArena,
    properties: &'heap ValueArena,
    sparse_arrays: Option<&'heap FxHashMap<Value, SparseElements>>,
    marks: &'heap [AtomicU64],
    objects: ParallelRecordMarks<'heap, Object>,
    arrays: ParallelRecordMarks<'heap, ArrayObject>,
    functions: ParallelRecordMarks<'heap, FunctionRecord>,
    promises: ParallelRecordMarks<'heap, PromiseCell>,
    awaits: ParallelRecordMarks<'heap, AwaitState>,
    iterators: ParallelRecordMarks<'heap, IteratorRecord>,
    strings: ParallelRecordMarks<'heap, JsString>,
    v128s: ParallelRecordMarks<'heap, [u8; crate::wasm::V128_BYTES]>,
    /// The keys weak map values wait for, which change only between rounds.
    ephemerons: &'heap weak::EphemeronWork,
    /// `EnvironmentTrace::reads_closures` for this collection.
    reads_closures: bool,
    /// `EnvironmentTrace::projection_function` for this collection.
    projection_function: Option<u32>,
}

// SAFETY: the view is built from the collector's exclusive borrow of the heap and lives only
// within one stop-the-world round. Slots, records, the property arena and sparse arrays are
// not mutated until all Rayon work joins; workers only read them, without touching reference
// counts. The only shared writes are atomic mark bits.
unsafe impl Send for ParallelMarkView<'_> {}
// SAFETY: same stopped-mutator and read-only-payload invariant as above.
unsafe impl Sync for ParallelMarkView<'_> {}

/// What one worker marked and what it left for the collector (see the module documentation).
struct ParallelMarkBatch {
    /// Shape roots, as `(owner, shape, prototype)`, without a shareable trace.
    object_roots: Vec<(Value, u32, bool)>,
    /// Function records whose closure environment edges the trace follows.
    closures: Vec<RecordId>,
    /// Environments reached whole, for the collector's serial visit.
    environments: Vec<Value>,
    /// Newly marked weak maps and awaited keys.
    ephemeron_owners: Vec<Value>,
    #[cfg(feature = "profile-aggregate")]
    work_items: u64,
    #[cfg(feature = "profile-aggregate")]
    max_worklist: usize,
    #[cfg(feature = "profile-aggregate")]
    marked_kinds: [u64; CellKind::COUNT],
}

impl Default for ParallelMarkBatch {
    // Arrays longer than 32 have no derived `Default`.
    fn default() -> Self {
        Self {
            object_roots: Vec::new(),
            closures: Vec::new(),
            environments: Vec::new(),
            ephemeron_owners: Vec::new(),
            #[cfg(feature = "profile-aggregate")]
            work_items: 0,
            #[cfg(feature = "profile-aggregate")]
            max_worklist: 0,
            #[cfg(feature = "profile-aggregate")]
            marked_kinds: [0; CellKind::COUNT],
        }
    }
}

impl ParallelMarkBatch {
    /// Counts a value of `kind` newly marked, as the serial mark counts it.
    #[inline(always)]
    fn count_marked(&mut self, _kind: CellKind) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.marked_kinds[_kind as usize] += 1;
        }
    }
}

/// The process's mark pool, when it has workers: the collector thread marks too, so the
/// pool has one thread fewer than the markers. Unit tests mark with every marker the pool
/// may have on any host, so they cover the parallel path wherever they run.
pub(super) fn parallel_mark_pool() -> Option<&'static ThreadPool> {
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
        let total_threads = if cfg!(test) {
            PARALLEL_MARK_MAX_THREADS
        } else {
            available.min(requested)
        };
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

impl<'heap> ParallelMarkView<'heap> {
    fn new(
        heap: &'heap mut Heap,
        ephemerons: &'heap weak::EphemeronWork,
        reads_closures: bool,
        projection_function: Option<u32>,
    ) -> Self {
        Self {
            slots: &heap.slots,
            properties: &heap.properties,
            sparse_arrays: heap.sparse_arrays.as_deref(),
            marks: &heap.marks,
            objects: heap.object_records.parallel_marks(),
            arrays: heap.array_records.parallel_marks(),
            functions: heap.function_records.parallel_marks(),
            promises: heap.promise_records.parallel_marks(),
            awaits: heap.await_records.parallel_marks(),
            iterators: heap.iterator_records.parallel_marks(),
            strings: heap.string_records.parallel_marks(),
            v128s: heap.v128_records.parallel_marks(),
            ephemerons,
            reads_closures,
            projection_function,
        }
    }

    fn cell(&self, index: usize) -> Option<&'heap Cell> {
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the bound proves the flat slot index is initialized, and no slot is
        // mutated or reclaimed until the round joins.
        unsafe { self.slots.get_unchecked(index).cell.as_ref() }
    }

    fn mark_slot_if_new(&self, index: usize) -> bool {
        // The mark words cover every allocated slot.
        let word = &self.marks[index / u64::BITS as usize];
        mark_word_if_new(word, 1_u64 << (index % u64::BITS as usize))
    }

    /// Pushes the elements a sparse `array` keeps beside its dense part.
    fn sparse_elements(&self, array: Value, work: &mut Vec<Value>) {
        if let Some(elements) = self.sparse_arrays.and_then(|arrays| arrays.get(&array)) {
            work.extend(elements.values.values().copied());
        }
    }

    /// Leaves newly marked `value` for `EphemeronWork::newly_marked` when some weak map
    /// value waits for it. Pending keys change only between rounds, when the collector scans
    /// the weak maps marked in a round: a key marked in the same round is marked by then and
    /// releases its value at the scan, and any other was pending when its round began.
    fn note_marked_key(&self, value: Value, batch: &mut ParallelMarkBatch) {
        if self.ephemerons.awaits_key(value) {
            batch.ephemeron_owners.push(value);
        }
    }

    /// `Heap::mark_value` on a worker: marks `value` if new and pushes what it keeps alive,
    /// leaving to `batch` what the collector must apply (see the module documentation).
    #[inline(always)]
    fn mark_value(
        &self,
        value: Value,
        work: &mut Vec<Value>,
        batch: &mut ParallelMarkBatch,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        trace_kind: GcTraceKind,
        last_proto: &mut Value,
    ) {
        let deferred_roots = &mut batch.object_roots;
        let mut object_roots = |kind: GcTraceKind, owner: Value, shape: u32, prototype: bool, roots: &mut Vec<Value>| {
            match parallel_object_roots {
                Some(trace) => trace(kind, owner, shape, prototype, roots),
                None => deferred_roots.push((owner, shape, prototype)),
            }
        };
        let properties = self.properties;
        let Some(reference) = value.heap_reference() else {
            return;
        };
        let kind = match reference {
            HeapReference::ObjectRecord(id) => {
                if !self.objects.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                let object = unsafe { self.objects.get(id) };
                Heap::object_children(value, object, properties, work, &mut object_roots, trace_kind, last_proto);
                self.note_marked_key(value, batch);
                CellKind::Object
            }
            HeapReference::ArrayRecord(id) => {
                if !self.arrays.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                let array = unsafe { self.arrays.get(id) };
                let strings = self.strings;
                Heap::array_children(
                    value,
                    array,
                    properties,
                    |string| {
                        strings.mark_if_new(string);
                    },
                    work,
                    &mut object_roots,
                    trace_kind,
                    last_proto,
                );
                self.sparse_elements(value, work);
                self.note_marked_key(value, batch);
                CellKind::Array
            }
            HeapReference::FunctionRecord(id) => {
                if !self.functions.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                let function = unsafe { self.functions.get(id) };
                Heap::function_children(value, function, properties, work, &mut object_roots, trace_kind, last_proto);
                if self.reads_closures {
                    batch.closures.push(id);
                } else {
                    work.push(function.env);
                }
                self.note_marked_key(value, batch);
                CellKind::Function
            }
            HeapReference::IteratorRecord(id) => {
                if !self.iterators.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                let iterator = unsafe { self.iterators.get(id) };
                Heap::object_children(value, &iterator.object, properties, work, &mut object_roots, trace_kind, last_proto);
                Heap::iterator_edges(iterator, work);
                self.note_marked_key(value, batch);
                CellKind::Iterator
            }
            HeapReference::PromiseRecord(id) => {
                if !self.promises.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                let promise = unsafe { self.promises.get(id) };
                Heap::object_children(value, &promise.object, properties, work, &mut object_roots, trace_kind, last_proto);
                promise.record.append_roots(work);
                self.note_marked_key(value, batch);
                CellKind::Object
            }
            HeapReference::AwaitRecord(id) => {
                if !self.awaits.mark_if_new(id) {
                    return;
                }
                // SAFETY: this view just marked the occupied record.
                unsafe { self.awaits.get(id) }.extend_roots(work);
                CellKind::AwaitState
            }
            // Strings and vectors hold no heap edges: marking one is the whole visit.
            HeapReference::StringRecord(id) => {
                if !self.strings.mark_if_new(id) {
                    return;
                }
                CellKind::String
            }
            HeapReference::V128Record(id) => {
                if !self.v128s.mark_if_new(id) {
                    return;
                }
                CellKind::WasmV128
            }
            HeapReference::LegacySlot(index) => {
                let index = index as usize;
                let Some(cell) = self.cell(index) else {
                    return;
                };
                if let Cell::Environment { function, .. } = cell
                    && (self.reads_closures || self.projection_function == Some(*function))
                {
                    batch.environments.push(value);
                    return;
                }
                if !self.mark_slot_if_new(index) {
                    return;
                }
                Heap::children(value, cell, properties, work, &mut object_roots, trace_kind, last_proto);
                if matches!(cell, Cell::Array { .. }) {
                    self.sparse_elements(value, work);
                }
                if matches!(cell, Cell::WeakMap { .. }) {
                    batch.ephemeron_owners.push(value);
                } else {
                    self.note_marked_key(value, batch);
                }
                Heap::cell_kind(cell)
            }
        };
        batch.count_marked(kind);
    }
}

/// What every marker of one parallel round shares.
#[derive(Clone, Copy)]
struct ParallelRound<'scope> {
    view: ParallelMarkView<'scope>,
    batches: &'scope Mutex<Vec<ParallelMarkBatch>>,
    object_roots: Option<&'scope ParallelObjectRootTrace<'scope>>,
    trace_kind: GcTraceKind,
    /// The stacks split off for the pool that no worker has taken yet.
    waiting: &'scope AtomicUsize,
    /// The pool's workers: a marker splits its stack only while fewer stacks than this wait.
    /// A waiting stack is one an idle worker can take; more would only queue. Splitting at
    /// every crossing of `PARALLEL_MARK_SPLIT_THRESHOLD` spawned a job every few marks on a
    /// wide record graph: bounding it cut a synthetic Throughput run's explicit full
    /// collections from 1514 to 1283 ms with two markers and from 690 to 617 ms with four
    /// (4-vCPU Linux x86_64).
    workers: usize,
}

impl<'scope> ParallelRound<'scope> {
    /// Marks from `work` until it is empty, splitting half of it off for an idle worker when
    /// it grows past `PARALLEL_MARK_SPLIT_THRESHOLD`, and hands its batch to the collector.
    fn visit(self, scope: &Scope<'scope>, mut work: Vec<Value>) {
        let mut batch = ParallelMarkBatch::default();
        #[cfg(feature = "profile-aggregate")]
        {
            batch.max_worklist = work.len();
        }
        let mut last_proto = Value::UNDEFINED;
        while let Some(value) = work.pop() {
            #[cfg(feature = "profile-aggregate")]
            {
                batch.work_items += 1;
            }
            self.view.mark_value(value, &mut work, &mut batch, self.object_roots, self.trace_kind, &mut last_proto);
            #[cfg(feature = "profile-aggregate")]
            {
                batch.max_worklist = batch.max_worklist.max(work.len());
            }
            if work.len() > PARALLEL_MARK_SPLIT_THRESHOLD && self.waiting.load(Ordering::Relaxed) < self.workers {
                self.waiting.fetch_add(1, Ordering::Relaxed);
                let spawned = work.split_off(work.len() / 2);
                scope.spawn(move |scope| {
                    self.waiting.fetch_sub(1, Ordering::Relaxed);
                    self.visit(scope, spawned);
                });
            }
        }
        self.batches
            .lock()
            .expect("parallel GC mark output mutex poisoned")
            .push(batch);
    }
}

impl Heap {
    /// One parallel round: workers mark from `work`, which this takes, and the collector then
    /// applies what they left (`finish_parallel_batch`), pushing the next round's work and the
    /// environments for its own visit (`left_to_collector`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mark_parallel_round(
        &mut self,
        parallel: ParallelMark<'_>,
        work: &mut Vec<Value>,
        left_to_collector: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
    ) {
        let initial_work = std::mem::take(work);
        let batches = {
            let batches = Mutex::new(Vec::new());
            let waiting = AtomicUsize::new(0);
            let round = ParallelRound {
                view: ParallelMarkView::new(
                    self,
                    ephemerons,
                    environments.reads_closures(),
                    environments.projection_function(),
                ),
                batches: &batches,
                object_roots: parallel.object_roots,
                trace_kind,
                waiting: &waiting,
                workers: parallel.pool.current_num_threads(),
            };
            // The collector marks the round's first stack itself, then waits for the workers.
            parallel.pool.in_place_scope(|scope| round.visit(scope, initial_work));
            batches.into_inner().expect("parallel GC mark output mutex poisoned")
        };
        for batch in batches {
            left_to_collector.extend_from_slice(&batch.environments);
            self.finish_parallel_batch(batch, work, object_roots, ephemerons, trace_kind, environments);
        }
    }

    /// Applies what a worker left for the collector (see the module documentation) but the
    /// environments, pushing the values it reaches onto `work`.
    fn finish_parallel_batch(
        &mut self,
        batch: ParallelMarkBatch,
        work: &mut Vec<Value>,
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        ephemerons: &mut weak::EphemeronWork,
        trace_kind: GcTraceKind,
        environments: &mut EnvironmentTrace<'_>,
    ) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.work_items += batch.work_items;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(batch.max_worklist as u64);
            for (kind, count) in batch.marked_kinds.iter().copied().enumerate() {
                self.gc_profile.marked += count;
                self.gc_profile.marked_kinds[kind] += count;
            }
        }
        for (owner, shape, prototype) in batch.object_roots {
            object_roots(trace_kind, owner, shape, prototype, work);
        }
        for id in batch.closures {
            // SAFETY: a worker marked this occupied function record in the round just joined.
            let function = unsafe { self.function_records.get_live(id) };
            environments.closure(function.env, &function.kind, work);
        }
        for owner in batch.ephemeron_owners {
            let cell = owner.legacy_heap_index().and_then(|_| self.get(owner));
            let mark_bits = mark_bits!(self);
            ephemerons.newly_marked(owner, cell, |target| mark_bits.is_marked(target), work);
        }
    }
}
