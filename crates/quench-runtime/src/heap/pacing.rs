//! When to collect, and whether a collection is minor or full.
//!
//! Survivors of every collection stay marked (old). A minor collection traces roots and
//! the old owners the write barriers logged, and sweeps only unmarked (young) cells and
//! records, so its cost follows survivors and garbage rather than the old heap. A full
//! collection clears the marks and traces everything. Allocation is charged in weight:
//! one per cell or record plus one per `PAYLOAD_BYTES_PER_ALLOCATION` of payload
//! (`Cell::payload_bytes`), recounted from the survivors by every full sweep. Survival
//! compares live weight with allocated weight, so payload growth of old arrays is not
//! mistaken for young cells surviving. Budgets stay
//! proportional to live cells and records: memory the weight does not see (compiled
//! code, the property arena) is held by cells, and sizing budgets by live weight let it
//! grow (web-ssr: 123 -> 307 MB).

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
    pub(super) maximum_young: usize,
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
    /// Smoothed young survival, in `SURVIVAL_SCALE` parts, above which minor collections
    /// stop paying: they would mark nearly everything they were meant to skip. The heap
    /// then collects fully, with `full_only_headroom` between collections.
    pub(super) survival_limit: u32,
    pub(super) full_only_headroom: Ratio,
    /// Full collections between minor collections that probe survival again.
    pub(super) probe_interval: u32,
    /// Growth of live weight (cells, records and payload) since the last full collection, as
    /// a share of what it kept, that makes the next collection full; zero disables it.
    pub(super) weight_growth: Ratio,
}

/// Fixed-point scale of the survival estimate.
pub(super) const SURVIVAL_SCALE: u32 = 1 << 10;

/// Which kind of collection the measured survival favors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Regime {
    /// Young allocations mostly die: minor collections reclaim them cheaply.
    Generational,
    /// Young allocations mostly survive: only full collections reclaim memory.
    FullOnly,
}

/// What pacing learned from the collections so far. Every transition is in
/// `GenerationalPacing::plan`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PacingState {
    /// Smoothed share of young allocations that survived their first minor collection.
    survival: u32,
    /// Full collections left before a minor collection probes survival again.
    probe_in: u32,
}

impl PacingState {
    pub(super) fn regime(&self, pacing: &GenerationalPacing) -> Regime {
        if self.survival > pacing.survival_limit {
            Regime::FullOnly
        } else {
            Regime::Generational
        }
    }
}

/// What a finished collection measured.
#[derive(Clone, Copy, Debug)]
pub(super) struct CollectionOutcome {
    pub(super) full: bool,
    /// Live cells and records after this collection, which size the budgets.
    pub(super) live: usize,
    /// Live weight (cells, records and their payload) after this collection and the
    /// previous one, which measures survival in the unit allocation is charged in.
    pub(super) live_weight: usize,
    pub(super) previous_live_weight: usize,
    pub(super) live_after_full: usize,
    /// Live weight after the last full collection.
    pub(super) live_weight_after_full: usize,
    /// Weight allocated between the previous collection and this one.
    pub(super) allocated: usize,
    /// Allocation weight since the last full collection.
    pub(super) since_full: usize,
}

/// When the next collection runs (allocation weight from now) and whether it is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NextCollection {
    pub(super) threshold: usize,
    pub(super) full: bool,
}

impl GenerationalPacing {
    /// The state transition after each collection, and the plan for the next one.
    pub(super) fn plan(&self, state: &mut PacingState, outcome: CollectionOutcome) -> NextCollection {
        if !outcome.full && outcome.allocated > 0 {
            let promoted = outcome.live_weight.saturating_sub(outcome.previous_live_weight);
            let survival = (promoted.min(outcome.allocated) as u64 * u64::from(SURVIVAL_SCALE)
                / outcome.allocated as u64) as u32;
            state.survival = (state.survival + survival) / 2;
        }
        let young = NextCollection {
            threshold: self.young_budget(outcome.live),
            full: self.full_due(outcome.live_after_full, outcome.live, outcome.since_full)
                || self.weight_growth_due(outcome.live_weight_after_full, outcome.live_weight),
        };
        match state.regime(self) {
            Regime::Generational => young,
            Regime::FullOnly => {
                if outcome.full {
                    state.probe_in = state.probe_in.saturating_sub(1);
                } else {
                    state.probe_in = self.probe_interval;
                }
                if outcome.full && state.probe_in == 0 {
                    return NextCollection { full: false, ..young };
                }
                // A probe runs inside the interval between full collections; it does not
                // lengthen it.
                let interval = self.full_only_headroom.of(outcome.live_after_full);
                NextCollection {
                    threshold: interval
                        .saturating_sub(outcome.since_full)
                        .max(self.minimum_young),
                    full: true,
                }
            }
        }
    }


    pub(super) fn young_budget(&self, live: usize) -> usize {
        self.young
            .of(live)
            .min(self.maximum_young)
            .max(self.minimum_young)
    }

    pub(super) fn full_due(&self, live_after_full: usize, live: usize, since_full: usize) -> bool {
        live.saturating_sub(live_after_full)
            >= self.old_growth.of(live_after_full).max(self.minimum_old_growth)
            || self.allocation_cap_reached(live_after_full, since_full)
    }

    pub(super) fn weight_growth_due(&self, after_full: usize, live_weight: usize) -> bool {
        self.weight_growth.numerator > 0
            && live_weight.saturating_sub(after_full) >= self.weight_growth.of(after_full)
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
            maximum_young: read("QUENCH_GC_MAX_YOUNG", self.maximum_young),
            old_growth: ratio("QUENCH_GC_OLD_GROWTH", self.old_growth),
            minimum_old_growth: read("QUENCH_GC_MIN_OLD_GROWTH", self.minimum_old_growth),
            allocation_cap: ratio("QUENCH_GC_ALLOCATION_CAP", self.allocation_cap),
            minimum_allocation_cap: read("QUENCH_GC_MIN_ALLOCATION_CAP", self.minimum_allocation_cap),
            survival_limit: read("QUENCH_GC_SURVIVAL_LIMIT", self.survival_limit as usize) as u32,
            full_only_headroom: ratio("QUENCH_GC_FULL_ONLY", self.full_only_headroom),
            probe_interval: read("QUENCH_GC_PROBE_INTERVAL", self.probe_interval as usize) as u32,
            weight_growth: ratio("QUENCH_GC_WEIGHT_GROWTH", self.weight_growth),
        }
    }
}

/// Production pacing on the Score lane (`v2-cloud-c`), whose owner sets V8-v7 Score as its
/// only target (2026-10-18, reaffirmed 2026-10-20: "only score, don't focus on mem/rss"),
/// diverging from the trunk's memory-first table (TOD-123: young collections every
/// sixty-fourth of a live set, a 768-weight young floor and a 2^17 young cap, 384-weight
/// full minimums, one live set of old growth or allocation per full collection). Young
/// collections run every eight live sets (at least `SCORE_MINIMUM_YOUNG`, with no cap), a
/// full collection once the old generation has grown by four live sets or four live sets of
/// allocation have passed since the last one (at least `SCORE_MINIMUM_FULL_INTERVAL`), and,
/// when more than half of young allocations survive, full collections with four live sets
/// between them, probing survival with a minor collection every fourth one; the live-weight
/// trigger stays off. Measured with the `gc-tuning` build against the trunk table, Linux
/// x86_64, 4 vCPU, three-round medians: Splay 4566 -> 5931, RayTrace 1910 -> 2430,
/// DeltaBlue 951 -> 1135, EarleyBoyer 1980 -> 2167, RegExp 931 -> 1042; doubling the
/// factors and minimums again, or a 2^17 young minimum, was within noise. Against the
/// trunk's 2026-10-20 table the Earley half of EarleyBoyer runs 2318M -> about 1190M
/// instructions (816 collections of the whole live set fewer). The trunk keeps its
/// memory-first table (this one raised npm peak RSS by up to 17 MiB per package); merges
/// from it keep this one on this lane.
pub(super) const PRODUCTION_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(8, 1),
    minimum_young: SCORE_MINIMUM_YOUNG,
    maximum_young: usize::MAX,
    old_growth: Ratio::new(4, 1),
    minimum_old_growth: SCORE_MINIMUM_FULL_INTERVAL,
    allocation_cap: Ratio::new(4, 1),
    minimum_allocation_cap: SCORE_MINIMUM_FULL_INTERVAL,
    survival_limit: SURVIVAL_SCALE / 2,
    full_only_headroom: Ratio::new(4, 1),
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
};

/// The fewest allocations between collections, whatever survived.
const SCORE_MINIMUM_YOUNG: usize = 1 << 15;
/// The fewest allocations, or old-generation growth, between full collections.
const SCORE_MINIMUM_FULL_INTERVAL: usize = 1 << 18;

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
        let growth = pacing.old_growth.of(live);
        assert!(!pacing.full_due(live, live + growth - 1, 0));
        assert!(pacing.full_due(live, live + growth, 0));
        assert!(pacing.full_due(live, live, pacing.allocation_cap.of(live)));
        assert_eq!(pacing.young_budget(0), pacing.minimum_young);
        // A live set whose young share lies between the minimum and the maximum budget.
        let between = pacing.minimum_young * 2 * pacing.young.denominator / pacing.young.numerator;
        assert_eq!(pacing.young_budget(between), pacing.young.of(between));
        // A capped table holds a huge live set's budget at its cap.
        if let Some(huge) = pacing.maximum_young.checked_mul(1024) {
            assert_eq!(pacing.young_budget(huge), pacing.maximum_young);
        }
    }

    fn minor(live: usize, previous_live: usize, allocated: usize) -> CollectionOutcome {
        CollectionOutcome {
            full: false,
            live,
            live_weight: live,
            previous_live_weight: previous_live,
            live_after_full: previous_live,
            live_weight_after_full: previous_live,
            allocated,
            since_full: allocated,
        }
    }

    #[test]
    fn high_young_survival_switches_to_full_collections_and_probes_back() {
        let pacing = PRODUCTION_PACING;
        let mut state = PacingState::default();
        let live = 1 << 20;
        // Young allocations die: minors.
        let next = pacing.plan(&mut state, minor(live, live, live / 2));
        assert_eq!(state.regime(&pacing), Regime::Generational);
        assert!(!next.full);
        // Nearly everything young survives, twice: full collections with full-only headroom.
        pacing.plan(&mut state, minor(live + live / 2, live, live / 2));
        let next = pacing.plan(&mut state, minor(2 * live, 3 * live / 2, live / 2));
        assert_eq!(state.regime(&pacing), Regime::FullOnly);
        // The next full collection keeps the interval measured from the last full one.
        let interval = pacing.full_only_headroom.of(3 * live / 2) - live / 2;
        assert_eq!(next, NextCollection { threshold: interval.max(pacing.minimum_young), full: true });
        // After `probe_interval` full collections, a minor probes survival again.
        let full = CollectionOutcome { full: true, since_full: 0, ..minor(live, live, 4 * live) };
        for _ in 1..pacing.probe_interval {
            assert!(pacing.plan(&mut state, full).full);
        }
        assert!(!pacing.plan(&mut state, full).full);
        // The probe finds young allocations dying again: back to minors.
        pacing.plan(&mut state, minor(live, live, live / 2));
        pacing.plan(&mut state, minor(live, live, live / 2));
        assert_eq!(state.regime(&pacing), Regime::Generational);
    }
}
