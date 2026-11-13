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
    pub(in crate::vm) priors: Priors,
}

/// The priors the model tests pin their arithmetic to.
#[cfg(test)]
pub(in crate::vm) const TEST_PRIORS: Priors = Priors {
    interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 3000, compile_wall_permille: 1000,
    region_entry_ns: 140, body_entry_ns: 330, remaining_divisor: 4, compile_forgetting_permille: 990, compile_prior_ir: 800,
};

/// Parts per thousand, the unit of a prior's share.
pub(in crate::vm) const PER_MILLE: u64 = 1000;

/// The priors the tier model starts from, each a statement about the world until the engine's own
/// measurements replace it (`tier-decision-rule-v1`): nothing here is fitted to a benchmark row.
pub(in crate::vm) struct Priors {
    /// Nanoseconds an interpreted lane bytecode takes.
    pub(in crate::vm) interpreter_ns: u64,
    /// A u-jit body's per-bytecode time as a share of the interpreter's, in thousandths: bodies
    /// run 2 to 5 times faster per entry than the interpreter once entered (inclusive timing).
    pub(in crate::vm) body_share: u64,
    /// Nanoseconds the code generator takes to compile one IR instruction, until the process's
    /// own compiles replace it (`tier_model::CompileCost`).
    pub(in crate::vm) compile_ns_per_ir: u64,
    /// The part per thousand of a compile's time that waits on the running program: compiles run
    /// on the code generator's thread, so most of their time overlaps it (two cores).
    pub(in crate::vm) compile_wall_permille: u64,
    /// Nanoseconds entering a loop region from the lane costs per call, and a body's entry
    /// (measured about 300 ns: some 1,100 instructions at 0.3 ns).
    pub(in crate::vm) region_entry_ns: u64,
    pub(in crate::vm) body_entry_ns: u64,
    /// The divisor of the entries so far that gives the credible lower bound of the entries to
    /// come, where the decision (a compile) is irreversible.
    pub(in crate::vm) remaining_divisor: u64,
    /// The forgetting factor of the compile-cost running ratio, per mille per compile, and the
    /// weight of its prior in IR instructions (eight compiles of a hundred).
    pub(in crate::vm) compile_forgetting_permille: u64,
    pub(in crate::vm) compile_prior_ir: u64,
}

/// The loop tier (OSR regions at backward jumps).
pub(in crate::vm) struct Loops {
    /// The backward jumps a loop takes in the interpreter before the tier model weighs its
    /// compile (`loop_model`): a resource and correctness floor, enough that the call sites of
    /// its body have linked their targets (which the region inlines) and its entry facts are
    /// observed. The model asks more where the compile does not yet pay.
    pub(in crate::vm) warmup_taken: u32,
    /// IR instructions a bytecode of a loop's span compiles to, which prices the compile before
    /// it runs (its own census replaces the guess in the learned cost per IR instruction).
    pub(in crate::vm) ir_per_bytecode: u64,
    /// The weight of the prior that a site's native entries repay themselves, in entries: the
    /// site's measured runs outweigh it as they mount (`loop_model::repays`), and it stops
    /// entering where its entries cost more than the iterations they ran save.
    pub(in crate::vm) prior_runs: u64,
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
    /// The share of a body's runs, in percent, its leaves may reach, from `reoptimize_exits`
    /// leaves on, before it specializes again.
    pub(in crate::vm) reoptimize_share: u8,
    /// The runs after which a body that no more recompile may change, and that left more than
    /// `retire_share` percent of them, stops being entered: it pays its entry and its exit for
    /// work the lane does anyway.
    pub(in crate::vm) retire_after_runs: u16,
    pub(in crate::vm) retire_share: u8,
    /// The leaves after which a body specializes again from the feedback its sites hold then:
    /// a region compiled while its call graph warmed keeps leaving at sites whose targets came
    /// later, the deoptimization that reoptimizes. Each recompile doubles it, as JSC's exit
    /// threshold backs off, and `max_recompiles` bounds it.
    pub(in crate::vm) reoptimize_exits: u16,
    pub(in crate::vm) max_recompiles: u8,
    /// The most compiles of one function's body, or of one of its loop entries: a request of
    /// another plan past them is declined, so what each came to is the last that holds.
    pub(in crate::vm) max_compiles: u8,
    /// The most bodies of one function lifecycle edges may discard before it runs in the lane
    /// for good: past them a compile costs more than its runs regain.
    pub(in crate::vm) max_lifetimes: u8,
    /// The warm-ups a function's first compile waits for callees still warming toward the
    /// bodies it would call directly; a callee that never gets one stops holding it back.
    pub(in crate::vm) max_callee_waits: u8,
    /// The times a function specializes again because a callee published its body
    /// (`LaneView::respecialize_callers`): each makes more of its calls direct, and the bound
    /// ends the exchange between functions that call each other.
    pub(in crate::vm) max_callee_recompiles: u8,
    pub(in crate::vm) late_call_calls: u32,
    /// The length below which a lane body without loops or inlined calls declines
    /// (`Vm::compile_lane_method`).
    pub(in crate::vm) min_instructions: usize,
    /// The most entries between looks for a compiling body's code, which start one entry apart
    /// and double (`LaneBody::Compiling`). A look costs a channel poll
    /// and the specialization's bookkeeping, some 300 instructions, at each backward jump of
    /// a hot loop while the code compiles (Richards' first body: 300 ms); the code waits at
    /// most this many entries once ready.
    pub(in crate::vm) compile_poll_interval: u16,
    /// The entries between timed runs of a published body's function, which alternate the body
    /// and the interpreter (`LaneView::audit_entry`): one in this many entries pays a clock
    /// read pair, and half of those run interpreted.
    pub(in crate::vm) audit_period: u32,
    /// The timed runs each side needs before the verdict (`LaneView::audit_verdict`).
    pub(in crate::vm) audit_samples: u16,
    /// How much faster, per thousand, the body's mean timed run must be than the interpreter's
    /// for the body to stay: a body only as fast pays its compile and its entry for nothing.
    pub(in crate::vm) audit_margin_permille: u64,
}

/// The per-function JavaScript tier: the numeric optimizing tier.
pub(in crate::vm) struct JsFunction {
    /// Prespecified looks for the Number-entry profile. Critical counts are the exact
    /// one-sided Binomial( n, 0.80 ) cutoffs at alpha = 0.05 / 4 per look. A union bound caps
    /// false promotion at 5% across looks for one function under a stationary Bernoulli model;
    /// it is not a VM-wide multiple-test bound.
    pub(in crate::vm) promotion_looks: [(u8, u8); 4],
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
/// What a speculated fact costs and saves, in nanoseconds, from which a site's break-even rate
/// derives (`site_posterior::break_even`) with the posterior's quantile (`tier-decision-rule-v1`
/// section 4). Per execution a speculating read pays `guard_read_ns` when the fact holds and
/// `guard_exit_ns` (the leave, the interpreter's replay and the re-entry) when it does not; the
/// in-place generic read (`field_probe`) pays `probe_read_ns` either way. Speculation wins when
/// `p (probe - guard) > (1 - p) (exit - probe)`, so the break-even rate is
/// `(exit - probe) / (exit - guard)`.
pub(in crate::vm) struct Speculation {
    pub(in crate::vm) guard_read_ns: u32,
    pub(in crate::vm) probe_read_ns: u32,
    pub(in crate::vm) guard_exit_ns: u32,
    /// The standard-normal deviate of the posterior's lower 5% quantile: the Beta's quantile is
    /// taken as mean minus this many standard deviations (conservative for a skewed Beta).
    pub(in crate::vm) lower_quantile_z: f64,
}

/// A guarded read is a load and a compare; the probe is the VM cache's hit path; an exit costs
/// the region entry and exit plumbing (`Priors::region_entry_ns`, 140 ns) and the replayed
/// instructions of the leave.
pub(in crate::vm) const SPECULATION: Speculation =
    Speculation { guard_read_ns: 1, probe_read_ns: 6, guard_exit_ns: 200, lower_quantile_z: 1.645 };
/// The trips a loop is assumed to take, in weighing the call sites it holds (`lane_unit`'s
/// inlining plan), before a native run of it has measured its mean: enough that a site in a loop
/// outweighs one outside it, and that nesting orders deeper sites first.
pub(in crate::vm) const ASSUMED_LOOP_TRIP: u64 = 8;

/// A `tier-stress` build: the floor row, and every JavaScript compilation arm on, so its
/// Test262 run and `tests/tier_differential.rs` check each tier against the interpreter.
pub(in crate::vm) const STRESS: bool = cfg!(feature = "tier-stress");

/// The searches a regexp program runs interpreted before it is compiled to native code
/// (`regexp_native`): compiling costs about what this many searches of a small program do.
pub(in crate::vm) const REGEXP_NATIVE_RUNS: u32 = if STRESS { 1 } else { 2000 };

#[cfg(test)]
thread_local! { static FORCED_ADMISSION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// Makes this thread's tier models admit every eligible compile, as `tier-stress` does for the
/// process: for tests of what compiled code does, on fixtures too small to repay a compile.
#[cfg(test)]
pub(in crate::vm) fn force_admission() { FORCED_ADMISSION.set(true); }

/// Whether the tier models admit every eligible compile (`tier-stress`, or a test that forced it).
pub(in crate::vm) fn admits_every_compile() -> bool {
    #[cfg(test)]
    if FORCED_ADMISSION.get() { return true; }
    STRESS
}

/// The thresholds every tier controller compares against: the measured row, or under
/// `STRESS` the floor row, which makes every eligible site compile on its first chance.
pub(in crate::vm) const TIERS: Tiers = if STRESS { FLOOR } else { MEASURED };

/// Each threshold at the least value its controller accepts. Thresholds only price
/// compilation, so this row must compute what `MEASURED` computes.
const FLOOR: Tiers = Tiers {
    loops: Loops { warmup_taken: 2, ..MEASURED.loops },
    lane_body: LaneBody { warmup_entries: 2, reoptimize_exits: 2, min_instructions: 0, compile_poll_interval: 0, max_callee_waits: 0, audit_period: u32::MAX, ..MEASURED.lane_body },
    js_function: JsFunction { promotion_looks: [(1, 1), (2, 2), (3, 3), (4, 4)] },
    wasm: Wasm { promotion_entries: 1, backedges_per_entry: 2, eager_module_bytecodes: usize::MAX },
    priors: MEASURED.priors,
};

/// The base a share in percent is a share of.
pub(in crate::vm) const PERCENT: u64 = 100;

const MEASURED: Tiers = Tiers {
    loops: Loops { warmup_taken: 64, ir_per_bytecode: 8, prior_runs: 128, widen_after_leaves: 4, max_compilations: 8 },
    lane_body: LaneBody {
        warmup_entries: 400,
        reoptimize_share: 1,
        retire_after_runs: 1000,
        retire_share: 10,
        reoptimize_exits: 64,
        max_recompiles: 1,
        max_compiles: 3,
        max_lifetimes: 8,
        max_callee_waits: 2,
        max_callee_recompiles: 3,
        late_call_calls: 4096,
        min_instructions: 40,
        compile_poll_interval: 64,
        audit_period: 16,
        audit_samples: 16,
        audit_margin_permille: 50,
    },
    js_function: JsFunction { promotion_looks: [(32, 31), (64, 59), (128, 113), (255, 219)] },
    wasm: Wasm { promotion_entries: 32, backedges_per_entry: 1024, eager_module_bytecodes: 4096 },
    priors: Priors { interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 3000, compile_wall_permille: 100, region_entry_ns: 140, body_entry_ns: 330, remaining_divisor: 4, compile_forgetting_permille: 990, compile_prior_ir: 800 },
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
    /// Whether the JavaScript function tiers (the per-function tier and the loop tier's site
    /// analysis) compile. Memory-first compiles none, on the
    /// owner's direction that it may trade Score for memory under `--jit` (TOD-123). Under
    /// memory-first their code and regalloc2 state set the `--jit` peaks: the function tiers
    /// took V8-v7 Crypto `--jit` from 3220 to 5496 KiB (median, bolted static x86_64).
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

/// The one tier predicate: whether any of `tiers` may run for a function under `policy` and the
/// process's JIT mode (`jit_mode::tiers_allowed`).
pub(in crate::vm) fn tier_may_run(policy: crate::MemoryPolicy, tiers: &[crate::jit_mode::Tier]) -> bool {
    by_policy(policy).js_function_tiers && crate::jit_mode::tiers_allowed(tiers)
}

/// What `policy` lets the tiers compile.
pub(in crate::vm) const fn by_policy(policy: crate::MemoryPolicy) -> &'static PolicyTiers {
    &BY_POLICY[policy as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tier_table_is_consistent() {
        let lane = &TIERS.lane_body;
        assert!(lane.warmup_entries > 0);
        // The warm-up doubles per dropped body, at most seven times, and must not overflow.
        assert!(u32::from(lane.warmup_entries) << 7 <= u32::from(u16::MAX));
        // The reoptimization threshold doubles per recompile and must not overflow its counter.
        assert!(u32::from(lane.reoptimize_exits) << lane.max_recompiles <= u32::from(u16::MAX));
        let looks = TIERS.js_function.promotion_looks;
        assert!(looks.windows(2).all(|pair| pair[0].0 < pair[1].0), "looks grow");
        assert!(looks.iter().all(|(observations, minimum)| minimum <= observations));
        for policy in crate::MemoryPolicy::ALL {
            let row = by_policy(policy);
            // The loop tier's site analysis belongs to the function tiers.
            assert!(!row.loop_tier || row.js_function_tiers, "{policy:?}");
        }
    }
}
