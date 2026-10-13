//! Stop-the-world marking spread over helper threads.
//!
//! While the mutator is stopped nothing writes a cell, a property arena value or a reference
//! count, so worker threads may read the heap concurrently. Each worker claims a cell by
//! setting its mark bit atomically and traces it with the same `Heap::children` the
//! single-threaded marker uses; surplus work moves between workers through a shared pool.
use super::{Cell, FxHashMap, Heap, SlotArena, SparseElements, Value, ValueArena};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Collections whose previous survivors reach this many cells mark in parallel; smaller heaps
/// finish before helper threads would start.
pub(super) const PARALLEL_MARK_MIN_LIVE_CELLS: usize = 1 << 16;
/// Marking threads, the collecting thread included.
const MAX_MARK_THREADS: usize = 4;
/// A worker whose stack grows past this donates half of it while another worker is idle.
const DONATION_THRESHOLD: usize = 64;

/// Per-object root discovery supplied by the VM: `visit` runs for every marked object with
/// its shape and may append further roots. Each marking thread keeps its own `Local` state,
/// which the caller merges after collection.
pub(crate) trait ObjectRootTracer: Sync {
    type Local: Send;
    fn local(&self) -> Self::Local;
    fn visit(&self, local: &mut Self::Local, owner: Value, shape: u32, roots: &mut Vec<Value>);
}

/// A tracer for collections without object-owned roots.
#[cfg(test)]
pub(crate) struct NoObjectRoots;

#[cfg(test)]
impl ObjectRootTracer for NoObjectRoots {
    type Local = ();
    fn local(&self) {}
    fn visit(&self, _: &mut (), _: Value, _: u32, _: &mut Vec<Value>) {}
}

/// Read access to mark bits, plain for the single-threaded marker and atomic for workers.
pub(super) trait MarkBits {
    /// Whether `index` names a slot whose bit is still clear; false beyond the bitmap.
    fn unmarked(&self, index: usize) -> bool;
}

impl MarkBits for [u64] {
    #[inline(always)]
    fn unmarked(&self, index: usize) -> bool {
        self.get(index / 64)
            .is_some_and(|word| word & (1 << (index % 64)) == 0)
    }
}

impl MarkBits for [AtomicU64] {
    #[inline(always)]
    fn unmarked(&self, index: usize) -> bool {
        self.get(index / 64)
            .is_some_and(|word| word.load(Ordering::Relaxed) & (1 << (index % 64)) == 0)
    }
}

/// The heap state workers read.
struct MarkContext<'a> {
    slots: &'a SlotArena,
    properties: &'a ValueArena,
    sparse_arrays: Option<&'a FxHashMap<u32, SparseElements>>,
    marks: &'a [AtomicU64],
}

// SAFETY: workers only read cells, property values and reference counts while the mutator is
// stopped, and mark bits are atomic; nothing in the context is written during marking.
unsafe impl Sync for MarkContext<'_> {}

/// Work moved between workers, and their termination state.
struct WorkPool {
    chunks: Mutex<Vec<Vec<Value>>>,
    idle: AtomicUsize,
    done: AtomicBool,
    workers: usize,
}

impl WorkPool {
    fn donate(&self, stack: &mut Vec<Value>) {
        let chunk = stack.split_off(stack.len() / 2);
        self.chunks.lock().unwrap().push(chunk);
    }

    /// Refills an empty `stack` from the pool, waiting while other workers still trace;
    /// returns false once every worker is idle and the pool is empty.
    fn refill(&self, stack: &mut Vec<Value>) -> bool {
        let mut counted_idle = false;
        loop {
            if self.done.load(Ordering::Acquire) {
                return false;
            }
            {
                let mut chunks = self.chunks.lock().unwrap();
                if let Some(chunk) = chunks.pop() {
                    *stack = chunk;
                    if counted_idle {
                        self.idle.fetch_sub(1, Ordering::AcqRel);
                    }
                    return true;
                }
                if !counted_idle {
                    counted_idle = true;
                    self.idle.fetch_add(1, Ordering::AcqRel);
                }
                // Donations happen only from a non-empty stack, so with every worker idle
                // under the lock no work remains anywhere.
                if self.idle.load(Ordering::Acquire) == self.workers {
                    self.done.store(true, Ordering::Release);
                    return false;
                }
            }
            std::thread::yield_now();
        }
    }
}

impl Heap {
    /// Whether this collection should mark in parallel: its previous live set was large.
    /// Ephemerons are resolved afterwards by `mark_ephemerons` on the collecting thread.
    pub(super) fn parallel_marking(&self) -> bool {
        self.last_survivors >= PARALLEL_MARK_MIN_LIVE_CELLS && mark_threads() > 1
    }

    /// Marks everything reachable from `roots` with helper threads.
    pub(super) fn mark_parallel<T: ObjectRootTracer>(
        &mut self,
        roots: Vec<Value>,
        tracer: &T,
    ) -> Vec<T::Local> {
        let workers = mark_threads();
        // SAFETY: `AtomicU64` has `u64`'s size and alignment, and the bitmap is accessed only
        // through this view until the scope below has joined every worker.
        let marks: &[AtomicU64] = unsafe {
            &*(std::ptr::from_mut::<[u64]>(self.marks.as_mut_slice()) as *const [AtomicU64])
        };
        let context = MarkContext {
            slots: &self.slots,
            properties: &self.properties,
            sparse_arrays: self.sparse_arrays.as_deref(),
            marks,
        };
        let pool = WorkPool {
            chunks: Mutex::new(Vec::new()),
            idle: AtomicUsize::new(0),
            done: AtomicBool::new(false),
            workers,
        };
        std::thread::scope(|scope| {
            let helpers: Vec<_> = (1..workers)
                .map(|_| {
                    scope.spawn(|| {
                        let mut local = tracer.local();
                        mark_worker(&context, &pool, tracer, &mut local, Vec::new());
                        local
                    })
                })
                .collect();
            let mut local = tracer.local();
            mark_worker(&context, &pool, tracer, &mut local, roots);
            std::iter::once(local)
                .chain(helpers.into_iter().map(|helper| helper.join().unwrap()))
                .collect()
        })
    }
}

pub(super) fn mark_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_MARK_THREADS)
}

fn mark_worker<T: ObjectRootTracer>(
    context: &MarkContext<'_>,
    pool: &WorkPool,
    tracer: &T,
    local: &mut T::Local,
    mut stack: Vec<Value>,
) {
    let mut last_shared_elements: *const Vec<Value> = std::ptr::null();
    loop {
        while let Some(value) = stack.pop() {
            let Some(index) = value.heap_index().map(|index| index as usize) else {
                continue;
            };
            if index >= context.slots.len() {
                continue;
            }
            // SAFETY: bounds-checked above; slots are not resized during marking.
            let Some(cell) = (unsafe { context.slots.get_unchecked(index) })
                .cell
                .as_ref()
            else {
                continue;
            };
            let bit = 1 << (index % 64);
            let word = &context.marks[index / 64];
            if word.load(Ordering::Relaxed) & bit != 0
                || word.fetch_or(bit, Ordering::Relaxed) & bit != 0
            {
                continue;
            }
            Heap::children(
                value,
                cell,
                context.properties,
                context.marks,
                &mut last_shared_elements,
                &mut stack,
                &mut |owner, shape, roots| tracer.visit(local, owner, shape, roots),
            );
            if let Some(arrays) = context.sparse_arrays
                && matches!(cell, Cell::Array { .. })
                && let Some(elements) = arrays.get(&(index as u32))
            {
                stack.extend(elements.values.values().copied());
            }
            if stack.len() > DONATION_THRESHOLD && pool.idle.load(Ordering::Relaxed) != 0 {
                pool.donate(&mut stack);
            }
        }
        if !pool.refill(&mut stack) {
            return;
        }
    }
}
