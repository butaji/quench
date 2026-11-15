//! A collection's sweep: the record families and the slots, with the strings swept on a worker
//! where a full collection has enough of them.
use super::collect::Settle;
use super::*;

/// The state a family's sweep threads through: the collection's kind and age, the property arena
/// and the payload ledger, and the lists the settling visits.
pub(super) struct SweepCx<'a> {
    pub(super) full: bool,
    pub(super) age: PromotionAge,
    pub(super) compacts: bool,
    pub(super) narrow_arrays: bool,
    pub(super) properties: &'a mut ValueArena,
    pub(super) payload_bytes: &'a mut usize,
    pub(super) live_payload_bytes: usize,
    pub(super) young_owners: Vec<(usize, YoungOwner)>,
    pub(super) direct_property_owners: Vec<(usize, Value)>,
    pub(super) young_arrays: Vec<RecordId>,
    pub(super) narrowable_arrays: Vec<RecordId>,
    #[cfg(feature = "profile-memory")]
    pub(super) memory_profile: &'a mut memory_profile::MemoryProfile,
}

/// What a collection's sweep of the records and slots found, and what its settling reads.
pub(super) struct Swept {
    pub(super) live: usize,
    pub(super) legacy_live: usize,
    pub(super) live_payload_bytes: usize,
    pub(super) property_owners: Vec<u64>,
    pub(super) trace_records_us: u128,
    pub(super) young_owners: Vec<(usize, YoungOwner)>,
    pub(super) direct_property_owners: Vec<(usize, Value)>,
    pub(super) young_arrays: Vec<RecordId>,
    pub(super) narrowable_arrays: Vec<RecordId>,
}

/// The strings a full collection sweeps on a worker once there are this many cells to pay for it.
const STRING_WORKER_MINIMUM_CELLS: usize = PARALLEL_MARK_MINIMUM_CELLS / 4;

/// The record families, borrowed apart from the strings a worker sweeps.
pub(super) struct Families<'a> {
    object_records: &'a mut ObjectRecordSpace,
    array_records: &'a mut ArrayRecordSpace,
    function_records: &'a mut FunctionRecordSpace,
    promise_records: &'a mut PromiseRecordSpace,
    await_records: &'a mut AwaitRecordSpace,
    v128_records: &'a mut V128RecordSpace,
    iterator_records: &'a mut IteratorRecordSpace,
    sparse_arrays: &'a mut Option<Box<FxHashMap<Value, SparseElements>>>,
    young_function_runs: &'a mut bool,
    abandoned_contexts: &'a mut AbandonedHostContexts,
}

/// What each family's sweep kept and freed, as `(live, freed)`.
#[derive(Default)]
pub(super) struct FamilyTotals {
    objects: (usize, usize),
    functions: (usize, usize),
    promises: (usize, usize),
    awaits: (usize, usize),
    v128: (usize, usize),
    iterators: (usize, usize),
    arrays: (usize, usize),
}

impl FamilyTotals {
    /// The live records of every family.
    pub(super) fn live(&self) -> usize {
        self.objects.0 + self.arrays.0 + self.functions.0 + self.promises.0 + self.awaits.0 + self.v128.0 + self.iterators.0
    }

    /// The freed records of every family.
    pub(super) fn freed(&self) -> usize {
        self.objects.1 + self.arrays.1 + self.functions.1 + self.promises.1 + self.awaits.1 + self.v128.1 + self.iterators.1
    }
}

impl Families<'_> {
    /// Sweeps every family, in the order the records' sweeps have always run.
    fn sweep(mut self, cx: &mut SweepCx<'_>) -> FamilyTotals {
        let young_runs = cx.properties.young_runs();
        let objects = self.sweep_objects(cx, young_runs);
        let functions = self.sweep_functions(cx, young_runs);
        let promises = self.sweep_promises(cx);
        let awaits = self.await_records.sweep(cx.full, cx.age, false, |_| {});
        // A vector owns nothing beyond its record.
        let v128 = self.v128_records.sweep(cx.full, cx.age, false, |_| {});
        let iterators = self.sweep_iterators(cx);
        let arrays = self.sweep_arrays(cx);
        FamilyTotals { objects, functions, promises, awaits, v128, iterators, arrays }
    }

    fn sweep_objects(&mut self, cx: &mut SweepCx<'_>, young_runs: YoungRuns) -> (usize, usize) {
        let (full, age, compacts) = (cx.full, cx.age, cx.compacts);
        let properties = &mut *cx.properties;
        let direct_property_owners = &mut cx.direct_property_owners;
        let young_owners = &mut cx.young_owners;
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut *cx.memory_profile;
        let dispose = |_record_id: RecordId, object: &Object| {
            #[cfg(feature = "profile-memory")]
            memory_profile.freed_record_cell(Value::direct_object(_record_id), properties.len(object.properties()));
            object.release_storage(properties)
        };
        self.object_records.sweep_disposing(full, age, records::sweep_support::objects_visited(young_runs, compacts), dispose, |record| match record {
            RecordSweep::Keep(record_id, object) => {
                if compacts && object.properties().has_compact_range() {
                    direct_property_owners.push((object.properties().start_offset(), Value::direct_object(record_id)));
                } else if young_runs.hold(object.properties()) {
                    note_young_owner(young_owners, object.properties().start_offset(), YoungOwner::Object(record_id));
                }
            }
            // Released in place: dropping it frees its extras.
            RecordSweep::Dead(..) => {}
        })
    }

    fn sweep_functions(&mut self, cx: &mut SweepCx<'_>, young_runs: YoungRuns) -> (usize, usize) {
        let (full, age, compacts) = (cx.full, cx.age, cx.compacts);
        let properties = &mut *cx.properties;
        let direct_property_owners = &mut cx.direct_property_owners;
        let young_owners = &mut cx.young_owners;
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut *cx.memory_profile;
        // A function's own properties may take a young run too (`Heap::finished_run_object`):
        // after one did, its sweep shows its survivors and lists those it keeps.
        let young_function_runs = std::mem::take(self.young_function_runs);
        self.function_records.sweep(full, age, full || compacts || young_function_runs, |record| match record {
            RecordSweep::Keep(record_id, function) => {
                if compacts && function.object.properties().has_compact_range() {
                    direct_property_owners.push((function.object.properties().start_offset(), Value::direct_function(record_id)));
                } else if young_runs.hold(function.object.properties()) {
                    note_young_owner(young_owners, function.object.properties().start_offset(), YoungOwner::Function(record_id));
                }
            }
            RecordSweep::Dead(_record_id, function) => {
                #[cfg(feature = "profile-memory")]
                let slots = properties.len(function.object.properties());
                properties.release(function.object.properties());
                #[cfg(feature = "profile-memory")]
                memory_profile.freed_record_cell(Value::direct_function(_record_id), slots);
            }
        })
    }

    fn sweep_promises(&mut self, cx: &mut SweepCx<'_>) -> (usize, usize) {
        let (full, age, compacts) = (cx.full, cx.age, cx.compacts);
        let properties = &mut *cx.properties;
        let direct_property_owners = &mut cx.direct_property_owners;
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut *cx.memory_profile;
        let abandoned_contexts = &mut *self.abandoned_contexts;
        self.promise_records.sweep(full, age, full, |record| match record {
            RecordSweep::Keep(record_id, promise) => {
                if compacts && promise.object.properties().has_compact_range() {
                    direct_property_owners.push((promise.object.properties().start_offset(), Value::direct_promise(record_id)));
                }
            }
            RecordSweep::Dead(_record_id, promise) => {
                abandoned_contexts.abandon(&promise.record);
                #[cfg(feature = "profile-memory")]
                let slots = properties.len(promise.object.properties());
                properties.release_ordinary(promise.object.properties());
                #[cfg(feature = "profile-memory")]
                memory_profile.freed_record_cell(Value::direct_promise(_record_id), slots);
            }
        })
    }

    fn sweep_iterators(&mut self, cx: &mut SweepCx<'_>) -> (usize, usize) {
        let (full, age, compacts) = (cx.full, cx.age, cx.compacts);
        let properties = &mut *cx.properties;
        let direct_property_owners = &mut cx.direct_property_owners;
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut *cx.memory_profile;
        self.iterator_records.sweep(full, age, full, |record| match record {
            RecordSweep::Keep(record_id, iterator) => {
                if compacts && iterator.object.properties().has_compact_range() {
                    direct_property_owners.push((iterator.object.properties().start_offset(), Value::direct_iterator(record_id)));
                }
            }
            RecordSweep::Dead(_record_id, iterator) => {
                #[cfg(feature = "profile-memory")]
                let slots = properties.len(iterator.object.properties());
                properties.release_ordinary(iterator.object.properties());
                #[cfg(feature = "profile-memory")]
                memory_profile.freed_record_cell(Value::direct_iterator(_record_id), slots);
            }
        })
    }

    /// The arrays' sweep: a dead array is released in place and, with sparse elements, its entry
    /// dropped; the live young and narrowable runs are listed for the settling.
    fn sweep_arrays(&mut self, cx: &mut SweepCx<'_>) -> (usize, usize) {
        let (full, age, compacts, narrow_arrays) = (cx.full, cx.age, cx.compacts, cx.narrow_arrays);
        let mut dead_arrays = Vec::new();
        let sparse = self.sparse_arrays.is_some();
        let payload_bytes = &mut *cx.payload_bytes;
        let properties = &mut *cx.properties;
        let live_payload_bytes = &mut cx.live_payload_bytes;
        let young_arrays = &mut cx.young_arrays;
        let narrowable_arrays = &mut cx.narrowable_arrays;
        let direct_property_owners = &mut cx.direct_property_owners;
        #[cfg(feature = "profile-memory")]
        let memory_profile = &mut *cx.memory_profile;
        let dispose = |record_id: RecordId, array: &ArrayObject| {
            *payload_bytes = payload_bytes.saturating_sub(array_payload_bytes(array));
            #[cfg(feature = "profile-memory")]
            memory_profile.freed_record_cell(Value::direct_array(record_id), properties.len(array.properties()));
            properties.release_ordinary(array.properties());
            if sparse {
                dead_arrays.push(Value::direct_array(record_id));
            }
            array.needs_drop()
        };
        self.array_records.warm_before_drop(records::sweep_support::warm_array_elements);
        let totals = self.array_records.sweep_disposing(full, age, true, dispose, |record| match record {
            RecordSweep::Keep(record_id, array) => {
                if full {
                    **live_payload_bytes += array_payload_bytes(array);
                }
                if array.elements().is_young() {
                    young_arrays.push(record_id);
                } else if full && narrow_arrays && array.elements().may_narrow() {
                    narrowable_arrays.push(record_id);
                }
                if compacts && array.properties().has_compact_range() {
                    direct_property_owners.push((array.properties().start_offset(), Value::direct_array(record_id)));
                }
            }
            // Released in place: dropping it frees what it owns.
            RecordSweep::Dead(..) => {}
        });
        if let Some(arrays) = &mut *self.sparse_arrays {
            for array in dead_arrays {
                arrays.remove(&array);
            }
            if arrays.is_empty() {
                *self.sparse_arrays = None;
            }
        }
        totals
    }
}

impl Heap {
    /// The sweep of the records and the slots (`sweep_full_parallel` on the pool where a full
    /// collection is large, else `sweep_serial`), with the record families and the strings
    /// swept, then the owners carried to the next minor collection.
    pub(super) fn sweep_records(&mut self, settle: &mut Settle, compacts: bool) -> Swept {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let sweep_pool = if settle.full && !self.has_direct_records() && self.slots.len() >= PARALLEL_MARK_MINIMUM_CELLS && !cfg!(feature = "profile-memory") {
            parallel_mark::parallel_mark_pool()
        } else {
            None
        };
        let output = match sweep_pool {
            Some(pool) => self.sweep_full_parallel(pool, compacts),
            None => self.sweep_serial(settle.full, compacts, settle.age, &mut settle.remembered_owners),
        };
        if output.freed_cells > 0 {
            // A freed slot may next hold another cell (`view_epoch`): one bump for the sweep.
            buffer_bytes::bump_view_epoch();
        }
        let legacy_live = output.live;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += self.record_families().iter().map(|(family, _)| family.high_water()).sum::<usize>() as u64;
        }
        let trace_records_started = settle.clock.trace.map(|_| std::time::Instant::now());
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let records_started = std::time::Instant::now();
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[0] += (records_started - sweep_started).as_nanos() as u64;
        }
        let worker = (settle.full && !cfg!(feature = "profile-memory") && (self.parallel_mark_forced() || self.string_records.live_count() + self.small_strings.live_count() >= STRING_WORKER_MINIMUM_CELLS)).then(parallel_mark::parallel_mark_pool).flatten();
        let strings = StoppedStringSweep { records: &mut self.string_records, small: &mut self.small_strings, atoms: &mut self.string_atoms, full: settle.full, age: settle.age };
        let families = Families {
            object_records: &mut self.object_records,
            array_records: &mut self.array_records,
            function_records: &mut self.function_records,
            promise_records: &mut self.promise_records,
            await_records: &mut self.await_records,
            v128_records: &mut self.v128_records,
            iterator_records: &mut self.iterator_records,
            sparse_arrays: &mut self.sparse_arrays,
            young_function_runs: &mut self.young_function_runs,
            abandoned_contexts: &mut self.abandoned_contexts,
        };
        let mut cx = SweepCx {
            full: settle.full,
            age: settle.age,
            compacts,
            narrow_arrays: self.narrow_runs,
            properties: &mut self.properties,
            payload_bytes: &mut self.payload_bytes,
            live_payload_bytes: output.live_payload_bytes,
            young_owners: Vec::new(),
            direct_property_owners: Vec::with_capacity(if compacts { self.property_owner_counts.1 } else { 0 }),
            young_arrays: Vec::new(),
            narrowable_arrays: Vec::new(),
            #[cfg(feature = "profile-memory")]
            memory_profile: &mut self.memory_profile,
        };
        let (mut string_out, totals) = sweep_families_and_strings(&mut cx, strings, families, worker);
        let SweepCx { live_payload_bytes, young_owners, direct_property_owners, young_arrays, narrowable_arrays, .. } = cx;
        // The strings that share an `Rc` die on the collector thread.
        drop(std::mem::take(&mut string_out.rc_strings));
        drop(std::mem::take(&mut string_out.rc_small));
        self.payload_bytes = self.payload_bytes.saturating_sub(string_out.dead_bytes);
        let live_payload_bytes = live_payload_bytes + string_out.live_bytes;
        let live = legacy_live + totals.live() + string_out.live;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.freed += (totals.freed() + string_out.freed) as u64;
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.sweep_slots += output.swept_slots as u64;
            self.gc_profile.freed += output.freed_cells as u64;
        }
        if settle.carries_owners {
            for (family, handle) in self.record_families_mut() {
                settle.remembered_owners.extend(family.take_promoted().into_iter().map(handle));
            }
            self.carried_owners = std::mem::take(&mut settle.remembered_owners);
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[1] += records_started.elapsed().as_nanos() as u64;
        }
        Swept {
            live,
            legacy_live,
            live_payload_bytes,
            property_owners: output.property_owners,
            trace_records_us: trace_records_started.map_or(0, |started| started.elapsed().as_micros()),
            young_owners,
            direct_property_owners,
            young_arrays,
            narrowable_arrays,
        }
    }

}

/// The record families' sweep, with the strings' sweep on `worker` (a pool) where it is given.
fn sweep_families_and_strings(cx: &mut SweepCx<'_>, strings: StoppedStringSweep<'_>, families: Families<'_>, worker: Option<&ThreadPool>) -> (StringSweepOutput, FamilyTotals) {
    let mut string_out = StringSweepOutput::default();
    let totals = match worker {
        Some(pool) => pool.in_place_scope(|scope| {
            let out = &mut string_out;
            scope.spawn(move |_| *out = strings.run(true));
            families.sweep(cx)
        }),
        None => {
            string_out = strings.run(false);
            families.sweep(cx)
        }
    };
    (string_out, totals)
}
