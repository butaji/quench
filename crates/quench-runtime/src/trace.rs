//! The process's two diagnostic settings, each read once.
//!
//! `QUENCH_TRACE` names reports (`Channel`); `QUENCH_DEV` names temporary development
//! experiments (`Dev`). Each is a comma list of names, and a name that takes an argument,
//! such as a path, is written `name=value`: `QUENCH_TRACE=gc,js-jit,jit-dump=/tmp/jit`.
//! Reports never change what a program computes; experiments are off in every measured
//! configuration, and each one is deleted once it is either adopted or rejected.

use std::sync::OnceLock;

/// The names a setting was given, as bits indexed by its enum, with their arguments.
struct Named {
    on: u64,
    values: Vec<Option<String>>,
}

fn parse(variable: &str, names: &[&str]) -> Named {
    let mut named = Named {
        on: 0,
        values: vec![None; names.len()],
    };
    let Some(spec) = std::env::var_os(variable) else {
        return named;
    };
    for entry in spec.to_string_lossy().split(',').filter(|entry| !entry.is_empty()) {
        let (name, value) = entry
            .split_once('=')
            .map_or((entry, None), |(name, value)| (name, Some(value)));
        match names.iter().position(|known| *known == name) {
            Some(index) => {
                named.on |= 1 << index;
                named.values[index] = value.map(str::to_owned);
            }
            None => eprintln!("Quench: unknown {variable} name `{name}`; known: {}", names.join(",")),
        }
    }
    named
}

macro_rules! setting {
    ($(#[$doc:meta])* $variable:literal, $setting:ident { $($(#[$variant_doc:meta])* $variant:ident = $name:literal,)* }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $setting {
            $($(#[$variant_doc])* $variant,)*
        }

        impl $setting {
            const NAMES: &[&str] = &[$($name,)*];

            fn named() -> &'static Named {
                const _: () = assert!($setting::NAMES.len() <= u64::BITS as usize);
                static NAMED: OnceLock<Named> = OnceLock::new();
                NAMED.get_or_init(|| parse($variable, Self::NAMES))
            }

            /// Whether the setting names this.
            pub fn on(self) -> bool {
                Self::named().on & (1 << self as u64) != 0
            }

            /// The argument written after `=` for this name, if any.
            pub fn value(self) -> Option<&'static str> {
                Self::named().values[self as usize].as_deref()
            }
        }
    };
}

setting! {
    /// One diagnostic report, named in `QUENCH_TRACE`.
    "QUENCH_TRACE", Channel {
        Memory = "memory",
        MemoryPeak = "memory-peak",
        Allocator = "allocator",
        Gc = "gc",
        GcTime = "gc-time",
        /// At each full collection, the live values per container and how many have a narrow
        /// form: what 4-byte storage could save (`Heap::trace_storage_census`).
        StorageCensus = "storage-census",
        Shapes = "shapes",
        Rewrite = "rewrite",
        /// Every loaded program's finished bytecode, printed at exit; `bytecode=lanes` prints
        /// each function's lane view instead (`Vm::report_lane_plan`), the static input of the
        /// instruction-count predictor (`quench-gate --only ir-predict`).
        Bytecode = "bytecode",
        JsJit = "js-jit",
        /// BaseJIT region exits by instruction, most frequent first (printed at exit).
        BjitLeave = "bjit-leave",
        WasmJit = "wasm-jit",
        OsrReject = "osr-reject",
        JitPeak = "jit-peak",
        JitDump = "jit-dump",
        /// Per-loop residual counts of each compiled hot loop (`quench-bench --residuals`).
        Residuals = "residuals",
        PerfMap = "perf-map",
        NameAtoms = "name-atoms",
        FieldPrimitives = "field-primitives",
        FunctionUse = "function-use",
        AllPrograms = "all-programs",
        Census = "census",
        ProfileTrace = "profile-trace",
        LazyFacts = "lazy-facts",
        LazyBodies = "lazy-bodies",
        Replay = "replay",
        RssPhases = "rss-phases",
        CodeResidency = "code-residency",
        /// Compiled code's share of CPU time, sampled (`jit_coverage`).
        JitCoverage = "jit-coverage",
        /// Whether a startup snapshot was restored, and why not.
        StartupSnapshot = "startup-snapshot",
        /// No-JIT loop kernels: each loop's kernel or the reason it has none.
        LoopKernel = "loop-kernel",
        /// Every run of a lane body that ends anywhere but its completion: function and exit.
        BodyLeave = "body-leave",
        /// Each AOT image compile: how many functions got a lane body, and why each other did not.
        AotLanes = "aot-lanes",
    }
}

setting! {
    /// One temporary development experiment, named in `QUENCH_DEV`; none is on by default.
    "QUENCH_DEV", Dev {
        /// The memory-first heap policy in place of the default.
        MemoryFirst = "memory-first",
        /// Array backings never take the double element kind (`heap::array_elements::DOUBLE_LEN`),
        /// which they otherwise take on their first Number that is no int32.
        NoDoubleElements = "no-double-elements",
        /// A requested compile runs on the requesting thread and installs at once, as in tests:
        /// instruction counts then see the code a run installs.
        SyncCompile = "sync-compile",
        /// Every tier controller reads the floor rows (`tier_policy::TierRows::Floor`), as the
        /// tier oracle's tests do, so a failure they show reproduces in a process.
        FloorRows = "floor-rows",
        /// Every JavaScript JIT compile at `CompileLevel::Quick` (no mid-end optimization,
        /// one-pass register allocation), to measure the level's cost and code quality.
        QuickCompile = "quick-compile",
        /// Number loop regions build from BaseJIT stencils (`quench-bjit`) where every
        /// instruction has a row, instead of Cranelift IR.
        Bjit = "bjit",
        /// Selective Wasm hot-root tiering (`wasm-selective`).
        WasmSelective = "wasm-selective",
        /// A bounded root compilation of the residual Wasm function `wasm-root=N`.
        WasmRoot = "wasm-root",
        /// The Cranelift opt level of that root (`wasm-root-opt-level=none|speed|speed_and_size`).
        WasmRootOptLevel = "wasm-root-opt-level",
        /// Only the selected root compiles; hot callees stay interpreted.
        WasmRootOnly = "wasm-root-only",
        /// Tiering starts at the selected root and promotes hot fallback callees.
        WasmRootFirst = "wasm-root-first",
        /// Functions of a direct eval's source compile on first call.
        DirectEvalStubs = "direct-eval-stubs",
        /// Old records keep reference counts, and minor collections free the records whose
        /// count drops to zero (`heap::rc`).
        RcGc = "rc-gc",
        /// Under `rc-gc`, checks before each minor sweep that nothing live names a record it
        /// frees, and reports what does (`Heap::rc_verify_frees`).
        RcVerify = "rc-verify",
        /// JavaScript-visible clocks and `Math.random` are deterministic (`crate::clock`):
        /// instruction counts stop depending on host speed. Printed scores are then meaningless.
        VirtualClock = "virtual-clock",
        /// Engine policies (GC old-generation sizing) read work, not wall time (`crate::clock::is_work`).
        WorkClock = "work-clock",
        /// Each lazily parsed body is checked both by the parse's verdict and by the semantic
        /// analysis, and differences are printed (`lazy_bodies::verify_early`); `=stat` also
        /// prints each body's verdict.
        EarlyVerify = "early-verify",
        /// Every loop-free function that never suspends runs its unlowered record for good
        /// (`bytecode::tier0`), the differential that checks tier 0's semantics.
        Tier0Always = "tier0-always",
        /// No function runs unlowered: each lowers at its first read.
        Tier0Off = "tier0-off",
        /// Lazily parsed bodies are recognized without their trees (`ParseOptions::lazy_recognize`):
        /// `=off` builds them, `=verify` builds them as well and prints where the verdicts differ.
        Recognize = "recognize",
        /// `process.__irMark()` exists and calls `quench_ir_mark`, where callgrind splits its
        /// counts (`--dump-before=quench_ir_mark`): the per-primitive instruction budget
        /// (`quench-gate`'s `ir-budget` step) measures each primitive between two marks.
        IrMark = "ir-mark",
        /// Emitted direct calls take the helper row instead of pushing native headers
        /// (one-frame-one-call.md F5), which is the default: to compare against the old path.
        NoNativeFrames = "no-native-frames",
    }
}
