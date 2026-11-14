//! Adaptive old-generation sizing, after HotSpot's `GCTimeRatio` with `MinHeapFreeRatio` /
//! `MaxHeapFreeRatio` and OpenJ9's `-Xmint` / `-Xmaxt`: the growth past what the last full
//! collection kept that makes the next one full widens while full collections take a large
//! share of the time and tightens while they take little, between the table's bounds.

use super::Ratio;
use std::time::Instant;

/// The unit of the growth bounds: eighths of what the last full collection kept.
const GROWTH_DENOMINATOR: usize = 8;

/// One table's old-generation sizing (`GenerationalPacing::old_sizing`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OldGenerationSizing {
    /// The old growth and allocation cap between full collections, in eighths of what the
    /// last full collection kept, at their tightest and their widest, where a heap starts.
    pub(super) tightest_eighths: u32,
    pub(super) widest_eighths: u32,
    /// The share of the time since the previous full collection that the last one took,
    /// at or above which the growth widens one eighth.
    pub(super) widen_from: Ratio,
    /// The share below which it tightens one eighth.
    pub(super) tighten_below: Ratio,
    /// The full-only headroom (`Regime::FullOnly`), in eighths of what the last full
    /// collection kept, at its tightest, where a heap starts, and at its widest.
    pub(super) headroom_tightest_eighths: u32,
    pub(super) headroom_widest_eighths: u32,
    /// Eighths the headroom moves per full collection.
    pub(super) headroom_step_eighths: u32,
    /// The shares of time a full collection takes at or above which the headroom widens and
    /// below which it tightens: where collections cost little, a tighter headroom buys memory.
    pub(super) headroom_widen_from: Ratio,
    pub(super) headroom_tighten_below: Ratio,
}

/// Where a heap's sizing stands and the clock readings that move it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OldGenerationState {
    /// Eighths the growth has tightened from `widest_eighths`.
    tightened: u32,
    /// Eighths the full-only headroom has widened from `headroom_tightest_eighths`.
    headroom_widened: u32,
    /// When the full collection under way started.
    full_started: Option<Instant>,
    /// When the last full collection ended.
    last_full_end: Option<Instant>,
}

impl OldGenerationState {
    /// Notes the start of a full collection.
    pub(crate) fn start_full(&mut self) {
        self.full_started = Some(Instant::now());
    }
}

impl OldGenerationSizing {
    /// The growth ratio a heap starts at, which the tables' own old growth and allocation
    /// cap hold until `GenerationalPacing::sized` sets the heap's.
    pub(super) const fn widest(&self) -> Ratio {
        Ratio::new(self.widest_eighths as usize, GROWTH_DENOMINATOR)
    }

    /// The growth ratio now: the old growth and allocation cap of `GenerationalPacing`.
    pub(super) fn growth(&self, state: &OldGenerationState) -> Ratio {
        let eighths = self.widest_eighths.saturating_sub(state.tightened).max(self.tightest_eighths);
        Ratio::new(eighths as usize, GROWTH_DENOMINATOR)
    }

    /// The headroom between full collections while most young allocations survive: a share
    /// of what the last full collection kept (`GenerationalPacing::plan`).
    pub(super) fn headroom(&self, state: &OldGenerationState) -> Ratio {
        let eighths = (self.headroom_tightest_eighths + state.headroom_widened).min(self.headroom_widest_eighths);
        Ratio::new(eighths as usize, GROWTH_DENOMINATOR)
    }

    /// After a collection, when `full`: widens or tightens the growth one eighth from the
    /// share of the time since the previous full collection's end that this one took.
    pub(super) fn after_collection(&self, full: bool, state: &mut OldGenerationState) {
        if !full {
            return;
        }
        let now = Instant::now();
        let started = state.full_started.take();
        let previous = state.last_full_end.replace(now);
        let (Some(started), Some(previous)) = (started, previous) else {
            return;
        };
        let full = now.saturating_duration_since(started).as_nanos();
        let cycle = now.saturating_duration_since(previous).as_nanos();
        let (full, cycle) = (usize::try_from(full).unwrap_or(usize::MAX), usize::try_from(cycle).unwrap_or(usize::MAX));
        let range = self.widest_eighths.saturating_sub(self.tightest_eighths);
        if full >= self.widen_from.of(cycle) {
            state.tightened = state.tightened.saturating_sub(1);
        } else if full < self.tighten_below.of(cycle) {
            state.tightened = (state.tightened + 1).min(range);
        }
        let headroom_range = self.headroom_widest_eighths.saturating_sub(self.headroom_tightest_eighths);
        if full >= self.headroom_widen_from.of(cycle) {
            state.headroom_widened = (state.headroom_widened + self.headroom_step_eighths).min(headroom_range);
        } else if full < self.headroom_tighten_below.of(cycle) {
            state.headroom_widened = state.headroom_widened.saturating_sub(self.headroom_step_eighths);
        }
    }
}

/// Throughput's old-generation sizing: a full collection once the old generation has grown
/// by what the last one kept (two live sets, the fixed table before) while full collections
/// take `THROUGHPUT_FULL_TIME_TARGET` of the time or more, tightening an eighth at a time to
/// half of what it kept while they take less.
pub(super) const THROUGHPUT_OLD_SIZING: OldGenerationSizing = OldGenerationSizing {
    tightest_eighths: 4,
    widest_eighths: 8,
    widen_from: THROUGHPUT_FULL_TIME_TARGET,
    tighten_below: THROUGHPUT_FULL_TIME_TARGET,
    headroom_tightest_eighths: THROUGHPUT_FULL_ONLY_HEADROOM_TIGHTEST,
    headroom_widest_eighths: THROUGHPUT_FULL_ONLY_HEADROOM_WIDEST,
    headroom_step_eighths: THROUGHPUT_FULL_ONLY_HEADROOM_STEP,
    headroom_widen_from: THROUGHPUT_FULL_ONLY_TIME_TARGET,
    headroom_tighten_below: THROUGHPUT_FULL_ONLY_TIME_TARGET,
};
/// The share of time full collections may take before the old generation widens: G1's
/// default `GCTimeRatio` of 12, one part in thirteen.
const THROUGHPUT_FULL_TIME_TARGET: Ratio = Ratio::new(1, 13);
/// Memory-first's old-generation sizing: fixed at what the last full collection kept, its
/// table's growth before sizing adapted.
pub(super) const MEMORY_FIRST_OLD_SIZING: OldGenerationSizing = OldGenerationSizing {
    tightest_eighths: 8,
    widest_eighths: 8,
    widen_from: THROUGHPUT_FULL_TIME_TARGET,
    tighten_below: THROUGHPUT_FULL_TIME_TARGET,
    headroom_tightest_eighths: MEMORY_FIRST_FULL_ONLY_HEADROOM_EIGHTHS,
    headroom_widest_eighths: MEMORY_FIRST_FULL_ONLY_HEADROOM_EIGHTHS,
    headroom_step_eighths: 0,
    headroom_widen_from: THROUGHPUT_FULL_TIME_TARGET,
    headroom_tighten_below: THROUGHPUT_FULL_TIME_TARGET,
};

/// Allocation between full collections, as a share of the live set, once most of each young
/// generation survives (`Regime::FullOnly`) under throughput, in eighths: from a live set and
/// a half, where a heap starts, to three live sets as full collections take a quarter of the
/// time (`OldGenerationSizing::headroom`; doxbee-promise max RSS 68 -> 51-60 MB, doxbee-async
/// 43 -> 36.5, Splay unchanged: its full collections widen it, x86_64 4 vCPU, 2026-11-12).
/// Three live sets: a heap whose
/// survivors fill each young generation, as V8-v7 Splay's tree does, collects only fully, and
/// each full collection marks the whole live set: at one live set Splay ran 26 full
/// collections a run (2026-11-01: Score 5140-5204 at 31.8 MB; 3/2: 5960-6124 at 39.3 MB). With
/// strings of parts (2026-11-11, experiment profile, 8 interleaved runs): 3/2 5613 at 56 MB, 3: 5934 at 82 MB (kept), 4: 6131 at 100 MB (JIT adds 15-25 MB, bar 113).
const THROUGHPUT_FULL_ONLY_HEADROOM_WIDEST: u32 = 24;
const THROUGHPUT_FULL_ONLY_HEADROOM_TIGHTEST: u32 = 12;
/// Eighths the headroom moves per full collection: half a live set, three steps across the range.
const THROUGHPUT_FULL_ONLY_HEADROOM_STEP: u32 = 4;
/// The share of time a full collection takes at which the headroom turns from tightening to widening: a quarter.
const THROUGHPUT_FULL_ONLY_TIME_TARGET: Ratio = Ratio::new(1, 4);

/// Memory-first lets half a live set of allocation pass between full collections once most of
/// each young generation survives, not a whole one, which let the heap reach twice its live
/// set. V8-v7 MemoryFirst, static x86_64 production build, 3 interleaved runs, worst max RSS /
/// Score (2026-10-27): Splay 31668 -> 24792 KiB / 5388 -> 4617, EarleyBoyer 6704 -> 6072 /
/// 4437 -> 3951, the six rows that stay generational unchanged; within the Score this table
/// already trades for RSS (`YOUNG_MINIMUM_ALLOCATION`). A quarter cut Splay to 21596 KiB but
/// its Score to 3411, and the TypeScript compiler on `@types/node` (live set about 70 MB)
/// is within 1% either way at a half.
const MEMORY_FIRST_FULL_ONLY_HEADROOM_EIGHTHS: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SIZING: OldGenerationSizing = OldGenerationSizing {
        tightest_eighths: 4,
        widest_eighths: 8,
        widen_from: Ratio::new(1, 13),
        tighten_below: Ratio::new(1, 13),
        headroom_tightest_eighths: 12,
        headroom_widest_eighths: 24,
        headroom_step_eighths: 4,
        headroom_widen_from: Ratio::new(1, 4),
        headroom_tighten_below: Ratio::new(1, 4),
    };

    /// A full collection that took `full` of a cycle of `cycle` since the last one ended.
    fn full_taking(state: &mut OldGenerationState, full: Duration, cycle: Duration) {
        let now = Instant::now();
        state.last_full_end = now.checked_sub(cycle);
        state.full_started = now.checked_sub(full);
        SIZING.after_collection(true, state);
    }

    #[test]
    fn cheap_full_collections_tighten_to_the_bound_and_costly_ones_widen() {
        let mut state = OldGenerationState::default();
        assert_eq!(SIZING.growth(&state), Ratio::new(8, 8));
        SIZING.after_collection(false, &mut state);
        assert_eq!(SIZING.growth(&state), Ratio::new(8, 8), "a minor collection moves nothing");
        for _ in 0..8 {
            full_taking(&mut state, Duration::from_millis(1), Duration::from_secs(1));
        }
        assert_eq!(SIZING.growth(&state), Ratio::new(4, 8));
        full_taking(&mut state, Duration::from_millis(500), Duration::from_secs(1));
        assert_eq!(SIZING.growth(&state), Ratio::new(5, 8));
    }

    #[test]
    fn full_only_headroom_starts_tight_widens_under_costly_collections_and_tightens_when_cheap() {
        let mut state = OldGenerationState::default();
        assert_eq!(SIZING.headroom(&state), Ratio::new(12, 8));
        full_taking(&mut state, Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(SIZING.headroom(&state), Ratio::new(12, 8), "cheap collections keep the tightest headroom");
        for _ in 0..4 {
            full_taking(&mut state, Duration::from_millis(500), Duration::from_secs(1));
        }
        assert_eq!(SIZING.headroom(&state), Ratio::new(24, 8), "costly ones widen to the bound");
        full_taking(&mut state, Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(SIZING.headroom(&state), Ratio::new(20, 8));
    }

    #[test]
    fn the_first_full_collection_only_starts_the_clock() {
        let mut state = OldGenerationState::default();
        state.start_full();
        SIZING.after_collection(true, &mut state);
        assert_eq!(SIZING.growth(&state), Ratio::new(8, 8));
        assert!(state.last_full_end.is_some());
    }
}
