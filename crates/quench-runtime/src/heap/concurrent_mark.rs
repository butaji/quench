//! Mostly-concurrent full marking, snapshot at the beginning (SATB): which heaps may run a
//! cycle instead of a stop-the-world full collection, and the hook every operation that needs
//! a consistent heap calls first (`finish_cycle_now`).
//!
//! No cycle starts yet: `satb_active` stays false outside tests, so `finish_cycle_now` has
//! nothing to finish and the snapshot barrier (`RecordSpace::satb_note`) never runs.
use super::*;

/// Live legacy cells past which no cycle starts: the initial mark pushes every live cell's
/// children in its pause, as cells pass no snapshot barrier.
pub(super) const CONCURRENT_CYCLE_MAX_LEGACY_CELLS: usize = 64 * 1024;

/// The first gate a heap fails, which makes its next full collection stop the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CycleRefusal {
    /// Concurrent cycles are an experiment, on only under `QUENCH_DEV=concurrent-gc`.
    Disabled,
    /// A JIT tier may run: native stores inline the generational guard
    /// (`NATIVE_RECORD_MARK_OFFSET`) and would bypass the snapshot barrier. `--no-jit` only.
    NativeCode,
    /// Pacing favors minor collections; a cycle replaces full collections only where they
    /// alone reclaim memory (`Regime::FullOnly`).
    Generational,
    /// A weak collection, weak reference or finalization registry was made: ephemerons and
    /// weak clearing need a remark that knows them.
    WeakCells,
    /// The property arena lives in a moving allocation, not its fixed reservation, so a
    /// marker could read a buffer the mutator freed.
    MovingProperties,
    /// More live legacy cells than the initial mark's pause should visit
    /// (`CONCURRENT_CYCLE_MAX_LEGACY_CELLS`).
    LegacyCells,
    /// The memory profile attributes each record as the stop-the-world sweep frees it.
    ProfilingMemory,
}

impl Heap {
    /// Why this heap's next full collection cannot run as a concurrent cycle, or `None` when
    /// every gate passes.
    pub(crate) fn concurrent_cycle_refusal(&self) -> Option<CycleRefusal> {
        self.concurrent_cycle_refusal_with(
            crate::trace::Dev::ConcurrentGc.on(),
            crate::jit_mode::tiers_allowed(crate::jit_mode::Tier::ANY),
        )
    }

    /// `concurrent_cycle_refusal` with the process-wide inputs given.
    pub(super) fn concurrent_cycle_refusal_with(&self, enabled: bool, native_code: bool) -> Option<CycleRefusal> {
        if !enabled {
            Some(CycleRefusal::Disabled)
        } else if native_code {
            Some(CycleRefusal::NativeCode)
        } else if self.pacing_state.regime(&self.pacing()) != pacing::Regime::FullOnly {
            Some(CycleRefusal::Generational)
        } else if self.weak_cells_made {
            Some(CycleRefusal::WeakCells)
        } else if !self.properties.values_stay_in_place() {
            Some(CycleRefusal::MovingProperties)
        } else if self.live_slots() > CONCURRENT_CYCLE_MAX_LEGACY_CELLS {
            Some(CycleRefusal::LegacyCells)
        } else if cfg!(feature = "profile-memory") {
            Some(CycleRefusal::ProfilingMemory)
        } else {
            None
        }
    }

    /// Whether a concurrent cycle is marking: the snapshot barrier is on and allocation is
    /// black. Never set yet.
    #[inline(always)]
    pub(crate) fn satb_active(&self) -> bool {
        self.satb_active
    }

    /// Finishes a running concurrent cycle on this thread, before an operation that needs a
    /// consistent heap: an explicit or minor collection, a reset, a snapshot, a walk over
    /// every live cell. No cycle runs yet, so there is nothing to finish.
    #[inline(always)]
    pub(crate) fn finish_cycle_now(&mut self) {
        debug_assert!(!self.satb_active || cfg!(test), "no concurrent cycle starts yet");
    }
}

/// The snapshot barrier's copy of a record's children (`RecordSpace::satb_note`): what the
/// mark would push for it, with no shape roots (pushed per shape when a cycle starts) and
/// every string left to the cycle's mark.
fn snapshot_object(object: &Object, properties: &ValueArena, out: &mut Vec<Value>) {
    Heap::object_children(
        Value::UNDEFINED,
        object,
        properties,
        out,
        &mut |_, _, _, _, _| {},
        GcTraceKind::Collection,
        &mut PushedProtos::default(),
    );
}

impl records::SatbSnapshot for Object {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(self, properties, out);
    }
}

impl records::SatbSnapshot for ArrayObject {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        Heap::array_children(
            Value::UNDEFINED,
            self,
            properties,
            |_| false,
            out,
            &mut |_, _, _, _, _| {},
            GcTraceKind::Collection,
            &mut PushedProtos::default(),
        );
    }
}

impl records::SatbSnapshot for FunctionRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        out.extend([self.realm, self.env]);
    }
}

impl records::SatbSnapshot for PromiseCell {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        self.record.append_roots(out);
    }
}

impl records::SatbSnapshot for IteratorRecord {
    fn snapshot(&self, properties: &ValueArena, out: &mut Vec<Value>) {
        snapshot_object(&self.object, properties, out);
        Heap::iterator_edges(self, out);
    }
}

impl records::SatbSnapshot for AwaitState {
    fn snapshot(&self, _properties: &ValueArena, out: &mut Vec<Value>) {
        self.extend_roots(out);
    }
}

impl Heap {
    /// Turns the snapshot barrier on or off in every record family, as a concurrent cycle's
    /// start and remark will; the children it copies are taken with `take_satb_values`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn set_satb(&mut self, active: bool) {
        self.satb_active = active;
        for (family, _) in self.record_families_mut() {
            if active { family.begin_satb() } else { family.end_satb() }
        }
    }

    /// Allocation is black while a cycle marks: a cell made now is marked at once, newer than
    /// the snapshot, so the cycle neither traces nor frees it.
    #[cold]
    #[inline(never)]
    pub(super) fn blacken_slot(&mut self, index: usize) {
        let (word, bit) = slot_bit(index);
        *self.marks[word].get_mut() |= bit;
    }

    /// The children the snapshot barrier copied in every family since the last call.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn take_satb_values(&mut self) -> Vec<Value> {
        let mut values = Vec::new();
        for (family, _) in self.record_families_mut() {
            values.append(&mut family.take_satb_values());
        }
        values
    }
}

/// An incremental cycle's mark state between steps: the values left to mark and the state the
/// serial mark keeps across them. Steps run on the mutator thread (`Heap::mark_step`), between
/// which the mutator writes through the snapshot barrier and allocates black.
pub(super) struct ConcurrentCycle {
    work: Vec<Value>,
    ephemerons: weak::EphemeronWork,
    last_proto: PushedProtos,
}

impl Heap {
    /// Starts an incremental cycle, as a full collection starts: the marks are cleared, and
    /// the snapshot is pushed: `roots`, the root table, and the children of every live cell
    /// (cells pass no snapshot barrier, so what one holds now is all the cycle needs of it),
    /// with sparse elements. The snapshot barrier and black allocation then run until
    /// `finish_cycle`.
    ///
    /// The caller has passed the gates (`concurrent_cycle_refusal`): there is no weak cell, so
    /// no ephemeron waits on a key; shape roots are among `roots`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn begin_cycle(&mut self, roots: impl IntoIterator<Item = Value>) {
        assert!(self.cycle.is_none() && !self.satb_active, "one cycle at a time");
        debug_assert!(!self.weak_cells_made, "a heap with weak cells runs no cycle");
        Self::clear_marks(&self.marks);
        self.remembered.clear();
        self.remembered_marks.fill(0);
        self.carried_owners.clear();
        self.aged.clear();
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
            family.forget_ages();
        }
        let mut work: Vec<Value> = roots.into_iter().collect();
        work.extend(self.roots.values());
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(cell) = &slot.cell {
                Self::children(
                    Value::heap(index as u32),
                    cell,
                    &self.properties,
                    &mut work,
                    &mut |_, _, _, _, _| {},
                    GcTraceKind::Collection,
                    &mut last_proto,
                );
            }
        }
        if let Some(arrays) = &self.sparse_arrays {
            for elements in arrays.values() {
                work.extend(elements.values.values().copied());
            }
        }
        self.set_satb(true);
        self.cycle = Some(Box::new(ConcurrentCycle {
            work,
            ephemerons: weak::EphemeronWork::default(),
            last_proto: PushedProtos::default(),
        }));
    }

    /// Whether an incremental cycle is running (`begin_cycle`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn cycle_running(&self) -> bool {
        self.cycle.is_some()
    }

    /// Marks up to `budget` values of the running cycle on this thread, after taking what the
    /// snapshot barrier copied since the last step; whether its marking is done (nothing left
    /// to mark, nothing copied).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn mark_step(&mut self, budget: usize) -> bool {
        let mut cycle = self.cycle.take().expect("a running cycle");
        cycle.work.append(&mut self.take_satb_values());
        // No closure-read source: closure environment edges are ordinary edges, and every
        // environment is traced whole.
        let mut environments = EnvironmentTrace::new(None, EnvironmentReads::Projections);
        let ConcurrentCycle { work, ephemerons, last_proto } = &mut *cycle;
        for _ in 0..budget {
            let Some(value) = work.pop() else { break };
            self.mark_value(value, work, &mut |_, _, _, _, _| {}, ephemerons, GcTraceKind::Collection, &mut environments, last_proto);
        }
        debug_assert!(!environments.has_pending());
        let done = cycle.work.is_empty();
        self.cycle = Some(cycle);
        done && self.satb_values_empty()
    }

    fn satb_values_empty(&mut self) -> bool {
        self.record_families_mut().iter_mut().all(|(family, _)| family.satb_values_empty())
    }

    /// Finishes the running cycle: marks what is left, ends the snapshot barrier (which marks
    /// what the cycle allocated), and runs a full collection that keeps those marks, tracing
    /// `roots` again from them and sweeping: the remark. What the snapshot reached, and what
    /// was made since, survives.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn finish_cycle(&mut self, roots: impl IntoIterator<Item = Value>) -> Vec<(Value, Value)> {
        while !self.mark_step(usize::MAX) {}
        self.cycle = None;
        self.set_satb(false);
        self.cycle_marked = true;
        self.collect_with_object_roots(roots, |_, _, _, _, _| {}, None, None, true)
    }
}
