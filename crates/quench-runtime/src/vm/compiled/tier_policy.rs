//! When code moves between tiers: every threshold the tier controllers read, in one table.
//!
//! The controllers (`LoopTier` and `LoopSite`, `LaneBody`, `JsTierState`, `WasmTierState`)
//! own their states and transitions; this module owns the numbers those transitions compare
//! against, and what each memory policy lets compile. A tuning experiment changes a row here,
//! never a controller.

/// The thresholds every tier controller reads.
pub(in crate::vm) struct Tiers {
    pub(in crate::vm) loops: Loops,
    pub(in crate::vm) lane_body: LaneBody,
    pub(in crate::vm) js_function: JsFunction,
    pub(in crate::vm) wasm: Wasm,
}

/// The loop tier (OSR regions at backward jumps).
pub(in crate::vm) struct Loops {
    /// Backward jumps a loop takes in the interpreter before it compiles: enough that loops
    /// which run briefly never pay the compilation.
    pub(in crate::vm) threshold: u32,
    /// Native runs that complete no iteration, less the iterations other runs complete, before
    /// the site stops entering native code: a loop whose runs keep leaving before their
    /// backward jump (a guard that fails each iteration, a loop that seldom iterates) is the
    /// interpreter's. Richards --jit entered one list walk 6M times for 0.02 iterations a run.
    pub(in crate::vm) max_exits: u32,
    /// Entries a site's guards may decline in a row before it stops trying: a field whose cache
    /// never hits at entry (an array's `length`) made V8-v7 Crypto's sum loop decline at each of
    /// its iterations.
    pub(in crate::vm) max_declines: u32,
    /// Runs that leave at one operation before the site compiles again with that operation's
    /// int32 speculation dropped (`LoopTier::left_at`): a sum that overflows on most runs
    /// (SHA-256's rounds) is a double's, and one that overflowed once is not.
    pub(in crate::vm) widen_after_leaves: u16,
    /// Compilations a site may make: each after the first widens a value the last one held as
    /// an `i32`, and a region has few such values.
    pub(in crate::vm) max_compilations: u32,
}

/// The frameless lane-body tier (`LaneView::tier_lane`).
pub(in crate::vm) struct LaneBody {
    /// The entries a function makes before its lane body specializes: enough that its call
    /// sites have linked their targets, which the body inlines (`lane_unit::unit`), and that
    /// only a function called repeatedly pays for compiling one.
    pub(in crate::vm) warmup_entries: u16,
    /// The runs over which a specialized body proves it completes more often than it leaves:
    /// a body whose runs mostly leave pays its entry and the lane's resumption for work the
    /// lane does anyway (`LaneView::note_lane_body_run`).
    pub(in crate::vm) trial_runs: u16,
    /// The leaves after which a body specializes again from the feedback its sites hold then:
    /// a region compiled while its call graph warmed keeps leaving at sites whose targets came
    /// later, the deoptimization that reoptimizes. Each recompile doubles it, as JSC's exit
    /// threshold backs off, and `max_recompiles` bounds it.
    pub(in crate::vm) reoptimize_exits: u16,
    pub(in crate::vm) max_recompiles: u8,
    /// The bodies a function may lose without any run of theirs completing before it
    /// declines: a shape compaction or a dead native call drops a body, and one that never
    /// completed paid its compile for nothing. Each such body doubles the next one's warm-up.
    pub(in crate::vm) max_wasted: u8,
    /// The most bodies of one function lifecycle edges may discard before it runs in the lane
    /// for good: past them a compile costs more than its runs regain.
    pub(in crate::vm) max_lifetimes: u8,
    /// The length below which a lane body without loops or inlined calls declines
    /// (`Vm::compile_lane_method`).
    pub(in crate::vm) min_instructions: usize,
    /// The entries between looks for a compiling body's code. A look costs a channel poll
    /// and the specialization's bookkeeping, some 300 instructions, at each backward jump of
    /// a hot loop while the code compiles (Richards' first body: 300 ms); the code waits at
    /// most this many entries once ready.
    pub(in crate::vm) compile_poll_interval: u16,
}

/// The per-function JavaScript tiers: the numeric optimizing tier and the general
/// tagged-dispatch baseline.
pub(in crate::vm) struct JsFunction {
    /// Prespecified looks for the Number-entry profile. Critical counts are the exact
    /// one-sided Binomial( n, 0.80 ) cutoffs at alpha = 0.05 / 4 per look. A union bound caps
    /// false promotion at 5% across looks for one function under a stationary Bernoulli model;
    /// it is not a VM-wide multiple-test bound.
    pub(in crate::vm) promotion_looks: [(u8, u8); 4],
    /// Entries after which a function is hot enough to price its general tagged-dispatch
    /// baseline (`GeneralPromotionPending`); unlike the optimizing tier, the baseline needs no
    /// numeric arguments or bytecode-shape candidate.
    pub(in crate::vm) general_promotion_entries: u8,
    /// A specialized artifact that repeatedly rejects entry values is useful only for its
    /// matching profile. Eight confirmed misses cap this experiment before paying for the
    /// complete-semantics baseline.
    pub(in crate::vm) specialized_guard_misses: u8,
    /// The general baseline's compile cost in interpreted-instruction units: a fixed part and
    /// a part per bytecode instruction. Measured on Linux x86_64 with `QUENCH_TRACE=js-jit`
    /// against roughly 50 ns per interpreted instruction: requiring jackspeak, about 0.8 ms
    /// plus 32 µs per instruction; npm's semver scenario, 1.4 ms for a four-instruction body
    /// and 40 to 76 µs per instruction for 160 to 200.
    pub(in crate::vm) general_compile_fixed_cost: u64,
    pub(in crate::vm) general_compile_cost_per_instruction: u64,
    /// The share of an entry's interpreted instructions the baseline saves, as a fraction. It
    /// keeps the interpreter's tagged semantics and calls back into the VM for most operations
    /// (`js_step_callout` was 11% of npm browserslist with it on), so a quarter until the
    /// baseline's savings are measured.
    pub(in crate::vm) general_saved_share: (u64, u64),
    /// What each native entry of the baseline costs beyond a frameless lane activation, in
    /// interpreted-instruction units: a due function's calls take a VM frame and the general
    /// loop (`Vm::js_tier_needs_general_entry`). Measured on npm's browserslist scenario: a
    /// six-instruction body's 72,427 native entries cost about 650 machine instructions each
    /// more than its lane calls, some 32 lane-interpreted instructions.
    pub(in crate::vm) general_entry_overhead: u64,
}

/// The Wasm function and module tiers.
pub(in crate::vm) struct Wasm {
    /// Promote a Wasm function only after repeated interpreter entries, or the loop work of as
    /// many (`backedges_per_entry`). Wasm signatures and bytecode are fixed, so this is a
    /// hotness gate.
    pub(in crate::vm) promotion_entries: u8,
    /// The backward jumps a lane run of a Wasm function counts as one interpreter entry toward
    /// `promotion_entries`: a loop of this many iterations does at least the work of an entry
    /// through a short call, so a function entered rarely but looping hot promotes after
    /// `promotion_entries` such stretches.
    pub(in crate::vm) backedges_per_entry: u32,
    /// The size, in bytecodes, up to which a Wasm module compiles whole at its first entry
    /// rather than starting in the interpreter: compiling it costs about what
    /// `WasmModuleCompile::after` would have spent interpreting it, so waiting buys nothing.
    /// Whole compiles take 3-7 us a bytecode (Linux x86_64): quicksort-wasm, 2,412, compiles in
    /// 16 ms and lost 13% of its JetStream Score waiting; source-map's module, 12,971, takes
    /// 67 ms, which its one short parse never repays.
    pub(in crate::vm) eager_module_bytecodes: usize,
}

/// The distinct cases (shapes, callees, targets) a site's feedback keeps before it widens to
/// the generic case: the width of every polymorphic cache and of each guarded switch a tier
/// specializes a site into. One bound for every lattice of site feedback.
pub(in crate::vm) const POLYMORPHISM: usize = 4;

/// What a site does with a receiver shape that arrives once every way holds an entry.
#[derive(Clone, Copy)]
pub(in crate::vm) enum Overflow {
    /// Spill the entries to a hash of at most `limit`; past it the site is megamorphic.
    Hash { limit: usize },
    /// Replace the oldest entry: the site keeps the newest `ways` shapes however many arrive.
    EvictOldest,
}

/// One kind of site's room for feedback: `inline_ways` entries live in the site's own slot,
/// `ways` in all, and `overflow` decides what a further shape does.
#[derive(Clone, Copy)]
pub(in crate::vm) struct SitePolicy {
    pub(in crate::vm) inline_ways: usize,
    pub(in crate::vm) ways: usize,
    pub(in crate::vm) overflow: Overflow,
}

/// The `SitePolicy` row of every kind of site feedback: the interpreter's caches and the
/// call sites' callee history each read their own row, so a width or overflow rule is
/// changed in one place. `POLYMORPHISM` stays the width of the guarded switch a tier
/// specializes a site into.
pub(in crate::vm) struct SitePolicies {
    /// Field loads: entries per receiver shape and holder, spilled to a hash past four; unseen
    /// shapes past the hash's limit keep the generic fallback.
    pub(in crate::vm) field_load: SitePolicy,
    /// Field adds, one entry per source shape and prototype chain. Objects whose optional
    /// fields are assigned in varying order reach one add site with several shapes. The first
    /// lives in the set, the rest in a box made when a second arrives.
    pub(in crate::vm) field_add: SitePolicy,
    /// Inherited field loads: a method inherited from beyond the immediate prototype, read on
    /// instances of several subclasses (DeltaBlue's constraints), reaches one site with a
    /// shape per class.
    pub(in crate::vm) inherited: SitePolicy,
    /// Accessor field loads: a getter on a prototype is read from instances that differ by
    /// which fields their factory set, as Air's `Arg` kinds do (about ten shapes reading
    /// `isStack`); a site past its ways leaves the lane at each miss and evicts an entry
    /// another shape needs next.
    pub(in crate::vm) accessor: SitePolicy,
    /// Method calls: two ways in the site's slot, the rest in its set.
    pub(in crate::vm) method: SitePolicy,
    /// Callee history of a call site: sites stay monomorphic (mandreel: 131K, none
    /// polymorphic), so one entry sits unspilled at 32 bytes instead of 112.
    pub(in crate::vm) call_targets: SitePolicy,
}

pub(in crate::vm) const SITE_POLICIES: SitePolicies = SitePolicies {
    field_load: SitePolicy {
        inline_ways: POLYMORPHISM,
        ways: POLYMORPHISM,
        overflow: Overflow::Hash { limit: 256 },
    },
    field_add: SitePolicy { inline_ways: 1, ways: POLYMORPHISM, overflow: Overflow::EvictOldest },
    inherited: SitePolicy { inline_ways: POLYMORPHISM, ways: POLYMORPHISM, overflow: Overflow::EvictOldest },
    accessor: SitePolicy { inline_ways: 16, ways: 16, overflow: Overflow::EvictOldest },
    method: SitePolicy { inline_ways: 2, ways: POLYMORPHISM, overflow: Overflow::EvictOldest },
    call_targets: SitePolicy { inline_ways: 1, ways: POLYMORPHISM, overflow: Overflow::EvictOldest },
};
// An MRU chain reuses its oldest entry, which must not be the head the new entry links to.
const _: () = assert!(SITE_POLICIES.inherited.ways > 1 && SITE_POLICIES.accessor.ways > 1);

/// How a site's feedback stands, from what it holds and its policy's rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::vm) enum SiteState {
    Empty,
    Monomorphic,
    Polymorphic,
    /// A site that refused a shape: the entries it holds answer, the rest take the generic
    /// path. It never leaves this state, since its entries only go with a reset of the site.
    Megamorphic,
}

/// What a site does with a shape it holds no entry for (`SiteState::after_miss`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::vm) enum Admission {
    /// Add it beside the entries.
    Append,
    /// Add it to the overflow hash.
    Spill,
    /// Add it in place of the oldest entry.
    EvictOldest,
    /// Leave it out.
    Reject,
}

impl SiteState {
    /// The state of a site holding `held` entries under `policy`.
    pub(in crate::vm) const fn of(held: usize, policy: &SitePolicy) -> Self {
        let exhausted = match policy.overflow {
            Overflow::Hash { limit } => held >= limit,
            Overflow::EvictOldest => false,
        };
        match held {
            0 => Self::Empty,
            _ if exhausted => Self::Megamorphic,
            1 => Self::Monomorphic,
            _ => Self::Polymorphic,
        }
    }

    /// The one place a site changes state: the admission of a shape the site holds no entry
    /// for, which moves a site holding `held` entries one step along Empty, Monomorphic,
    /// Polymorphic: an append while a way is free, then the policy's overflow rule. A
    /// hash overflow ends in Megamorphic at its limit, and a megamorphic site rejects every
    /// shape from then on. An evicting site never
    /// becomes megamorphic: it churns its newest shapes.
    pub(in crate::vm) const fn after_miss(held: usize, policy: &SitePolicy) -> Admission {
        match Self::of(held, policy) {
            Self::Megamorphic => Admission::Reject,
            _ if held < policy.ways => Admission::Append,
            _ => match policy.overflow {
                Overflow::Hash { .. } => Admission::Spill,
                Overflow::EvictOldest => Admission::EvictOldest,
            },
        }
    }
}

/// A `tier-stress` build: the floor row, and every JavaScript compilation arm on, so its
/// Test262 run and `tests/tier_differential.rs` check each tier against the interpreter.
pub(in crate::vm) const STRESS: bool = cfg!(feature = "tier-stress");

/// The thresholds every tier controller compares against: the measured row, or under
/// `STRESS` the floor row, which makes every eligible site compile on its first chance.
pub(in crate::vm) const TIERS: Tiers = if STRESS { FLOOR } else { MEASURED };

/// Each threshold at the least value its controller accepts. Thresholds only price
/// compilation, so this row must compute what `MEASURED` computes.
const FLOOR: Tiers = Tiers {
    loops: Loops { threshold: 2, ..MEASURED.loops },
    lane_body: LaneBody { warmup_entries: 2, trial_runs: 2, reoptimize_exits: 2, min_instructions: 0, compile_poll_interval: 0, ..MEASURED.lane_body },
    js_function: JsFunction { promotion_looks: [(1, 1), (2, 2), (3, 3), (4, 4)], general_promotion_entries: 1, general_compile_fixed_cost: 0, general_compile_cost_per_instruction: 0, ..MEASURED.js_function },
    wasm: Wasm { promotion_entries: 1, backedges_per_entry: 2, eager_module_bytecodes: usize::MAX },
};

const MEASURED: Tiers = Tiers {
    loops: Loops { threshold: 1000, max_exits: 1000, max_declines: 64, widen_after_leaves: 4, max_compilations: 8 },
    lane_body: LaneBody {
        warmup_entries: 400,
        trial_runs: 1024,
        reoptimize_exits: 64,
        max_recompiles: 4,
        max_wasted: 2,
        max_lifetimes: 8,
        min_instructions: 24,
        compile_poll_interval: 64,
    },
    js_function: JsFunction {
        promotion_looks: [(32, 31), (64, 59), (128, 113), (255, 219)],
        general_promotion_entries: 32,
        specialized_guard_misses: 8,
        general_compile_fixed_cost: 16_000,
        general_compile_cost_per_instruction: 1_200,
        general_saved_share: (1, 4),
        general_entry_overhead: 32,
    },
    wasm: Wasm { promotion_entries: 32, backedges_per_entry: 1024, eager_module_bytecodes: 4096 },
};

/// Whether a general loop may enter a JS tier. A parameter-initialization run (`stops`)
/// observes every PC up to its stop, and a tier would run past it.
pub(in crate::vm) const fn tiers_enter(stops: bool) -> bool {
    crate::DYNAMIC_EXECUTION_ENABLED && !stops
}

/// What a memory policy lets the tiers compile.
pub(in crate::vm) struct PolicyTiers {
    /// The Cranelift opt level loop regions compile at, or `None` where no loop region
    /// compiles. Region compilation is the largest transient allocation of a `--jit` run:
    /// regalloc2's state and the code pages of the compiler and of the region.
    ///
    /// Memory-first compiles no loop region, on the owner's direction of 2026-10-24 (TOD-123:
    /// memory-first may trade Score for memory under `--jit`). The per-function tier, which
    /// this leaves running, kept Richards, RayTrace and EarleyBoyer `--jit` below their
    /// interpreted peaks. NavierStokes `--jit` peaked at 7468 KiB compiled and 3988
    /// interpreted, Crypto at 4000 and 3128 (static BOLT x86_64, memory-first, median of 5,
    /// 2026-10-24). Compiling at `none`, without the e-graph pass, saved only 576 KiB on
    /// Crypto and 648 on NavierStokes (`tod137-loop-tier-opt-level-…`).
    pub(in crate::vm) loop_tier: bool,
    /// Whether the JavaScript function tiers (the per-function tier and the general-dispatch
    /// tier, and the loop tier's site analysis) compile. Memory-first compiles none, on the
    /// owner's direction that it may trade Score for memory under `--jit` (TOD-123). Under
    /// memory-first their code and regalloc2 state set the `--jit` peaks: the general-dispatch
    /// tier took V8-v7 Crypto `--jit` from 3220 to 5496 KiB (median, bolted static x86_64).
    /// With both off, every V8-v7 `--jit` row peaked 84-380 KiB lower, at its no-JIT level.
    /// Scores moved both ways: EarleyBoyer +125%, Splay +15%, RayTrace +11%, NavierStokes +8%,
    /// but DeltaBlue -9%, Richards -7%, RegExp -7% (same unbolted static binary, 5 runs,
    /// 2026-10-24).
    pub(in crate::vm) js_function_tiers: bool,
}

/// One row per memory policy, in `MemoryPolicy` order.
const BY_POLICY: [PolicyTiers; crate::MemoryPolicy::ALL.len()] = [
    PolicyTiers { loop_tier: true, js_function_tiers: true },
    PolicyTiers { loop_tier: false, js_function_tiers: false },
];

/// What `policy` lets the tiers compile.
pub(in crate::vm) const fn by_policy(policy: crate::MemoryPolicy) -> &'static PolicyTiers {
    &BY_POLICY[policy as usize]
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_site_moves_along_its_policy_and_never_leaves_megamorphic() {
        use super::{Admission, SITE_POLICIES, SiteState};
        let loads = &SITE_POLICIES.field_load;
        let super::Overflow::Hash { limit } = loads.overflow else { panic!("loads spill to a hash") };
        assert_eq!(SiteState::of(0, loads), SiteState::Empty);
        assert_eq!(SiteState::of(1, loads), SiteState::Monomorphic);
        assert_eq!(SiteState::of(loads.ways, loads), SiteState::Polymorphic);
        assert_eq!(SiteState::after_miss(loads.ways - 1, loads), Admission::Append);
        assert_eq!(SiteState::after_miss(loads.ways, loads), Admission::Spill);
        assert_eq!(SiteState::after_miss(limit - 1, loads), Admission::Spill);
        for held in limit..limit + 3 {
            assert_eq!(SiteState::of(held, loads), SiteState::Megamorphic);
            assert_eq!(SiteState::after_miss(held, loads), Admission::Reject);
        }
        let methods = &SITE_POLICIES.method;
        assert_eq!(SiteState::after_miss(methods.ways, methods), Admission::EvictOldest);
        assert_eq!(SiteState::of(methods.ways, methods), SiteState::Polymorphic);
    }

    use super::*;

    #[test]
    fn the_tier_table_is_consistent() {
        let lane = &TIERS.lane_body;
        assert!(lane.warmup_entries > 0 && lane.trial_runs > 0);
        // The warm-up doubles per dropped body, at most seven times, and must not overflow.
        assert!(u32::from(lane.warmup_entries) << 7 <= u32::from(u16::MAX));
        // The reoptimization threshold doubles per recompile and must not overflow its counter.
        assert!(u32::from(lane.reoptimize_exits) << lane.max_recompiles <= u32::from(u16::MAX));
        let looks = TIERS.js_function.promotion_looks;
        assert!(looks.windows(2).all(|pair| pair[0].0 < pair[1].0), "looks grow");
        assert!(looks.iter().all(|(observations, minimum)| minimum <= observations));
        let (saved, of) = TIERS.js_function.general_saved_share;
        assert!(saved > 0 && saved < of);
        for policy in crate::MemoryPolicy::ALL {
            let row = by_policy(policy);
            // The loop tier's site analysis belongs to the function tiers.
            assert!(!row.loop_tier || row.js_function_tiers, "{policy:?}");
        }
    }
}
