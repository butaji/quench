//! Mostly-concurrent full marking, snapshot at the beginning (SATB): which heaps may run a
//! cycle instead of a stop-the-world full collection, and the hook every operation that needs
//! a consistent heap calls first (`finish_cycle_now`).
//!
//! No cycle starts yet: `satb_active` stays false, so `finish_cycle_now` has nothing to finish
//! and every barrier branch on it is never taken.
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
        debug_assert!(!self.satb_active, "no concurrent cycle starts yet");
    }
}
