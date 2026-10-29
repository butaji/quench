//! When to collect, and whether a collection is minor or full.
//!
//! Survivors of a full collection, and of as many minor ones as the promotion age counts
//! (`PromotionAge`), stay marked (old). A minor collection traces roots and the old owners
//! the write barriers logged or that point at what stays young, and sweeps only unmarked
//! (young) cells and records, so its cost follows survivors and garbage rather than the old
//! heap. A full collection clears the marks and traces everything. Allocation is charged in
//! weight: one per cell or record plus one per `PAYLOAD_BYTES_PER_ALLOCATION` of payload
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
    /// Smoothed young survival, in `SURVIVAL_SCALE` parts, from which the young budget is
    /// `surviving_young_factor` times larger (`surviving_young_budget`): where much of each
    /// young generation survives a collection, much of it is short-lived data that a later
    /// collection would find dead, and each minor collection marks it all.
    pub(super) surviving_young_from: u32,
    pub(super) surviving_young_factor: usize,
    /// Growth of the live set between two full collections, as a share of what the earlier
    /// kept, within which the live set counts as steady (`PacingState::steady`). Only a steady
    /// heap's young budget widens: a heap still growing, as while a program loads its
    /// modules, keeps what its minor collections promote, and a wider budget only raised its
    /// peak (npm top-300: +5-16 MB on the heaviest rows with the factor alone).
    pub(super) steady_live_growth: Ratio,
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
    /// Backing-store bytes (ArrayBuffer contents) allocated since the last collection that
    /// make the next one due, at least; the budget grows to the live external bytes a
    /// collection leaves (`Heap::charge_external_growth`). Buffers die only with their cells,
    /// so streaming code that drops one buffer per chunk retains every chunk until a
    /// collection, and this bounds that garbage.
    pub(super) minimum_external_budget: usize,
    /// The minor collections a young cell or record survives before it is old
    /// (`PromotionAge`), unless promotions are dying old (`promoted_garbage_aging`).
    pub(super) promotion_age: PromotionAge,
    /// The share of the weight minor collections promoted since the last full collection that
    /// a full collection must find dead for minors to promote at `PromotionAge::OLDEST`
    /// instead (`PacingState::promotion_age`): objects that outlive one young collection and
    /// die soon after, as a server's in-flight requests do. Zero disables it.
    pub(super) promoted_garbage_aging: Ratio,
    /// Full collections after aging was turned off, for freeing too few of the survivors it
    /// aged, before promotions dying old may turn it on again. Aging stays on while it frees
    /// what it ages (`AGED_DEATH_TO_KEEP_AGING`): with it on, little that minors promote dies
    /// old, so the promoted-garbage share would turn it off exactly where it works.
    pub(super) aging_interval: u32,
}

/// How many minor collections a young cell or record survives before it is promoted (old,
/// traced and swept again only by a full collection). Promoting on the first survival frees
/// nothing that dies soon after it, as a server's request objects or a parser's short-lived
/// lists do, until a full collection; the second survival lets one more minor collection
/// find them dead. A young survivor's age is one aged bit beside its mark bit (`Heap::aged`,
/// the record blocks' `aged` words), so the oldest age is `PromotionAge::OLDEST`.
///
/// Under `Second` the survival pacing measures (`GenerationalPacing::plan`) is net: live
/// weight after a minor collection counts this collection's first survivors, less the aged
/// survivors of the previous one that died since. Under `First`, nothing survives young, and
/// it is first survival exactly, as before ages existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromotionAge {
    /// Every minor survivor is promoted: no cell or record is young after a collection.
    First = 1,
    /// A first-time survivor stays young (aged); an aged survivor is promoted.
    Second = 2,
}

impl PromotionAge {
    /// Every age, youngest first.
    pub(crate) const ALL: [Self; 2] = [Self::First, Self::Second];
    /// The most minor collections any policy lets an object survive young.
    pub(crate) const OLDEST: Self = Self::ALL[Self::ALL.len() - 1];

    /// The minor collections a survivor needs to become old.
    pub(crate) const fn minors(self) -> u8 {
        self as u8
    }

    /// Whether a first-time minor survivor stays young, aged.
    pub(crate) const fn ages(self) -> bool {
        matches!(self, Self::Second)
    }

    /// The age of `minors` survivals, if one bit can count it.
    #[cfg(feature = "gc-tuning")]
    fn from_minors(minors: usize) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|age| usize::from(age.minors()) == minors)
    }
}

/// Memory-first promotes on the first survived minor collection: the second was neutral on its
/// peaks (V8-v7 and five JetStream rows within +-3% max RSS, gc-tuning build, x86_64 4 vCPU,
/// 2026-10-28), so it would only add the second trace of every survivor.
const MEMORY_FIRST_PROMOTION_AGE: PromotionAge = PromotionAge::First;
/// Throughput promotes on the first survived minor collection, and on the second while full
/// collections find promotions dying old (`THROUGHPUT_PROMOTED_GARBAGE_AGING`). Always the
/// second, a server's request objects, which outlive one young collection and die before the
/// next, were freed by minor collections (web's tput.sh, 500 connections, final VmHWM,
/// gc-tuning build of v2-web 191db23f3b with v2-gc 9cfcb96743, x86_64 4 vCPU, 2026-10-28,
/// 3 rounds, medians: fastify 39.6 -> 37.8 MB, hono 42.8 -> 39.1 MB; with
/// `THROUGHPUT_MAXIMUM_YOUNG` at 2^20, 37.2 and 33.4 MB, Bun 50-52 and 37-40), but cost V8-v7
/// EarleyBoyer 10.2% and Splay 1.5% of instructions (v2-cloud-c, fixed-work callgrind, all
/// threads), three fifths of it rebuilding the remembered sets (`Heap::remember_young_edges`)
/// and the rest the second trace. Aging only while it frees what it ages, carrying the old
/// owners a minor logged to the next instead of rebuilding remembered sets
/// (`Heap::carried_owners`), and turned on by consecutive fulls
/// (`FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE`), keeps both (gc-tuning build of v2-gc-adaptive-aging
/// d56fb2f5d5, x86_64 4 vCPU, 2026-10-29): final VmHWM at 500 connections, 3 rounds, medians,
/// fastify 34.1 MB against 35.8 never aging and 34.1 always, hono 35.7 against 38.5 and 35.1;
/// EarleyBoyer's long run 14.06G instructions against 14.05G never and 14.70G always, and
/// fixed-work EarleyBoyer -0.7%, +0.2% and +0.6% against never at three lengths.
const THROUGHPUT_PROMOTION_AGE: PromotionAge = PromotionAge::First;
/// The share of what minors promoted that a full collection must find dead to promote at the
/// second survival: half, so most promotions were garbage held to the full collection.
const THROUGHPUT_PROMOTED_GARBAGE_AGING: Ratio = Ratio::new(1, 2);
/// Full collections after aging was turned off before it may turn on again.
const THROUGHPUT_AGING_INTERVAL: u32 = 4;
/// Memory-first measures no aging: `MEMORY_FIRST_PROMOTION_AGE` always.
const NO_AGING: Ratio = Ratio::new(0, 1);

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
    /// What the last full collection kept, which the next one compares its own with
    /// (`steady`); zero before the first.
    live_after_last_full: usize,
    /// Whether the last full collection kept no more than the one before it, within
    /// `steady_live_growth`: the live set has stopped growing, so what minor collections keep
    /// dies before a full one (`surviving_young_budget`).
    steady: bool,
    /// Weight minor collections promoted since the last full collection, which it compares
    /// with the old garbage it reclaims (`GenerationalPacing::promoted_garbage_aging`).
    promoted_since_full: usize,
    /// Whether minors promote at `PromotionAge::OLDEST`: turned on by promotions dying old,
    /// off by aged survivors outliving their second minor collection.
    aging: bool,
    /// Smoothed share, in `SURVIVAL_SCALE` parts, of aged survivors their second minor
    /// collection freed, while aging is on (`AGED_DEATH_TO_KEEP_AGING`); none until the first
    /// minor collection after aging turned on measures it.
    aged_death: Option<u32>,
    /// Consecutive full collections that found promotions dying old, which turn aging on at
    /// `FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE`.
    promoted_garbage_fulls: u32,
    /// Full collections left before promotions dying old may turn aging on again, after aging
    /// was turned off for freeing too few of the survivors it aged.
    aging_off_for: u32,
}

impl PacingState {
    /// The minor collections a survivor needs to become old now: the table's, or the oldest
    /// while promotions were found dying old (`GenerationalPacing::promoted_garbage_aging`).
    pub(super) fn promotion_age(&self, pacing: &GenerationalPacing) -> PromotionAge {
        if self.aging { PromotionAge::OLDEST } else { pacing.promotion_age }
    }

    pub(super) fn regime(&self, pacing: &GenerationalPacing) -> Regime {
        if self.survival > pacing.survival_limit {
            Regime::FullOnly
        } else {
            Regime::Generational
        }
    }
}

/// What a minor collection that kept survivors young found of the aged ones
/// (`PromotionAge::Second`): freed at their second minor collection, or promoted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgedFates {
    pub(crate) died: usize,
    pub(crate) promoted: usize,
}

impl AgedFates {
    pub(crate) fn add(&mut self, other: AgedFates) {
        self.died += other.died;
        self.promoted += other.promoted;
    }
}

/// The smoothed share, in `SURVIVAL_SCALE` parts, of aged survivors their second minor
/// collection frees, under which aging is turned off: tracing a survivor twice costs about
/// what promoting one that dies old does, so aging pays while at least half die young.
const AGED_DEATH_TO_KEEP_AGING: u32 = SURVIVAL_SCALE / 2;

/// Consecutive full collections that must find promotions dying old to turn aging on: one
/// such full is often a phase change (a start-up's garbage), whose aging is traced twice and
/// promoted anyway.
const FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE: u32 = 2;

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
    /// What this collection found of the survivors the previous one aged.
    pub(super) aged: AgedFates,
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
            if state.aging {
                // Measured by its aged survivors' fates, below.
            } else if state.aging_off_for > 0 {
                state.aging_off_for -= 1;
            } else if self.promoted_garbage_aging.numerator > 0
                && state.promoted_since_full > 0
                && reclaimed >= self.promoted_garbage_aging.of(state.promoted_since_full)
            {
                state.promoted_garbage_fulls += 1;
                if state.promoted_garbage_fulls >= FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
                    state.aging = true;
                    state.promoted_garbage_fulls = 0;
                    // Aging must show it frees what it ages from its first measurement on.
                    state.aged_death = None;
                }
            } else {
                state.promoted_garbage_fulls = 0;
            }
            state.promoted_since_full = 0;
            state.steady = state.live_after_last_full != 0
                && outcome.live
                    <= state.live_after_last_full.saturating_add(self.steady_live_growth.of(state.live_after_last_full));
            state.live_after_last_full = outcome.live;
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
            state.promoted_since_full = state.promoted_since_full.saturating_add(promoted);
            let aged = outcome.aged.died + outcome.aged.promoted;
            if state.aging && aged > 0 {
                let death = (outcome.aged.died as u64 * u64::from(SURVIVAL_SCALE) / aged as u64) as u32;
                let smoothed = state.aged_death.map_or(death, |previous| (previous + death) / 2);
                state.aged_death = Some(smoothed);
                // The survivors aging keeps young mostly outlive their second minor collection:
                // each is traced twice and promoted anyway.
                if smoothed < AGED_DEATH_TO_KEEP_AGING {
                    state.aging = false;
                    state.aging_off_for = self.aging_interval;
                }
            }
            let survival = (promoted.min(outcome.allocated) as u64 * u64::from(SURVIVAL_SCALE)
                / outcome.allocated as u64) as u32;
            state.survival = (state.survival + survival) / 2;
        }
        let young = NextCollection {
            threshold: self.surviving_young_budget(outcome.live, state.survival, state.steady),
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


    /// `young_budget` of a heap whose smoothed young survival is `survival`, and whose live
    /// set has stopped growing when `steady`.
    pub(super) fn surviving_young_budget(&self, live: usize, survival: u32, steady: bool) -> usize {
        let budget = self.young_budget(live);
        if steady && survival >= self.surviving_young_from {
            budget.saturating_mul(self.surviving_young_factor)
        } else {
            budget
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
            surviving_young_from: read("QUENCH_GC_SURVIVING_YOUNG_FROM", self.surviving_young_from as usize) as u32,
            surviving_young_factor: read("QUENCH_GC_SURVIVING_YOUNG_FACTOR", self.surviving_young_factor),
            steady_live_growth: ratio("QUENCH_GC_STEADY_LIVE_GROWTH", self.steady_live_growth),
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
            minimum_external_budget: read("QUENCH_GC_MIN_EXTERNAL", self.minimum_external_budget),
            promotion_age: PromotionAge::from_minors(read(
                "QUENCH_GC_PROMOTION_AGE",
                usize::from(self.promotion_age.minors()),
            ))
            .unwrap_or(self.promotion_age),
            promoted_garbage_aging: ratio("QUENCH_GC_PROMOTED_GARBAGE_AGING", self.promoted_garbage_aging),
            aging_interval: read("QUENCH_GC_AGING_INTERVAL", self.aging_interval as usize) as u32,
        }
    }
}

/// How a runtime trades memory for speed (TOD-167): one choice per runtime, made at startup
/// (`Runtime::set_memory_policy`), that selects the collection pacing table, whether the
/// host returns pages at collection boundaries (`Host::at_collection_boundary`),
/// whether and how the loop tier compiles (`LOOP_TIER_OPT_LEVELS`), whether the
/// JavaScript function tiers compile (`JS_FUNCTION_TIERS`), and how much IR a Wasm module's
/// compile workers hold at once (`WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT`).
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
/// least `THROUGHPUT_MINIMUM_YOUNG` and at most `THROUGHPUT_MAXIMUM_YOUNG` weight
/// (`THROUGHPUT_SURVIVING_YOUNG_FACTOR` times that past a quarter's young survival on a steady
/// heap); a full one once the
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
    maximum_young: THROUGHPUT_MAXIMUM_YOUNG,
    surviving_young_from: SURVIVING_YOUNG_FROM,
    surviving_young_factor: THROUGHPUT_SURVIVING_YOUNG_FACTOR,
    steady_live_growth: STEADY_LIVE_GROWTH,
    old_growth: Ratio::new(1, 1),
    minimum_old_growth: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    allocation_cap: Ratio::new(1, 1),
    minimum_allocation_cap: THROUGHPUT_MINIMUM_FULL_INTERVAL,
    productive_full: Ratio::new(1, 8),
    maximum_cap_backoff: 2,
    survival_limit: THROUGHPUT_SURVIVAL_LIMIT,
    full_only_headroom: Ratio::new(1, 1),
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    minimum_external_budget: THROUGHPUT_MINIMUM_EXTERNAL_BUDGET,
    promotion_age: THROUGHPUT_PROMOTION_AGE,
    promoted_garbage_aging: THROUGHPUT_PROMOTED_GARBAGE_AGING,
    aging_interval: THROUGHPUT_AGING_INTERVAL,
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
    // RSS is this table's first criterion (`MEMORY_FIRST_SURVIVING_YOUNG_FACTOR`).
    surviving_young_from: SURVIVING_YOUNG_FROM,
    surviving_young_factor: MEMORY_FIRST_SURVIVING_YOUNG_FACTOR,
    steady_live_growth: STEADY_LIVE_GROWTH,
    old_growth: Ratio::new(1, 1),
    minimum_old_growth: MEMORY_FIRST_MINIMUM_ALLOCATION,
    allocation_cap: Ratio::new(1, 1),
    minimum_allocation_cap: MEMORY_FIRST_MINIMUM_ALLOCATION,
    productive_full: Ratio::new(1, 8),
    // Memory-first: the cap never backs off (`maximum_cap_backoff`), as an unproductive full
    // collection's back-off trades max RSS for fewer collections (v2-cloud-jet 551f170f7).
    maximum_cap_backoff: 0,
    survival_limit: SURVIVAL_SCALE / 2,
    full_only_headroom: MEMORY_FIRST_FULL_ONLY_HEADROOM,
    probe_interval: 4,
    weight_growth: Ratio::new(0, 1),
    minimum_external_budget: MEMORY_FIRST_MINIMUM_EXTERNAL_BUDGET,
    promotion_age: MEMORY_FIRST_PROMOTION_AGE,
    promoted_garbage_aging: NO_AGING,
    aging_interval: 0,
};

/// The external budget's floor (`GenerationalPacing::minimum_external_budget`) under
/// throughput. Like V8's external memory limit it bounds dead buffers without collecting at
/// every chunk: on the SaltyAom video route (64 KiB chunks, Linux x86_64, 4 vCPU) one
/// collection per 64 KiB buffer cut peak RSS 112 -> 47 MB and throughput by two thirds. A
/// stream that reads each chunk into a fresh buffer (`fs.createReadStream`) left the floor
/// of dead buffers resident: at 8 MiB a fastify server's video phase added 3.5-5.8 MB, and
/// web's board found 2-4 MiB as low as 1 MiB there. Below 4 MiB hono's video route, which
/// collects about ninety times a second anyway, paid for the extra collections: at 1 MiB
/// 1447-1680 collections in 10 s against 923-961 at 8 MiB and 898-959 at 4 MiB, 3.5% fewer
/// requests a second and a 1.4 MB higher peak (pinned server, bombardier 10 connections,
/// 4 rounds, plain x86_64 build, 2026-10-26).
const THROUGHPUT_MINIMUM_EXTERNAL_BUDGET: usize = 4 << 20;
/// The external budget's floor under memory-first: the lowest fastify video-phase peak of the
/// sweep above, run under throughput pacing (final VmHWM 39.0-40.2 MB against 39.8-41.3 at
/// 1 MiB and 39.4-43.1 at 8 MiB); 256 KiB gave no lower peak. Memory-first trades the extra
/// collections for it.
const MEMORY_FIRST_MINIMUM_EXTERNAL_BUDGET: usize = 512 << 10;

/// Memory-first lets half a live set of allocation pass between full collections once most of
/// each young generation survives, not a whole one, which let the heap reach twice its live
/// set. V8-v7 MemoryFirst, static x86_64 production build, 3 interleaved runs, worst max RSS /
/// Score (2026-10-27): Splay 31668 -> 24792 KiB / 5388 -> 4617, EarleyBoyer 6704 -> 6072 /
/// 4437 -> 3951, the six rows that stay generational unchanged; within the Score this table
/// already trades for RSS (`YOUNG_MINIMUM_ALLOCATION`). A quarter cut Splay to 21596 KiB but
/// its Score to 3411, and the TypeScript compiler on `@types/node` (live set about 70 MB)
/// is within 1% either way at a half.
const MEMORY_FIRST_FULL_ONLY_HEADROOM: Ratio = Ratio::new(1, 2);

/// The fewest allocations between collections under memory-first, whatever survived: the
/// pre-generational minimum headroom.
const MEMORY_FIRST_MINIMUM_ALLOCATION: usize = 384;
/// The most allocations between young collections, however large the live set, under
/// memory-first. Without it the young budget is eight live sets: a server whose startup leaves a
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
/// RSS (v2-cloud-jet: stanford-crypto-aes 29 -> 60 MB); memory-first's 768 left UniPoker at
/// 0.7x this table's Score, and the Score lane's 2^15 against 2^17 was within noise on V8-v7
/// (v2-cloud-c). Three quarters of 2^16: JetStream rows whose young generations are all
/// garbage peak lower with no Score cost (v2-cloud-jet, one gc-tuning binary, cold, 7
/// interleaved runs, x86_64 4 vCPU, 2026-10-25; Score / max RSS): stanford-crypto-aes +10% /
/// 34.8 -> 30.4 MB, bigint-noble-ed25519 +4% / 31.4 -> 28.7, UniPoker flat / 22.1 -> 19.9,
/// hash-map flat / 27.3 -> 25.9, pbkdf2 and sha256 within noise, acorn-wtb +4% / +2.6 MB.
const THROUGHPUT_MINIMUM_YOUNG: usize = 3 << 14;
/// The most allocations between young collections under throughput: 2^19, for Score, which
/// this table serves (memory-first keeps RSS first). V8-v7 fixed work, gc-tuning build,
/// callgrind main thread, 2^19 young with the full interval at 2^21 against 3 << 14 and 2^18
/// (v2-cloud-c, x86_64 4 vCPU, 2026-10-28): EarleyBoyer x0.838, RegExp x0.883, Splay x0.889,
/// Crypto x0.974, DeltaBlue x0.980, RayTrace x0.982, Richards x0.994, NavierStokes x0.993 of
/// instructions; stock Score, 7 CPU-alternating pairs: EarleyBoyer x1.08, RayTrace x1.05,
/// Splay x1.03. A ceiling, not a floor: as the floor too it made every young generation that
/// size, and a server's young generations are mostly garbage held while hundreds of requests
/// are in flight (TOD-167, gc-tuning build, 500 connections, ping/query/body/video, final
/// VmHWM, 2 rounds): fastify 81.5 / 78.1 MB and hono 90.7 / 86.3 MB with 2^19 fixed and the
/// full interval at 2^21; 80.4 / 78.3 and 89.0 / 89.8 with only the old-growth floor back at
/// 2^18; 42.2 / 40.7 and 41.6 / 39.4 with this ceiling over `THROUGHPUT_MINIMUM_YOUNG` and
/// `THROUGHPUT_MINIMUM_FULL_INTERVAL` at 2^18 (before the raise: 43.9 and 45.0 MB), request
/// rates within the run spread. Raised to 2^20 with `THROUGHPUT_PROMOTION_AGE`: fastify
/// 37.6 / 37.8 -> 37.1 / 37.2 MB, hono 36.2 / 34.2 -> 33.4 / 33.4 MB (2 rounds); a live set
/// past 2^23 reaches it, which no V8-v7 row does (EarleyBoyer and Splay unchanged).
const THROUGHPUT_MAXIMUM_YOUNG: usize = 1 << 20;
/// The fewest allocations, or old-generation growth, between full collections under
/// throughput: the JetStream, Score and web lanes' common minimum (v2-cloud-jet, v2-cloud-c,
/// v2-web). Against memory-first's 384 the Score lane's table, which shares it, ran the Earley
/// half of EarleyBoyer in about 1190M instructions instead of 2318M, with 816 fewer
/// collections of the whole live set (v2-cloud-c, x86_64 4 vCPU). At 2^21 a server's promoted
/// request objects, which die soon after their first young collection, wait that long for a
/// full one (`THROUGHPUT_MAXIMUM_YOUNG`'s rows).
const THROUGHPUT_MINIMUM_FULL_INTERVAL: usize = 1 << 18;
/// Smoothed young survival past which throughput collects fully (`Regime::FullOnly`): three
/// quarters. npm module loading keeps about 52% of each young generation (QUENCH_GC_TRACE,
/// typescript-eslint: 531/1024), and at half the heap went FullOnly and fully re-marked the
/// growing module graph four times (live weight 159K -> 248K -> 383K -> 603K). At three
/// quarters it stays generational there (v2-npm, Throughput, no JIT, warm snapshot, 4-vCPU
/// x86_64 VM, 2026-10-24, 9 interleaved runs, CPU / max RSS): typescript-eslint
/// 841 -> 767 ms / 126.4 -> 122.6 MB (fulls 4 -> 2, full mark 115 -> 28 ms),
/// @typescript-eslint/eslint-plugin 940 -> 841 / 125.8 -> 124.1, @testing-library/dom
/// 728 -> 719 / 97.6 -> 93.6, playwright 476 -> 472 / 77.9 -> 77.9, jsdom (median of 15)
/// 500 -> 428 / 85.6 -> 83.8; packages without full collections within noise.
const THROUGHPUT_SURVIVAL_LIMIT: u32 = SURVIVAL_SCALE * 3 / 4;
/// Smoothed young survival from which a steady heap's young budget widens, under either
/// policy (`GenerationalPacing::surviving_young_from`): a quarter. V8-v7 survival
/// (QUENCH_GC_TRACE, per 1024): RayTrace 1, Crypto 5, DeltaBlue 20, RegExp 116,
/// EarleyBoyer 322; Splay's 950 collects fully (`THROUGHPUT_SURVIVAL_LIMIT`). A larger young
/// budget for every heap cost RayTrace x0.89 of Score (its young generations are all garbage
/// and a larger one only leaves the caches), where EarleyBoyer, whose minor collections each
/// marked about 4.8M instructions' worth of survivors, gained.
const SURVIVING_YOUNG_FROM: u32 = SURVIVAL_SCALE / 4;
/// Live-set growth between full collections within which a heap counts as steady
/// (`GenerationalPacing::steady_live_growth`): an eighth. EarleyBoyer's live set after each
/// full collection moves within 6-10K weight in both directions; a loading jsdom grows
/// 121K -> 188K between its two.
const STEADY_LIVE_GROWTH: Ratio = Ratio::new(1, 8);
/// The young budget's factor past `SURVIVING_YOUNG_FROM` under throughput, on a steady heap
/// only (`PacingState::steady`): two. Eight gave EarleyBoyer Score x1.025 at max RSS
/// 11.1 -> 15.1 MB (pinned, 11 runs, 2026-10-26; ungated x1.047), and on npm's heaviest rows
/// the steady gate kept max RSS within 0.5 MB of the unwidened table and CPU within 5%
/// (TOD-197; ungated, the factor raised it 5-16 MB). On JetStream's steady heaps eight was
/// mostly garbage: against two, max RSS geomean x1.07 over the 34 default rows for Score
/// x1.006, web-ssr 106 -> 163 MB, prettier-wtb 42 -> 65, sync-fs 25 -> 42, hash-map 31 -> 45,
/// doxbee-async 45 -> 54 (median of 3, Score +6.7%); one against two gave RSS x0.99 more
/// for doxbee-async's Score x0.94 (one gc-tuning binary, cold, interleaved, x86_64 4 vCPU,
/// 2026-10-27, v2-cloud-jet).
const THROUGHPUT_SURVIVING_YOUNG_FACTOR: usize = 2;
/// The young budget's factor past `SURVIVING_YOUNG_FROM` under memory-first: one, so
/// memory-first's budget does not widen. Four, on a steady heap, gave EarleyBoyer Score
/// 4002 -> 4629 at 7328 -> 7740 KiB but RegExp 6316 -> 6932 KiB for no Score (pinned, 7 runs,
/// 2026-10-26): one warm-up window of RegExp crosses a quarter's survival. Neither requiring
/// up to four steady full collections nor a steady tolerance up to one live set separates the
/// two; a survival threshold does only in a 288-320 band, too narrow to hold beyond V8-v7.
const MEMORY_FIRST_SURVIVING_YOUNG_FACTOR: usize = 1;

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
        // Survival widens a steady heap's young budget past a quarter only.
        let below = SURVIVING_YOUNG_FROM - 1;
        assert_eq!(throughput.surviving_young_budget(0, below, true), throughput.young_budget(0));
        assert_eq!(
            throughput.surviving_young_budget(0, SURVIVING_YOUNG_FROM, true),
            throughput.young_budget(0) * THROUGHPUT_SURVIVING_YOUNG_FACTOR
        );
        // A growing heap keeps its budget whatever survives.
        assert_eq!(
            throughput.surviving_young_budget(0, SURVIVAL_SCALE, false),
            throughput.young_budget(0)
        );
        assert_eq!(
            memory_first.surviving_young_budget(0, SURVIVAL_SCALE, true),
            memory_first.young_budget(0) * MEMORY_FIRST_SURVIVING_YOUNG_FACTOR
        );
        assert_eq!(
            memory_first.surviving_young_budget(0, SURVIVAL_SCALE, false),
            memory_first.young_budget(0)
        );
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
        // The growth and the allocation that make a full collection due, each at least the
        // table's minimum interval.
        let growth = pacing.old_growth.of(live).max(pacing.minimum_old_growth);
        let cap = pacing.allocation_cap.of(live).max(pacing.minimum_allocation_cap);
        assert!(!pacing.full_due(live, live + growth - 1, 0, 0));
        assert!(pacing.full_due(live, live + growth, 0, 0));
        assert!(!pacing.full_due(live, live, cap - 1, 0));
        assert!(pacing.full_due(live, live, cap, 0));
        assert_eq!(pacing.young_budget(0), pacing.minimum_young);
        // A live set whose young share lies between the minimum and the maximum budget, in a
        // table whose budget may grow with the live set.
        if pacing.minimum_young < pacing.maximum_young {
            let between = pacing.minimum_young * 2 * pacing.young.denominator / pacing.young.numerator;
            assert_eq!(pacing.young_budget(between), pacing.young.of(between));
        }
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
            aged: AgedFates::default(),
        }
    }

    /// A full collection after minors promoted `promoted` weight onto `live`, which keeps
    /// `kept` of it.
    fn full_after_promoting(live: usize, promoted: usize, kept: usize) -> CollectionOutcome {
        CollectionOutcome { full: true, since_full: 0, ..minor(live + kept, live + promoted, 0) }
    }

    /// Minors promote `promoted` onto `live`, and the full collection after them finds every
    /// promotion dead.
    fn promotions_die_old(pacing: &GenerationalPacing, state: &mut PacingState, live: usize, promoted: usize) {
        pacing.plan(state, minor(live + promoted, live, promoted));
        pacing.plan(state, full_after_promoting(live, promoted, 0));
    }

    #[test]
    fn promotions_dying_old_turn_aging_on_until_aged_survivors_outlive_their_second_minor() {
        let pacing = MemoryPolicy::Throughput.table();
        let mut state = PacingState::default();
        let live = 1 << 20;
        let promoted = 1 << 16;
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        // One full collection finding promotions dead is not enough; one that finds them
        // living on in between starts the count again.
        promotions_die_old(&pacing, &mut state, live, promoted);
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        pacing.plan(&mut state, full_after_promoting(live, promoted, promoted));
        for _ in 1..FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
            promotions_die_old(&pacing, &mut state, live, promoted);
            assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        }
        promotions_die_old(&pacing, &mut state, live, promoted);
        assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
        // Aged survivors that their second minor frees keep aging on.
        let freed = AgedFates { died: 9, promoted: 1 };
        pacing.plan(&mut state, CollectionOutcome { aged: freed, ..minor(live, live, promoted) });
        assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
        // Full collections that find little promoted garbage, because aging freed it young,
        // leave it on: the server case.
        for _ in 0..=pacing.aging_interval {
            pacing.plan(&mut state, full_after_promoting(live, promoted, promoted));
            pacing.plan(&mut state, CollectionOutcome { aged: freed, ..minor(live, live, promoted) });
            assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
        }
        // Aged survivors that outlive it turn aging off, for the aging interval of fulls.
        let outliving = AgedFates { died: 1, promoted: 9 };
        for _ in 0..2 {
            pacing.plan(&mut state, CollectionOutcome { aged: outliving, ..minor(live, live, promoted) });
        }
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        for _ in 0..pacing.aging_interval + FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE - 1 {
            promotions_die_old(&pacing, &mut state, live, promoted);
            assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        }
        promotions_die_old(&pacing, &mut state, live, promoted);
        assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
    }

    #[test]
    fn aging_that_outlives_its_first_measurement_turns_off_at_once() {
        let pacing = MemoryPolicy::Throughput.table();
        let mut state = PacingState::default();
        let live = 1 << 20;
        let promoted = 1 << 16;
        for _ in 0..FULLS_FINDING_PROMOTED_GARBAGE_TO_AGE {
            promotions_die_old(&pacing, &mut state, live, promoted);
        }
        assert_eq!(state.promotion_age(&pacing), PromotionAge::OLDEST);
        let outliving = AgedFates { died: 1, promoted: 9 };
        pacing.plan(&mut state, CollectionOutcome { aged: outliving, ..minor(live, live, promoted) });
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
    }

    #[test]
    fn promotions_that_live_on_keep_the_first_age_and_memory_first_never_ages() {
        let live = 1 << 20;
        let promoted = 1 << 16;
        let pacing = MemoryPolicy::Throughput.table();
        let mut state = PacingState::default();
        pacing.plan(&mut state, minor(live + promoted, live, promoted));
        pacing.plan(&mut state, full_after_promoting(live, promoted, promoted));
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
        let pacing = MemoryPolicy::MemoryFirst.table();
        let mut state = PacingState::default();
        pacing.plan(&mut state, minor(live + promoted, live, promoted));
        pacing.plan(&mut state, full_after_promoting(live, promoted, 0));
        assert_eq!(state.promotion_age(&pacing), pacing.promotion_age);
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
        // Nearly everything young survives until smoothed survival passes the table's limit:
        // full collections with full-only headroom.
        while (state.survival + SURVIVAL_SCALE) / 2 <= pacing.survival_limit {
            pacing.plan(&mut state, minor(live + live / 2, live, live / 2));
        }
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
