//! Sweeping spread over helper threads after a parallel mark.
//!
//! Each worker owns a disjoint run of slabs. It counts survivors from the mark bitmap and
//! visits only occupied, unmarked slots (marking already recorded the arena owners compaction
//! needs, so survivors are not read again). It frees dead plain objects and arrays whose property storage is
//! inline or empty and which have no sparse elements: such a cell touches no shared table. An
//! array's element backing is dropped by the worker when the array is its only owner; a shared
//! backing's references are counted and released by the collecting thread, so no reference
//! count is written concurrently. Workers mark the slots they free in the free-slot bitmap,
//! whose words their disjoint slab runs never share. Every other dead cell is reported back and
//! freed on the collecting thread.
use super::slots::SLOTS_PER_SLAB;
use super::{Cell, FxHashMap, Heap, Slot, SparseElements, Value, ValueArena, ValueVec};
use std::rc::Rc;

/// One worker's results: survivors, arena owners, the slots it freed and the dead cells it left
/// to the collecting thread.
#[derive(Default)]
struct SweepShare {
    live: usize,
    freed: usize,
    deferred: Vec<u32>,
    /// Freed objects' arena storage, released to the arena by the collecting thread.
    released: Vec<ValueVec>,
    /// References to shared element backings held by freed arrays, by backing.
    shared_backings: FxHashMap<*const Vec<Value>, usize>,
    /// The latest shared backing and its references not yet in `shared_backings`: dead
    /// arrays from one literal share a backing, so most releases count here.
    last_backing: Option<(*const Vec<Value>, usize)>,
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
    /// Sweeps in parallel and returns the survivors; freed objects' arena ranges are released.
    /// Each dead cell the workers left in place is then freed by `free_cell`.
    pub(super) fn sweep_parallel(&mut self) -> usize {
        self.start_gc_workers();
        let workers = self.gc_workers.as_ref().expect("started above").workers();
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
        let runs: Vec<std::sync::Mutex<Option<SlabRun<'_>>>> = runs
            .into_iter()
            .map(|run| std::sync::Mutex::new(Some(run)))
            .collect();
        let shares: Vec<SweepShare> =
            self.gc_workers
                .as_ref()
                .expect("started above")
                .run(|index| {
                    runs.get(index)
                        .and_then(|run| run.lock().unwrap().take())
                        .map(|run| sweep_run(&context, run))
                        .unwrap_or_default()
                });
        let mut live = 0;
        let mut deferred = Vec::new();
        for mut share in shares {
            share.flush_backing();
            for properties in share.released.drain(..) {
                self.properties.release(properties);
            }
            live += share.live;
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
        live
    }
}

impl SweepShare {
    fn release_properties(&mut self, arena: &ValueArena, properties: ValueVec) {
        if !arena.release_is_noop(properties) {
            self.released.push(properties);
        }
    }

    /// Counts one reference to the shared element backing `backing` for release.
    fn release_backing(&mut self, backing: *const Vec<Value>) {
        match &mut self.last_backing {
            Some((last, references)) if *last == backing => *references += 1,
            _ => {
                self.flush_backing();
                self.last_backing = Some((backing, 1));
            }
        }
    }

    fn flush_backing(&mut self) {
        if let Some((backing, references)) = self.last_backing.take() {
            *self.shared_backings.entry(backing).or_default() += references;
        }
    }
}

fn sweep_run(context: &SweepContext<'_>, run: SlabRun<'_>) -> SweepShare {
    const WORD_BITS: usize = u64::BITS as usize;
    const _: () = assert!(SLOTS_PER_SLAB % WORD_BITS == 0);
    let mut share = SweepShare::default();
    for (base, slab) in run.0 {
        for (word_offset, slots) in slab.chunks_mut(WORD_BITS).enumerate() {
            let word = base / WORD_BITS + word_offset;
            let marks = context.marks[word];
            share.live += marks.count_ones() as usize;
            let present = u64::MAX >> (WORD_BITS - slots.len());
            // Only this worker writes this run's free words, so it reads and replaces them whole.
            let free = context.free[word].load(std::sync::atomic::Ordering::Relaxed);
            let mut dead = present & !marks & !free;
            // Slots this word frees, published with one store once the word is done.
            let mut freed: u64 = 0;
            while dead != 0 {
                let bit = dead.trailing_zeros() as usize;
                dead &= dead - 1;
                let index = word * WORD_BITS + bit;
                let slot = &mut slots[bit];
                debug_assert!(
                    slot.cell.is_some(),
                    "a slot without a free bit holds a cell"
                );
                match &slot.cell {
                    None => {}
                    // A dead object's arena range is discarded by the compaction that follows the
                    // sweep, which keeps only live owners; only dictionary storage needs a release.
                    Some(Cell::Object(object))
                        if object.owns_no_allocation() && !object.properties().is_dictionary() =>
                    {
                        share.release_properties(context.properties, object.properties());
                        std::mem::forget(slot.cell.take());
                        freed |= 1 << bit;
                    }
                    Some(Cell::String(text)) if text.is_uniquely_owned() => {
                        // No other value shares the buffers, so freeing them here is thread-local.
                        drop(slot.cell.take());
                        freed |= 1 << bit;
                    }
                    Some(Cell::Array { object })
                        if object.owns_no_allocation()
                            && !object.properties().is_dictionary()
                            && context
                                .sparse_arrays
                                .is_none_or(|arrays| !arrays.contains_key(&(index as u32))) =>
                    {
                        share.release_properties(context.properties, object.properties());
                        let elements = object.elements();
                        let unique =
                            Rc::strong_count(elements) == 1 && Rc::weak_count(elements) == 0;
                        let Some(Cell::Array { object }) = slot.cell.take() else {
                            unreachable!("matched an array above");
                        };
                        let (object, elements) = object.into_parts();
                        std::mem::forget(object);
                        if unique {
                            // No other cell holds this backing, so no other thread touches it.
                            drop(elements);
                        } else {
                            share.release_backing(Rc::into_raw(elements));
                        }
                        freed |= 1 << bit;
                    }
                    Some(_) => share.deferred.push(index as u32),
                }
            }
            if freed != 0 {
                context.free[word].store(free | freed, std::sync::atomic::Ordering::Relaxed);
                share.freed += freed.count_ones() as usize;
            }
        }
    }
    share
}
