//! Sweeping spread over helper threads after a parallel mark.
//!
//! Each worker owns a disjoint run of slabs. It counts survivors, records arena-backed
//! survivors for compaction, and frees dead plain objects and arrays whose property storage is
//! inline or empty and which have no sparse elements: such a cell touches no shared table. An
//! array's element backing is dropped by the worker when the array is its only owner; a shared
//! backing's references are counted and released by the collecting thread, so no reference
//! count is written concurrently. Workers mark the slots they free in the free-slot bitmap,
//! whose words their disjoint slab runs never share. Every other dead cell is reported back and
//! freed on the collecting thread.
use super::parallel_mark::mark_threads;
use super::{Cell, FxHashMap, Heap, Slot, SparseElements, Value, ValueArena, pack_offset_owner};
use std::rc::Rc;

/// One worker's results: survivors, arena owners, the slots it freed and the dead cells it left
/// to the collecting thread.
#[derive(Default)]
struct SweepShare {
    live: usize,
    property_owners: Vec<u64>,
    freed: usize,
    deferred: Vec<u32>,
    /// A freed object's arena storage was dropped without release.
    dropped_ranges: bool,
    /// References to shared element backings held by freed arrays, by backing.
    shared_backings: FxHashMap<*const Vec<Value>, usize>,
}

// SAFETY: a share's backing pointers are strong references handed to the collecting thread,
// which alone releases them after the workers have joined.
unsafe impl Send for SweepShare {}

/// A worker's slabs, each with its first flat index.
struct SlabRun<'a>(Vec<(usize, &'a mut [Slot])>);

// SAFETY: a worker only reads its own slots and frees cells that own no allocation and no
// reference count; the slabs of different workers are disjoint.
unsafe impl Send for SlabRun<'_> {}

/// Shared read-only state for sweep workers.
struct SweepContext<'a> {
    marks: &'a [u64],
    free: &'a [std::sync::atomic::AtomicU64],
    properties: &'a ValueArena,
    sparse_arrays: Option<&'a FxHashMap<u32, SparseElements>>,
}

// SAFETY: workers only read the mark bitmap, the property arena's metadata and the sparse
// element table.
unsafe impl Sync for SweepContext<'_> {}

impl Heap {
    /// Sweeps in parallel; returns survivors and arena owners. Each dead cell the workers left
    /// in place is then freed by `free_cell`.
    pub(super) fn sweep_parallel(&mut self) -> (usize, Vec<u64>, bool) {
        let workers = mark_threads();
        self.free.cover(self.slots.len());
        let slabs: Vec<_> = self.slots.slabs_mut().collect();
        let per_worker = slabs.len().div_ceil(workers).max(1);
        let mut runs: Vec<SlabRun<'_>> = Vec::new();
        let mut slabs = slabs.into_iter().peekable();
        while slabs.peek().is_some() {
            runs.push(SlabRun(slabs.by_ref().take(per_worker).collect()));
        }
        let context = SweepContext {
            marks: &self.marks,
            free: self.free.atomic_bits(),
            properties: &self.properties,
            sparse_arrays: self.sparse_arrays.as_deref(),
        };
        let shares: Vec<SweepShare> = std::thread::scope(|scope| {
            let mut runs = runs.into_iter();
            let own = runs.next();
            let helpers: Vec<_> = runs
                .map(|run| scope.spawn(|| sweep_run(&context, run)))
                .collect();
            own.map(|run| sweep_run(&context, run))
                .into_iter()
                .chain(helpers.into_iter().map(|helper| helper.join().unwrap()))
                .collect()
        });
        let mut live = 0;
        let mut property_owners = Vec::new();
        let mut deferred = Vec::new();
        let mut dropped_ranges = false;
        for share in shares {
            dropped_ranges |= share.dropped_ranges;
            live += share.live;
            property_owners.extend(share.property_owners);
            self.free.add_inserted(share.freed);
            deferred.extend(share.deferred);
            for (backing, references) in share.shared_backings {
                for _ in 0..references {
                    // SAFETY: each count is a strong reference a freed array gave up through
                    // `Rc::into_raw`; the backing outlives all but the last of them.
                    unsafe { Rc::decrement_strong_count(backing) };
                }
            }
        }
        for index in deferred {
            // SAFETY: workers report only occupied, in-bounds slots they left untouched.
            let cell = unsafe { self.slots.get_unchecked_mut(index as usize) }
                .cell
                .take()
                .unwrap();
            self.free_cell(index as usize, cell);
        }
        (live, property_owners, dropped_ranges)
    }
}

impl SweepShare {
    fn free(&mut self, context: &SweepContext<'_>, index: usize) {
        context.free[index / u64::BITS as usize].fetch_or(
            1 << (index % u64::BITS as usize),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.freed += 1;
    }
}

fn sweep_run(context: &SweepContext<'_>, run: SlabRun<'_>) -> SweepShare {
    let mut share = SweepShare::default();
    for (base, slab) in run.0 {
        for (offset, slot) in slab.iter_mut().enumerate() {
            let index = base + offset;
            if Heap::marked(context.marks, index) {
                share.live += 1;
                if let Some(object) = slot.cell.as_ref().and_then(Cell::object)
                    && context.properties.has_compact_range(object.properties())
                {
                    share
                        .property_owners
                        .push(pack_offset_owner(object.properties().start_offset(), index));
                }
                continue;
            }
            match &slot.cell {
                None => {}
                // A dead object's arena range is discarded by the compaction that follows the
                // sweep, which keeps only live owners; only dictionary storage needs a release.
                Some(Cell::Object(object))
                    if object.owns_no_allocation() && !object.properties().is_dictionary() =>
                {
                    share.dropped_ranges |=
                        !context.properties.release_is_noop(object.properties());
                    std::mem::forget(slot.cell.take());
                    share.free(context, index);
                }
                Some(Cell::String(text)) if text.is_uniquely_owned() => {
                    // No other value shares the buffers, so freeing them here is thread-local.
                    drop(slot.cell.take());
                    share.free(context, index);
                }
                Some(Cell::Array { object })
                    if object.owns_no_allocation()
                        && !object.properties().is_dictionary()
                        && context
                            .sparse_arrays
                            .is_none_or(|arrays| !arrays.contains_key(&(index as u32))) =>
                {
                    share.dropped_ranges |=
                        !context.properties.release_is_noop(object.properties());
                    let elements = object.elements();
                    let unique = Rc::strong_count(elements) == 1 && Rc::weak_count(elements) == 0;
                    let Some(Cell::Array { object }) = slot.cell.take() else {
                        unreachable!("matched an array above");
                    };
                    let (object, elements) = object.into_parts();
                    std::mem::forget(object);
                    if unique {
                        // No other cell holds this backing, so no other thread touches it.
                        drop(elements);
                    } else {
                        *share
                            .shared_backings
                            .entry(Rc::into_raw(elements))
                            .or_default() += 1;
                    }
                    share.free(context, index);
                }
                Some(_) => share.deferred.push(index as u32),
            }
        }
    }
    share
}
