//! A collection's phases: the marking from its roots, which the sweep and the settling of the
//! survivors (`collect_sweep`, `collect_settle`) follow.
use super::*;

/// When a collection's traced phases started (`QUENCH_TRACE=gc`, and the cycle judgement).
#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub(super) trace: Option<std::time::Instant>,
    pub(super) full: Option<std::time::Instant>,
}

/// What a collection carries from its roots through its marking.
pub(super) struct Marking<'a, F> {
    pub(super) full: bool,
    pub(super) age: PromotionAge,
    pub(super) carries_owners: bool,
    pub(super) premarked: bool,
    pub(super) work: Vec<Value>,
    pub(super) remembered_owners: Vec<Value>,
    pub(super) object_roots: F,
    pub(super) environments: EnvironmentTrace<'a>,
    pub(super) environment_reads: EnvironmentReads,
    pub(super) trims_environments: bool,
    pub(super) closure_reads: Option<&'a dyn ClosureReadSource>,
    #[cfg(feature = "profile-aggregate")]
    pub(super) allocations_between: u64,
    #[cfg(debug_assertions)]
    pub(super) verify: VerifyRoots,
}

/// The roots a debug build checks the marking against (`debug_verify_full`, `debug_verify_minor`).
#[cfg(debug_assertions)]
pub(super) struct VerifyRoots {
    roots: Vec<Value>,
    old_records: Option<Vec<Value>>,
    remembered: Vec<Value>,
}

impl Heap {
    /// A collection of the heap from `roots` and the object roots of the VM (`object_roots`), with
    /// the closure environments `closure_reads` describes; the sweep frees what it did not reach.
    /// Returns the finalization jobs the sweep made due.
    pub(crate) fn collect_with_object_roots<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        object_roots: F,
        parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>,
        closure_reads: Option<&dyn ClosureReadSource>,
        full: bool,
    ) -> Vec<(Value, Value)> {
        let full = self.start_collection(full);
        // `QUENCH_TRACE=gc` reports the mark, sweep, and record-sweep times.
        let clock = Clock {
            trace: crate::trace::Channel::Gc.on().then(std::time::Instant::now),
            // A full collection's mark time chooses between cycles and stopping the world
            // (`Heap::judge_cycles`).
            full: full.then(std::time::Instant::now),
        };
        let mut marking = self.begin_marking(roots, object_roots, closure_reads, full);
        self.mark_from_roots(&mut marking, parallel_object_roots);
        let (settle, finalization_jobs) = self.finish_marking(marking, clock);
        self.sweep_and_settle(settle, finalization_jobs)
    }

    /// The start of a collection's marking: the per-collection tables and the cycle's marks, the
    /// roots gathered into the mark stack, and the environments the trace reads.
    fn begin_marking<'a, F>(&mut self, roots: impl IntoIterator<Item = Value>, object_roots: F, closure_reads: Option<&'a dyn ClosureReadSource>, full: bool) -> Marking<'a, F> {
        // The start of the mark: the per-collection tables, and the cycle's marks, which a
        // collection while a cycle runs finishes (the remark).
        self.small_views.clear();
        let age = self.promotion_age();
        // A minor collection that leaves survivors young carries the old owners that may point at
        // them to the next one (`carried_owners`): those the barriers logged, then those it promotes.
        let carries_owners = !full && age.ages();
        // The shared unit strings are no roots: one the sweep frees must not be handed out.
        if let Some(strings) = &mut self.ascii_unit_strings {
            strings.fill(Value::UNDEFINED);
        }
        self.charge_materialized_text();
        // The sweep may free an array whose record a later array reuses.
        retire_array_storage_proofs();
        debug_assert_eq!(self.collection_state, CollectionState::Idle);
        self.collection_state = if full { CollectionState::FullMarking } else { CollectionState::MinorMarking };
        // Live slots grow only by allocation and shrink only by sweeping, so the peak is
        // observed here and when it is reported rather than on every allocation.
        self.peak_live = self.peak_live.max(self.live_slots());
        self.collections += 1;
        self.marked_prototypes = [None; MARKED_PROTOTYPE_WAYS];
        #[cfg(feature = "profile-aggregate")]
        let allocations_between = self.allocated_since_collection() as u64;
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        if full {
            self.gc_profile.full_collections += 1;
        } else {
            self.gc_profile.minor_collections += 1;
            #[cfg(feature = "profile-aggregate")]
            {
                self.gc_profile.remembered_owner_scans += self.remembered.len() as u64;
                self.gc_profile.young_cells_swept += self.young.count() as u64;
            }
        }
        let premarked = self.complete_cycle_marking();
        debug_assert!(!premarked || full);
        self.last_collection_premarked = premarked;
        if full && self.rc_active {
            self.rc_full_start();
        }
        if full {
            self.clear_full_marks(premarked);
        }
        // The roots become the mark stack: collecting a vector's own iterator keeps its
        // buffer, so a collection holds one root-sized buffer, not a copy beside it.
        let mut work: Vec<Value> = roots.into_iter().collect();
        work.reserve(self.roots.slot_count());
        work.extend(self.roots.values());
        #[cfg(debug_assertions)]
        let verify_roots = work.clone();
        #[cfg(debug_assertions)]
        let verify_old_records = (!full).then(|| self.marked_record_values());
        #[cfg(debug_assertions)]
        let roots_len = work.len();
        // Under `carries_owners`, the old owners that may point at what stays young.
        let mut remembered_owners: Vec<Value> = Vec::new();
        if !full {
            self.gather_remembered(&mut work, &mut remembered_owners, carries_owners, age);
        }
        #[cfg(debug_assertions)]
        let verify = VerifyRoots {
            roots: verify_roots,
            old_records: verify_old_records,
            remembered: if full { Vec::new() } else { work[roots_len..].to_vec() },
        };
        // A full collection traces closure environments through what closures can read, once
        // the live set has grown since the last that did: environments over-captured since
        // then are what grew it, and slots cleared before stay cleared. Between them the
        // trace's per-environment state would only add to each full collection's peak
        // (V8-v7 max RSS +50 to +156 KiB on most fixtures when every full collection traced).
        // A remark traces only what the cycle did not reach, so it cannot tell which slots
        // closures read: environments are trimmed by stop-the-world collections only.
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            self.gc_profile.roots += work.len() as u64;
            self.gc_profile.max_worklist = self.gc_profile.max_worklist.max(work.len() as u64);
        }
        let trims_environments = full && !premarked && self.environment_trim_due();
        let environment_reads = if trims_environments { EnvironmentReads::Closures } else { EnvironmentReads::Projections };
        Marking {
            full,
            age,
            carries_owners,
            premarked,
            work,
            remembered_owners,
            object_roots,
            environments: EnvironmentTrace::new(closure_reads.filter(|_| full), environment_reads),
            environment_reads,
            trims_environments,
            closure_reads,
            #[cfg(feature = "profile-aggregate")]
            allocations_between,
            #[cfg(debug_assertions)]
            verify,
        }
    }

    /// Clears what a full collection starts without: the marks, unless a cycle marked them,
    /// the remembered sets, the carried owners, the ages, and the record spaces' marks.
    fn clear_full_marks(&mut self, premarked: bool) {
        #[cfg(feature = "profile-aggregate")]
        let mark_clear_started = std::time::Instant::now();
        if !premarked {
            Self::clear_marks(&self.marks);
        }
        self.remembered.clear();
        self.remembered_marks.fill(0);
        self.carried_owners.clear();
        // A full collection promotes every survivor, aged or not.
        self.aged.clear();
        for (family, _) in self.record_families_mut() {
            if !premarked {
                family.clear_marks();
            }
            family.forget_ages();
        }
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.mark_clear_nanos += mark_clear_started.elapsed().as_nanos() as u64;
        }
    }

    /// The values of the record spaces' marked records, for the debug verification of a minor
    /// collection, which the marking must leave marked.
    #[cfg(debug_assertions)]
    fn marked_record_values(&self) -> Vec<Value> {
        let mut old = Vec::new();
        for (family, handle) in self.record_families() {
            family.for_each_marked(&mut |id| old.push(handle(id)));
        }
        old
    }

    /// A minor collection's start from the barriers' logs: the owners the barriers logged and the
    /// ones carried from the last collection are roots; the newborn records they name are roots
    /// too, and `carries_owners` keeps the old owners that may point at what stays young.
    fn gather_remembered(&mut self, work: &mut Vec<Value>, remembered_owners: &mut Vec<Value>, carries_owners: bool, age: PromotionAge) {
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile.remembered_owner_scans += self.record_families().iter().map(|(family, _)| family.remembered_count()).sum::<usize>() as u64;
        }
        // What the barriers logged since the last collection is carried to the next one
        // (`carried_owners`); what the last one carried is traced, then dropped.
        if carries_owners {
            self.aged.resize(self.marks.len(), 0);
            remembered_owners.extend(self.remembered.iter().map(|&index| Value::heap(index)));
        }
        // The newborn records are named while the carried owners are still marked old, so
        // only the barriers' log needs excluding from them.
        let written = self.record_families_mut().map(|(family, _)| {
            let written = family.take_remembered();
            family.prepare_minor(&written, age);
            written
        });
        for owner in std::mem::take(&mut self.carried_owners) {
            self.write_barrier(owner);
        }
        for ((family, handle), mut logged) in self.record_families_mut().into_iter().zip(written) {
            let written = logged.len();
            logged.extend(family.take_remembered());
            let first = work.len();
            work.extend(logged.into_iter().map(handle));
            if carries_owners {
                remembered_owners.extend_from_slice(&work[first..first + written]);
            }
        }
        if self.rc_active {
            let started = std::time::Instant::now();
            self.rc_marking = true;
            self.rc_pre_mark(work);
            self.rc_trace[0] = started.elapsed().as_micros() as u64;
        }
    }

    /// Marks everything the marking starts from: the remembered cells of a minor collection,
    /// traced with their sparse elements and remembered ephemerons, then the worklist, in
    /// parallel rounds where the marking may.
    fn mark_from_roots<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, marking: &mut Marking<'_, F>, parallel_object_roots: Option<&ParallelObjectRootTrace<'_>>) {
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        let mark_started = std::time::Instant::now();
        let mut ephemerons = weak::EphemeronWork::default();
        if !marking.full {
            self.trace_remembered(marking, &mut ephemerons);
        }
        let parallel = self.parallel_mark(parallel_object_roots);
        self.mark_work(&mut marking.work, &mut marking.object_roots, parallel, &mut ephemerons, GcTraceKind::Collection, &mut marking.environments);
        #[cfg(any(feature = "profile-aggregate", feature = "profile-gc-time"))]
        {
            let mark_nanos = mark_started.elapsed().as_nanos() as u64;
            self.gc_profile.mark_nanos += mark_nanos;
            *if marking.full { &mut self.gc_profile.full_mark_nanos } else { &mut self.gc_profile.minor_mark_nanos } += mark_nanos;
        }
    }

    /// A minor collection's remembered cells, each traced as a root: its children, its sparse
    /// elements, and the remembered ephemerons it settles.
    fn trace_remembered<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, marking: &mut Marking<'_, F>, ephemerons: &mut weak::EphemeronWork) {
        let properties = &self.properties;
        let remembered = &self.remembered;
        let mut last_proto = PushedProtos::default();
        for index in remembered.iter().copied() {
            let value = Value::heap(index);
            let Some(cell) = self.get(value) else { continue };
            Self::children(value, cell, properties, &mut marking.work, &mut marking.object_roots, GcTraceKind::Collection, &mut last_proto);
            if let Some(arrays) = &self.sparse_arrays
                && let Some(elements) = arrays.get(&Value::heap(index))
            {
                marking.work.extend(elements.values.values().copied());
            }
            let mark_bits = mark_bits!(self);
            ephemerons.remembered(value, cell, |target| mark_bits.is_marked(target), &mut marking.work);
        }
    }

    /// After marking: the debug verification, the reference counts' check and epoch, the
    /// release of the unread environment slots, the mark stack kept, and the weak entries
    /// pruned. Returns the finalization jobs.
    fn finish_marking<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, marking: Marking<'_, F>, clock: Clock) -> (Settle, Vec<(Value, Value)>) {
        #[cfg(debug_assertions)]
        let verify = marking.verify;
        #[cfg(feature = "profile-aggregate")]
        let marking_allocations = marking.allocations_between;
        let Marking { full, age, carries_owners, premarked, remembered_owners, trims_environments, closure_reads, environment_reads, mut work, mut object_roots, environments, .. } = marking;
        #[cfg(debug_assertions)]
        self.verify_marking(full, verify, &mut object_roots, closure_reads, environment_reads);
        if self.rc_active && crate::trace::Dev::RcVerify.on() {
            if full {
                self.rc_check_suspects();
            } else {
                // The candidates the mark left are kept, and marked through, for the next
                // full collection to judge.
                let suspects = self.rc_unmarked_candidates();
                work.extend(suspects.iter().copied());
                self.rc_suspects.extend(suspects);
                let mut ephemerons = weak::EphemeronWork::default();
                self.mark_work(&mut work, &mut object_roots, None, &mut ephemerons, GcTraceKind::Collection, &mut environments);
            }
        }
        self.rc_marking = false;
        self.clear_unread_environment_slots(environments);
        self.mark_stack = {
            work.clear();
            work
        };
        let finalization_jobs = self.prune_weak_entries(full, age);
        if self.rc_active {
            let started = std::time::Instant::now();
            if full { self.rc_full_end() } else { self.rc_epoch() }
            self.rc_trace[1] = started.elapsed().as_micros() as u64;
        }
        let settle = Settle {
            clock,
            full,
            age,
            carries_owners,
            premarked,
            remembered_owners,
            trims_environments,
            closure_reads: closure_reads.is_some(),
            #[cfg(feature = "profile-aggregate")]
            allocations_between: marking_allocations,
        };
        (settle, finalization_jobs)
    }

    /// The debug build's check of the marking: a full collection's marks against a trace from the
    /// same roots, a minor collection's against the records it must leave marked.
    #[cfg(debug_assertions)]
    fn verify_marking<F: FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>)>(&mut self, full: bool, verify: VerifyRoots, object_roots: &mut F, closure_reads: Option<&dyn ClosureReadSource>, environment_reads: EnvironmentReads) {
        // The verifying marks count nothing.
        let rc_marking = std::mem::replace(&mut self.rc_marking, false);
        if full {
            self.debug_verify_full(&verify.roots, object_roots, closure_reads, environment_reads);
        } else {
            let mut pinned = verify.old_records.unwrap_or_default();
            pinned.extend(verify.remembered);
            self.debug_verify_minor(&verify.roots, &pinned, object_roots);
        }
        self.rc_marking = rc_marking;
    }
}

/// What a collection's sweep and settling need from its marking.
pub(super) struct Settle {
    pub(super) clock: Clock,
    pub(super) full: bool,
    pub(super) age: PromotionAge,
    pub(super) carries_owners: bool,
    pub(super) premarked: bool,
    pub(super) remembered_owners: Vec<Value>,
    pub(super) trims_environments: bool,
    pub(super) closure_reads: bool,
    #[cfg(feature = "profile-aggregate")]
    pub(super) allocations_between: u64,
}
