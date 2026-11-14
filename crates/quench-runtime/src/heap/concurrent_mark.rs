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
