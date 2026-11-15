//! A collection's settling: the survivors of its sweep (young and narrow runs, the compaction of
//! the property arena), and the pacing of the next collection.
use super::collect::Settle;
use super::collect_sweep::Swept;
use super::*;

/// The arena grows by this eighth of its last measured size before a narrow pass runs.
const NARROW_PASS_GROWTH_DIVISOR: usize = 8;
/// The full collections between narrow passes, once the arena stops growing.
const NARROW_PASS_FULL_INTERVAL: usize = 8;
/// The share of a remark's live weight its cycle's allocation takes black: at most half.
const CYCLE_BLACK_SHARE_DIVISOR: usize = 2;

impl Heap {
    /// The sweep of a collection, the settling of its survivors, then the pacing of the next
    /// collection. Returns the finalization jobs.
    pub(super) fn sweep_and_settle(&mut self, mut settle: Settle, finalization_jobs: Vec<(Value, Value)>) -> Vec<(Value, Value)> {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let sweep_started = std::time::Instant::now();
        let trace_sweep_started = settle.clock.trace.map(|_| std::time::Instant::now());
        let full_mark_micros = settle.clock.full.map_or(0, |started| started.elapsed().as_micros() as u64);
        // Property-arena owners are listed only for a full collection that compacts: the
        // lists hold an entry per live owner at the heap's peak. A compaction that this
        // sweep's releases make due waits for the next full collection.
        let compacts = settle.full && self.properties.compaction_due();
        let mut swept = self.sweep_records(&mut settle, compacts);
        self.settle_survivors(&settle, &mut swept, compacts);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_nanos += sweep_started.elapsed().as_nanos() as u64;
        }
        let trace_times = settle.clock.trace.zip(trace_sweep_started).map_or((0, 0), |(started, sweep)| ((sweep - started).as_micros(), sweep.elapsed().as_micros()));
        let live = self.pace_collection(&settle, &swept, full_mark_micros, trace_times);
        #[cfg(feature = "profile-aggregate")]
        self.record_collection_profile(&settle, live);
        self.peak_survivors = self.peak_survivors.max(live);
        self.retire_dead_prototypes();
        self.collection_state = CollectionState::Idle;
        #[cfg(feature = "profile-memory")]
        self.memory_profile.collection_finished();
        finalization_jobs
    }

    /// The survivors of a sweep: the young runs are settled (survive or leave the nursery), the
    /// narrowable arrays narrowed, the property arena compacted or its young runs evacuated, and
    /// a full collection's payload ledger and arena settled.
    fn settle_survivors(&mut self, settle: &Settle, swept: &mut Swept, compacts: bool) {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let compaction_started = std::time::Instant::now();
        if settle.full {
            // A full sweep recounts the ledger from the survivors: shrinking a live array
            // (truncation, `pop`, `splice`) is not charged back between full collections.
            self.payload_bytes = swept.live_payload_bytes;
        }
        for record_id in std::mem::take(&mut swept.young_arrays) {
            // SAFETY: the sweep listed these from the records it kept.
            let array = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
            // Native tiers read backings in place as wide values: only an interpreted run
            // keeps narrow ones (`narrow_runs`).
            if self.narrow_runs {
                array.survive_nursery();
            } else {
                array.leave_nursery();
            }
        }
        let narrowable = std::mem::take(&mut swept.narrowable_arrays);
        let (candidates, mut narrowed, mut saved_total) = (narrowable.len(), 0usize, 0usize);
        for record_id in narrowable {
            // SAFETY: the sweep listed these from the records it kept.
            let saved = unsafe { self.array_records.get_live_mut_unbarriered(record_id) }.narrow_elements();
            narrowed += usize::from(saved != 0);
            saved_total += saved;
            self.payload_bytes = self.payload_bytes.saturating_sub(saved);
        }
        if settle.full && crate::trace::Channel::Gc.on() {
            eprintln!("[narrow-elements] candidates={candidates} narrowed={narrowed} saved={}KB", saved_total / 1024);
        }
        if compacts {
            // Compaction moves the young survivors with every other run and forgets the
            // nursery (`ValueArena::finish_compaction`): due before the sweep, it stays due,
            // as the sweep only releases.
            debug_assert!(self.properties.compaction_due());
            self.property_owner_counts = (swept.property_owners.len(), swept.direct_property_owners.len());
            self.compact_property_arena(std::mem::take(&mut swept.property_owners), std::mem::take(&mut swept.direct_property_owners));
        } else {
            self.evacuate_young_runs(std::mem::take(&mut swept.young_owners));
            self.narrow_pass_if_due(settle.full);
        }
        if settle.full {
            self.properties.settle_after_full();
        }
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.sweep_phase_nanos[2] += compaction_started.elapsed().as_nanos() as u64;
        }
    }

    /// The narrow pass of a full collection that did not compact. It visits every object record,
    /// so it pays where property storage is large (8 MB and up), and is a cost of its own for a
    /// small arena of many objects: it runs once the arena grew, or every few full collections.
    fn narrow_pass_if_due(&mut self, full: bool) {
        if !(full && self.narrow_runs && self.properties.resident_words() >= Self::NARROW_PASS_MIN_ARENA_WORDS) {
            return;
        }
        let resident = self.properties.resident_words();
        let (last, fulls) = self.narrow_pass;
        if resident > last + last / NARROW_PASS_GROWTH_DIVISOR || fulls >= NARROW_PASS_FULL_INTERVAL {
            self.narrow_live_runs();
            self.narrow_pass = (resident, 0);
        } else {
            self.narrow_pass.1 = fulls + 1;
        }
    }

    /// The pacing after a collection: the live set and its weight, with a remark's black share
    /// taken out, the planned next collection, and the trace line. Returns the live set.
    fn pace_collection(&mut self, settle: &Settle, swept: &Swept, full_mark_micros: u64, trace_times: (u128, u128)) -> usize {
        // Live weight in the units allocation is charged in: one per cell or record plus
        // its payload.
        let live_weight = swept.live + self.payload_bytes / payload_unit();
        // A remark keeps everything its cycle allocated (black), garbage or not, which the
        // next cycle frees: pacing sizes the next interval from the snapshot's live set, the
        // marks less the cycle's allocation weight, at most half of them.
        let (live, live_weight, black_share) = if settle.premarked {
            let black = self.last_cycle_extent.weight.min(live_weight / CYCLE_BLACK_SHARE_DIVISOR);
            let records = (black as u128 * swept.live as u128 / live_weight.max(1) as u128) as usize;
            (swept.live - records, live_weight - black, (black, live_weight))
        } else {
            (swept.live, live_weight, (0, 1))
        };
        let allocated = self.allocated_since_collection();
        self.allocated_at_collection = self.allocated;
        self.external_since_collection = 0;
        self.external_collection_budget = self.external_bytes.max(self.pacing().minimum_external_budget);
        if settle.full {
            self.weight_between_fulls = self.allocated_since_full();
            self.allocated_at_full = self.allocated;
            self.reclaimable_program_bytes_since_full = 0;
            self.old_live = swept.legacy_live;
            self.young.clear();
            self.remembered.clear();
            self.remembered_marks.fill(0);
            self.live_after_full = live;
            self.live_weight_after_full = live_weight;
            let resident = self.occupied_resident_bytes();
            self.resident_after_full = resident - (resident as u128 * black_share.0 as u128 / black_share.1 as u128) as usize;
        }
        if settle.trims_environments && settle.closure_reads {
            self.live_at_environment_trim = Some(live);
        }
        let next = self.plan_next_collection(settle, live, live_weight, allocated, full_mark_micros);
        if crate::trace::Channel::Gc.on() {
            self.trace_collection(settle, (live, live_weight, allocated), &next, trace_times, swept.trace_records_us);
        }
        if settle.full && crate::trace::Channel::StorageCensus.on() {
            self.trace_storage_census();
        }
        self.live_weight_after_previous = live_weight;
        self.set_young_budget(next.threshold);
        self.full_due = next.full;
        self.cycle_due_at = usize::MAX;
        if next.full {
            self.plan_cycle(settle.premarked, next.threshold);
        }
        live
    }

    /// The next collection the pacing plans from this one's outcome, and the cycle judgement a
    /// full collection makes of its mark time and pause.
    fn plan_next_collection(&mut self, settle: &Settle, live: usize, live_weight: usize, allocated: usize, full_mark_micros: u64) -> pacing::NextCollection {
        let since_full = self.allocated_since_full();
        let mut aged = std::mem::take(&mut self.aged_fates);
        for (family, _) in self.record_families_mut() {
            aged.add(family.take_aged_fates());
        }
        let mut next = self.pacing().plan(
            &mut self.pacing_state,
            pacing::CollectionOutcome {
                full: settle.full,
                live,
                live_weight,
                previous_live_weight: self.live_weight_after_previous,
                live_after_full: self.live_after_full,
                live_weight_after_full: self.live_weight_after_full,
                allocated,
                since_full,
                aged,
                resident_at_start: if settle.full { self.resident_at_full_start } else { 0 },
                resident_live: if settle.full { self.resident_after_full } else { 0 },
                since_previous_full: if settle.full { self.weight_between_fulls } else { 0 },
            },
        );
        if settle.full {
            let pause_micros = settle.clock.full.map_or(0, |started| started.elapsed().as_micros() as u64);
            self.judge_cycles(settle.premarked, full_mark_micros, pause_micros);
        }
        if self.rc_active {
            next = self.rc_plan(live);
        }
        next
    }

    /// The `QUENCH_TRACE=gc` line of a collection: its times, its live set and weight, and the
    /// pacing's plan for the next one.
    fn trace_collection(&self, settle: &Settle, (live, live_weight, allocated): (usize, usize, usize), next: &pacing::NextCollection, trace_times: (u128, u128), trace_records_us: u128) {
        let (mark_us, sweep_us) = trace_times;
        let concurrent = if settle.full { format!(" concurrent={:?} {}", self.concurrent_cycle_refusal(), self.cycle_gate) } else { String::new() };
        let concurrent = if self.last_collection_premarked { format!("{concurrent} {}", self.last_cycle_extent) } else { concurrent };
        // Resident set size at this collection's end, so a pacing change can be judged
        // by its peak from one traced run.
        let rss_kb = resident_set_kib();
        eprintln!("gc rss_kb={rss_kb} live_weight={live_weight} full={}{concurrent} mark_us={mark_us} rc={:?} sweep_us={sweep_us} records_us={trace_records_us} resident_start={} resident_kept={} weight_between_fulls={} live={live} prev={} after_full={} alloc={} since_full={} next={next:?} state={:?} {}", settle.full, self.rc_trace, self.resident_at_full_start, self.resident_after_full, self.weight_between_fulls, self.live_weight_after_previous, self.live_after_full, allocated, self.allocated_since_full(), self.pacing_state, self.properties.trace_summary());
    }

    /// The allocation and survivor counts a collection adds to the profile (`profile-aggregate`).
    #[cfg(feature = "profile-aggregate")]
    fn record_collection_profile(&mut self, settle: &Settle, live: usize) {
        let allocations_between_collections = settle.allocations_between;
        self.gc_profile.allocations_between_collections_total += allocations_between_collections;
        self.gc_profile.survivors_total += live as u64;
        if self.collections == 1 {
            self.gc_profile.allocations_between_collections_min = allocations_between_collections;
            self.gc_profile.allocations_between_collections_max = allocations_between_collections;
            self.gc_profile.survivors_min = live as u64;
            self.gc_profile.survivors_max = live as u64;
        } else {
            self.gc_profile.allocations_between_collections_min = self.gc_profile.allocations_between_collections_min.min(allocations_between_collections);
            self.gc_profile.allocations_between_collections_max = self.gc_profile.allocations_between_collections_max.max(allocations_between_collections);
            self.gc_profile.survivors_min = self.gc_profile.survivors_min.min(live as u64);
            self.gc_profile.survivors_max = self.gc_profile.survivors_max.max(live as u64);
        }
    }
}
