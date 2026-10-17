//! When to collect, and whether a collection is minor or full.
//!
//! Survivors of every collection stay marked (old). A minor collection traces roots and
//! the old owners the write barriers logged, and sweeps only unmarked (young) cells and
//! records, so its cost follows survivors and garbage rather than the old heap. A full
//! collection clears the marks and traces everything. Budgets are in allocation weight:
//! one per cell or record plus one per `PAYLOAD_BYTES_PER_ALLOCATION` of payload.

/// `numerator / denominator` of a live count, without overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ratio {
    pub(super) numerator: usize,
    pub(super) denominator: usize,
}

impl Ratio {
    pub(super) const fn new(numerator: usize, denominator: usize) -> Self {
        assert!(denominator > 0);
        Self {
            numerator,
            denominator,
        }
    }

    pub(super) fn of(self, live: usize) -> usize {
        let whole = live / self.denominator;
        let remainder = live % self.denominator;
        whole
            .saturating_mul(self.numerator)
            .saturating_add(remainder.saturating_mul(self.numerator) / self.denominator)
    }
}

/// The one authority for collection pacing.
#[derive(Clone, Copy, Debug)]
pub(super) struct GenerationalPacing {
    /// Allocation weight between collections, as a share of the live count.
    pub(super) young: Ratio,
    /// The smallest allocation weight between collections, whatever survived.
    pub(super) minimum_young: usize,
    /// Growth of the old generation since the last full collection, as a share of what that
    /// collection kept, that makes the next collection full.
    pub(super) old_growth: Ratio,
    /// The smallest old growth that makes the next collection full.
    pub(super) minimum_old_growth: usize,
    /// Allocation weight since the last full collection, as a share of what it kept, after
    /// which a collection is full whatever the old generation did: shape, program and
    /// property-arena compaction run only on full collections.
    pub(super) allocation_cap: Ratio,
    pub(super) minimum_allocation_cap: usize,
}

impl GenerationalPacing {
    pub(super) fn young_budget(&self, live: usize) -> usize {
        self.young.of(live).max(self.minimum_young)
    }

    pub(super) fn full_due(&self, live_after_full: usize, live: usize, since_full: usize) -> bool {
        live.saturating_sub(live_after_full)
            >= self.old_growth.of(live_after_full).max(self.minimum_old_growth)
            || self.allocation_cap_reached(live_after_full, since_full)
    }

    pub(super) fn allocation_cap_reached(&self, live_after_full: usize, since_full: usize) -> bool {
        since_full >= self.allocation_cap.of(live_after_full).max(self.minimum_allocation_cap)
    }

    /// Diagnostic builds may override each knob from the environment to measure a policy
    /// table; production reads only the constants.
    #[cfg(feature = "gc-tuning")]
    pub(super) fn from_environment(self) -> Self {
        fn read(name: &str, default: usize) -> usize {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        }
        fn ratio(name: &str, default: Ratio) -> Ratio {
            Ratio::new(
                read(&format!("{name}_NUM"), default.numerator),
                read(&format!("{name}_DEN"), default.denominator).max(1),
            )
        }
        Self {
            young: ratio("QUENCH_GC_YOUNG", self.young),
            minimum_young: read("QUENCH_GC_MIN_YOUNG", self.minimum_young),
            old_growth: ratio("QUENCH_GC_OLD_GROWTH", self.old_growth),
            minimum_old_growth: read("QUENCH_GC_MIN_OLD_GROWTH", self.minimum_old_growth),
            allocation_cap: ratio("QUENCH_GC_ALLOCATION_CAP", self.allocation_cap),
            minimum_allocation_cap: read("QUENCH_GC_MIN_ALLOCATION_CAP", self.minimum_allocation_cap),
        }
    }
}

/// Production pacing. Starting point for the policy table (TOD-137 stage 3): young
/// collections every half live set (at least 2^15), a full collection once the old
/// generation has doubled, and never more than eight live sets of allocation between
/// full collections.
pub(super) const PRODUCTION_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(1, 2),
    minimum_young: 1 << 15,
    old_growth: Ratio::new(1, 1),
    minimum_old_growth: 1 << 15,
    allocation_cap: Ratio::new(8, 1),
    minimum_allocation_cap: 1 << 18,
};

#[cfg(not(feature = "gc-tuning"))]
pub(super) fn pacing() -> GenerationalPacing {
    PRODUCTION_PACING
}

#[cfg(feature = "gc-tuning")]
pub(super) fn pacing() -> GenerationalPacing {
    static PACING: std::sync::OnceLock<GenerationalPacing> = std::sync::OnceLock::new();
    *PACING.get_or_init(|| PRODUCTION_PACING.from_environment())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_of_live_does_not_overflow_or_lose_remainders() {
        assert_eq!(Ratio::new(1, 2).of(7), 3);
        assert_eq!(Ratio::new(3, 2).of(7), 10);
        assert_eq!(Ratio::new(8, 1).of(usize::MAX), usize::MAX);
    }

    #[test]
    fn full_collection_follows_old_growth_or_the_allocation_cap() {
        let pacing = PRODUCTION_PACING;
        let live = 1 << 20;
        assert!(!pacing.full_due(live, live + (live / 2), 0));
        assert!(pacing.full_due(live, 2 * live, 0));
        assert!(pacing.full_due(live, live, 8 * live));
        assert_eq!(pacing.young_budget(0), pacing.minimum_young);
        assert_eq!(pacing.young_budget(live), live / 2);
    }
}
