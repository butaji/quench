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
    /// Native runs in a row (no run leaving through the loop's own exit between them) that may
    /// leave at a general-path instruction before the site stops entering native code: a loop
    /// whose values keep leaving the subset is the interpreter's.
    pub(in crate::vm) max_exits: u32,
    /// Entries a site's guards may decline in a row before it stops trying: a field whose cache
    /// never hits at entry (an array's `length`) made V8-v7 Crypto's sum loop decline at each of
    /// its iterations.
    pub(in crate::vm) max_declines: u32,
    /// Elements of an array read to guess whether it holds `i32`s; a wrong guess costs one
    /// compilation.
    pub(in crate::vm) element_sample: usize,
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
    /// The length below which a lane body without loops or inlined calls declines
    /// (`Vm::compile_lane_method`).
    pub(in crate::vm) min_instructions: usize,
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
    /// a part per bytecode instruction. Measured on Linux x86_64 with `QUENCH_JS_JIT_STATS`
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

pub(in crate::vm) const TIERS: Tiers = Tiers {
    loops: Loops { threshold: 1000, max_exits: 1000, max_declines: 64, element_sample: 8, max_compilations: 8 },
    lane_body: LaneBody {
        warmup_entries: 400,
        trial_runs: 1024,
        reoptimize_exits: 64,
        max_recompiles: 4,
        min_instructions: 24,
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
    /// Crypto and 648 on NavierStokes (`tod137-loop-tier-opt-level-…`). Throughput compiles
    /// at `speed`.
    pub(in crate::vm) loop_opt_level: Option<&'static str>,
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
    PolicyTiers { loop_opt_level: Some("speed"), js_function_tiers: true },
    PolicyTiers { loop_opt_level: None, js_function_tiers: false },
];

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
        assert!(lane.warmup_entries > 0 && lane.trial_runs > 0);
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
            assert!(row.loop_opt_level.is_none() || row.js_function_tiers, "{policy:?}");
        }
    }
}
