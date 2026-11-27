//! When code moves between tiers: every threshold the tier controllers read, in one table.
//!
//! The controllers (`LoopTier` and `LoopSite`, `LaneBody`, `JsTierState`, `WasmTierState`)
//! own their states and transitions; this module owns the numbers those transitions compare
//! against, and what each memory policy lets compile. A tuning experiment changes a row here,
//! never a controller.

use crate::bytecode::{FieldBase, FieldLookup, Function, ResidualProgram};
use crate::compile::call_graph::{CallGraph, LOOP_WEIGHT};
use crate::jit_mode::Tier;
use crate::staged::meaning::{means, Meaning};
use crate::staged::Operation;

/// The thresholds every tier controller reads.
pub struct Tiers {
    pub loops: Loops,
    pub lane_body: LaneBody,
    pub(crate) js_function: JsFunction,
    pub wasm: Wasm,
    pub stitch: Stitch,
    pub priors: Priors,
    /// Whether every eligible compile is admitted and every JavaScript compilation arm is on, so a
    /// run checks each tier against the interpreter (`Tiers::admits_all`).
    pub(crate) stress: bool,
    /// Whether the tier models admit every eligible compile, whatever it would repay.
    pub admits_all: bool,
}

/// The priors the model tests pin their arithmetic to.
#[cfg(any(test, feature = "test-support"))]
pub const TEST_PRIORS: Priors = Priors {
    interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 3000, compile_wall_permille: 1000,
    region_entry_ns: 140, body_entry_ns: 330, base_body_share: 500, remaining_divisor: 4, plan_ns_per_ir: 1500,
};

/// The runs a loop-free function makes unlowered before a later call lowers it
/// (`bytecode::tier0`): the runs a lane view waits for (`LANE_VIEW_DEFERRED_RUNS`), so the
/// lowering lands before its view derives. Octane-code-load instructions against lowering
/// at the first read: -9.8% at four, -8.2% at two; mobx-startup -0.5% at both.
pub(crate) const TIER0_RUNS: u8 = 4;

/// The calls past `TIER0_RUNS` whose lowering a live frame of the function blocked, after
/// which its lane view derives from the unlowered record (`ResidualProgram::tier0_blocked`).
pub(crate) const TIER0_BLOCKED_RUNS: u8 = 16;

/// Stitch-inlining: the callee rows the b-jit stitcher splices in place of a direct-cell call row
/// (`base_region::inline_plan`). Which callees inline is `inlines`; this is the stitch's room.
pub struct Stitch {
    /// The callee bytecodes one stitch inlines in all, so a hot loop's code stays a few call
    /// bodies' worth.
    pub max_total_instructions: usize,
    /// The frame slots above the caller's registers the inlined callees' windows may take: the room
    /// a body's recovery reserves past its activation (`LaneRecoverySpace`).
    pub window_registers: u32,
    /// The inline depth: activations a leave inside inlined code materializes, the caller's
    /// included.
    pub max_depth: usize,
}

/// Parts per thousand, the unit of a prior's share.
pub const PER_MILLE: u64 = 1000;

/// The priors the tier model starts from, each a statement about the world until the engine's own
/// measurements replace it (`tier-decision-rule-v1`): nothing here is fitted to a benchmark row.
pub struct Priors {
    /// Nanoseconds an interpreted lane bytecode takes.
    pub interpreter_ns: u64,
    /// A u-jit body's per-bytecode time as a share of the interpreter's, in thousandths: bodies
    /// run 2 to 5 times faster per entry than the interpreter once entered (inclusive timing).
    pub body_share: u64,
    /// Nanoseconds the code generator takes to compile one IR instruction: a fixed price on the
    /// compile's work (the running ratio it replaced settled at 1.3-2.2 us on JS3 rows).
    pub compile_ns_per_ir: u64,
    /// The part per thousand of a compile's time that waits on the running program: compiles run
    /// on the code generator's thread, so most of their time overlaps it (two cores).
    pub compile_wall_permille: u64,
    /// Nanoseconds entering a loop region from the lane costs per call, and a body's entry
    /// (measured about 300 ns: some 1,100 instructions at 0.3 ns).
    pub region_entry_ns: u64,
    pub body_entry_ns: u64,
    /// A b-jit body's per-bytecode time as a share of the interpreter's: stencil code runs about
    /// half of it (values stay in the frame). Its entry is `body_entry_ns`, as a u-jit body's: both
    /// publish a `LaneMethod` the one entry driver runs (`lane_body_entry`, `Vm::run_lane_body`),
    /// whose binding of the lane frame is the crossing's cost, not the code the body enters.
    pub base_body_share: u64,
    /// The divisor of the entries and the interpreter time so far that gives the credible lower
    /// bound of those to come, where the decision (a compile) is irreversible.
    pub remaining_divisor: u64,
    /// Nanoseconds the plan of a lane body (inlining, proofs, lowering to IR) takes per IR
    /// instruction on the program's own thread (measured 1.46 us on Box2D): a fixed price on its
    /// work, none of which overlaps the program as a compile on the code generator's thread does.
    pub plan_ns_per_ir: u64,
}

/// The loop tier (OSR regions at backward jumps).
pub struct Loops {
    /// The backward jumps a loop takes in the interpreter before the tier model weighs its
    /// compile (`loop_model`): a resource and correctness floor, enough that the call sites of
    /// its body have linked their targets (which the region inlines) and its entry facts are
    /// observed. The model asks more where the compile does not yet pay.
    pub warmup_taken: u32,
    /// IR instructions a bytecode of a loop's span compiles to, which prices the compile before
    /// it runs (its own census replaces the guess in the learned cost per IR instruction).
    pub ir_per_bytecode: u64,
    /// The weight of the prior that a site's native entries repay themselves, in entries: the
    /// site's measured runs outweigh it as they mount (`loop_model::repaid_entries`), and it stops
    /// entering where its entries cost more than the iterations they ran save.
    pub prior_runs: u64,
    /// Runs that leave at one operation before the site compiles again with that operation's
    /// int32 speculation dropped (`LoopTier::left_at`): a sum that overflows on most runs
    /// (SHA-256's rounds) is a double's, and one that overflowed once is not.
    pub widen_after_leaves: u16,
    /// Compilations a site may make: each after the first widens a value the last one held as
    /// an `i32`, and a region has few such values.
    pub max_compilations: u32,
}

/// The frameless lane-body tier (`LaneView::tier_lane`).
pub struct LaneBody {
    /// The executions a site's record is credited with before its statistics are read
    /// (`site_posterior::SiteEvidence::of`): a bound a static fact supplies, no count of the run.
    pub site_executions: u16,
    /// The share of a body's runs, in percent, its leaves may reach, from `reoptimize_exits`
    /// leaves on, before it specializes again.
    pub reoptimize_share: u8,
    /// The runs after which a body that no more recompile may change, and that left more than
    /// `retire_share` percent of them, stops being entered: it pays its entry and its exit for
    /// work the lane does anyway.
    pub retire_after_runs: u16,
    pub retire_share: u8,
    /// The leaves after which a body specializes again from the feedback its sites hold then:
    /// a region compiled while its call graph warmed keeps leaving at sites whose targets came
    /// later, the deoptimization that reoptimizes. Each recompile doubles it, as JSC's exit
    /// threshold backs off, and `max_recompiles` bounds it.
    pub reoptimize_exits: u16,
    pub max_recompiles: u8,
    /// The most compiles of one function's body from its entry: a request of another plan past
    /// them is declined, so what each came to is the last that holds.
    pub(crate) max_compiles: u8,
    /// The same for each loop entry (OSR) of a function: its own budget, which the entry
    /// compiles of the function do not draw on and which they do not exhaust.
    pub(crate) max_osr_compiles: u8,
    /// The most bodies of one function lifecycle edges may discard before it runs in the lane
    /// for good: past them a compile costs more than its runs regain.
    pub max_lifetimes: u8,
    /// The times a function specializes again because a callee published its body
    /// (`LaneView::respecialize_callers`): each makes more of its calls direct, and the bound
    /// ends the exchange between functions that call each other. One: a second and third
    /// respecialization cost more compile than their direct calls regained (deltablue at 120
    /// runs: total Ir -2%, richards unchanged; Nov 18).
    pub max_callee_recompiles: u8,
    pub late_call_calls: u32,
    /// The most entries between looks for a compiling body's code, which start one entry apart
    /// and double (`LaneBody::Compiling`). A look costs a channel poll
    /// and the specialization's bookkeeping, some 300 instructions, at each backward jump of
    /// a hot loop while the code compiles (Richards' first body: 300 ms); the code waits at
    /// most this many entries once ready.
    pub compile_poll_interval: u16,
}

/// The per-function JavaScript tier: the numeric optimizing tier.
pub(crate) struct JsFunction {
    /// Prespecified looks for the Number-entry profile. Critical counts are the exact
    /// one-sided Binomial( n, 0.80 ) cutoffs at alpha = 0.05 / 4 per look. A union bound caps
    /// false promotion at 5% across looks for one function under a stationary Bernoulli model;
    /// it is not a VM-wide multiple-test bound.
    pub(crate) promotion_looks: [(u8, u8); 4],
}

/// The Wasm function and module tiers.
pub struct Wasm {
    /// Promote a Wasm function only after repeated interpreter entries, or the loop work of as
    /// many (`backedges_per_entry`). Wasm signatures and bytecode are fixed, so this is a
    /// hotness gate.
    pub(crate) promotion_entries: u8,
    /// The backward jumps a lane run of a Wasm function counts as one interpreter entry toward
    /// `promotion_entries`: a loop of this many iterations does at least the work of an entry
    /// through a short call, so a function entered rarely but looping hot promotes after
    /// `promotion_entries` such stretches.
    pub backedges_per_entry: u32,
    /// The size, in bytecodes, up to which a Wasm module compiles whole at its first entry
    /// rather than starting in the interpreter: compiling it costs about what the work budget
    /// (`WASM_MODULE_WORK_BUDGET`) would have spent interpreting it, so waiting buys nothing.
    /// Whole compiles take 3-7 us a bytecode (Linux x86_64): quicksort-wasm, 2,412, compiles in
    /// 16 ms and lost 13% of its JetStream Score waiting; source-map's module, 12,971, takes
    /// 67 ms, which its one short parse never repays.
    pub(crate) eager_module_bytecodes: usize,
}

/// The prototype links a specializer unrolls a staged walk over (`Stage::walk_prototypes`): an
/// instance, its class chain and `Object.prototype` end within it, and each link is a block of
/// code, so a longer chain is the VM's residual rather than more code.
pub const PROTOTYPE_UNROLL_LINKS: u32 = 4;

/// The distinct cases (shapes, callees, targets) a site's feedback keeps before it widens to
/// the generic case: the width of every polymorphic cache and of each guarded switch a tier
/// specializes a site into. One bound for every lattice of site feedback.
pub const POLYMORPHISM: usize = 4;

/// Whether a closed-set call's plan inlines its arms' bodies, so the call is lowered as the class-layout compare chain
/// (`closed_set_by_layout`) rather than the indirect call through the class's method vector (`closed_set_indirect`):
/// at most three arms, each of which the rule `inlines` admits.
pub fn arms_inline(plan: &crate::compile::call_plan::CallPlan, graph: &CallGraph, program: &ResidualProgram) -> bool {
    plan.arms.len() <= 3 && plan.arms.iter().all(|arm| inlines(graph, program, arm.function))
}

/// What a speculated fact costs and saves, in nanoseconds, from which a site's break-even rate
/// derives (`site_posterior::break_even`) with the posterior's quantile (`tier-decision-rule-v1`
/// section 4). Per execution a speculating read pays `guard_read_ns` when the fact holds and
/// `guard_exit_ns` (the leave, the interpreter's replay and the re-entry) when it does not; the
/// in-place generic read (`field_probe`) pays `probe_read_ns` either way. Speculation wins when
/// `p (probe - guard) > (1 - p) (exit - probe)`, so the break-even rate is
/// `(exit - probe) / (exit - guard)`.
pub struct Speculation {
    pub guard_read_ns: u32,
    pub probe_read_ns: u32,
    pub guard_exit_ns: u32,
    /// The standard-normal deviate of the posterior's lower 5% quantile: the Beta's quantile is
    /// taken as mean minus this many standard deviations (conservative for a skewed Beta).
    pub lower_quantile_z: f64,
}

/// A guarded read is a load and a compare; the probe is the VM cache's hit path; an exit costs
/// the region entry and exit plumbing (`Priors::region_entry_ns`, 140 ns) and the replayed
/// instructions of the leave.
pub const SPECULATION: Speculation =
    Speculation { guard_read_ns: 1, probe_read_ns: 6, guard_exit_ns: 200, lower_quantile_z: 1.645 };
#[cfg(any(test, feature = "test-support"))]
thread_local! { static FORCED_ADMISSION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// Makes the VMs this thread makes next admit every eligible compile (`ADMITTING`), as the floor
/// row does: for tests of what compiled code does, on fixtures too small to repay a compile.
#[cfg(any(test, feature = "test-support"))]
pub fn force_admission() { FORCED_ADMISSION.set(true); }

/// The rows a VM's tier controllers read, held by the VM from its construction: the measured
/// thresholds a process runs by, or the floor row, which makes every eligible site compile on
/// its first chance, so a Test262 run and the tier differential check each tier against the
/// interpreter. Code compiled under a row takes it as a Static fact (`VmFacts::tiers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierRows { Measured, Floor }

impl TierRows {
    /// The rows a process starts with: the floor under a `tier-stress` build, the measured otherwise.
    pub(crate) const PROCESS: Self = if cfg!(feature = "tier-stress") { Self::Floor } else { Self::Measured };

    /// The rows a VM made now holds: `PROCESS`, or the floor rows under `QUENCH_DEV=floor-rows`,
    /// admitting every compile where a test forced it.
    pub(crate) fn made() -> &'static Tiers {
        #[cfg(any(test, feature = "test-support"))]
        if FORCED_ADMISSION.get() { return &ADMITTING; }
        if crate::trace::Dev::FloorRows.on() { return &FLOOR; }
        Self::PROCESS.table()
    }

    #[quench_macros::test_visible]
    pub(crate) fn table(self) -> &'static Tiers {
        match self { Self::Measured => &MEASURED, Self::Floor => &FLOOR }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl crate::vm::Vm {
    #[quench_macros::test_visible]
    /// A VM whose tier controllers read `rows`: a stress run needs no build of its own.
    pub(crate) fn with_tier_rows<T: crate::Host>(host: T, rows: TierRows) -> Self {
        let mut vm = Self::new(host);
        vm.programs.hold_tiers(rows.table());
        vm
    }
}

/// Each threshold at the least value its controller accepts. Thresholds only price
/// compilation, so this row must compute what `MEASURED` computes.
const FLOOR: Tiers = Tiers {
    loops: Loops { warmup_taken: 2, ..MEASURED.loops },
    lane_body: LaneBody { site_executions: 2, reoptimize_exits: 2, compile_poll_interval: 0, ..MEASURED.lane_body },
    js_function: JsFunction { promotion_looks: [(1, 1), (2, 2), (3, 3), (4, 4)] },
    wasm: Wasm { promotion_entries: 1, backedges_per_entry: 2, eager_module_bytecodes: usize::MAX },
    stitch: MEASURED.stitch,
    priors: MEASURED.priors,
    stress: true,
    admits_all: true,
};

impl LaneBody {
    /// The compiles allowed a function's whole body, or of one of its loop entries (OSR).
    pub fn compile_bound(&self, osr: bool) -> u8 {
        if osr { self.max_osr_compiles } else { self.max_compiles }
    }

}

impl Tiers {
    /// The length a body without loops or calls needs for one entry to pay in the lane body
    /// `tier` compiles: none where every eligible compile is admitted (`Tiers::admits_all`), so
    /// a fixture too short to repay one still gets its body. Each tier asks the entry's crossing
    /// (`body_entry_ns`, the one entry driver's) over the interpretation its code saves per
    /// bytecode (`Priors::repaying_length`): a stencil body keeps `base_body_share` of it, a
    /// Cranelift body `body_share`.
    pub fn amortizing_instructions(&self, tier: Tier) -> usize {
        if self.admits_all { return 0; }
        match tier {
            Tier::Base => self.priors.repaying_length(self.priors.base_body_share),
            Tier::Quick | Tier::Ultra | Tier::Ahead => self.priors.repaying_length(self.priors.body_share),
        }
    }
}

impl Priors {
    /// The fewest straight-line bytecodes whose interpretation, less the `share` per mille code
    /// still takes of it, covers a body's entry (`body_entry_ns`): shorter, no entry repays itself.
    pub(crate) fn repaying_length(&self, share: u64) -> usize {
        let saved = (self.interpreter_ns * (PER_MILLE - share)).max(1);
        (self.body_entry_ns * PER_MILLE).div_ceil(saved) as usize
    }
}

/// The tier a lane body of a function with predicted tier `pick` runs in, or `None` where the interpreter runs it
/// (design decision 4.9). The pick is the one input; the mode only names a compiled tier the mode allows, where it
/// allows no other: `--b-jit` runs Base alone, `--q-jit` Quick alone, `--u-jit` Ultra alone, and a mode that
/// leaves Base out (aarch64's default) runs Ultra for a Base pick.
pub fn lane_body_tier(pick: PredictedTier) -> Option<Tier> {
    let wanted = match pick {
        PredictedTier::Interpreter => return None,
        PredictedTier::Base => Tier::Base,
        PredictedTier::Ultra => Tier::Ultra,
    };
    if crate::jit_mode::tiers_allowed(&[wanted]) {
        return Some(wanted);
    }
    [Tier::Ultra, Tier::Base, Tier::Quick].into_iter().find(|tier| crate::jit_mode::tiers_allowed(&[*tier]))
}

/// The base a share in percent is a share of.
pub const PERCENT: u64 = 100;


const MEASURED: Tiers = Tiers {
    loops: Loops { warmup_taken: 64, ir_per_bytecode: 8, prior_runs: 128, widen_after_leaves: 4, max_compilations: 16 },
    lane_body: LaneBody {
        site_executions: 400,
        reoptimize_share: 1,
        retire_after_runs: 1000,
        retire_share: 10,
        reoptimize_exits: 64,
        max_recompiles: 1,
        max_compiles: 3,
        max_osr_compiles: 3,
        max_lifetimes: 8,
        max_callee_recompiles: 1,
        late_call_calls: 4096,
        compile_poll_interval: 64,
    },
    js_function: JsFunction { promotion_looks: [(32, 31), (64, 59), (128, 113), (255, 219)] },
    wasm: Wasm { promotion_entries: 32, backedges_per_entry: 1024, eager_module_bytecodes: 4096 },
    stitch: Stitch {
        max_total_instructions: 96,
        window_registers: 32,
        max_depth: 3,
    },
    priors: Priors { interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 2000, compile_wall_permille: 100, region_entry_ns: 140, body_entry_ns: 330, base_body_share: 500, remaining_divisor: 4, plan_ns_per_ir: 1500 },
    stress: false,
    admits_all: false,
};

/// The rows code compiled for no particular VM is compiled under (`VmFacts::STATIC`).
pub const STATIC_ROWS: &Tiers = &MEASURED;

/// The measured rows admitting every eligible compile: for tests of what compiled code does, on
/// fixtures too small to repay one (`force_admission`).
#[cfg(any(test, feature = "test-support"))]
const ADMITTING: Tiers = Tiers { admits_all: true, ..MEASURED };

/// Whether a general loop may enter a JS tier. A parameter-initialization run (`stops`)
/// observes every PC up to its stop, and a tier would run past it.
#[inline(always)]
pub(crate) const fn tiers_enter(stops: bool) -> bool {
    crate::DYNAMIC_EXECUTION_ENABLED && !stops
}

/// What a memory policy lets the tiers compile.
pub struct PolicyTiers {
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
    pub loop_tier: bool,
    /// Whether the JavaScript function tiers (the per-function tier and the loop tier's site
    /// analysis) compile. Memory-first compiles none, on the
    /// owner's direction that it may trade Score for memory under `--jit` (TOD-123). Under
    /// memory-first their code and regalloc2 state set the `--jit` peaks: the function tiers
    /// took V8-v7 Crypto `--jit` from 3220 to 5496 KiB (median, bolted static x86_64).
    /// With both off, every V8-v7 `--jit` row peaked 84-380 KiB lower, at its no-JIT level.
    /// Scores moved both ways: EarleyBoyer +125%, Splay +15%, RayTrace +11%, NavierStokes +8%,
    /// but DeltaBlue -9%, Richards -7%, RegExp -7% (same unbolted static binary, 5 runs,
    /// 2026-10-24).
    pub(crate) js_function_tiers: bool,
}

/// One row per memory policy, in `MemoryPolicy` order.
const BY_POLICY: [PolicyTiers; crate::MemoryPolicy::ALL.len()] = [
    PolicyTiers { loop_tier: true, js_function_tiers: true },
    PolicyTiers { loop_tier: false, js_function_tiers: false },
];

/// The one tier predicate: whether any of `tiers` may run for a function under `policy` and the
/// process's JIT mode (`jit_mode::tiers_allowed`).
#[inline]
pub fn tier_may_run(policy: crate::MemoryPolicy, tiers: &[crate::jit_mode::Tier]) -> bool {
    by_policy(policy).js_function_tiers && crate::jit_mode::tiers_allowed(tiers)
}

/// Whether a compile may start at all: some compiling tier is allowed.
pub fn may_compile() -> bool {
    crate::jit_mode::tiers_allowed(Tier::ANY)
}

/// Whether `tier` may publish a body: a tier the mode allows, with QuickJIT only where it is the only Cranelift tier.
pub fn may_publish(tier: Tier) -> bool {
    match tier {
        Tier::Quick => crate::jit_mode::quick_only(),
        tier => crate::jit_mode::tiers_allowed(&[tier]),
    }
}

/// Whether QuickJIT is the only Cranelift tier (`--q-jit`): every Cranelift compile is `CompileLevel::Quick`, and a
/// published lane body is a QuickJIT body.
pub fn quick_only() -> bool {
    crate::jit_mode::quick_only()
}

/// What `policy` lets the tiers compile.
pub const fn by_policy(policy: crate::MemoryPolicy) -> &'static PolicyTiers {
    &BY_POLICY[policy as usize]
}

// Lane entry (dispatch_fast): the runs a lane entry must repay
/// The shortest straight-line lane run the general loop enters. Entering
/// builds the lane context and memory view, which one or two lane
/// instructions do not repay: on V8-v7 fixed work, three keeps every
/// JavaScript benchmark within one percent of the general path or better
/// (one and two cost DeltaBlue three percent), and leaves Wasm loops,
/// whose runs are long, unchanged.
pub const LANE_ENTRY_MIN_RUN: usize = 3;

/// `LANE_ENTRY_MIN_RUN` in a function with no loop the lane runs whole: each call the
/// general loop makes into it enters the lane anew, so a run must repay a fresh lane
/// context.
pub const LANE_ENTRY_MIN_CALL_RUN: usize = 8;

// Loop sites (dispatch_fast/loop_body, loop_runs): body planning, entry backoff
/// The bodies a loop site plans, each from its predecessors' leaves.
pub const LOOP_BODY_ATTEMPTS: u8 = 4;

/// The runs of a loop's body that leave inside the loop before the next body plans around them.
pub const LOOP_BODY_REOPTIMIZE: u16 = 16;

/// The share of a loop body's runs, in percent, that may leave inside the loop for it to stay.
pub const LOOP_BODY_KEEP_SHARE: u64 = 10;

/// The backedges between looks for a loop body's compiling code.
pub const LOOP_BODY_POLL: u16 = 16;

/// The widest backoff after declined entries: `2^16 - 1` entries pass untried between attempts.
pub const MAX_ENTRY_BACKOFF: u32 = 16;

// Lane bodies: compile admission (compiled/lane_compile, lane_region, lane_unit)
/// The IR size above which a lane body's registers are allocated in one pass. The backtracking
/// allocator's scratch memory grows with the function, and a body this large (JetStream-scale
/// units of 23k-28k IR instructions) set the process's peak resident size while its code
/// compiled: V8-v7 raytrace peaked at 40.0 MB with it and 33.0 MB without, richards and
/// deltablue, whose largest bodies are 17k, keep it and the speed it buys (2026-11-11, lane
/// mode, 5 interleaved runs). Smaller bodies keep backtracking: single-pass for all lost 20-30%
/// on richards and deltablue.
pub const LANE_BACKTRACKING_MAX_INSTS: usize = 20_000;

/// The executable memory a lane body may take, a resource limit: the code and compile time a
/// hot loop's inlined call graph (Richards' ~900 instructions) costs. A body whose code exceeds it is declined: the
/// static inlining cap (`INLINE_GROWTH_PERCENT`) decides the unit's size once, with no replanning.
pub const LANE_BODY_MAX_CODE_BYTES: usize = 115 * 1024;

/// The code one ahead-of-time image body may take. An image compiles once, offline, into a file
/// the process loads: nothing of its cost is paid under a guest's warm-up, so the JIT's lane-body
/// limit (a warm-up resource bound) does not apply, and a body it would replan is instead part of
/// the image. The bound stays a resource limit: the image is loaded whole.
pub const AHEAD_BODY_MAX_CODE_BYTES: usize = 4 * LANE_BODY_MAX_CODE_BYTES;

/// The growth a unit's inlining may add, in percent of its root body's instructions (design decision 4.6): the callee
/// instructions one unit copies in all are at most this share of the root's own size, spent on the call sites in
/// program order (`lane_unit`'s planner). A callee with exactly one call site adds no growth (`charges_growth`).
pub const INLINE_GROWTH_PERCENT: usize = 10;

/// The longest body a callee with no call in it inlines: an accessor, an arithmetic helper. Its copy costs code at
/// each site, and its call costs most relative to its work (design decision 4.6: 64).
pub const INLINE_LEAF_MAX_INSTRUCTIONS: usize = 64;

/// The longest body a callee with exactly one call site in the closed world inlines. That one site takes the one copy
/// the body makes, so its bound is the compile time a unit may spend on one body, not the code its copies cost.
pub const INLINE_SINGLE_SITE_MAX_INSTRUCTIONS: usize = 400;

/// The longest body an accessor-shaped callee inlines (`accessor_shaped`): a field read, compare or store of `this`
/// and the moves and returns around them, whose call is the whole of its cost.
pub const INLINE_ACCESSOR_MAX_INSTRUCTIONS: usize = 12;

/// Whether inlining `callee` adds code to its unit: a callee with one call site in the closed world moves its one body
/// rather than copying it, so it adds no growth and is not charged to `INLINE_GROWTH_PERCENT`.
pub fn charges_growth(graph: &CallGraph, callee: u32) -> bool {
    !graph.single_call_site(callee)
}

/// Whether the callee `callee` of `program` inlines at a call site the planner has proved a single target for
/// (an arm of its call plan). The one rule every tier asks, and it reads only static
/// facts: the body's size and shape, and the program's call graph (`CallGraph::single_call_site`). A callee inlines
/// where (a) it is a leaf, with no call, under `INLINE_LEAF_MAX_INSTRUCTIONS`; or (b) it has exactly one call site in
/// the closed world, under `INLINE_SINGLE_SITE_MAX_INSTRUCTIONS`; or (c) it is accessor-shaped, under
/// `INLINE_ACCESSOR_MAX_INSTRUCTIONS`. Size is the instruction count of the callee's body.
pub fn inlines(graph: &CallGraph, program: &ResidualProgram, callee: u32) -> bool {
    let Some(body) = program.function_get(callee as usize) else { return false };
    let size = body.instructions().count();
    let leaf = !body.instructions().any(|instruction| means(instruction, Meaning::Call) || means(instruction, Meaning::Construct));
    (leaf && size <= INLINE_LEAF_MAX_INSTRUCTIONS)
        || (graph.single_call_site(callee) && size <= INLINE_SINGLE_SITE_MAX_INSTRUCTIONS)
        || (size <= INLINE_ACCESSOR_MAX_INSTRUCTIONS && accessor_shaped(body))
}

/// Whether `body` is accessor-shaped: it reads or writes a field of `this` (a `FieldLookup::Atom` on `FieldBase::THIS`),
/// and every other instruction is a number operation, a move, a constant, a `this` load, or a `Return`.
fn accessor_shaped(body: &Function) -> bool {
    let mut touches_this = false;
    let shaped = body.instructions().all(|instruction| match Operation::<u64, ()>::of(instruction) {
        Some(Operation::GetField { .. } | Operation::SetField { .. }) => match instruction.field_lookup() {
            Some(FieldLookup::Atom { base, .. }) if base == FieldBase::THIS => {
                touches_this = true;
                true
            }
            _ => false,
        },
        Some(Operation::Number(_) | Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadThis { .. } | Operation::Return { .. }) => true,
        _ => false,
    });
    shaped && touches_this
}

// Lane bodies: history, recurrence and loop views (lane_body_tier, dispatch_frame, program_store)
/// The backward jumps a JavaScript activation takes in the general path before its function's
/// lane view is derived and the lane runs its loop (`Vm::note_back_edge`). Deriving a view
/// costs some hundreds of instructions per instruction of the function; a module body that
/// loops a few times (over its exports, say) ran faster without one.
pub(crate) const HOT_LOOP_BACK_EDGES: u16 = 16;

/// The loop work, in instructions its back edges span per instruction of the function, after
/// which a view-less activation derives its function's lane view (`HotLoop`). A view costs
/// some hundreds of instructions per instruction of the whole function, so a short loop in a
/// large body that runs once (a module body filling a table) ran
/// faster without one. Measured in instructions, warm compile cache, against deriving at
/// `HOT_LOOP_BACK_EDGES` alone (x86_64 VM, 2026-10-26): @babel/core -11.0% (one: +0.9%,
/// four: -9.9%), and within 0.2% on schema-utils, axios, semver, ajv, eslint, jiti, undici
/// and source-map.
pub(crate) const LANE_VIEW_LOOP_PAYBACK: u32 = 8;

/// The completed handler runs of a body per credit to the callees it calls natively: a callee's
/// completions inside those runs are bounded by the caller's, so the credit lags by fewer than this.
pub(crate) const NATIVE_CREDIT_RUNS: u64 = 64;

/// The tier a function runs in under default mode (design decision 4.9): the interpreter, or a compiled body. `Tier`
/// names the compiled tiers only, so the interpreter is this enum's own variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictedTier {
    Interpreter,
    Base,
    Ultra,
}

/// IR instructions the interpreter spends per bytecode of one run of a function.
pub const INTERPRETER_IR_PER_OP: u64 = 40;
/// IR instructions a b-jit (stencil) body spends per bytecode of one run.
pub const BJIT_IR_PER_OP: u64 = 12;
/// IR instructions a u-jit body spends per bytecode of one run.
pub const UJIT_IR_PER_OP: u64 = 4;
/// IR instructions the b-jit's one-time compile of a body spends per bytecode of the body.
pub const BJIT_COMPILE_IR_PER_OP: u64 = 60;
/// IR instructions the u-jit's one-time compile of a body spends per bytecode of the body.
pub const UJIT_COMPILE_IR_PER_OP: u64 = 3000;
/// IR instructions an interpreted call of a function spends on its frame, per call.
pub const ENTRY_IR_INTERPRETER: u64 = 40;
/// IR instructions a b-jit body's entry spends per call: the stencil crossing and its lane frame.
pub const ENTRY_IR_BJIT: u64 = 150;
/// IR instructions a u-jit body's entry spends per call: its entry record and its prologue.
pub const ENTRY_IR_UJIT: u64 = 300;

// The tier order is locked: each compiled tier runs a bytecode for less IR than the one before it, and compiles for more.
const _: () = assert!(INTERPRETER_IR_PER_OP > BJIT_IR_PER_OP && BJIT_IR_PER_OP > UJIT_IR_PER_OP);
const _: () = assert!(BJIT_COMPILE_IR_PER_OP < UJIT_COMPILE_IR_PER_OP);

/// The tier a function runs in: the argmin over the three tiers of its predicted IR. A run of the function costs its
/// work per activation (`CallGraph::loop_work`, its bytecodes each weighed by the loops it sits in) times its static
/// frequency (`CallGraph::frequency`) times the tier's cost per bytecode; a compiled tier adds its one-time compile
/// over the bytecodes; each call of the function pays its tier's entry (`ENTRY_IR_*`). `called_by_loop` is a loop
/// region's call of the function, one loop more. Ties go to the tier that compiles less: the interpreter, then the
/// b-jit. Pure: no count and no clock.
pub fn predicted_tier(graph: &CallGraph, program: &ResidualProgram, function: u32, called_by_loop: bool) -> PredictedTier {
    let bytecodes = program.function_get(function as usize).map_or(0, |body| body.instructions().count() as u64);
    let frequency = graph.frequency(function).saturating_mul(if called_by_loop { LOOP_WEIGHT } else { 1 });
    let work = graph.loop_work(function);
    let run = |per_op: u64| frequency.saturating_mul(work).saturating_mul(per_op);
    let compile = |per_op: u64| bytecodes.saturating_mul(per_op);
    let entry = |per_call: u64| frequency.saturating_mul(per_call);
    let interpreter = run(INTERPRETER_IR_PER_OP).saturating_add(entry(ENTRY_IR_INTERPRETER));
    let base = run(BJIT_IR_PER_OP).saturating_add(compile(BJIT_COMPILE_IR_PER_OP)).saturating_add(entry(ENTRY_IR_BJIT));
    let ultra = run(UJIT_IR_PER_OP).saturating_add(compile(UJIT_COMPILE_IR_PER_OP)).saturating_add(entry(ENTRY_IR_UJIT));
    if interpreter <= base && interpreter <= ultra {
        PredictedTier::Interpreter
    } else if base <= ultra {
        PredictedTier::Base
    } else {
        PredictedTier::Ultra
    }
}

/// Shared-matcher IR per exec of a regexp pattern: the interpreter over the pattern's compact program
/// (`quench_regexp::backtrack::Program`). Estimate: `regexp.md` section 1 puts 100 to 800 extra IR per find on a
/// 290 IR body; unmeasured.
pub const REGEXP_SHARED_IR_PER_EXEC: u64 = 400;
/// IR per exec of a pattern's own compiled (u-jit) matcher. Estimate from `regexp.md` section 1's 290 IR body, unmeasured.
pub const REGEXP_COMPILED_IR_PER_EXEC: u64 = 120;
/// IR the u-jit's one-time compile spends per instruction of the pattern's compact program (as `UJIT_COMPILE_IR_PER_OP`).
pub const REGEXP_COMPILE_IR_PER_INST: u64 = 3000;
/// Machine-code bytes a compiled pattern emits per instruction of its compact program. Estimate: PCRE2-JIT's 982 B per
/// pattern (`review-4-policy.md`, 4.10) times the Cranelift factor of 3 to 6, over a typical pattern's instructions; unmeasured.
pub const REGEXP_CODE_BYTES_PER_INST: u64 = 64;
/// IR a code byte of a compiled pattern weighs in the compile decision (`regexp_compiles`). Placeholder for the C5
/// size weight, which has no number in this folder; the first measured pattern fixes it.
pub const REGEXP_BYTE_WEIGHT: u64 = 8;

/// Whether a regexp pattern gets its own compiled matcher (design decision 4.10): the argmin of 4.9, with code bytes
/// priced in. The predicted IR saved is the pattern's static frequency (`CallGraph::regexp_site_frequency`, the
/// frequencies of the functions that use it) times the exec IR the compiled matcher saves, against the compile's IR
/// plus its code bytes times `REGEXP_BYTE_WEIGHT`. `pattern_size` is the compact program's instruction count. A pattern
/// the decision rejects runs the shared matcher over that program. Pure: no count and no clock.
pub fn regexp_compiles(pattern_static_frequency: u64, pattern_size: usize) -> bool {
    let size = pattern_size as u64;
    let saved = pattern_static_frequency.saturating_mul(REGEXP_SHARED_IR_PER_EXEC.saturating_sub(REGEXP_COMPILED_IR_PER_EXEC));
    let code_bytes = size.saturating_mul(REGEXP_CODE_BYTES_PER_INST);
    let cost = size.saturating_mul(REGEXP_COMPILE_IR_PER_INST).saturating_add(code_bytes.saturating_mul(REGEXP_BYTE_WEIGHT));
    saved > cost
}

/// The runs a function makes on the general path before it derives its lane view, which costs
/// about a thousand instructions per bytecode instruction, so a function that runs fewer times
/// costs no view. Measured in instructions against deriving at the second run: four takes 3.3%
/// from the jiti npm scenario (Babel's many functions that run a few times) and 0.1-0.6% from
/// semver, ajv, Babel, color-convert and typescript-eslint; eight adds at most 0.3%.
pub(crate) const LANE_VIEW_DEFERRED_RUNS: u8 = 4;

// OSR loop regions: compile admission (compiled/osr_loop, osr_compile)
/// The most instructions a region spans. Native code polls termination only at backward
/// jumps, so the bound keeps compile time and code size proportional to small loop bodies.
pub const MAX_REGION_INSTRUCTIONS: usize = 512;

/// The most registers, captures and arrays a region names, bounding entry guards and the
/// entry record.
pub const MAX_REGION_REGISTERS: usize = 128;

pub const MAX_REGION_ARRAYS: usize = 8;

pub const MAX_REGION_CAPTURES: usize = 16;

pub const MAX_REGION_FIELDS: usize = 16;

pub const MAX_REGION_FIELD_STORES: usize = 16;

/// A lane region carries each inlined callee's full body and its guards, so a loop
/// that inlines two calls may legitimately exceed one function's code budget. The code
/// is published once per hot loop and retires with its site, so the larger per-site
/// bound stays bounded by the lane plan's own admission.
pub const REGION_CODE_BUDGET: usize = 2 * JS_COMPILED_MAX_CODE_BYTES;

// Compiled code budgets (compiled.rs): roots, threaded copies, unrolling, Wasm in flight, lane entry
pub const MAX_JIT_ROOT_FUNCTIONS: usize = 128;

pub const MAX_JIT_ROOT_BYTECODES_PER_FUNCTION: usize = 80_000;

pub const MAX_JIT_ROOT_CODE_BYTES: usize = 2 * 1024 * 1024;

/// A selective root may include a few hot callees learned from earlier
/// indirect-call executions. Keep the profile-guided closure bounded so one
/// polymorphic module cannot turn a tier promotion into eager compilation.
pub const MAX_JIT_PROFILED_INDIRECT_TARGETS: usize = 8;

pub const MAX_JIT_TIER_BYTECODES_PER_FUNCTION: usize = 2_048;

/// Instructions a threaded copy may repeat between a constant's jump and the
/// branch table the constant decides; bounds the code each copy adds. A
/// dispatch copy repeats its table too, which counts as its edges: TSF's
/// 290-way state table, copied at each of its 503 jumps, compiled one
/// function to 10.6 MB of edge moves (3.5 s, 220 MiB peak); charged by its
/// edges it compiles in 0.1 s.
pub const THREAD_BUDGET: usize = 16;

/// Instructions all copies of an unrolled loop body may hold together, and the most copies.
/// A body of up to a fourth of the budget runs as four copies: one interrupt check, one back
/// edge and one set of loop-carried moves per four iterations (V8's optimizing tier unrolls
/// small innermost Wasm loops for the same reasons); a body of more than half of it is not
/// unrolled.
pub const UNROLL_BUDGET: usize = 48;

pub const MAX_UNROLL_FACTOR: usize = 4;

/// The Cranelift IR instructions the compile workers may hold at once, queued and compiling,
/// under each memory policy (`MemoryPolicy`). Every worker holds its function's whole IR and
/// Cranelift's working set grows with it, so the parallel compile of a whole module peaks
/// with the functions in flight; a function heavier than the bound compiles alone. A bound
/// on the count of queued functions did not move the peak (one queued per worker instead
/// of two, memory-first `--jit`, tsf-wasm 43.5-44.5 MB either way).
/// Memory-first bounds them at 50,000, about a third of tsf-wasm's 157k (its largest function
/// 53.6k). JetStream `--jit`, memory-first, unpinned x86_64, 4 cores, median of 6,
/// 2026-10-27: tsf-wasm max RSS 46.6 -> 42.0 MB, Score 23.2 -> 22.1, First 11.2 -> 10.0;
/// sqlite3-wasm 73.6 -> 58.4 MB, Score 16.7 -> 16.2, First 2.96 -> 2.80. Throughput keeps
/// every worker fed (unbounded; the same rows, median of 8, were unchanged).
pub const WASM_COMPILE_INSTRUCTIONS_IN_FLIGHT: [Option<usize>; crate::MemoryPolicy::ALL.len()] =
    [None, Some(50_000)];

/// `compiled`'s body run directly on `arguments`, its parameters, when each is a Number
/// its guards admit: the call a known callee makes without a frame (`js_jit_compiled_call`).
/// The lane records a loop-free body runs below which a frameless activation is cheaper than
/// the JavaScript tier's native entry of it. A direct native entry (`tier_prologue_in_lane`,
/// `direct_call`, the shared entry) costs about 380 machine instructions past the
/// activation, and a lane record about 20 (callgrind, typescript-lib under `--jit`,
/// 2026-10-28: 9.7M native entries of 3- to 12-instruction predicates such as
/// `isLineBreak`), so a body shorter than this cannot recoup its entries.
pub const JS_TIER_LANE_ENTRY_RECORDS: usize = 20;

// Program JIT admission and clocks (program_store): artifact and mapped-byte budgets, Wasm module compiles
pub(crate) const JS_JIT_MAX_FUNCTIONS: usize = 64;

// Executable mappings were once page-rounded. A 64 KiB cap then admitted only
// sixteen tiny JS artifacts on 4 KiB systems, starving later high-frequency
// helpers; the function-count cap still bounds artifact proliferation.
pub(crate) const JS_JIT_MAX_MAPPED_BYTES: usize = 512 * 1024;

pub(crate) const WASM_JIT_MAX_TIER_ROOTS: usize = 32;

/// The work (entries and loop backedges, `TIERS.wasm.backedges_per_entry`) a selective Wasm
/// module observes before its whole-module compile is due: a module that runs this long gets
/// the code eager compilation would have made at its first entry.
pub(crate) const WASM_MODULE_WORK_BUDGET: u32 = 4096;

pub(crate) const WASM_JIT_MAX_TIER_MAPPED_BYTES: usize = 4 * 1024 * 1024;

/// The eager artifacts of Wasm modules together. A module's eager artifact is all of its code
/// at once, compiled before its first call: held to the shared ceiling, a module larger than
/// it compiled and was then discarded, leaving every function interpreted and promoting hot
/// roots one by one into a tier budget that filled (sqlite3-wasm, a 5.2 MiB artifact: Score
/// 0.61 against 12.29 kept, first iteration 17.3 s against 3.3 s; max RSS unchanged at 100
/// MiB, JetStream 3 `--jit`, Linux x86_64, 2026-10-24). This bounds a VM's eager Wasm code
/// to a few large modules.
pub(crate) const WASM_JIT_MAX_BASELINE_MAPPED_BYTES: usize = 32 * 1024 * 1024;

/// One VM-local admission ceiling shared by JS and Wasm artifacts.
pub(crate) const JIT_MAX_ARTIFACTS: usize = JS_JIT_MAX_FUNCTIONS + WASM_JIT_MAX_TIER_ROOTS;

pub(crate) const JIT_MAX_MAPPED_BYTES: usize = WASM_JIT_MAX_TIER_MAPPED_BYTES;

/// The share of a module's bytecodes, in per mille, its whole compile must hold code for to be
/// kept. Below it most calls cross between native code and the interpreter, and each crossing
/// (a general-path activation, a callout) costs more than the native function saves: the Wasm
/// GC modules of JetStream 3 (j2cl-box2d, Dart-flute) compile a few tiny leaves and run 10-40%
/// slower than with no JIT.
pub(crate) const WASM_MODULE_MIN_COVERAGE: usize = 500;

/// The fewest functions a module has for its whole compile's coverage to be weighed: a smaller
/// module compiles in microseconds and makes few crossings, whatever it holds.
pub(crate) const WASM_MODULE_WEIGHED_FUNCTIONS: usize = 64;

// Native graph, regexp and JS region compile budgets (native_graph, regexp_native, js_compiled)
// Bounds the working CFG, not the source function. Large unpartitioned native
// graphs exhausted 14 GiB on a 50 KiB input; callers may choose any nonzero budget.
pub(crate) const DEFAULT_REGION_INSTRUCTION_BUDGET: std::num::NonZeroUsize = std::num::NonZeroUsize::new(1024).unwrap();

/// The largest native search, in bytes of machine code.
pub const REGEXP_NATIVE_MAX_CODE_BYTES: usize = 256 * 1024;

// Keep native regions short so an interpreter interrupt cannot be delayed by a large body.
pub const JS_COMPILED_MAX_INSTRUCTIONS: usize = 256;

pub const JS_COMPILED_MAX_REGISTERS: usize = 256;

pub const JS_COMPILED_MAX_CODE_BYTES: usize = 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    /// A body pays for the one entry both tiers' bodies cross in the bytecodes whose saved
    /// interpretation covers it: a stencil body needs more than a Cranelift body, which saves more
    /// per bytecode behind the same entry.
    #[test]
    fn each_lane_body_tier_has_its_own_amortizing_length() {
        let tiers = TierRows::PROCESS.table();
        if tiers.admits_all { return; }
        let (base, ultra) = (tiers.amortizing_instructions(Tier::Base), tiers.amortizing_instructions(Tier::Ultra));
        let priors = &tiers.priors;
        let saves = |share: u64, length: usize| length as u64 * priors.interpreter_ns * (PER_MILLE - share) / PER_MILLE;
        assert!(saves(priors.base_body_share, base) >= priors.body_entry_ns && saves(priors.base_body_share, base - 1) < priors.body_entry_ns, "{base}");
        assert!(saves(priors.body_share, ultra) >= priors.body_entry_ns && saves(priors.body_share, ultra - 1) < priors.body_entry_ns, "{ultra}");
        assert!(ultra <= base, "{base} and {ultra}");
        assert_eq!(ultra, tiers.amortizing_instructions(Tier::Quick));
    }

    #[test]
    fn the_tier_table_is_consistent() {
        let lane = &TierRows::PROCESS.table().lane_body;
        assert!(lane.site_executions > 0);
        // The reoptimization threshold doubles per recompile and must not overflow its counter.
        assert!(u32::from(lane.reoptimize_exits) << lane.max_recompiles <= u32::from(u16::MAX));
        let looks = TierRows::PROCESS.table().js_function.promotion_looks;
        assert!(looks.windows(2).all(|pair| pair[0].0 < pair[1].0), "looks grow");
        assert!(looks.iter().all(|(observations, minimum)| minimum <= observations));
        for policy in crate::MemoryPolicy::ALL {
            let row = by_policy(policy);
            // The loop tier's site analysis belongs to the function tiers.
            assert!(!row.loop_tier || row.js_function_tiers, "{policy:?}");
        }
    }
}


