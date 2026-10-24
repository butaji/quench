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
    /// The share of its expected live weight a full collection must reclaim to count as
    /// productive. After an unproductive one the allocation cap doubles, up to
    /// `maximum_cap_backoff` times; a productive one restores it (`PacingState::cap_backoff`).
    pub(super) productive_full: Ratio,
    pub(super) maximum_cap_backoff: u32,
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
    /// Growth of the young budget with smoothed survival (capped at `survival_limit`): the
    /// budget is `young_budget × (SURVIVAL_SCALE + survival_growth × survival) / SURVIVAL_SCALE`.
    /// A program whose young allocations survive collects them less often; one whose
    /// allocations die keeps its budget. Zero keeps the budget as `young` sets it.
    pub(super) survival_growth: u32,
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
    /// Doublings of the allocation cap after full collections that reclaimed little.
    pub(super) cap_backoff: u32,
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
        if outcome.full {
            // What a minor collection would have kept: the previous live weight and this
            // cycle's expected survivors. A full collection pays when it reclaims old garbage
            // beyond that. Past the survival limit the estimate measures nothing minor
            // collections could keep, and full collections are the only ones (`Regime`), so
            // the cap stays as set.
            let survivors = (outcome.allocated as u64 * u64::from(state.survival) / u64::from(SURVIVAL_SCALE)) as usize;
            let expected = outcome.previous_live_weight.saturating_add(survivors);
            let reclaimed = expected.saturating_sub(outcome.live_weight);
            state.cap_backoff = if state.regime(self) == Regime::FullOnly
                || reclaimed >= self.productive_full.of(expected)
            {
                0
            } else {
                (state.cap_backoff + 1).min(self.maximum_cap_backoff)
            };
        }
        if !outcome.full && outcome.allocated > 0 {
            let promoted = outcome.live_weight.saturating_sub(outcome.previous_live_weight);
            let survival = (promoted.min(outcome.allocated) as u64 * u64::from(SURVIVAL_SCALE)
                / outcome.allocated as u64) as u32;
            state.survival = (state.survival + survival) / 2;
        }
        let survival = u64::from(state.survival.min(self.survival_limit));
        let grown = self.young_budget(outcome.live) as u64
            * (u64::from(SURVIVAL_SCALE) + u64::from(self.survival_growth) * survival)
            / u64::from(SURVIVAL_SCALE);
        let young = NextCollection {
            threshold: usize::try_from(grown).unwrap_or(usize::MAX),
            full: self.full_due(outcome.live_after_full, outcome.live, outcome.since_full, state.cap_backoff)
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

    pub(super) fn full_due(&self, live_after_full: usize, live: usize, since_full: usize, cap_backoff: u32) -> bool {
        live.saturating_sub(live_after_full)
            >= self.old_growth.of(live_after_full).max(self.minimum_old_growth)
            || self.allocation_cap_reached(live_after_full, since_full, cap_backoff)
    }

    pub(super) fn weight_growth_due(&self, after_full: usize, live_weight: usize) -> bool {
        self.weight_growth.numerator > 0
            && live_weight.saturating_sub(after_full) >= self.weight_growth.of(after_full)
    }

    pub(super) fn allocation_cap_reached(&self, live_after_full: usize, since_full: usize, cap_backoff: u32) -> bool {
        let cap = self.allocation_cap.of(live_after_full).max(self.minimum_allocation_cap);
        since_full >= cap.checked_shl(cap_backoff).unwrap_or(usize::MAX)
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
            productive_full: ratio("QUENCH_GC_PRODUCTIVE_FULL", self.productive_full),
            maximum_cap_backoff: read("QUENCH_GC_MAX_CAP_BACKOFF", self.maximum_cap_backoff as usize) as u32,
            survival_limit: read("QUENCH_GC_SURVIVAL_LIMIT", self.survival_limit as usize) as u32,
            full_only_headroom: ratio("QUENCH_GC_FULL_ONLY", self.full_only_headroom),
            probe_interval: read("QUENCH_GC_PROBE_INTERVAL", self.probe_interval as usize) as u32,
            weight_growth: ratio("QUENCH_GC_WEIGHT_GROWTH", self.weight_growth),
            survival_growth: read("QUENCH_GC_SURVIVAL_GROWTH", self.survival_growth as usize) as u32,
        }
    }
}

/// How a runtime trades memory for speed (TOD-167): one choice per runtime, made at startup
/// (`Runtime::set_memory_policy`), that selects the collection pacing table, whether the
/// host returns pages at collection boundaries (`Host::at_collection_boundary`),
/// whether and how the loop tier compiles (`LOOP_TIER_OPT_LEVELS`), and whether the
/// general-dispatch tier compiles (`GENERAL_DISPATCH_TIER`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MemoryPolicy {
    /// Fewer, larger collections, and pages stay resident across them (`THROUGHPUT_PACING`).
    /// The default: on the memory-first settings Quench lost 5 of the 8 V8-v7 benchmarks it
    /// wins on these (EarleyBoyer and RayTrace at 0.42x the best rival), and HTTP servers ran
    /// at about half their throughput.
    #[default]
    Throughput,
    /// The smallest peak RSS: frequent young collections and pages returned at every
    /// collection boundary (`MEMORY_FIRST_PACING`).
    MemoryFirst,
}

impl MemoryPolicy {
    /// Every policy, in declaration order (`self as usize` indexes it).
    pub const ALL: [Self; 2] = [Self::Throughput, Self::MemoryFirst];

    /// The policy's name in configuration (an embedder's environment variable or flag).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Throughput => "throughput",
            Self::MemoryFirst => "memory-first",
        }
    }

    /// The policy `name` names, if any.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|policy| policy.name() == name)
    }

    /// Whether the host returns the memory the process no longer needs resident (read-only
    /// pages, the allocator's free pages) at collection boundaries. Under throughput it does
    /// not: releasing them after every collection refaulted the run's hot code after each
    /// young one (EarleyBoyer, dynamic x86_64 production build, 2026-10-20: ~63K minor faults
    /// and ~0.11 s of system time per run against ~15K and ~0.035 s without; v2-web).
    pub const fn returns_pages_at_collections(self) -> bool {
        matches!(self, Self::MemoryFirst)
    }

    /// The policy's own pacing table.
    const fn table(self) -> GenerationalPacing {
        match self {
            Self::Throughput => THROUGHPUT_PACING,
            Self::MemoryFirst => MEMORY_FIRST_PACING,
        }
    }

    #[cfg(not(feature = "gc-tuning"))]
    pub(super) fn pacing(self) -> GenerationalPacing {
        self.table()
    }

    /// Diagnostic builds read the environment's overrides over the policy's table, once.
    #[cfg(feature = "gc-tuning")]
    pub(super) fn pacing(self) -> GenerationalPacing {
        static TUNED: [std::sync::OnceLock<GenerationalPacing>; MemoryPolicy::ALL.len()] =
            [const { std::sync::OnceLock::new() }; MemoryPolicy::ALL.len()];
        *TUNED[self as usize].get_or_init(|| self.table().from_environment())
    }
}

/// Throughput pacing, the default (TOD-167): the JetStream lane's table (v2-cloud-jet), which
/// the other lanes' measurements bound. Young collections run every eighth of a live set, at
/// least `THROUGHPUT_MINIMUM_YOUNG` and at most `MAXIMUM_YOUNG` weight; a full one once the
/// old generation has doubled or one live set of allocation has passed since the last one, at
/// least `THROUGHPUT_MINIMUM_FULL_INTERVAL` apart; an unproductive full collection doubles the
/// allocation cap, at most twice; past half survival, full collections with one live set
/// between them, probing survival every fourth one.
///
/// - The eighth: the npm lane's young share (v2-npm abd3484b9, npm top-300, x86_64 4 CPUs,
///   matched against the memory-first table: median wall 63.1 -> 57.9 ms, fastest of three
///   143 -> 164 of 299, median peak RSS 14.4 -> 14.7 MiB) and the JetStream lane's (UniPoker
///   Score 12.6-13.1 -> 18.4-19.2 at max RSS 12 -> 16-17 MB, stanford-crypto-aes 25.7-27.5 ->
///   29.4-32.1 at 22.5 -> 32 MB, Bun no-JIT 47 MB; x86_64 4 vCPU, 2026-10-21). The Score and
///   web lanes ran eight live sets per young collection; with `MAXIMUM_YOUNG` (web) that is
///   the cap on any live set past 2^14, and this table reaches the same cap from 2^20.
/// - Full collections: one live set of growth or allocation, as under memory-first, with the
///   cap doubling after full collections that reclaim under an eighth of their expected live
///   weight. Two doublings reach the Score and web lanes' four live sets only where full
///   collections do not pay (v2-cloud-jet, x86_64 4 vCPU, 2026-10-22, median of 3-5 against
///   no back-off: source-map-wtb 6.58 -> 7.46 at max RSS 56 -> 58 MB, chai-wtb 14.6 -> 15.8,
///   web-ssr 18.9 -> 19.6 at 103 -> 110 MB, Bun 119.5; three doublings took web-ssr to 154 MB).
pub(super) const THROUGHPUT_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(1, 8),
    minimum_young: THROUGHPUT_MINIMUM_YOUNG,
    maximum_young: MAXIMUM_YOUNG,
    old_growth: Ratio::new(1, 1),
    minimum_old_growth: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    allocation_cap: Ratio::new(1, 1),
    minimum_allocation_cap: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    productive_full: Ratio::new(1, 8),
    maximum_cap_backoff: 2,
    survival_limit: SURVIVAL_SCALE / 2,
    full_only_headroom: Ratio::new(1, 1),
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    survival_growth: 0,
};

/// Memory-first pacing on the owner's direction of 2026-10-17 (TOD-123), selected by
/// `MemoryPolicy::MemoryFirst` since TOD-167: young collections every sixty-fourth of a live
/// set (at least 384 weight; from an eighth, with read-only pages released after every
/// collection: V8-v7 max RSS RegExp 7244-7500 -> 6852 KiB, EarleyBoyer 7792-8000 -> 9040-9160,
/// the floor-bound fixtures unchanged, static x86_64 unbolted, 2026-10-18; RSS is this
/// table's only criterion), a full collection once the old generation has doubled or one live
/// set of allocation has passed since the last one, and, when more than half of young
/// allocations survive, full collections with one live set between them, probing survival
/// with a minor collection every fourth one. The full-collection minimums are the pre-generational 384-cell
/// headroom. The Score lane's table (eight live sets per young collection, four per full one,
/// 2^15 and 2^18 minimums) raised npm peak RSS by up to 17 MiB per package (semver 27.6 ->
/// 45.2, typescript-eslint 188 -> 226).
///
/// The young floor is `YOUNG_MINIMUM_ALLOCATION`, not the 384 headroom: at a sixty-fourth of a
/// small live set the floor decides the interval, and 384 collected small programs every few
/// hundred allocations without lowering their peak (TOD-161, quench-node scorecard, x86_64
/// 4 vCPU, 2026-10-20: 768 against 384 gave V8-v7 Score x1.161 with RayTrace x1.54 and
/// RegExp x1.60 in 7/7 pairs, frameworks x1.023, max RSS geomean x0.997 and x0.999).
pub(super) const MEMORY_FIRST_PACING: GenerationalPacing = GenerationalPacing {
    young: Ratio::new(1, 64),
    minimum_young: YOUNG_MINIMUM_ALLOCATION,
    maximum_young: MAXIMUM_YOUNG,
    old_growth: Ratio::new(1, 1),
    minimum_old_growth: MEMORY_FIRST_MINIMUM_ALLOCATION,
    allocation_cap: Ratio::new(1, 1),
    minimum_allocation_cap: MEMORY_FIRST_MINIMUM_ALLOCATION,
    productive_full: Ratio::new(1, 8),
    // Memory-first: the cap never backs off (`maximum_cap_backoff`), as an unproductive full
    // collection's back-off trades max RSS for fewer collections (v2-cloud-jet 551f170f7).
    maximum_cap_backoff: 0,
    survival_limit: SURVIVAL_SCALE / 2,
    full_only_headroom: Ratio::new(1, 1),
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    survival_growth: 0,
};

/// The fewest allocations between collections under memory-first, whatever survived: the
/// pre-generational minimum headroom.
const MEMORY_FIRST_MINIMUM_ALLOCATION: usize = 384;
/// The most allocations between young collections, however large the live set, under either
/// policy. Without it the young budget is eight live sets: a server whose startup leaves a
/// few hundred thousand live cells (the Node library plus a framework) allocated millions
/// between collections, and its peak RSS was mostly garbage. Measured with the `gc-tuning`
/// build, Linux x86_64, 4 vCPU, 500 connections (evidence/web-gc-young-cap-linux-2026-10-19):
/// peak RSS hono 84 -> 53 MB, fastify 96 -> 72 MB, express 85 -> 53 MB; V8-v7 earley-boyer
/// 46 -> 28 MB and splay 161 -> 92 MB, with Scores within the run-to-run band. 2^16 cut little
/// more memory.
const MAXIMUM_YOUNG: usize = 1 << 17;

/// The fewest allocations between young collections under memory-first. At 768 (twice the
/// headroom, TOD-161) small heaps collected thousands of times, and collection-time page
/// release refaulted the run's code after each: V8-v7 Score x0.80 of throughput's, x0.82 with
/// `--jit`. At 2^13 that is x0.93 and x0.98 with max RSS geomean x0.64 and x0.65 of
/// throughput's against x0.63 and x0.62 at 768; 2^12 gave x0.89 and x0.94 (TOD-167 sweep,
/// static + BOLT x86_64, 4 vCPU, 2026-10-22, 5 and 3 runs; releasing pages only near the
/// peak added nothing at 2^13). 1024 raised koa's max RSS by 1.1 MB under the earlier table.
/// Since the first-call and memo-table changes, 2^12 against 2^13 lowers median max RSS on 14
/// of 16 V8-v7 rows by 76-416 KiB (Splay flat) and the worst run on 14, for Score x0.86-0.88 on
/// DeltaBlue, RayTrace and RegExp, x0.91 on EarleyBoyer and none elsewhere; this table's
/// criterion is RSS (one gc-tuning binary, 9 interleaved runs, x86_64 4 vCPU, 2026-10-23;
/// 2^11 cost RayTrace x0.65 for little more).
const YOUNG_MINIMUM_ALLOCATION: usize = 1 << 12;

/// The fewest allocations between young collections under throughput: above the npm lane's
/// 2^14 (measured only against memory-first's 768) and below the 2^17 that doubled small-heap
/// RSS (v2-cloud-jet: stanford-crypto-aes 29 -> 60 MB). 2^16 kept that under Bun's, and
/// memory-first's 768 left UniPoker at 0.7x this table's Score; the Score lane's 2^15 against
/// 2^17 was within noise on V8-v7 (v2-cloud-c).
const THROUGHPUT_MINIMUM_YOUNG: usize = 1 << 16;
/// The fewest allocations, or old-generation growth, between full collections under
/// throughput: the JetStream, Score and web lanes' common minimum (v2-cloud-jet, v2-cloud-c,
/// v2-web). Against memory-first's 384 the Score lane's table, which shares it, ran the Earley
/// half of EarleyBoyer in about 1190M instructions instead of 2318M, with 816 fewer
/// collections of the whole live set (v2-cloud-c, x86_64 4 vCPU).
const THROUGHPUT_MINIMUM_FULL_INTERVAL: usize = 1 << 18;

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
    fn policies_name_their_tables_and_page_release() {
        for (index, policy) in MemoryPolicy::ALL.into_iter().enumerate() {
            assert_eq!(policy as usize, index);
            assert_eq!(MemoryPolicy::from_name(policy.name()), Some(policy));
        }
        assert_eq!(MemoryPolicy::default(), MemoryPolicy::Throughput);
        assert_eq!(MemoryPolicy::from_name("speed"), None);
        assert!(MemoryPolicy::MemoryFirst.returns_pages_at_collections());
        assert!(!MemoryPolicy::Throughput.returns_pages_at_collections());
        // Throughput collects no more often than memory-first at any live size.
        let (throughput, memory_first) = (THROUGHPUT_PACING, MEMORY_FIRST_PACING);
        for live in [0, 1 << 10, 1 << 16, 1 << 20, 1 << 24] {
            assert!(throughput.young_budget(live) >= memory_first.young_budget(live));
        }
        assert!(throughput.minimum_allocation_cap > memory_first.minimum_allocation_cap);
        assert!(throughput.maximum_cap_backoff > memory_first.maximum_cap_backoff);
    }

    #[test]
    fn full_collection_follows_old_growth_or_the_allocation_cap() {
        for policy in MemoryPolicy::ALL {
            full_collection_follows_old_growth_or_the_allocation_cap_under(policy.table());
        }
    }

    fn full_collection_follows_old_growth_or_the_allocation_cap_under(pacing: GenerationalPacing) {
        let live = 1 << 20;
        let growth = pacing.old_growth.of(live);
        assert!(!pacing.full_due(live, live + growth - 1, 0, 0));
        assert!(pacing.full_due(live, live + growth, 0, 0));
        assert!(pacing.full_due(live, live, pacing.allocation_cap.of(live), 0));
        assert_eq!(pacing.young_budget(0), pacing.minimum_young);
        // A live set whose young share lies between the minimum and the maximum budget.
        let between = pacing.minimum_young * 2 * pacing.young.denominator / pacing.young.numerator;
        assert_eq!(pacing.young_budget(between), pacing.young.of(between));
        let huge = pacing.maximum_young * 1024;
        assert_eq!(pacing.young_budget(huge), pacing.maximum_young);
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
    fn unproductive_full_collections_back_off_the_allocation_cap_up_to_the_table_limit() {
        for policy in MemoryPolicy::ALL {
            let pacing = policy.table();
            let mut state = PacingState::default();
            let live = 1 << 20;
            // Full collections that keep everything they were expected to keep reclaim nothing.
            let unproductive = CollectionOutcome { full: true, since_full: 0, ..minor(live, live, 0) };
            for _ in 0..=pacing.maximum_cap_backoff {
                pacing.plan(&mut state, unproductive);
            }
            assert_eq!(state.cap_backoff, pacing.maximum_cap_backoff);
            let cap = pacing.allocation_cap.of(live).max(pacing.minimum_allocation_cap);
            let backed_off = cap << pacing.maximum_cap_backoff;
            assert!(!pacing.allocation_cap_reached(live, backed_off - 1, state.cap_backoff));
            assert!(pacing.allocation_cap_reached(live, backed_off, state.cap_backoff));
            // A productive one restores the cap.
            let productive = CollectionOutcome { live_weight: 0, ..unproductive };
            pacing.plan(&mut state, productive);
            assert_eq!(state.cap_backoff, 0);
        }
    }

    #[test]
    fn high_young_survival_switches_to_full_collections_and_probes_back() {
        for policy in MemoryPolicy::ALL {
            high_young_survival_switches_to_full_collections_and_probes_back_under(policy.table());
        }
    }

    fn high_young_survival_switches_to_full_collections_and_probes_back_under(pacing: GenerationalPacing) {
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
