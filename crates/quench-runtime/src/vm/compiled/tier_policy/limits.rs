//! The shapes of the sizes, budgets and lane-view thresholds; `tier_policy` owns their values.

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
    /// The most parameters of a callee a loop region inlines.
    pub(in crate::vm) params: usize,
    /// A replanned body inlines with this fraction (1 / divisor) of the budget whose compile
    /// failed.
    pub(in crate::vm) replan_divisor: usize,
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

/// What a VM keeps of its compiled code and of the tier records that lead to it
/// (`ProgramStore`'s `JitCodeCacheBudget`).
pub(in crate::vm) struct Budgets {
    /// The JavaScript function artifacts a VM keeps at once; the count bounds artifact
    /// proliferation.
    pub(in crate::vm) js_functions: usize,
    /// The functions whose tier records a VM keeps. Tier observations include dynamically
    /// generated functions across all retained programs. Web-SSR exposes more than 1,300
    /// functions in one VM; this fixed metadata bound is large enough to observe that workload
    /// without changing the independent compiled-artifact or executable-memory budgets.
    pub(in crate::vm) js_tracked_functions: usize,
    /// The functions whose unsupported bytecode shape a VM remembers.
    pub(in crate::vm) js_rejected_functions: usize,
    /// The executable bytes of the JavaScript function artifacts. Executable mappings were once
    /// page-rounded: a 64 KiB cap then admitted only sixteen tiny JS artifacts on 4 KiB systems,
    /// starving later high-frequency helpers; the function count still bounds proliferation.
    pub(in crate::vm) js_mapped_bytes: usize,
    /// The Wasm hot-root tier artifacts a VM keeps at once.
    pub(in crate::vm) wasm_tier_roots: usize,
    /// The executable bytes of the JavaScript and Wasm tier artifacts together, the one
    /// admission ceiling they share.
    pub(in crate::vm) wasm_tier_mapped_bytes: usize,
    /// The eager artifacts of Wasm modules together. A module's eager artifact is all of its
    /// code at once, compiled before its first call: held to the shared ceiling, a module larger
    /// than it compiled and was then discarded, leaving every function interpreted and promoting
    /// hot roots one by one into a tier budget that filled (sqlite3-wasm, a 5.2 MiB artifact:
    /// Score 0.61 against 12.29 kept, first iteration 17.3 s against 3.3 s; max RSS unchanged at
    /// 100 MiB, JetStream 3 `--jit`, Linux x86_64, 2026-10-24). This bounds a VM's eager Wasm
    /// code to a few large modules.
    pub(in crate::vm) wasm_baseline_mapped_bytes: usize,
    /// The general-dispatch artifacts a VM keeps for reuse by equivalent dynamic functions,
    /// evicted least recently used; the key holds no program, function, heap or atom owners.
    pub(in crate::vm) general_artifacts: usize,
    /// The bytes of those artifacts' keys together, and the longest key one admits.
    pub(in crate::vm) general_artifact_key_bytes: usize,
}
