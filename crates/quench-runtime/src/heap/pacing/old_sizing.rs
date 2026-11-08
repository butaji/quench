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
}

/// Where a heap's sizing stands and the clock readings that move it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OldGenerationState {
    /// Eighths the growth has tightened from `widest_eighths`.
    tightened: u32,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SIZING: OldGenerationSizing = OldGenerationSizing {
        tightest_eighths: 4,
        widest_eighths: 8,
        widen_from: Ratio::new(1, 13),
        tighten_below: Ratio::new(1, 13),
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
    fn the_first_full_collection_only_starts_the_clock() {
        let mut state = OldGenerationState::default();
        state.start_full();
        SIZING.after_collection(true, &mut state);
        assert_eq!(SIZING.growth(&state), Ratio::new(8, 8));
        assert!(state.last_full_end.is_some());
    }
}
