//! When code moves between tiers: every threshold the tier controllers read, in one table.
//!
//! The controllers (`RegionTier` and `BodyTier` of a loop site, `LaneBody`, `JsTier`, `WasmTier`)
//! own their states and transitions; this module owns the numbers those transitions compare
//! against, and what each memory policy lets compile. A tuning experiment changes a row here,
//! never a controller.

/// The thresholds every tier controller reads.
pub(in crate::vm) struct Tiers {
    pub(in crate::vm) loops: Loops,
    pub(in crate::vm) lane_body: LaneBody,
    pub(in crate::vm) js_function: JsFunction,
    pub(in crate::vm) wasm: Wasm,
    pub(in crate::vm) limits: Limits,
    pub(in crate::vm) lane_view: LaneView,
}

/// When the general loop derives a function's lane view and enters the lane.
pub(in crate::vm) struct LaneView {
    /// The runs a function makes on the general path before it derives its lane view, which
    /// costs about a thousand instructions per bytecode instruction, so a function that runs
    /// fewer times costs no view. Measured in instructions against deriving at the second run:
    /// four takes 3.3% from the jiti npm scenario (Babel's many functions that run a few times)
    /// and 0.1-0.6% from semver, ajv, Babel, color-convert and typescript-eslint; eight adds at
    /// most 0.3%.
    pub(in crate::vm) deferred_runs: u8,
    /// The backward jumps a JavaScript activation takes in the general path before its
    /// function's lane view is derived and the lane runs its loop (`Vm::note_back_edge`).
    /// Deriving a view costs some hundreds of instructions per instruction of the function; a
    /// module body that loops a few times (over its exports, say) ran faster without one.
    pub(in crate::vm) hot_back_edges: u16,
    /// The loop work, in instructions its back edges span per instruction of the function,
    /// after which a view-less activation derives its function's lane view (`HotLoop`). A view
    /// costs some hundreds of instructions per instruction of the whole function, so a short
    /// loop in a large body that runs once (a module body filling a table) ran faster without
    /// one. Measured in instructions, warm compile cache, against deriving at `hot_back_edges`
    /// alone (x86_64 VM, 2026-10-26): @babel/core -11.0% (one: +0.9%, four: -9.9%), and within
    /// 0.2% on schema-utils, axios, semver, ajv, eslint, jiti, undici and source-map.
    pub(in crate::vm) loop_payback: u32,
    /// The shortest straight-line lane run the general loop enters. Entering builds the lane
    /// context and memory view, which one or two lane instructions do not repay: on V8-v7 fixed
    /// work, three keeps every JavaScript benchmark within one percent of the general path or
    /// better (one and two cost DeltaBlue three percent), and leaves Wasm loops, whose runs are
    /// long, unchanged.
    pub(in crate::vm) entry_min_run: usize,
    /// `entry_min_run` in a function with no loop the lane runs whole: each call the general
    /// loop makes into it enters the lane anew, so a run must repay a fresh lane context.
    pub(in crate::vm) entry_min_call_run: usize,
}

/// The sizes of what the compilers accept: a body past one stays with the interpreter.
pub(in crate::vm) struct Limits {
    /// The numeric function tier: native regions stay short so an interpreter interrupt cannot
    /// be delayed by a large body.
    pub(in crate::vm) js_function: JsFunctionLimits,
    /// The most bytecode instructions the general baseline compiles for one function: every
    /// instruction is a block, and the baseline's return statuses encode a block's PC
    /// (`JS_GENERAL_RETURN_STATUS_BASE`).
    pub(in crate::vm) general_blocks: usize,
    /// The most instructions a lane unit (a lane body with its inlined callees) lowers to a
    /// native region.
    pub(in crate::vm) lane_region_blocks: usize,
    /// The loop region's bounds.
    pub(in crate::vm) region: RegionLimits,
    /// The lane unit's inlining budgets.
    pub(in crate::vm) inlining: InliningLimits,
}

/// What one OSR loop region may span and name.
pub(in crate::vm) struct RegionLimits {
    /// The most instructions a region spans. Native code polls termination only at backward
    /// jumps, so the bound keeps compile time and code size proportional to small loop bodies.
    pub(in crate::vm) instructions: usize,
    /// The most registers a region names, bounding entry guards and the entry record.
    pub(in crate::vm) registers: usize,
    /// The most arrays a region names, bounding entry guards and the entry record.
    pub(in crate::vm) arrays: usize,
    /// The most captures a region names, bounding entry guards and the entry record.
    pub(in crate::vm) captures: usize,
    /// The most field loads a region names, bounding entry guards and the entry record.
    pub(in crate::vm) fields: usize,
    /// The most field stores a region names, bounding entry guards and the entry record.
    pub(in crate::vm) field_stores: usize,
}

/// How much a lane unit inlines.
pub(in crate::vm) struct InliningLimits {
    /// The longest callee body the unit inlines: each copy of it costs code and compile time at
    /// every call site, and short bodies (accessors, field updates) are where a call costs most
    /// relative to its work.
    pub(in crate::vm) callee_instructions: usize,
    /// The callee instructions one unit inlines in all, the region's budget: a hot loop's call
    /// graph, inlined, is the region an optimizing tier compiles as one function. Its compile
    /// cost grows with it (Richards' 600-instruction units cost ~63M instructions to compile),
    /// so it is TurboFan's cumulative inlining budget (`max_inlined_bytecode_size_cumulative`).
    pub(in crate::vm) unit_instructions: usize,
    /// How deep calls nest inside a unit: JSC's inlining depth (`maximumInliningDepth`).
    pub(in crate::vm) depth: usize,
}

/// What the numeric function tier admits and emits.
pub(in crate::vm) struct JsFunctionLimits {
    /// The most bytecode instructions of a candidate function.
    pub(in crate::vm) instructions: usize,
    /// The most registers of a candidate function.
    pub(in crate::vm) registers: usize,
    /// The most parameters of a candidate function.
    pub(in crate::vm) parameters: usize,
    /// The most bytes of native code one function (and, doubled, one loop region) emits.
    pub(in crate::vm) code_bytes: usize,
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
    /// int32 speculation dropped (`RegionController::judge`): a sum that overflows on most runs
    /// (SHA-256's rounds) is a double's, and one that overflowed once is not.
    pub(in crate::vm) widen_after_leaves: u16,
    /// The runs of a loop's lane body that leave inside the loop before the next body plans
    /// around where they left.
    pub(in crate::vm) body_reoptimize_leaves: u16,
    /// The backedges between looks for a loop body's compiling code.
    pub(in crate::vm) body_poll_interval: u16,
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
    /// The leaves after which a body specializes again from the feedback its sites hold then:
    /// a region compiled while its call graph warmed keeps leaving at sites whose targets came
    /// later, the deoptimization that reoptimizes. `SPECULATIVE_BODIES` bounds the bodies.
    pub(in crate::vm) reoptimize_exits: u16,
    /// The length below which a lane body without loops or inlined calls declines
    /// (`Vm::compile_lane_method`).
    pub(in crate::vm) min_instructions: usize,
    /// The entries between looks for a compiling body's code. A look costs a channel poll
    /// and the specialization's bookkeeping, some 300 instructions, at each backward jump of
    /// a hot loop while the code compiles (Richards' first body: 300 ms); the code waits at
    /// most this many entries once ready.
    pub(in crate::vm) compile_poll_interval: u16,
    /// What compiling one IR instruction of a body costs, in interpreted-instruction units: the
    /// V8-v7 compiles took 7 us an IR instruction (`QUENCH_LANE_COMPILE`), an interpreted
    /// instruction some 50 ns.
    pub(in crate::vm) compile_cost_per_ir: u64,
    /// The share of a function's interpreted work its compiled body saves, as a fraction.
    pub(in crate::vm) saved_share: (u64, u64),
    /// How many times the compile's cost the saved work must reach before the body compiles.
    pub(in crate::vm) benefit_over_cost: u64,
    /// The entries, with `recurrent_ticks`, past which a function specializes by its recurrence
    /// alone: entered at least this many times, read at every multiple of it
    /// (`LaneController::recurrent`).
    pub(in crate::vm) recurrent_entries: u32,
    /// The program's warming clock ticks since a function's first entry that its recurrence
    /// must span (`LaneController::recurrent`).
    pub(in crate::vm) recurrent_ticks: u32,
    /// The doublings of the warm-up a function's dropped bodies may cause.
    pub(in crate::vm) max_backoff: u8,
    /// Recoveries of one function's frameless activations in one collection epoch after which
    /// its calls keep frames for the rest of the epoch, so warm-up exits do not disable it for
    /// good.
    pub(in crate::vm) materialization_limit: u32,
    /// The slots of the frameless callee table, indexed by the callee's bits: a power of two.
    pub(in crate::vm) frameless_callee_slots: usize,
    /// The lane records a loop-free body runs below which a frameless activation is cheaper than
    /// the JavaScript tier's native entry of it. A direct native entry (`lane_tier_prologue`,
    /// `direct_call`, the shared entry) costs about 380 machine instructions past the
    /// activation, and a lane record about 20 (callgrind, typescript-lib under `--jit`,
    /// 2026-10-28: 9.7M native entries of 3- to 12-instruction predicates such as
    /// `isLineBreak`), so a body shorter than this cannot recoup its entries.
    pub(in crate::vm) native_entry_records: usize,
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
    /// The work (`WasmModule::note_work`) at which a module running under `--jit` compiles
    /// whole (`WasmModuleCompile::ADAPTIVE`).
    pub(in crate::vm) module_work: u32,
    /// The milliseconds after a module's first work at which its next entry compiles it
    /// whole, for a module whose work the counts undersell.
    pub(in crate::vm) module_after_millis: u64,
    /// How often, in work, a selective module reads the clock for `module_after_millis`:
    /// often enough that the bound lands within a few interpreted calls, rarely enough that
    /// the clock stays off an interpreted call's path.
    pub(in crate::vm) module_clock_interval: u32,
    /// The most bytecodes of a Wasm function a hot-root tier promotion compiles.
    pub(in crate::vm) tier_function_bytecodes: usize,
}

/// The distinct cases (shapes, callees, targets) a site's feedback keeps before it widens to
/// the generic case: the width of every polymorphic cache and of each guarded switch a tier
/// specializes a site into. One bound for every lattice of site feedback.
pub(in crate::vm) const POLYMORPHISM: usize = 4;

/// The speculative bodies a tier compiles for one owner (`tier_state::Tier`) before the next is
/// the permanent generic one: a body is replaced by a successor that drops what its leaves
/// showed, once per distinct case a site's feedback may hold.
pub(in crate::vm) const SPECULATIVE_BODIES: u8 = POLYMORPHISM as u8;

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
    lane_body: LaneBody { warmup_entries: 2, reoptimize_exits: 2, min_instructions: 0, compile_poll_interval: 0, ..MEASURED.lane_body },
    js_function: JsFunction { promotion_looks: [(1, 1), (2, 2), (3, 3), (4, 4)], general_promotion_entries: 1, general_compile_fixed_cost: 0, general_compile_cost_per_instruction: 0, ..MEASURED.js_function },
    wasm: Wasm { promotion_entries: 1, backedges_per_entry: 2, eager_module_bytecodes: usize::MAX, ..MEASURED.wasm },
    limits: MEASURED.limits,
    lane_view: MEASURED.lane_view,
};

const MEASURED: Tiers = Tiers {
    loops: Loops { threshold: 1000, max_exits: 1000, max_declines: 64, widen_after_leaves: 4, body_reoptimize_leaves: 16, body_poll_interval: 16 },
    lane_body: LaneBody {
        warmup_entries: 400,
        reoptimize_share: 1,
        reoptimize_exits: 64,
        min_instructions: 0,
        compile_poll_interval: 64,
        compile_cost_per_ir: 5,
        saved_share: (1, 2),
        benefit_over_cost: 1,
        recurrent_entries: 16,
        recurrent_ticks: 256,
        max_backoff: 7,
        materialization_limit: 16,
        frameless_callee_slots: 256,
        native_entry_records: 20,
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
    wasm: Wasm {
        promotion_entries: 32,
        backedges_per_entry: 1024,
        eager_module_bytecodes: 4096,
        module_work: 4096,
        module_after_millis: 20,
        module_clock_interval: 16,
        tier_function_bytecodes: 2048,
    },
    limits: Limits {
        js_function: JsFunctionLimits { instructions: 256, registers: 256, parameters: 8, code_bytes: 64 * 1024 },
        general_blocks: 2048,
        lane_region_blocks: 2048,
        region: RegionLimits { instructions: 512, registers: 128, arrays: 8, captures: 16, fields: 16, field_stores: 16 },
        inlining: InliningLimits { callee_instructions: 96, unit_instructions: 920, depth: 5 },
    },
    lane_view: LaneView { deferred_runs: 4, hot_back_edges: 16, loop_payback: 8, entry_min_run: 3, entry_min_call_run: 8 },
};

/// Whether a general loop may enter a JS tier. A parameter-initialization run (`stops`)
/// observes every PC up to its stop, and a tier would run past it.
pub(in crate::vm) const fn tiers_enter(stops: bool) -> bool {
    crate::DYNAMIC_EXECUTION_ENABLED && !stops
}

/// Which tiers a program store runs, read once when the store is made from the development
/// experiments (`QUENCH_DEV`); a `tier-stress` build has every arm on and nothing priced.
pub(in crate::vm) struct Switches {
    /// Whether ordinary JS functions promote to the general native dispatch tier. The memory
    /// policy (`PolicyTiers::js_function_tiers`) and the entry price still decide which
    /// function the tier takes.
    pub(in crate::vm) general_dispatch: bool,
    /// Whether the lane prologue specializes a declined function into a frameless lane body.
    pub(in crate::vm) lane_methods: bool,
    /// Whether a hot function's general baseline waits until its entries pay for the compile;
    /// tests of the baseline's semantics compile it once hot.
    pub(in crate::vm) general_priced: bool,
    /// Whether the numeric entry specializers run; a test turns them off to select a tier
    /// without depending on which shapes they support.
    #[cfg(test)]
    pub(in crate::vm) entry_specializations: bool,
}

impl Switches {
    pub(in crate::vm) fn read() -> Self {
        use crate::trace::Dev;
        Self {
            general_dispatch: STRESS || Dev::JsGeneralDispatch.on(),
            lane_methods: STRESS || Dev::JsLaneMethods.on(),
            general_priced: !(STRESS || Dev::JsUnpricedGeneralDispatch.on()),
            #[cfg(test)]
            entry_specializations: true,
        }
    }
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
    use super::*;

    #[test]
    fn the_tier_table_is_consistent() {
        let lane = &TIERS.lane_body;
        assert!(lane.warmup_entries > 0);
        // The warm-up doubles per dropped body, at most `max_backoff` times, and must not overflow.
        assert!(u32::from(lane.warmup_entries) << lane.max_backoff <= u32::from(u16::MAX));
        // The reoptimization threshold doubles per recompile and must not overflow its counter.
        assert!(u32::from(lane.reoptimize_exits) << SPECULATIVE_BODIES <= u32::from(u16::MAX));
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
