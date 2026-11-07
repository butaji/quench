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
    on: u32,
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
                const _: () = assert!($setting::NAMES.len() <= u32::BITS as usize);
                static NAMED: OnceLock<Named> = OnceLock::new();
                NAMED.get_or_init(|| parse($variable, Self::NAMES))
            }

            /// Whether the setting names this.
            pub fn on(self) -> bool {
                Self::named().on & (1 << self as u32) != 0
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
        Shapes = "shapes",
        Rewrite = "rewrite",
        JsJit = "js-jit",
        WasmJit = "wasm-jit",
        OsrReject = "osr-reject",
        JitPeak = "jit-peak",
        JitDump = "jit-dump",
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
    }
}

setting! {
    /// One temporary development experiment, named in `QUENCH_DEV`; none is on by default.
    "QUENCH_DEV", Dev {
        /// The memory-first heap policy in place of the default.
        MemoryFirst = "memory-first",
        /// JavaScript functions promote to the general native dispatch tier.
        JsGeneralDispatch = "js-general-dispatch",
        /// That tier promotes once hot instead of once its entries pay for the compile.
        JsUnpricedGeneralDispatch = "js-unpriced-general-dispatch",
        /// The lane prologue specializes declined functions into frameless lane bodies.
        JsLaneMethods = "js-lane-methods",
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
    }
}
