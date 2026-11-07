//! Diagnostic report channels, read once from `QUENCH_TRACE`.
//!
//! `QUENCH_TRACE` is a comma list of channel names. A channel that takes an argument, such
//! as a path, is written `name=value`: `QUENCH_TRACE=gc,js-jit,jit-dump=/tmp/jit`.
//! Channels only report; none changes what a program computes.

use std::sync::OnceLock;

macro_rules! channels {
    ($($variant:ident = $name:literal,)*) => {
        /// One diagnostic report, named in `QUENCH_TRACE` by its kebab-case name.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Channel {
            $($variant,)*
        }

        const NAMES: &[&str] = &[$($name,)*];
    };
}

channels! {
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
}

const _: () = assert!(NAMES.len() <= u32::BITS as usize);

struct Trace {
    on: u32,
    values: Vec<Option<String>>,
}

fn trace() -> &'static Trace {
    static TRACE: OnceLock<Trace> = OnceLock::new();
    TRACE.get_or_init(|| {
        let mut trace = Trace {
            on: 0,
            values: vec![None; NAMES.len()],
        };
        let Some(spec) = std::env::var_os("QUENCH_TRACE") else {
            return trace;
        };
        for entry in spec.to_string_lossy().split(',').filter(|entry| !entry.is_empty()) {
            let (name, value) = entry
                .split_once('=')
                .map_or((entry, None), |(name, value)| (name, Some(value)));
            match NAMES.iter().position(|known| *known == name) {
                Some(index) => {
                    trace.on |= 1 << index;
                    trace.values[index] = value.map(str::to_owned);
                }
                None => eprintln!(
                    "Quench: unknown QUENCH_TRACE channel `{name}`; known: {}",
                    NAMES.join(",")
                ),
            }
        }
        trace
    })
}

impl Channel {
    /// Whether `QUENCH_TRACE` names this channel.
    pub fn on(self) -> bool {
        trace().on & (1 << self as u32) != 0
    }

    /// The argument written after `=` for this channel, if any.
    pub fn value(self) -> Option<&'static str> {
        trace().values[self as usize].as_deref()
    }
}
