//! The process's diagnostic reports, named in `QUENCH_TRACE` (`Channel`): a comma list of
//! names, and a name that takes an argument, such as a path, is written `name=value`:
//! `QUENCH_TRACE=gc,js-jit,jit-dump=/tmp/jit`. Reports never change what a program computes.

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
                named.on |= 1u64 << index;
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
                Self::named().on & (1u64 << self as u32) != 0
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
        Rewrite = "rewrite",
        /// Every loaded program's finished bytecode, printed at exit; `bytecode=lanes` prints
        /// each function's lane view instead (`Vm::report_lane_plan`): which handler each record runs.
        Bytecode = "bytecode",
        JsJit = "js-jit",
        WasmJit = "wasm-jit",
        OsrReject = "osr-reject",
        JitPeak = "jit-peak",
        JitDump = "jit-dump",
        /// Keep a failed `compile`'s build directory (objects, linker command) and name it, so a
        /// relocation or link defect can be inspected after the failure; and keep every compiled
        /// image's unstripped executable beside it (`<output>.unstripped`), for a debugger.
        BuildDir = "build-dir",
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
        /// Each AOT image compile: how many functions got a lane body, and why each other did not.
        AotLanes = "aot-lanes",
    }
}
