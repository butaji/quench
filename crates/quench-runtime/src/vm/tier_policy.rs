//! When code moves between tiers: the predicted-tier and admission constants the tier controllers read, in one table.
//!
//! The controllers (`LoopTier` and `LoopSite`, `LaneBody`, `JsTierState`, `WasmTierState`)
//! own their states and transitions; this module owns the numbers those transitions compare
//! against, and what each memory policy lets compile. A tuning experiment changes a row here,
//! never a controller.

use crate::bytecode::{FieldBase, FieldLookup, Function, ResidualProgram};
use crate::compile::call_graph::CallGraph;
pub use crate::jit_mode::Tier;
use crate::staged::meaning::{means, Meaning};
use crate::staged::Operation;
#[allow(unused_imports)] use crate::staged::glue_prelude::*;

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


/// The stitch's room (`Stitch`) for every b-jit stitch.
pub const STITCH: Stitch = Stitch {
    max_total_instructions: 96,
    window_registers: 32,
    max_depth: 3,
};

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

/// The tier a lane body of a function with predicted tier `pick` runs in, or `None` where the interpreter runs it
/// (design decision 4.9). The pick is the one input; the mode only names a compiled tier the mode allows, and a mode
/// that leaves Base out (aarch64's default) runs Ultra for a Base pick.
pub fn lane_body_tier(pick: PredictedTier) -> Option<Tier> {
    let wanted = match pick {
        PredictedTier::Interpreter => return None,
        PredictedTier::Base => Tier::Base,
        PredictedTier::Ultra => Tier::Ultra,
    };
    if crate::jit_mode::tiers_allowed(&[wanted]) {
        return Some(wanted);
    }
    [Tier::Ultra, Tier::Base].into_iter().find(|tier| crate::jit_mode::tiers_allowed(&[*tier]))
}

/// The base a share in percent is a share of.
pub const PERCENT: u64 = 100;


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

/// Whether any of `tiers` runs in a VM in execution `mode` under `policy`: compiled execution, a
/// memory policy that compiles them (`tier_may_run`), and a JIT mode that allows one. A build whose
/// profiles count interpreted work exactly runs none. The one question every VM-level tier check asks.
#[inline]
pub fn tiers_run(mode: crate::ExecutionMode, policy: crate::MemoryPolicy, tiers: &[Tier]) -> bool {
    !cfg!(quench_profiled) && mode == crate::ExecutionMode::Compiled && tier_may_run(policy, tiers)
}

/// Operators up to which a Wasm module lowered for compiled code inlines its leaves as the
/// interpreter does. A compiled call is cheap, but its activation still costs a frame, a
/// stack check, an interrupt check and a status test, and a copy lets Cranelift optimize the
/// leaf with its caller's values. A copy compiles with its caller's root, so it is compiled
/// whether or not it runs; past this size the copies' compile time outweighs them on a module's
/// first runs, and larger modules keep every call.
pub const WASM_COMPILED_INLINING_MAX_OPERATORS: usize = 32_768;

/// Whether a Wasm module lowered for `mode` inlines its leaf calls (`wasm::inline::leaf_budget`).
/// The interpreter inlines them; compiled code inlines them up to `WASM_COMPILED_INLINING_MAX_OPERATORS`
/// operators; native-only execution keeps every call for the native backend to resolve.
pub fn wasm_module_inlines_leaves(mode: crate::ExecutionMode, module_operators: impl FnOnce() -> usize) -> bool {
    match mode {
        crate::ExecutionMode::Interpreted => true,
        #[cfg(test)]
        crate::ExecutionMode::Compiled if crate::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(std::cell::Cell::get) => false,
        crate::ExecutionMode::Compiled => module_operators() <= WASM_COMPILED_INLINING_MAX_OPERATORS,
        #[cfg(feature = "native-execution")]
        crate::ExecutionMode::NativeOnly => false,
    }
}

/// Whether a compile may start at all: some compiling tier is allowed.
pub fn may_compile() -> bool {
    crate::jit_mode::tiers_allowed(Tier::ANY)
}

/// Whether `tier` may publish a body: a tier the mode allows.
pub fn may_publish(tier: Tier) -> bool {
    crate::jit_mode::tiers_allowed(&[tier])
}

/// Whether BaseJIT (stencil) bodies and regions run in this process.
pub fn base_runs() -> bool {
    crate::jit_mode::tiers_allowed(&[Tier::Base])
}

/// Whether a Cranelift tier (UltraJIT) runs: the tier that compiles what BaseJIT does not build.
pub fn cranelift_runs() -> bool {
    crate::jit_mode::tiers_allowed(Tier::CRANELIFT)
}

/// What `policy` lets the tiers compile.
pub const fn by_policy(policy: crate::MemoryPolicy) -> &'static PolicyTiers {
    &BY_POLICY[policy as usize]
}

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
/// IR instructions an interpreted call of a function spends on its frame, per call. Not re-derived: the interpreter's
/// push is the `push_in_loop_call` chain (`vm/dispatch.rs`, `push_general_user_frame`), too long to count by reading,
/// so this keeps its earlier value until the first build measures it.
pub const ENTRY_IR_INTERPRETER: u64 = 40;
/// The b-jit entry's fixed IR per call of a body from a b-jit direct row (one IR per machine instruction, read off the
/// code; the row's stage counts are read, not measured): the row's `entry_rows` hop (`stencils/entry.rs`: sub, call,
/// add, ret = 4), and the direct row's room check (~4), header, tops, depth and register stores (~8), and settle: pop,
/// result store and ended check (~8). 4 + 20 = 24.
pub const ENTRY_IR_BJIT_BASE: u64 = 24;
/// The b-jit entry's IR per register of the callee: the window's registers past the arguments are written `undefined`
/// at the push (`DirectFrame`'s fill), one store each. The callee's `registers` bounds them.
pub const ENTRY_IR_BJIT_PER_REGISTER: u64 = 1;
/// The u-jit entry's fixed IR per call: every native entry to a u-jit body passes the body shim (`lane_body_abi::body_shim`:
/// its prologue, `call_indirect` and return, 6 instructions), then the body's prologue (stack check 3, frame 2), its
/// epilogue (3), and the caller's status test (2). 6 + 3 + 2 + 3 + 2 = 16.
pub const ENTRY_IR_UJIT_BASE: u64 = 16;
/// The u-jit entry's IR per register of the callee: one save and one restore per register, the upper bound on the
/// callee-saved registers its body keeps live across a call. Conservative, not measured.
pub const ENTRY_IR_UJIT_PER_REGISTER: u64 = 2;
/// The IR of a call that enters a b-jit body from a u-jit caller through the body ABI (`entry_body`, 20 instructions),
/// over the `entry_rows` hop a b-jit direct call pays (4): 20 - 4 = 16.
pub const CROSSING_IR_INTO_BJIT: u64 = 16;

// The tier order is locked: each compiled tier runs a bytecode for less IR than the one before it, and compiles for more.
const _: () = assert!(INTERPRETER_IR_PER_OP > BJIT_IR_PER_OP && BJIT_IR_PER_OP > UJIT_IR_PER_OP);
const _: () = assert!(BJIT_COMPILE_IR_PER_OP < UJIT_COMPILE_IR_PER_OP);

// The solver that picks every function's tier from the call graph (`predicted_tiers`, `predicted_cost`).
mod cost;
pub use cost::{predicted_cost, predicted_tiers};


/// The predicted tier of a lane body (design decision 4.9): the u-jit in an AOT program (`aot`), whose
/// image compiles every function, otherwise the fixed-point pick `predicted` (`predicted_tiers`).
pub fn body_tier(aot: bool, predicted: PredictedTier) -> PredictedTier {
    if aot { PredictedTier::Ultra } else { predicted }
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

// Program JIT admission and clocks (program_store): artifact and mapped-byte budgets, Wasm module compiles
pub(crate) const JS_JIT_MAX_FUNCTIONS: usize = 64;

// Executable mappings were once page-rounded. A 64 KiB cap then admitted only
// sixteen tiny JS artifacts on 4 KiB systems, starving later high-frequency
// helpers; the function-count cap still bounds artifact proliferation.
pub(crate) const JS_JIT_MAX_MAPPED_BYTES: usize = 512 * 1024;

pub(crate) const WASM_JIT_MAX_TIER_ROOTS: usize = 32;

pub(crate) const WASM_JIT_MAX_TIER_MAPPED_BYTES: usize = 4 * 1024 * 1024;

/// One VM-local admission ceiling shared by JS and Wasm artifacts.
pub(crate) const JIT_MAX_ARTIFACTS: usize = JS_JIT_MAX_FUNCTIONS + WASM_JIT_MAX_TIER_ROOTS;

pub(crate) const JIT_MAX_MAPPED_BYTES: usize = WASM_JIT_MAX_TIER_MAPPED_BYTES;

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


    #[test]
    fn the_tier_table_is_consistent() {
        for policy in crate::MemoryPolicy::ALL {
            let row = by_policy(policy);
            // The loop tier's site analysis belongs to the function tiers.
            assert!(!row.loop_tier || row.js_function_tiers, "{policy:?}");
        }
    }
}
