use crate::Value;
use super::wtf16::JsString;
use crate::bytecode::ResidualProgram;
use std::rc::{Rc, Weak};
use crate::heap::WeakHandle;
use rustc_hash::FxHashMap;
use std::io::Write as _;

mod snapshot;

#[derive(Default)]
struct JsJitCounters {
    source_name: Option<Box<str>>,
    function_name: Option<Box<str>>,
    parameter_count: u16,
    instruction_count: u32,
    guard_conditional_branches: u32,
    guard_typeof_ops: u32,
    guard_strict_comparisons: u32,
    guard_relational_comparisons: u32,
    entries: u64,
    lane_entries: u64,
    numeric_entries: u64,
    promotions: u64,
    compile_attempts: u64,
    compile_successes: u64,
    rejection_reason: Option<&'static str>,
    compile_ns: u64,
    native_entries: u64,
    field_cache_probes: u64,
    field_cache_hits: u64,
    field_store_cache_probes: u64,
    field_store_cache_hits: u64,
    field_return_candidate_entries: u64,
    field_return_fixed_this_entries: u64,
    field_return_fast_entries: u64,
    field_increment_candidate_entries: u64,
    field_increment_fixed_this_entries: u64,
    field_increment_fast_entries: u64,
    numeric_find_fast_entries: u64,
    fallback_entries: u64,
    interpreter_completions: u64,
    interpreter_inclusive_ns: u64,
    opcode_counts: Vec<(u16, u32)>,
    general_dispatch_opcode_counts: Vec<(u16, u64)>,
    code_bytes: usize,
}

/// Prespecified looks for the Number-entry profile. Critical counts are the
/// exact one-sided Binomial( n, 0.80 ) cutoffs at alpha = 0.05 / 4 per look.
/// A union bound caps false promotion at 5% across looks for one function
/// under a stationary Bernoulli model; it is not a VM-wide multiple-test bound.
pub(super) const JS_JIT_PROMOTION_LOOKS: [(u8, u8); 4] =
    [(32, 31), (64, 59), (128, 113), (255, 219)];
/// Entries after which a function is hot enough to price its general tagged-dispatch
/// baseline (`GeneralPromotionPending`); unlike the optimizing tier, the baseline needs
/// no numeric arguments or bytecode-shape candidate.
const JS_JIT_GENERAL_PROMOTION_ENTRIES: u8 = 32;
/// A specialized artifact that repeatedly rejects entry values is useful only
/// for its matching profile. Eight confirmed misses cap this experiment before
/// paying for the complete-semantics baseline.
pub(super) const JS_JIT_SPECIALIZED_GUARD_MISS_THRESHOLD: u8 = 8;
/// The general baseline's compile cost in interpreted-instruction units: a fixed part
/// and a part per bytecode instruction. Measured on Linux x86_64 with `QUENCH_JS_JIT_STATS`
/// against roughly 50 ns per interpreted instruction: requiring jackspeak, about 0.8 ms
/// plus 32 µs per instruction; npm's semver scenario, 1.4 ms for a four-instruction body
/// and 40 to 76 µs per instruction for 160 to 200.
const JS_JIT_GENERAL_COMPILE_FIXED_COST: u64 = 16_000;
const JS_JIT_GENERAL_COMPILE_COST_PER_INSTRUCTION: u64 = 1_200;
/// The share of an entry's interpreted instructions the baseline saves, as a fraction. It
/// keeps the interpreter's tagged semantics and calls back into the VM for most operations
/// (`js_step_callout` was 11% of npm browserslist with it on), so a quarter until the
/// baseline's savings are measured.
const JS_JIT_GENERAL_SAVED_SHARE: (u64, u64) = (1, 4);
/// What each native entry of the baseline costs beyond a frameless lane activation, in
/// interpreted-instruction units: a due function's calls take a VM frame and the general
/// loop (`Vm::js_tier_needs_general_entry`). Measured on npm's browserslist scenario: a
/// six-instruction body's 72,427 native entries cost about 650 machine instructions each
/// more than its lane calls, some 32 lane-interpreted instructions.
const JS_JIT_GENERAL_ENTRY_OVERHEAD: u64 = 32;

/// Whether `entries` of a function of `instructions` bytecode instructions have paid for
/// its general baseline, taking an entry to run its instructions once (a loop hot within
/// one entry is OSR's to compile) and the savings of the entries so far to stand for
/// those still to come. A body whose entry saves less than the baseline's entry overhead
/// never pays.
fn general_promotion_due(entries: u32, instructions: usize) -> bool {
    let Some(saved_per_entry) = general_saved_per_entry(instructions) else {
        return false;
    };
    let instructions = u64::try_from(instructions).unwrap_or(u64::MAX);
    let (_, of) = JS_JIT_GENERAL_SAVED_SHARE;
    u64::from(entries).saturating_mul(saved_per_entry)
        >= JS_JIT_GENERAL_COMPILE_FIXED_COST
            .saturating_add(JS_JIT_GENERAL_COMPILE_COST_PER_INSTRUCTION.saturating_mul(instructions))
            .saturating_mul(of)
}

/// Whether observed bytecode work has paid for one complete-semantics native
/// baseline. An unpriced experiment still waits for 32 whole-function work
/// units, matching the established general-tier entry floor.
fn general_dispatch_work_due(work: u64, instructions: u64, priced: bool) -> bool {
    let instructions = instructions.max(1);
    if !priced {
        return work >= instructions.saturating_mul(u64::from(JS_JIT_GENERAL_PROMOTION_ENTRIES));
    }
    let (saved, of) = JS_JIT_GENERAL_SAVED_SHARE;
    work.saturating_mul(saved)
        >= JS_JIT_GENERAL_COMPILE_FIXED_COST
            .saturating_add(JS_JIT_GENERAL_COMPILE_COST_PER_INSTRUCTION.saturating_mul(instructions))
            .saturating_mul(of)
}

fn accumulate_general_dispatch_work(accumulated_work: &mut u64, work: usize) {
    *accumulated_work = accumulated_work
        .saturating_add(u64::try_from(work).unwrap_or(u64::MAX));
}

/// Promote an eligible state once accumulated work reaches the selected
/// pricing floor. Other tier states keep their established ownership.
fn promote_general_dispatch_if_paid(
    state: &mut JsTierState,
    accumulated_work: u64,
    instructions: usize,
    priced: bool,
) -> bool {
    if !matches!(
        state,
        JsTierState::Hot { .. }
            | JsTierState::Rejected { .. }
            | JsTierState::GeneralPromotionPending { .. }
            | JsTierState::Unsupported
    ) || !general_dispatch_work_due(
        accumulated_work,
        u64::try_from(instructions.max(1)).unwrap_or(u64::MAX),
        priced,
    ) {
        return false;
    }
    *state = JsTierState::GeneralCompileDue;
    true
}

/// What an entry of a body of `instructions` saves under the general baseline, in units of
/// `1 / JS_JIT_GENERAL_SAVED_SHARE.1` interpreted instruction; `None` where it saves
/// nothing past the baseline's entry overhead.
fn general_saved_per_entry(instructions: usize) -> Option<u64> {
    let (saved, of) = JS_JIT_GENERAL_SAVED_SHARE;
    u64::try_from(instructions)
        .unwrap_or(u64::MAX)
        .saturating_mul(saved)
        .checked_sub(JS_JIT_GENERAL_ENTRY_OVERHEAD.saturating_mul(of))
        .filter(|saved| *saved > 0)
}

const JS_JIT_MAX_FUNCTIONS: usize = 64;
// Tier observations include dynamically generated functions across all retained
// programs. Web-SSR exposes more than 1,300 functions in one VM; keep a fixed
// metadata bound large enough to observe that workload without changing the
// independent compiled-artifact or executable-memory budgets.
const JS_JIT_MAX_TRACKED_FUNCTIONS: usize = 2048;
const JS_JIT_MAX_DIAGNOSTIC_FUNCTIONS: usize = 4096;
const JS_JIT_MAX_REJECTED_FUNCTIONS: usize = 4096;
const JS_JIT_REJECTION_DIAGNOSTIC_ENTRIES: u64 = 32;
// Cranelift executable mappings are page-rounded. A 64 KiB cap admitted only
// sixteen tiny JS artifacts on 4 KiB systems, starving later high-frequency
// helpers; the function-count cap still bounds artifact proliferation.
const JS_JIT_MAX_MAPPED_BYTES: usize = 512 * 1024;
/// Promote a Wasm function only after repeated interpreter entries. Wasm
/// signatures and bytecode are fixed, so this is a call-hotness gate.
pub(super) const WASM_JIT_PROMOTION_ENTRIES: u8 = 32;
const WASM_JIT_MAX_TIER_ROOTS: usize = 32;
const WASM_JIT_MAX_TIER_MAPPED_BYTES: usize = 4 * 1024 * 1024;
/// The eager artifacts of Wasm modules together. A module's eager artifact is all of its code
/// at once, compiled before its first call: held to the shared ceiling, a module larger than
/// it compiled and was then discarded, leaving every function interpreted and promoting hot
/// roots one by one into a tier budget that filled (sqlite3-wasm, a 5.2 MiB artifact: Score
/// 0.61 against 12.29 kept, first iteration 17.3 s against 3.3 s; max RSS unchanged at 100
/// MiB, JetStream 3 `--jit`, Linux x86_64, 2026-10-24). This bounds a VM's eager Wasm code
/// to a few large modules.
const WASM_JIT_MAX_BASELINE_MAPPED_BYTES: usize = 32 * 1024 * 1024;
/// The least an artifact maps: executable mappings are page-rounded.
const JIT_MIN_ARTIFACT_BYTES: usize = 4096;
/// One VM-local admission ceiling shared by JS and Wasm artifacts.
const JIT_MAX_ARTIFACTS: usize = JS_JIT_MAX_FUNCTIONS + WASM_JIT_MAX_TIER_ROOTS;
const JIT_MAX_MAPPED_BYTES: usize = WASM_JIT_MAX_TIER_MAPPED_BYTES;

#[derive(Clone, Copy)]
enum JitArtifactKind {
    BaselineWasm,
    JsFunction,
    WasmTierRoot,
}

/// One VM-local executable-code budget for the JavaScript, Wasm tier and eager
/// compiled-program artifacts. Tier-kind limits and the JS and Wasm tiers' aggregate
/// capacity are admitted and accounted together here; eager module artifacts have their own
/// ceiling (`WASM_JIT_MAX_BASELINE_MAPPED_BYTES`).
#[derive(Default)]
struct JitCodeCacheBudget {
    /// The JavaScript and Wasm tier artifacts, which share `JIT_MAX_ARTIFACTS` and
    /// `JIT_MAX_MAPPED_BYTES`.
    artifacts: usize,
    mapped_bytes: usize,
    /// Eager Wasm module artifacts, bounded apart (`WASM_JIT_MAX_BASELINE_MAPPED_BYTES`).
    baseline_artifacts: usize,
    baseline_mapped_bytes: usize,
    js_functions: usize,
    js_mapped_bytes: usize,
    wasm_tier_roots: usize,
    wasm_tier_mapped_bytes: usize,
}

impl JitCodeCacheBudget {
    fn can_retain(&self, kind: JitArtifactKind, mapped_bytes: usize) -> bool {
        let (count, bytes, max_count, max_bytes) = match kind {
            JitArtifactKind::BaselineWasm => {
                return self.baseline_mapped_bytes.saturating_add(mapped_bytes)
                    <= WASM_JIT_MAX_BASELINE_MAPPED_BYTES;
            }
            JitArtifactKind::JsFunction => (
                self.js_functions,
                self.js_mapped_bytes,
                JS_JIT_MAX_FUNCTIONS,
                JS_JIT_MAX_MAPPED_BYTES,
            ),
            JitArtifactKind::WasmTierRoot => (
                self.wasm_tier_roots,
                self.wasm_tier_mapped_bytes,
                WASM_JIT_MAX_TIER_ROOTS,
                WASM_JIT_MAX_TIER_MAPPED_BYTES,
            ),
        };
        count < max_count
            && bytes.saturating_add(mapped_bytes) <= max_bytes
            && self.artifacts < JIT_MAX_ARTIFACTS
            && self.mapped_bytes.saturating_add(mapped_bytes) <= JIT_MAX_MAPPED_BYTES
    }

    fn retain(&mut self, kind: JitArtifactKind, mapped_bytes: usize) -> bool {
        if !self.can_retain(kind, mapped_bytes) {
            return false;
        }
        if matches!(kind, JitArtifactKind::BaselineWasm) {
            self.baseline_artifacts += 1;
            self.baseline_mapped_bytes += mapped_bytes;
            return true;
        }
        self.artifacts += 1;
        self.mapped_bytes += mapped_bytes;
        match kind {
            JitArtifactKind::BaselineWasm => {}
            JitArtifactKind::JsFunction => {
                self.js_functions += 1;
                self.js_mapped_bytes += mapped_bytes;
            }
            JitArtifactKind::WasmTierRoot => {
                self.wasm_tier_roots += 1;
                self.wasm_tier_mapped_bytes += mapped_bytes;
            }
        }
        true
    }

    fn release(&mut self, kind: JitArtifactKind, artifacts: usize, mapped_bytes: usize) {
        if matches!(kind, JitArtifactKind::BaselineWasm) {
            self.baseline_artifacts = self.baseline_artifacts.saturating_sub(artifacts);
            self.baseline_mapped_bytes = self.baseline_mapped_bytes.saturating_sub(mapped_bytes);
            return;
        }
        self.artifacts = self.artifacts.saturating_sub(artifacts);
        self.mapped_bytes = self.mapped_bytes.saturating_sub(mapped_bytes);
        match kind {
            JitArtifactKind::BaselineWasm => {}
            JitArtifactKind::JsFunction => {
                self.js_functions = self.js_functions.saturating_sub(artifacts);
                self.js_mapped_bytes = self.js_mapped_bytes.saturating_sub(mapped_bytes);
            }
            JitArtifactKind::WasmTierRoot => {
                self.wasm_tier_roots = self.wasm_tier_roots.saturating_sub(artifacts);
                self.wasm_tier_mapped_bytes = self
                    .wasm_tier_mapped_bytes
                    .saturating_sub(mapped_bytes);
            }
        }
    }
}

#[derive(Clone)]
pub(super) enum JsTierState {
    Hot { observations: u8, numeric: u8 },
    /// The numeric profile passed its look, so wait for a numeric entry before
    /// compiling the specialized artifact.
    PromotionPending,
    /// The function is hot but not numeric: count its `entries` until they pay for the
    /// tagged semantics baseline (`general_promotion_due`). Its calls stay as they were.
    GeneralPromotionPending { entries: u32 },
    /// The entries paid for the baseline: its calls take a VM frame
    /// (`Vm::js_tier_needs_general_entry`), whose next entry compiles it.
    GeneralCompileDue,
    Rejected {
        entries: u64,
        reason: &'static str,
    },
    Unsupported,
    Compiled(Rc<super::compiled::CompiledJsFunction>),
}

#[derive(Default)]
struct WasmTierState {
    /// The high bit records a one-shot promotion attempt; low bits hold the
    /// number of interpreter entries observed before that attempt.
    hotness: u8,
    compiled: Option<Rc<super::compiled::CompiledProgram>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct JsTierEntryDecision {
    tracked: bool,
    promoted: bool,
    compile: bool,
}

/// One sparse tier-state table per program. Frontend-specific transitions and
/// artifacts stay typed, while JS and Wasm share the same state owner and
/// avoid carrying a second empty hash-map header in every program entry.
enum JitTierManager {
    Empty,
    JavaScript(FxHashMap<u32, JsTierState>),
    Wasm(FxHashMap<u32, WasmTierState>),
}

impl Default for JitTierManager {
    fn default() -> Self {
        Self::Empty
    }
}

impl JitTierManager {
    /// Apply JavaScript's bounded statistical promotion transition to the
    /// manager-owned tier record. ProgramStore handles only global counters
    /// and diagnostic side effects after this decision is complete.
    fn note_js_entry(
        &mut self,
        function: u32,
        numeric: bool,
        first_entry_candidate: bool,
        general_candidate: bool,
        compiled_functions: usize,
        can_track: bool,
    ) -> JsTierEntryDecision {
        let mut decision = JsTierEntryDecision::default();
        if self.is_wasm() {
            return decision;
        }
        if self.js(function).is_none() {
            if !can_track {
                return decision;
            }
            decision.tracked = true;
            let initial = if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                JsTierState::Unsupported
            } else if first_entry_candidate {
                decision.promoted = true;
                JsTierState::PromotionPending
            } else {
                JsTierState::Hot {
                    observations: 1,
                    numeric: u8::from(numeric),
                }
            };
            let inserted = self.js_insert(function, initial);
            debug_assert!(inserted);
            decision.compile = decision.promoted;
            return decision;
        }

        let Some(state) = self.js_mut(function) else {
            return decision;
        };
        decision.compile = match state {
            JsTierState::Hot {
                observations,
                numeric: numeric_entries,
            } => {
                *observations = observations.saturating_add(1);
                *numeric_entries = numeric_entries.saturating_add(u8::from(numeric));
                let promotion = numeric_promotion_decision(*numeric_entries, *observations);
                if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                    *state = JsTierState::Unsupported;
                    false
                } else if first_entry_candidate {
                    decision.promoted = true;
                    true
                } else if promotion == Some(true) {
                    decision.promoted = true;
                    if numeric {
                        true
                    } else {
                        *state = JsTierState::PromotionPending;
                        false
                    }
                } else if promotion == Some(false) && general_candidate {
                    decision.promoted = true;
                    *state = JsTierState::GeneralPromotionPending { entries: u32::from(*observations) };
                    false
                } else {
                    if promotion == Some(false) && *observations == u8::MAX {
                        *state = JsTierState::Unsupported;
                    }
                    false
                }
            }
            JsTierState::PromotionPending => {
                if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                    *state = JsTierState::Unsupported;
                    false
                } else {
                    numeric
                }
            }
            JsTierState::GeneralPromotionPending { .. } | JsTierState::GeneralCompileDue => false,
            JsTierState::Rejected { .. }
            | JsTierState::Unsupported
            | JsTierState::Compiled(_) => false,
        };
        decision
    }

    /// Advance the Wasm call-hotness state. Both the lane handoff and the
    /// ordinary dispatcher call this transition owner, so the threshold and
    /// one-shot attempt bit have one implementation.
    fn note_wasm_entry(&mut self, function: u32, lane_handoff: bool) -> bool {
        let Some(state) = self.wasm_mut_or_default(function) else {
            return false;
        };
        if state.compiled.is_some() || state.hotness & 0x80 != 0 {
            return false;
        }
        let count = state.hotness & 0x7f;
        if lane_handoff && count.saturating_add(1) >= WASM_JIT_PROMOTION_ENTRIES {
            return true;
        }
        let count = count.saturating_add(1);
        state.hotness = count;
        if count < WASM_JIT_PROMOTION_ENTRIES {
            return false;
        }
        state.hotness |= 0x80;
        true
    }

    fn js(&self, function: u32) -> Option<&JsTierState> {
        match self {
            Self::JavaScript(functions) => functions.get(&function),
            Self::Empty | Self::Wasm(_) => None,
        }
    }

    fn js_mut(&mut self, function: u32) -> Option<&mut JsTierState> {
        match self {
            Self::JavaScript(functions) => functions.get_mut(&function),
            Self::Empty | Self::Wasm(_) => None,
        }
    }

    fn js_insert(&mut self, function: u32, state: JsTierState) -> bool {
        if matches!(self, Self::Empty) {
            *self = Self::JavaScript(FxHashMap::default());
        }
        match self {
            Self::JavaScript(functions) => {
                if let std::collections::hash_map::Entry::Vacant(slot) = functions.entry(function) {
                    slot.insert(state);
                    true
                } else {
                    false
                }
            }
            Self::Empty | Self::Wasm(_) => false,
        }
    }

    fn js_values(&self) -> impl Iterator<Item = &JsTierState> {
        let functions = match self {
            Self::JavaScript(functions) => Some(functions),
            Self::Empty | Self::Wasm(_) => None,
        };
        functions.into_iter().flat_map(|functions| functions.values())
    }

    fn wasm(&self, function: u32) -> Option<&WasmTierState> {
        match self {
            Self::Wasm(functions) => functions.get(&function),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    fn wasm_mut_or_default(&mut self, function: u32) -> Option<&mut WasmTierState> {
        if matches!(self, Self::Empty) {
            *self = Self::Wasm(FxHashMap::default());
        }
        match self {
            Self::Wasm(functions) => Some(functions.entry(function).or_default()),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    fn wasm_values(&self) -> impl Iterator<Item = &WasmTierState> {
        let functions = match self {
            Self::Wasm(functions) => Some(functions),
            Self::Empty | Self::JavaScript(_) => None,
        };
        functions.into_iter().flat_map(|functions| functions.values())
    }

    fn is_javascript(&self) -> bool {
        matches!(self, Self::JavaScript(_))
    }

    fn is_wasm(&self) -> bool {
        matches!(self, Self::Wasm(_))
    }
}

/// A program's residual: decoded, or a restored snapshot's encoding that its first use
/// decodes. A snapshot holds every builtin a preload loaded, and a process runs few of
/// them, so a restore decodes none: their checksums held when the snapshot was read.
struct Residual {
    /// The program's cache site counts, known before it is decoded.
    sites: crate::bytecode::CacheSiteCounts,
    decoded: std::cell::OnceCell<Rc<ResidualProgram>>,
    encoded: std::cell::Cell<Option<Box<EncodedResidual>>>,
}

/// What decoding a restored program needs (`ResidualProgram::decode_snapshot`).
struct EncodedResidual {
    bytes: crate::KeptBytes,
    text: Option<crate::bytecode::SourceText>,
    atoms: Rc<crate::bytecode::AtomSequence>,
}

impl Residual {
    fn decoded(program: Rc<ResidualProgram>) -> Self {
        Self {
            sites: program.cache_site_counts(),
            decoded: std::cell::OnceCell::from(program),
            encoded: std::cell::Cell::new(None),
        }
    }

    fn encoded(sites: crate::bytecode::CacheSiteCounts, encoded: EncodedResidual) -> Self {
        Self { sites, decoded: std::cell::OnceCell::new(), encoded: std::cell::Cell::new(Some(Box::new(encoded))) }
    }
}

impl std::ops::Deref for Residual {
    type Target = Rc<ResidualProgram>;

    fn deref(&self) -> &Rc<ResidualProgram> {
        self.decoded.get_or_init(|| {
            let EncodedResidual { bytes, text, atoms } =
                *self.encoded.take().expect("a residual is decoded or holds its encoding");
            let program = ResidualProgram::decode_snapshot(bytes, text, &atoms)
                .expect("a snapshot program whose checksum held decodes");
            debug_assert_eq!(program.cache_site_counts(), self.sites);
            program.prime_loop_decoding();
            Rc::new(program)
        })
    }
}

struct ProgramEntry {
    residual: Residual,
    wasm_signatures: Option<Rc<crate::wasm::WasmSignatures>>,
    constants: Vec<Value>,
    const_arrays: Vec<Option<crate::heap::ArrayElements>>,
    function_sources: Vec<Option<Value>>,
    /// Each RegExp literal site's record, shared by every object it evaluates to, or its
    /// compile error.
    regexp_literals: Vec<Option<Result<Rc<crate::heap::RegExpMeta>, Rc<str>>>>,
    module_environment: Option<Value>,
    import_meta: Option<Value>,
    module_imports: Vec<(u16, ModuleImport)>,
    /// Each root slot's rule, derived from the residual and `module_imports` on first read.
    root_slots: std::cell::OnceCell<Box<[crate::bytecode::RootSlotRule]>>,
    module: bool,
    /// Per-function lane views, derived from the residual on first lane entry.
    lane_views: LaneViews,
    /// Per function: what the JavaScript tier found at its entries (`js_entry_facts`).
    js_entry_facts: JsEntryFactsTable,
    /// Per function: the runs it has made without its lane view (`entry_lane_view`,
    /// `lane_call_may_derive`), up to `LANE_VIEW_DEFERRED_RUNS`.
    lane_deferred: Vec<std::cell::Cell<u8>>,
    /// The program's compiled functions, once the compiled mode first calls
    /// one; `Some(None)` records that none compile.
    compiled: Option<Option<Rc<super::compiled::CompiledProgram>>>,
    /// Sparse frontend-specific hotness state and compiled artifacts, owned
    /// by one VM-local tier manager.
    jit_tiers: JitTierManager,
    /// The environment the program's root body evaluates in.
    scope: ProgramScope,
    /// Source provenance is independent of the root binding environment.
    origin: CompilationOrigin,
    /// The Script or Module an eval program's code belongs to, which resolves
    /// its `import()` specifiers; `None` for a program that is its own.
    script_or_module: Option<Rc<str>>,
    /// Dynamic code (eval, `Function`, host scripts): reclaimed once nothing
    /// can run it. The main program and modules stay for the realm's life.
    reclaimable: bool,
    /// The reclamation pass current at insertion.
    pass: u64,
    /// Its root values changed since a collection last traced them. A minor collection
    /// traces only such programs: what an earlier collection reached stays marked until
    /// the next full collection, which traces every program.
    roots_pending: bool,
}

pub(super) const ANONYMOUS_EVAL_ORIGIN: &str = "eval at <anonymous>";

#[derive(Clone)]
pub(super) enum CompilationOrigin {
    Source,
    /// Execution state in another realm of one logical host Script.
    SharedScript { source: ProgramId },
    Eval { caller: Option<JsString> },
    FunctionConstructor { caller: JsString },
}

/// Where a stored program's root body evaluates, which decides whether its
/// by-name loads can only reach the global environment.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ProgramScope {
    /// As its kind dictates: Script code in the global environment.
    #[default]
    Own,
    /// An eval program evaluated in the global environment (indirect eval or a
    /// host script), whose names resolve as script code's do.
    Global,
    /// A sloppy direct eval of global code: it declares global variables but
    /// still sees its caller's lexical scope.
    GlobalVariables,
    /// A sloppy direct eval in a function's or module's scope: its variables
    /// belong to its caller's variable environment.
    Caller,
}

#[derive(Clone, Copy)]
pub(crate) enum ModuleImport {
    Value(Value),
    Binding(ProgramId, u16, Value),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ProgramId(u32);

impl ProgramId {
    pub(crate) const MAIN: Self = Self(0);

    fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

struct OwnedScript {
    owner: Weak<crate::api::CompiledScriptState>,
    instances: Vec<(WeakHandle, ProgramId)>,
}

pub(crate) struct ProgramStore {
    /// Indexed by `ProgramId`; a reclaimed program leaves an empty slot, so an
    /// id never names another program.
    programs: Vec<Option<ProgramEntry>>,
    /// Weak compiled-script identities for global eval; contexts are supplied per call.
    global_eval_programs: FxHashMap<JsString, ProgramId>,
    owned_scripts: FxHashMap<ProgramId, OwnedScript>,
    /// Completed reclamation passes; see `reclaim_unreferenced`.
    reclaim_passes: u64,
    /// Reclaimable programs inserted since the last reclamation pass.
    unchecked_reclaimable: usize,
    /// VM-local bounds for the optional JavaScript tier cache.
    js_jit_tracked_functions: usize,
    js_jit_tracking_exhaustions: u64,
    js_jit_rejected_functions: usize,
    /// Shared JS/Wasm executable-code cache admission and accounting.
    jit_code_cache: JitCodeCacheBudget,
    /// Whether a hot function's general baseline waits until its entries pay for
    /// the compile (`general_promotion_due`); tests of the baseline's semantics compile it
    /// once hot.
    general_baseline_priced: bool,
    /// Allocated only when opt-in JavaScript JIT diagnostics are requested.
    js_jit_diagnostics: Option<FxHashMap<(u32, u32), JsJitCounters>>,
    /// Whether ordinary JS functions promote to the general native dispatch tier
    /// (`note_js_general_dispatch_entry`). Off unless `QUENCH_JS_JIT_GENERAL_DISPATCH=1`:
    /// a fixed entry count promoted short runs into compile cost (npm p51, TOD-138) until
    /// promotion weighs size and compile cost.
    js_jit_general_dispatch_enabled: bool,
    /// Per-program, per-function estimated work from opt-in JS backedges.
    /// Both map levels are allocated only after the first eligible observation.
    js_general_dispatch_work: Option<FxHashMap<u32, FxHashMap<u32, u64>>>,
    js_jit_dataview_float64_enabled: bool,
    js_jit_dataview_float64_get_entries: u64,
    js_jit_dataview_float64_set_entries: u64,
}

impl Default for ProgramStore {
    fn default() -> Self {
        Self {
            programs: Vec::new(),
            global_eval_programs: FxHashMap::default(),
            owned_scripts: FxHashMap::default(),
            reclaim_passes: 0,
            unchecked_reclaimable: 0,
            js_jit_tracked_functions: 0,
            js_jit_tracking_exhaustions: 0,
            js_jit_rejected_functions: 0,
            jit_code_cache: JitCodeCacheBudget::default(),
            // TOD-191's correctness oracle may force hot candidates past the
            // payback model so guarded exits can be tested independently of
            // profitability. This is diagnostic-only and never set by score
            // harnesses.
            general_baseline_priced: !std::env::var_os(
                "QUENCH_JS_JIT_UNPRICED_GENERAL_DISPATCH",
            )
            .is_some_and(|value| value == "1"),
            js_jit_diagnostics: std::env::var_os("QUENCH_JS_JIT_STATS")
                .is_some_and(|value| value == "1")
                .then(FxHashMap::default),
            js_jit_general_dispatch_enabled: std::env::var_os("QUENCH_JS_JIT_GENERAL_DISPATCH")
                .is_some_and(|value| value == "1"),
            js_general_dispatch_work: None,
            js_jit_dataview_float64_enabled:
                std::env::var_os("QUENCH_JS_JIT_DATAVIEW_FLOAT64")
                    .is_none_or(|value| value != "0"),
            js_jit_dataview_float64_get_entries: 0,
            js_jit_dataview_float64_set_entries: 0,
        }
    }
}

impl ProgramStore {
    /// What the JavaScript tier finds at the entries of `function` (`JsEntryFacts`), derived on
    /// its first entry.
    pub(super) fn js_entry_facts(&self, id: ProgramId, function: u32) -> Option<Rc<super::js_compiled::JsEntryFacts>> {
        let entry = self.entry(id)?;
        let body = entry.residual.function_get(function as usize)?;
        let cells = entry.js_entry_facts.0.get_or_init(|| {
            entry.residual.functions.iter().map(|_| std::cell::OnceCell::new()).collect()
        });
        Some(Rc::clone(cells.get(function as usize)?.get_or_init(|| {
            Rc::new(super::js_compiled::JsEntryFacts::of(&entry.residual, body))
        })))
    }

    pub(super) fn cached_global_eval(&self, source: &JsString) -> Option<ProgramId> {
        let program = *self.global_eval_programs.get(source)?;
        self.entry(program).map(|_| program)
    }

    pub(super) fn cache_global_eval(&mut self, source: JsString, program: ProgramId) {
        self.global_eval_programs.insert(source, program);
    }

    pub(crate) fn len(&self) -> usize {
        self.programs.len()
    }

    fn entry(&self, id: ProgramId) -> Option<&ProgramEntry> {
        self.programs.get(id.index())?.as_ref()
    }

    /// Every mutation goes through here, so it also marks the program's roots pending.
    fn entry_mut(&mut self, id: ProgramId) -> Option<&mut ProgramEntry> {
        let entry = self.programs.get_mut(id.index())?.as_mut()?;
        entry.roots_pending = true;
        Some(entry)
    }

    /// Inserts dynamic code that `reclaim_unreferenced` may later drop.
    pub(crate) fn insert_reclaimable_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = self.insert_shared(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.reclaimable = true;
        }
        self.unchecked_reclaimable += 1;
        Some(id)
    }

    #[cfg(test)]
    pub(crate) fn live_count(&self) -> usize {
        self.programs.iter().flatten().count()
    }

    /// Whether dynamic code may be reclaimable, including dropped host owners.
    pub(crate) fn has_unchecked_reclaimable(&self) -> bool {
        self.unchecked_reclaimable > 0
            || self.owned_scripts.values().any(|script| script.owner.strong_count() == 0)
    }

    /// Drops every reclaimable program that `referenced` does not mark and that
    /// predates the previous pass. A program therefore survives the first pass
    /// after its insertion, while its first closure or frame is still being
    /// created. Returns the reclaimed ids.
    pub(crate) fn reclaim_unreferenced(&mut self, mut referenced: Vec<bool>) -> Vec<ProgramId> {
        // Owning a Script retains its code and live-realm execution state. An
        // escaped function retains its source record even after the owner drops.
        for (&source, script) in &self.owned_scripts {
            let owned = script.owner.strong_count() != 0;
            let live_instance = script.instances.iter().any(|(_, id)| {
                referenced.get(id.index()).copied().unwrap_or(false)
            });
            if (owned || live_instance) && let Some(mark) = referenced.get_mut(source.index()) { *mark = true; }
            if owned {
                for (_, id) in &script.instances {
                    if let Some(mark) = referenced.get_mut(id.index()) { *mark = true; }
                }
            }
        }
        let previous_pass = self.reclaim_passes;
        self.reclaim_passes += 1;
        let mut reclaimed = Vec::new();
        let mut reclaimed_tiers = (0usize, 0usize, 0usize);
        let mut reclaimed_wasm_tiers = (0usize, 0usize);
        let mut reclaimed_wasm_baselines = (0usize, 0usize);
        let mut reclaimed_rejected = 0usize;
        let mut pending = 0;
        for (index, slot) in self.programs.iter_mut().enumerate() {
            let Some(entry) = slot else { continue };
            if !entry.reclaimable || referenced.get(index).copied().unwrap_or(true) {
                continue;
            }
            if entry.pass >= previous_pass {
                pending += 1;
                continue;
            }
            reclaimed_tiers.0 += entry
                .jit_tiers
                .js_values()
                .filter(|state| !matches!(state, JsTierState::Rejected { .. }))
                .count();
            reclaimed_rejected += entry
                .jit_tiers
                .js_values()
                .filter(|state| matches!(state, JsTierState::Rejected { .. }))
                .count();
            for state in entry.jit_tiers.js_values() {
                if let JsTierState::Compiled(compiled) = state {
                    reclaimed_tiers.1 += 1;
                    reclaimed_tiers.2 += compiled.mapped_bytes();
                }
            }
            reclaimed_wasm_tiers.1 += entry
                .jit_tiers
                .wasm_values()
                .filter_map(|state| state.compiled.as_ref())
                .map(|compiled| compiled.mapped_bytes())
                .sum::<usize>();
            reclaimed_wasm_tiers.0 += entry
                .jit_tiers
                .wasm_values()
                .filter(|state| state.compiled.is_some())
                .count();
            if let Some(compiled) = entry.compiled.as_ref().and_then(Option::as_ref) {
                reclaimed_wasm_baselines.0 += 1;
                reclaimed_wasm_baselines.1 += compiled.mapped_bytes();
            }
            *slot = None;
            reclaimed.extend(ProgramId::from_index(index));
        }
        self.js_jit_tracked_functions = self
            .js_jit_tracked_functions
            .saturating_sub(reclaimed_tiers.0);
        self.js_jit_rejected_functions = self
            .js_jit_rejected_functions
            .saturating_sub(reclaimed_rejected);
        self.jit_code_cache.release(
            JitArtifactKind::JsFunction,
            reclaimed_tiers.1,
            reclaimed_tiers.2,
        );
        self.jit_code_cache.release(
            JitArtifactKind::WasmTierRoot,
            reclaimed_wasm_tiers.0,
            reclaimed_wasm_tiers.1,
        );
        self.jit_code_cache.release(
            JitArtifactKind::BaselineWasm,
            reclaimed_wasm_baselines.0,
            reclaimed_wasm_baselines.1,
        );
        self.unchecked_reclaimable = pending;
        if !reclaimed.is_empty() {
            let drop_work_map = self.js_general_dispatch_work.as_mut().is_some_and(|work| {
                for id in &reclaimed {
                    work.remove(&id.raw());
                }
                work.is_empty()
            });
            if drop_work_map {
                self.js_general_dispatch_work = None;
            }
            let programs = &self.programs;
            self.global_eval_programs.retain(|_, id| programs.get(id.index()).is_some_and(Option::is_some));
            self.owned_scripts.retain(|source, _| programs.get(source.index()).is_some_and(Option::is_some));
        }
        reclaimed
    }

    pub(crate) fn reset(&mut self, main: Rc<ResidualProgram>) -> ProgramId {
        self.global_eval_programs.clear();
        self.owned_scripts.clear();
        self.programs.clear();
        self.unchecked_reclaimable = 0;
        self.js_jit_tracked_functions = 0;
        self.js_jit_rejected_functions = 0;
        self.jit_code_cache = JitCodeCacheBudget::default();
        self.js_general_dispatch_work = None;
        if let Some(diagnostics) = &mut self.js_jit_diagnostics {
            diagnostics.clear();
        }
        let module = main.is_module();
        let id = self.insert_shared(main).unwrap_or(ProgramId::MAIN);
        if module && let Some(entry) = self.entry_mut(id) {
            entry.module = true;
        }
        id
    }

    pub(super) fn set_script_or_module(&mut self, id: ProgramId, name: Rc<str>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.script_or_module = Some(name);
        }
    }

    /// The name of the Script or Module the program's code belongs to.
    pub(super) fn script_or_module(&self, id: ProgramId) -> Option<Rc<str>> {
        let entry = self.entry(id)?;
        Some(
            entry
                .script_or_module
                .clone()
                .unwrap_or_else(|| Rc::from(entry.residual.source_name.as_str())),
        )
    }

    pub(super) fn register_owned_script(&mut self, script: &Rc<crate::api::CompiledScriptState>) {
        self.owned_scripts.insert(script.source, OwnedScript { owner: Rc::downgrade(script), instances: Vec::new() });
    }

    pub(super) fn owns_script(&self, script: &Rc<crate::api::CompiledScriptState>) -> bool {
        self.owned_scripts.get(&script.source).is_some_and(|entry| {
            std::ptr::eq(entry.owner.as_ptr(), Rc::as_ptr(script))
        })
    }

    pub(super) fn owned_script_instance(&self, source: ProgramId, realm: WeakHandle) -> Option<ProgramId> {
        self.owned_scripts.get(&source)?.instances.iter()
            .find_map(|(owner, id)| (*owner == realm).then_some(*id))
    }

    pub(super) fn bind_script_instance(&mut self, source: ProgramId, realm: WeakHandle, id: ProgramId) {
        self.set_origin(id, CompilationOrigin::SharedScript { source });
        self.owned_scripts.get_mut(&source).expect("registered Script source")
            .instances.push((realm, id));
    }

    pub(super) fn source_identity(&self, id: ProgramId) -> ProgramId {
        match self.entry(id).map(|entry| &entry.origin) {
            Some(CompilationOrigin::SharedScript { source }) => *source,
            _ => id,
        }
    }

    /// Prune realm identities before swept heap slots can be reused. Compiled
    /// Script ownership must never keep a realm global alive through this cache.
    pub(super) fn prune_owned_script_realms(&mut self, mut live: impl FnMut(WeakHandle) -> bool) {
        for script in self.owned_scripts.values_mut() {
            let before = script.instances.len();
            script.instances.retain(|(realm, _)| live(*realm));
            self.unchecked_reclaimable += before - script.instances.len();
        }
    }

    /// Clear call-site caches holding non-owning heap identities before swept
    /// slots can be reused for a different function Value.
    pub(super) fn clear_wasm_reference_caches(&self) {
        for entry in self.programs.iter().flatten() {
            if let Some(Some(compiled)) = &entry.compiled {
                compiled.clear_indirect_call_caches();
            }
            for state in entry.jit_tiers.wasm_values() {
                if let Some(compiled) = &state.compiled {
                    compiled.clear_indirect_call_caches();
                }
            }
        }
    }

    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {
        if let Some(entry) = self.entry_mut(id) { entry.origin = origin; }
    }

    pub(super) fn eval_origin(&self, id: ProgramId) -> Option<JsString> {
        match &self.entry(id)?.origin {
            CompilationOrigin::Source | CompilationOrigin::SharedScript { .. } => None,
            CompilationOrigin::Eval { caller } => Some(caller.clone()
                .unwrap_or_else(|| JsString::from_str(ANONYMOUS_EVAL_ORIGIN))),
            CompilationOrigin::FunctionConstructor { caller } => Some(caller.clone()),
        }
    }

    pub(super) fn is_function_constructor_body(&self, id: ProgramId, function: u32) -> bool {
        self.entry(id).is_some_and(|entry| {
            matches!(entry.origin, CompilationOrigin::FunctionConstructor { .. })
                && entry.residual.functions.get(function as usize)
                    .is_some_and(|function| function.header().parent == Some(super::ROOT_FUNCTION_ID))
        })
    }

    pub(crate) fn mark_global_scope(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::Global);
    }

    pub(crate) fn mark_global_variables(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::GlobalVariables);
    }

    /// Whether the program's root declares the realm's global variables.
    pub(crate) fn declares_global_variables(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| match entry.scope {
            ProgramScope::Own => entry.residual.kind == crate::bytecode::ProgramKind::Script,
            ProgramScope::Global | ProgramScope::GlobalVariables => true,
            ProgramScope::Caller => false,
        })
    }

    pub(crate) fn mark_caller_scope(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::Caller);
    }

    /// Whether the program's root variables belong to its caller.
    pub(crate) fn runs_in_caller_scope(&self, id: ProgramId) -> bool {
        self.entry(id)
            .is_some_and(|entry| entry.scope == ProgramScope::Caller)
    }

    fn set_scope(&mut self, id: ProgramId, scope: ProgramScope) {
        if let Some(entry) = self.entry_mut(id) {
            entry.scope = scope;
        }
    }

    /// Whether a by-name load the compiler left unresolved in `function` of the program can
    /// only denote a global binding, given that the function's names resolve statically: a
    /// module's environment holds only bindings the compiler resolved, at its root as in its
    /// functions; a global-scope program's root body may still meet bindings its own
    /// declarations add, so only its nested functions qualify.
    pub(crate) fn unresolved_names_are_global(&self, id: ProgramId, function: u32) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.residual.kind == crate::bytecode::ProgramKind::Module
                || (function != super::ROOT_FUNCTION_ID && self.runs_in_global_scope(id))
        })
    }

    /// Whether the program's root body evaluates in the global environment.
    pub(crate) fn runs_in_global_scope(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| match entry.scope {
            ProgramScope::Own => entry.residual.kind == crate::bytecode::ProgramKind::Script,
            ProgramScope::Global => true,
            ProgramScope::GlobalVariables | ProgramScope::Caller => false,
        })
    }

    pub(crate) fn insert(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.insert_shared(Rc::new(program))
    }

    pub(crate) fn find_shared(&self, program: &Rc<ResidualProgram>) -> Option<ProgramId> {
        self.programs
            .iter()
            .position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| Rc::ptr_eq(&entry.residual, program))
            })
            .and_then(ProgramId::from_index)
    }

    pub(crate) fn insert_shared(&mut self, mut program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = ProgramId::from_index(self.programs.len())?;
        // A function compiles again on its first call; until then it keeps only its header.
        // A program other owners share keeps its records.
        #[cfg(feature = "profile-aggregate")]
        let retain_profile_records = std::env::var_os("QUENCH_PROFILE_RETAIN_BYTECODE").is_some();
        #[cfg(not(feature = "profile-aggregate"))]
        let retain_profile_records = false;
        if !retain_profile_records && let Some(program) = Rc::get_mut(&mut program) {
            program.release_replayable_records();
        }
        let regexp_literals = vec![None; program.regexp_literal_sites.len()];
        let function_sources = vec![None; program.functions.len()];
        let lane_views = LaneViews::default();
        let lane_deferred = vec![std::cell::Cell::new(0); program.functions.len()];
        let origin = if program.kind == crate::bytecode::ProgramKind::Eval { CompilationOrigin::Eval { caller: None } } else { CompilationOrigin::Source };
        self.programs.push(Some(ProgramEntry {
            residual: Residual::decoded(program),
            wasm_signatures: None,
            constants: Vec::new(),
            const_arrays: Vec::new(),
            function_sources,
            regexp_literals,
            module_environment: None,
            import_meta: None,
            module_imports: Vec::new(),
            root_slots: std::cell::OnceCell::new(),
            module: false,
            lane_views,
            js_entry_facts: JsEntryFactsTable::default(),
            lane_deferred,
            compiled: None,
            jit_tiers: JitTierManager::default(),
            scope: ProgramScope::Own,
            origin,
            script_or_module: None,
            reclaimable: false,
            pass: self.reclaim_passes,
            roots_pending: true,
        }));
        Some(id)
    }

    /// Attach the module's canonical type facts; a code identity cannot acquire
    /// a different signature authority after functions have been registered.
    pub(crate) fn attach_wasm_signatures(
        &mut self,
        id: ProgramId,
        signatures: &Rc<crate::wasm::WasmSignatures>,
    ) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if let Some(existing) = &entry.wasm_signatures {
            return Rc::ptr_eq(existing, signatures);
        }
        if entry.residual.validate().is_err()
            || signatures.defined_count() != entry.residual.function_count()
        {
            return false;
        }
        for function in &entry.residual.functions {
            for instruction in &function.code {
                let instruction = if instruction.is_wide() {
                    function.wide[instruction.wide_index()]
                } else {
                    instruction.as_wide()
                };
                let valid = match instruction.op() {
                    crate::bytecode::Op::WasmRefFunc => {
                        signatures.get(instruction.imm() as usize).is_some()
                    }
                    crate::bytecode::Op::WasmIndirectTarget => {
                        signatures.type_signature(instruction.imm()).is_some()
                    }
                    op if crate::wasm::gc::StructConstruction::from_op(op).is_some() => {
                        let mode = crate::wasm::gc::StructConstruction::from_op(op).unwrap();
                        signatures
                            .declarations
                            .struct_fields(instruction.imm())
                            .is_some_and(|fields| {
                                mode.described()
                                    == signatures
                                        .declarations
                                        .descriptor_type(instruction.imm())
                                        .is_some()
                                    && fields.iter().all(|field| match field.element_type {
                                        wasmparser::StorageType::I8
                                        | wasmparser::StorageType::I16 => true,
                                        wasmparser::StorageType::Val(ty) => {
                                            signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some()
                                                && (!mode.defaulted() || ty.is_defaultable())
                                        }
                                    })
                                    && (mode == crate::wasm::gc::StructConstruction::Default
                                        || usize::from(instruction.register_window().count)
                                            == if mode.defaulted() {
                                                1
                                            } else {
                                                fields.len() + usize::from(mode.described())
                                            })
                            })
                    }
                    crate::bytecode::Op::WasmRefGetDesc => signatures
                        .declarations
                        .descriptor_type(instruction.imm())
                        .is_some(),
                    crate::bytecode::Op::WasmArrayNewData
                    | crate::bytecode::Op::WasmArrayNewElem => {
                        use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind};
                        instruction.register_window().count == ArraySegmentInput::COUNT
                            && signatures
                                .declarations
                                .array_field(instruction.imm())
                                .is_some_and(|field| {
                                    ArraySegmentKind::from_op(instruction.op())
                                        .unwrap()
                                        .accepts(field.element_type)
                                        && match field.element_type {
                                            wasmparser::StorageType::Val(ty) => signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some(),
                                            _ => true,
                                        }
                                })
                    }
                    crate::bytecode::Op::WasmArrayNew
                    | crate::bytecode::Op::WasmArrayNewDefault
                    | crate::bytecode::Op::WasmArrayNewFixed => signatures
                        .declarations
                        .array_field(instruction.imm())
                        .is_some_and(|field| {
                            let valid = match field.element_type {
                                wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => true,
                                wasmparser::StorageType::Val(ty) => {
                                    signatures.declarations.callable_value_type(ty).is_some()
                                        && (instruction.op()
                                            != crate::bytecode::Op::WasmArrayNewDefault
                                            || ty.is_defaultable())
                                }
                            };
                            valid
                                && (instruction.op() != crate::bytecode::Op::WasmArrayNewFixed
                                    || usize::from(instruction.register_b())
                                        .checked_add(usize::from(
                                            instruction.register_window().count,
                                        ))
                                        .is_some_and(|end| end <= usize::from(function.registers)))
                        }),
                    crate::bytecode::Op::WasmDescriptorTest
                    | crate::bytecode::Op::WasmDescriptorCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.descriptor_type(&signatures.declarations))
                            .is_some()
                    }
                    crate::bytecode::Op::WasmRefTest | crate::bytecode::Op::WasmRefCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.reference_type())
                            .and_then(|ty| {
                                signatures
                                    .declarations
                                    .callable_value_type(wasmparser::ValType::Ref(ty))
                            })
                            .is_some()
                    }
                    _ => true,
                };
                if !valid {
                    return false;
                }
            }
        }
        entry.wasm_signatures = Some(signatures.clone());
        true
    }

    pub(crate) fn wasm_function_signature(
        &self,
        id: ProgramId,
        index: u32,
    ) -> Option<&crate::WasmSignature> {
        self.entry(id)?
            .wasm_signatures
            .as_ref()?
            .defined_signature(index)
    }

    pub(crate) fn wasm_signatures(&self, id: ProgramId) -> Option<&crate::wasm::WasmSignatures> {
        self.entry(id)?.wasm_signatures.as_deref()
    }

    pub(crate) fn insert_module(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let id = self.insert(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.module = true;
        }
        Some(id)
    }

    /// Whether `slot` of `id`'s root environment is the binding itself: no global var,
    /// module-import or (unless `lexical_slots_are_bindings`) global lexical projection reads
    /// through it.
    pub(crate) fn root_slot_is_binding(
        &self,
        id: ProgramId,
        slot: u16,
        lexical_slots_are_bindings: bool,
    ) -> bool {
        use crate::bytecode::RootSlotRule;
        let Some(entry) = self.entry(id) else {
            return false;
        };
        let rules = entry.root_slots.get_or_init(|| {
            let locals = entry.residual.functions.first().map_or(0, |root| root.local_atoms.len());
            (0..locals)
                .map(|slot| {
                    let import = entry.module_imports.iter().any(|(local, _)| usize::from(*local) == slot);
                    if import { RootSlotRule::Projection } else { entry.residual.root_slot_rule(slot) }
                })
                .collect()
        });
        match rules.get(usize::from(slot)).copied() {
            Some(RootSlotRule::Binding) => true,
            Some(RootSlotRule::GlobalLexical) => lexical_slots_are_bindings,
            Some(RootSlotRule::Projection) => false,
            // A slot past the root's named locals is a temporary, the binding itself.
            None => !entry.module_imports.iter().any(|(local, _)| *local == slot),
        }
    }

    pub(crate) fn is_module(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| entry.module)
    }

    pub(crate) fn set_module_environment(&mut self, id: ProgramId, environment: Value) {
        if let Some(entry) = self.entry_mut(id)
            && entry.module
        {
            entry.module_environment = Some(environment);
        }
    }

    /// Every live program's compiled form.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn residuals(&self) -> impl Iterator<Item = &ResidualProgram> {
        self.programs.iter().flatten().map(|entry| &**entry.residual)
    }

    pub(crate) fn module_environment(&self, id: ProgramId) -> Option<Value> {
        self.entry(id)?.module_environment
    }

    pub(crate) fn import_meta(&self, id: ProgramId) -> Option<Value> {
        self.entry(id)?.import_meta
    }

    pub(crate) fn source_name(&self, id: ProgramId) -> Option<&str> {
        Some(&self.entry(id)?.residual.source_name)
    }

    pub(crate) fn set_import_meta(&mut self, id: ProgramId, value: Value) {
        if let Some(entry) = self.entry_mut(id)
            && entry.module
        {
            entry.import_meta = Some(value);
        }
    }

    pub(crate) fn set_module_imports(&mut self, id: ProgramId, imports: Vec<(u16, ModuleImport)>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.module_imports = imports;
            entry.root_slots = std::cell::OnceCell::new();
        }
    }

    pub(crate) fn module_imports(&self, id: ProgramId) -> &[(u16, ModuleImport)] {
        self.entry(id).map_or(&[], |entry| &entry.module_imports)
    }

    pub(crate) fn module_import(&self, id: ProgramId, slot: u16) -> Option<ModuleImport> {
        self.entry(id).and_then(|entry| {
            entry
                .module_imports
                .iter()
                .find_map(|(local, import)| (*local == slot).then_some(*import))
        })
    }

    /// The cache site counts of program `id`, which a restored program knows undecoded.
    pub(crate) fn cache_site_counts(&self, id: ProgramId) -> Option<crate::bytecode::CacheSiteCounts> {
        Some(self.entry(id)?.residual.sites)
    }

    /// Borrows the residual without the reference-count traffic of `get`.
    pub(crate) fn residual(&self, id: ProgramId) -> Option<&ResidualProgram> {
        self.entry(id).map(|entry| &**entry.residual)
    }

    pub(crate) fn get(&self, id: ProgramId) -> Option<Rc<ResidualProgram>> {
        self.entry(id).map(|entry| entry.residual.clone())
    }

    pub(crate) fn set_constants(&mut self, id: ProgramId, constants: Vec<Value>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.const_arrays = vec![None; constants.len()];
            entry.constants = constants;
            // Lane views carry constants, so they derive from the new set.
            entry.lane_views = LaneViews::default();
        }
    }

    /// The lane view of `function`, derived by `derive` on first use from the
    /// immutable residual and the program's constants.
    pub(super) fn compiled(
        &self,
        id: ProgramId,
    ) -> Option<Option<Rc<super::compiled::CompiledProgram>>> {
        self.entry(id)?.compiled.clone()
    }

    pub(super) fn has_compiled_function_or_note_fallback(
        &self,
        id: ProgramId,
        function: usize,
    ) -> bool {
        self.entry(id)
            .and_then(|entry| entry.compiled.as_ref()?.as_ref())
            .is_some_and(|program| program.has_function_or_note_fallback(function))
    }

    /// Whether an already retained selective-tier artifact contains this
    /// function. This check is intentionally borrow-only so lane dispatches
    /// do not clone its `Rc` on every call.
    pub(super) fn has_wasm_tier_containing(&self, id: ProgramId, function: usize) -> bool {
        let Some(tiers) = self.entry(id).map(|entry| &entry.jit_tiers) else {
            return false;
        };
        tiers
            .wasm(function as u32)
            .is_some_and(|state| state.compiled.is_some())
            || tiers.wasm_values().any(|state| {
                state
                    .compiled
                    .as_ref()
                    .is_some_and(|compiled| compiled.has_function(function))
            })
    }

    pub(super) fn set_compiled(
        &mut self,
        id: ProgramId,
        compiled: Option<Rc<super::compiled::CompiledProgram>>,
    ) {
        if let Some(previous_bytes) = self
            .entry(id)
            .and_then(|entry| entry.compiled.as_ref())
            .and_then(Option::as_ref)
            .map(|previous| previous.mapped_bytes())
        {
            self.jit_code_cache
                .release(JitArtifactKind::BaselineWasm, 1, previous_bytes);
        }
        let compiled = compiled.filter(|compiled| {
            self.jit_code_cache
                .retain(JitArtifactKind::BaselineWasm, compiled.mapped_bytes())
        });
        if let Some(entry) = self.entry_mut(id) {
            entry.compiled = Some(compiled);
        }
    }

    /// Record another interpreter entry and return true exactly once when a
    /// cold unsupported root reaches the threshold. Terminal attempt state
    /// prevents repeated compile failures from turning hot calls into work.
    pub(super) fn note_wasm_tier_entry(&mut self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        entry.jit_tiers.note_wasm_entry(function, false)
    }

    /// Count a nonterminal `CallKnown` activation that stays in the fast
    /// interpreter lane. On the threshold call, leave the count unchanged and
    /// hand it to the general dispatcher, whose normal entry hook records the
    /// final observation and claims the one-shot compile attempt.
    pub(super) fn note_wasm_tier_lane_entry(&mut self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if !entry
            .residual
            .function_get(function as usize)
            .is_some_and(|function| {
                function.code.len() <= super::compiled::MAX_JIT_TIER_BYTECODES_PER_FUNCTION
            })
        {
            return false;
        }
        entry.jit_tiers.note_wasm_entry(function, true)
    }

    pub(super) fn wasm_tier(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        self.entry(id)?
            .jit_tiers
            .wasm(function)?
            .compiled
            .clone()
    }

    /// Find a root artifact whose static closure includes this function.
    pub(super) fn wasm_tier_containing(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        let tiers = &self.entry(id)?.jit_tiers;
        tiers
            .wasm(function)
            .and_then(|state| state.compiled.clone())
            .or_else(|| {
                tiers
                    .wasm_values()
                    .filter_map(|state| state.compiled.as_ref())
                    .find(|compiled| compiled.has_function(function as usize))
                    .cloned()
            })
    }

    /// Find a selective tier with the requested function in the currently
    /// executing artifact. Indirect-call promotion can place both caller and
    /// profile-selected target in one closure; prefer that closure so the
    /// native call does not bounce through the baseline artifact.
    pub(super) fn wasm_tier_for_artifact_containing(
        &self,
        id: ProgramId,
        function: usize,
        artifact: *const super::compiled::CompiledProgram,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        self.entry(id)?
            .jit_tiers
            .wasm_values()
            .filter_map(|state| state.compiled.as_ref())
            .find(|compiled| {
                std::ptr::eq(Rc::as_ptr(compiled), artifact) && compiled.has_function(function)
            })
            .cloned()
    }

    /// Whether the code budget could still retain a Wasm tier artifact: a compile it could not
    /// keep is work thrown away (sqlite3-wasm `--jit`: hundreds of root compiles after the
    /// tier budget filled, 15 s of its first iteration).
    pub(super) fn wasm_tier_has_room(&self) -> bool {
        self.jit_code_cache.can_retain(JitArtifactKind::WasmTierRoot, JIT_MIN_ARTIFACT_BYTES)
    }

    /// Retain one compiled root closure if the VM-wide tier budget permits
    /// it. Active calls retain their own Rc through native return.
    pub(super) fn install_wasm_tier(
        &mut self,
        id: ProgramId,
        root: u32,
        compiled: Rc<super::compiled::CompiledProgram>,
    ) -> bool {
        if !compiled.has_function(root as usize) {
            return false;
        }
        let Some(entry) = self.entry(id) else {
            return false;
        };
        if entry.jit_tiers.is_javascript() {
            return false;
        }
        if entry
            .jit_tiers
            .wasm(root)
            .is_some_and(|state| state.compiled.is_some())
        {
            return true;
        }
        let bytes = compiled.mapped_bytes();
        if !self
            .jit_code_cache
            .can_retain(JitArtifactKind::WasmTierRoot, bytes)
        {
            return false;
        }
        let retained = self
            .jit_code_cache
            .retain(JitArtifactKind::WasmTierRoot, bytes);
        debug_assert!(retained, "shared JIT budget changed during Wasm admission");
        let entry = self.entry_mut(id).expect("entry checked above");
        let Some(state) = entry.jit_tiers.wasm_mut_or_default(root) else {
            self.jit_code_cache
                .release(JitArtifactKind::WasmTierRoot, 1, bytes);
            return false;
        };
        state.compiled = Some(compiled);
        true
    }

    #[cfg(test)]
    pub(super) fn wasm_tier_entry_count(&self, id: ProgramId, function: u32) -> Option<u8> {
        self.entry(id)
            .and_then(|entry| entry.jit_tiers.wasm(function))
            .map(|state| state.hotness & 0x7f)
    }

    #[cfg(test)]
    pub(super) fn wasm_tier_attempted(&self, id: ProgramId, function: u32) -> Option<bool> {
        self.entry(id)
            .and_then(|entry| entry.jit_tiers.wasm(function))
            .map(|state| state.hotness & 0x80 != 0)
    }

    pub(super) fn js_tier(&self, id: ProgramId, function: u32) -> Option<JsTierState> {
        self.entry(id)?.jit_tiers.js(function).cloned()
    }

    pub(super) fn js_tier_ref(&self, id: ProgramId, function: u32) -> Option<&JsTierState> {
        self.entry(id)?.jit_tiers.js(function)
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn enable_js_general_dispatch(&mut self) {
        self.js_jit_general_dispatch_enabled = true;
    }

    pub(super) fn js_jit_diagnostics_enabled(&self) -> bool {
        self.js_jit_diagnostics.is_some()
    }

    pub(super) fn js_jit_general_dispatch_enabled(&self) -> bool {
        self.js_jit_general_dispatch_enabled
    }

    /// Records the counters `QUENCH_JS_JIT_STATS` enables, for a test to read.
    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn enable_js_jit_diagnostics(&mut self) {
        self.js_jit_diagnostics.get_or_insert_with(FxHashMap::default);
    }

    pub(super) fn js_jit_dataview_float64_enabled(&self) -> bool {
        self.js_jit_dataview_float64_enabled
    }

    pub(super) fn note_js_jit_dataview_float64(&mut self, setter: bool) {
        if self.js_jit_diagnostics.is_none() {
            return;
        }
        if setter {
            self.js_jit_dataview_float64_set_entries += 1;
        } else {
            self.js_jit_dataview_float64_get_entries += 1;
        }
    }

    /// Record the input guard outcome. Promotion checks only at prespecified
    /// sample sizes; exact binomial cutoffs spend alpha across those looks.
    pub(super) fn note_js_function_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        numeric: bool,
        first_entry_candidate: bool,
        general_candidate: bool,
    ) -> bool {
        let compiled_functions = self.jit_code_cache.js_functions;
        let can_track = self.js_jit_tracked_functions < JS_JIT_MAX_TRACKED_FUNCTIONS;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let already_tracked = entry.jit_tiers.js(function).is_some();
        let decision = entry.jit_tiers.note_js_entry(
            function,
            numeric,
            first_entry_candidate,
            general_candidate,
            compiled_functions,
            can_track,
        );
        if decision.tracked {
            self.js_jit_tracked_functions += 1;
        } else if !can_track && !already_tracked {
            self.js_jit_tracking_exhaustions =
                self.js_jit_tracking_exhaustions.saturating_add(1);
        }
        if decision.promoted {
            self.record_js_jit_promotion(id, function);
        }
        decision.compile
    }

    /// Whether the general baseline could ever pay through entries for a body of
    /// `instructions`: unpriced, or its entries save something past the baseline's entry
    /// overhead. A loop-free body that cannot is not counted toward it at all.
    pub(super) fn general_baseline_may_pay(&self, instructions: usize) -> bool {
        !self.general_baseline_priced || general_saved_per_entry(instructions).is_some()
    }

    /// Whether the general baseline may ever take a body of `instructions` under `policy`:
    /// the tier is switched on, the memory policy compiles JS tiers, and the body could
    /// pay. An `acyclic` body pays only through its entries (`general_baseline_may_pay`);
    /// one with loops may also pay through its backedge work (`general_dispatch_work_due`).
    /// A function it may not take is neither claimed for the tier nor counted toward it.
    pub(super) fn general_tier_admits(
        &self,
        instructions: usize,
        acyclic: bool,
        policy: crate::MemoryPolicy,
    ) -> bool {
        self.js_jit_general_dispatch_enabled
            && super::compiled::js_function_tiers_compile(policy)
            && (!acyclic || self.general_baseline_may_pay(instructions))
    }

    /// Compile a hot function's general baseline without pricing it, for tests of its
    /// semantics.
    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn compile_general_baseline_when_hot(&mut self) {
        self.general_baseline_priced = false;
    }

    /// Promote any ordinary JS function to the complete-semantics native
    /// dispatch baseline after it is hot. The existing tier record and shared
    /// code-cache budget remain the single owners for both baseline and
    /// specialized artifacts.
    pub(super) fn note_js_general_dispatch_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        instructions: usize,
        policy: crate::MemoryPolicy,
    ) -> bool {
        // Admission: the tier's switch, and the memory policy's (`JS_FUNCTION_TIERS`).
        if !self.js_jit_general_dispatch_enabled || !super::compiled::js_function_tiers_compile(policy) {
            return false;
        }
        let priced = self.general_baseline_priced;
        let can_track = self.js_jit_tracked_functions < JS_JIT_MAX_TRACKED_FUNCTIONS;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if entry.jit_tiers.is_wasm() {
            return false;
        }
        let mut promoted = false;
        let Some(state) = entry.jit_tiers.js_mut(function) else {
            if !can_track {
                self.js_jit_tracking_exhaustions =
                    self.js_jit_tracking_exhaustions.saturating_add(1);
                return false;
            }
            let tracked = entry.jit_tiers.js_insert(
                function,
                JsTierState::Hot {
                    observations: 1,
                    numeric: 0,
                },
            );
            debug_assert!(tracked);
            if tracked {
                self.js_jit_tracked_functions += 1;
            }
            return false;
        };
        match state {
            JsTierState::Hot { observations, .. } => {
                *observations = observations.saturating_add(1);
                if *observations >= JS_JIT_GENERAL_PROMOTION_ENTRIES {
                    *state = JsTierState::GeneralPromotionPending { entries: u32::from(*observations) };
                    promoted = true;
                }
            }
            JsTierState::PromotionPending => return false,
            JsTierState::GeneralPromotionPending { entries } => {
                *entries = entries.saturating_add(1);
                if priced && !general_promotion_due(*entries, instructions) {
                    return false;
                }
                *state = JsTierState::GeneralCompileDue;
                return true;
            }
            JsTierState::GeneralCompileDue => return true,
            JsTierState::Rejected { entries, .. } => {
                *entries = entries.saturating_add(1);
                if *entries >= u64::from(JS_JIT_GENERAL_PROMOTION_ENTRIES) {
                    *state = JsTierState::GeneralPromotionPending {
                        entries: u32::try_from(*entries).unwrap_or(u32::MAX),
                    };
                    promoted = true;
                }
            }
            JsTierState::Compiled(_) | JsTierState::Unsupported => return false,
        }
        if promoted {
            self.record_js_jit_promotion(id, function);
        }
        false
    }

    /// Accumulate estimated bytecode work from a hot backward edge. The
    /// function must already have an eligible JS tier record, and this path is
    /// available only to the opt-in complete-semantics dispatcher.
    pub(super) fn note_js_general_dispatch_backedge(
        &mut self,
        id: ProgramId,
        function: u32,
        work: usize,
        instructions: usize,
    ) -> bool {
        if !self.js_jit_general_dispatch_enabled || work == 0 {
            return false;
        }
        // A zero-byte admission probe checks count and aggregate artifact
        // ceilings before this accounting path can request compilation.
        if self.jit_code_cache.js_functions >= JS_JIT_MAX_FUNCTIONS
            || !self.jit_code_cache.can_retain(JitArtifactKind::JsFunction, 0)
        {
            return false;
        }
        let priced = self.general_baseline_priced;
        if !self.entry(id).is_some_and(|entry| {
            !entry.jit_tiers.is_wasm()
                && matches!(
                    entry.jit_tiers.js(function),
                    Some(
                        JsTierState::Hot { .. }
                            | JsTierState::Rejected { .. }
                            | JsTierState::GeneralPromotionPending { .. }
                            | JsTierState::Unsupported
                    )
                )
        }) {
            return false;
        }
        let work_by_program = self.js_general_dispatch_work.get_or_insert_with(FxHashMap::default);
        let accumulated_work = work_by_program
            .entry(id.raw())
            .or_default()
            .entry(function)
            .or_default();
        accumulate_general_dispatch_work(accumulated_work, work);
        let accumulated_work = *accumulated_work;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let Some(state) = entry.jit_tiers.js_mut(function) else {
            return false;
        };
        let due = promote_general_dispatch_if_paid(state, accumulated_work, instructions, priced);
        if due {
            self.record_js_jit_promotion(id, function);
        }
        due
    }

    /// Count only an explicit pre-native guard rejection by an installed
    /// specialized artifact. Each such entry runs in the interpreter, so it is
    /// paid evidence for GeneralDispatch. Completed native values and native
    /// exits never reach this transition.
    pub(super) fn note_js_specialized_guard_miss(
        &mut self,
        id: ProgramId,
        function: u32,
        instructions: usize,
    ) -> bool {
        if !self.js_jit_general_dispatch_enabled {
            return false;
        }
        let priced = self.general_baseline_priced;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if entry.jit_tiers.is_wasm() {
            return false;
        }
        let Some(state) = entry.jit_tiers.js_mut(function) else {
            return false;
        };
        let (misses, mapped_bytes) = match state {
            JsTierState::Compiled(compiled) if !compiled.is_general_dispatch() => {
                let misses = compiled.note_specialized_guard_miss(
                    JS_JIT_SPECIALIZED_GUARD_MISS_THRESHOLD,
                );
                if misses < JS_JIT_SPECIALIZED_GUARD_MISS_THRESHOLD {
                    return false;
                }
                (misses, compiled.mapped_bytes())
            }
            _ => return false,
        };
        let entries = u32::from(misses);
        let compile_due = !priced || general_promotion_due(entries, instructions);
        *state = if compile_due {
            JsTierState::GeneralCompileDue
        } else {
            JsTierState::GeneralPromotionPending { entries }
        };
        self.jit_code_cache
            .release(JitArtifactKind::JsFunction, 1, mapped_bytes);
        self.record_js_jit_promotion(id, function);
        compile_due
    }

    /// Memoize an unsupported bytecode shape separately from tier candidates.
    /// Cold bootstrap helpers therefore cannot consume the hot-function
    /// promotion table. Return its accumulated count only once diagnostic
    /// reporting becomes useful.
    pub(super) fn reject_js_function(
        &mut self,
        id: ProgramId,
        function: u32,
        reason: &'static str,
    ) -> Option<u64> {
        let can_reject = self.js_jit_rejected_functions < JS_JIT_MAX_REJECTED_FUNCTIONS;
        let entry = self.entry_mut(id)?;
        let mut inserted = false;
        let count = if let Some(JsTierState::Rejected { entries, .. }) =
            entry.jit_tiers.js_mut(function)
        {
            *entries = entries.saturating_add(1);
            *entries
        } else {
            if !can_reject {
                return None;
            }
            inserted = entry
                .jit_tiers
                .js_insert(function, JsTierState::Rejected { entries: 1, reason });
            if !inserted {
                return None;
            }
            1
        };
        if inserted {
            self.js_jit_rejected_functions += 1;
        }
        (count >= JS_JIT_REJECTION_DIAGNOSTIC_ENTRIES).then_some(count)
    }

    pub(super) fn note_js_rejected_entry(
        &mut self,
        id: ProgramId,
        function: u32,
    ) -> Option<(u64, &'static str)> {
        let JsTierState::Rejected { entries, reason } = self
            .entry_mut(id)?
            .jit_tiers
            .js_mut(function)?
        else {
            return None;
        };
        *entries = entries.saturating_add(1);
        (*entries >= JS_JIT_REJECTION_DIAGNOSTIC_ENTRIES).then_some((*entries, *reason))
    }

    pub(super) fn record_js_jit_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        numeric: bool,
        parameter_count: u16,
    ) {
        self.record_js_jit_counter_entry(id, function, Some(numeric), parameter_count);
    }

    pub(super) fn record_js_jit_lane_entry(&mut self, id: ProgramId, function: u32) {
        let parameter_count = self
            .entry(id)
            .and_then(|entry| entry.residual.function_get(function as usize))
            .map_or(0, |function| function.params);
        self.record_js_jit_counter_entry(id, function, None, parameter_count);
    }

    fn record_js_jit_counter_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        numeric: Option<bool>,
        parameter_count: u16,
    ) {
        let key = (id.raw(), function);
        let Some(diagnostics) = self.js_jit_diagnostics.as_ref() else {
            return;
        };
        if diagnostics.contains_key(&key) {
            if let Some(counters) = self
                .js_jit_diagnostics
                .as_mut()
                .and_then(|diagnostics| diagnostics.get_mut(&key))
            {
                counters.parameter_count = parameter_count;
                if let Some(numeric) = numeric {
                    counters.entries = counters.entries.saturating_add(1);
                    counters.numeric_entries = counters
                        .numeric_entries
                        .saturating_add(u64::from(numeric));
                } else {
                    counters.lane_entries = counters.lane_entries.saturating_add(1);
                }
            }
            return;
        }
        if diagnostics.len() >= JS_JIT_MAX_DIAGNOSTIC_FUNCTIONS {
            return;
        }
        let (source_name, function_name, instruction_count, opcode_counts, guard_census) = self
            .entry(id)
            .map(|entry| {
                let slot = entry.residual.functions.get(function as usize);
                let function_code = slot.and_then(|slot| slot.record());
                let function_name = slot
                    .and_then(|slot| slot.header().name.or(slot.header().inferred_name))
                    .and_then(|atom| {
                        let index = atom as usize;
                        (index < entry.residual.atoms.len())
                            .then(|| entry.residual.atoms[index].to_owned())
                    })
                    .unwrap_or_default();
                let instruction_count = function_code.map_or(0, |function| {
                    u32::try_from(function.code.len()).unwrap_or(u32::MAX)
                });
                let mut opcode_counts = vec![0_u32; crate::bytecode::Op::COUNT];
                if let Some(function) = function_code {
                    for instruction in &function.code {
                        let opcode = instruction.as_wide().op() as usize;
                        opcode_counts[opcode] = opcode_counts[opcode].saturating_add(1);
                    }
                }
                let opcode_counts = opcode_counts
                    .into_iter()
                    .enumerate()
                    .filter_map(|(opcode, count)| {
                        (count != 0).then(|| (u16::try_from(opcode).unwrap_or(u16::MAX), count))
                    })
                    .collect();
                // This is a bytecode-site census, not a claim that a site is
                // pure, redundant, or safe to remove. Predicate purity and
                // path implication need separate proofs before optimization.
                let mut guard_census = (0_u32, 0_u32, 0_u32, 0_u32);
                if let Some(function) = function_code {
                    for instruction in &function.code {
                        let instruction = instruction.as_wide();
                        match instruction.op() {
                            crate::bytecode::Op::JumpFalse
                            | crate::bytecode::Op::JumpTrue
                            | crate::bytecode::Op::JumpBinaryFalse => {
                                guard_census.0 = guard_census.0.saturating_add(1);
                                if instruction.op() == crate::bytecode::Op::JumpBinaryFalse {
                                    match instruction.binary_operator_field() {
                                        x if x
                                            == oxc_ast::ast::BinaryOperator::StrictEquality as u32
                                            || x == oxc_ast::ast::BinaryOperator::StrictInequality as u32 =>
                                        {
                                            guard_census.2 = guard_census.2.saturating_add(1);
                                        }
                                        x if x == oxc_ast::ast::BinaryOperator::LessThan as u32
                                            || x == oxc_ast::ast::BinaryOperator::LessEqualThan as u32
                                            || x == oxc_ast::ast::BinaryOperator::GreaterThan as u32
                                            || x == oxc_ast::ast::BinaryOperator::GreaterEqualThan as u32 =>
                                        {
                                            guard_census.3 = guard_census.3.saturating_add(1);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            crate::bytecode::Op::Unary
                                if instruction.unary_operator()
                                    == oxc_ast::ast::UnaryOperator::Typeof as u32 =>
                            {
                                guard_census.1 = guard_census.1.saturating_add(1);
                            }
                            crate::bytecode::Op::Binary => {
                                match instruction.binary_operator() {
                                    x if x == oxc_ast::ast::BinaryOperator::StrictEquality as u32
                                        || x == oxc_ast::ast::BinaryOperator::StrictInequality as u32 =>
                                    {
                                        guard_census.2 = guard_census.2.saturating_add(1);
                                    }
                                    x if x == oxc_ast::ast::BinaryOperator::LessThan as u32
                                        || x == oxc_ast::ast::BinaryOperator::LessEqualThan as u32
                                        || x == oxc_ast::ast::BinaryOperator::GreaterThan as u32
                                        || x == oxc_ast::ast::BinaryOperator::GreaterEqualThan as u32 =>
                                    {
                                        guard_census.3 = guard_census.3.saturating_add(1);
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
                (
                    entry.residual.source_name.clone(),
                    function_name,
                    instruction_count,
                    opcode_counts,
                    guard_census,
                )
            })
            .unwrap_or_default();
        if let Some(diagnostics) = self.js_jit_diagnostics.as_mut() {
            let counters = diagnostics.entry(key).or_default();
            counters.source_name = Some(source_name.into_boxed_str());
            counters.function_name = Some(function_name.into_boxed_str());
            counters.parameter_count = parameter_count;
            counters.instruction_count = instruction_count;
            counters.opcode_counts = opcode_counts;
            counters.guard_conditional_branches = guard_census.0;
            counters.guard_typeof_ops = guard_census.1;
            counters.guard_strict_comparisons = guard_census.2;
            counters.guard_relational_comparisons = guard_census.3;
            if let Some(numeric) = numeric {
                counters.entries = counters.entries.saturating_add(1);
                counters.numeric_entries = counters
                    .numeric_entries
                    .saturating_add(u64::from(numeric));
            } else {
                counters.lane_entries = counters.lane_entries.saturating_add(1);
            }
        }
    }

    fn record_js_jit_promotion(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.promotions = counters.promotions.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_compile(
        &mut self,
        id: ProgramId,
        function: u32,
        elapsed_ns: u64,
        code_bytes: usize,
        installed: bool,
        rejection: Option<&'static str>,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.compile_attempts = counters.compile_attempts.saturating_add(1);
            counters.compile_successes = counters
                .compile_successes
                .saturating_add(u64::from(installed));
            counters.compile_ns = counters.compile_ns.saturating_add(elapsed_ns);
            counters.code_bytes = counters.code_bytes.saturating_add(code_bytes);
            if rejection.is_some() {
                counters.rejection_reason = rejection;
            }
        }
    }

    pub(super) fn record_js_jit_rejection(
        &mut self,
        id: ProgramId,
        function: u32,
        reason: &'static str,
        entries: u64,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.rejection_reason = Some(reason);
            counters.entries = entries;
        }
    }

    pub(super) fn record_js_jit_native_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.native_entries = counters.native_entries.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_field_cache_probe(
        &mut self,
        id: ProgramId,
        function: u32,
        hit: bool,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_cache_probes = counters.field_cache_probes.saturating_add(1);
            counters.field_cache_hits = counters
                .field_cache_hits
                .saturating_add(u64::from(hit));
        }
    }

    pub(super) fn record_js_jit_field_store_cache_probe(
        &mut self,
        id: ProgramId,
        function: u32,
        hit: bool,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_store_cache_probes =
                counters.field_store_cache_probes.saturating_add(1);
            counters.field_store_cache_hits = counters
                .field_store_cache_hits
                .saturating_add(u64::from(hit));
        }
    }

    pub(super) fn record_js_jit_field_return_candidate(
        &mut self,
        id: ProgramId,
        function: u32,
        fixed_this: bool,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_return_candidate_entries =
                counters.field_return_candidate_entries.saturating_add(1);
            counters.field_return_fixed_this_entries = counters
                .field_return_fixed_this_entries
                .saturating_add(u64::from(fixed_this));
        }
    }

    pub(super) fn record_js_jit_field_return_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_return_fast_entries =
                counters.field_return_fast_entries.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_field_increment_candidate(
        &mut self,
        id: ProgramId,
        function: u32,
        fixed_this: bool,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_increment_candidate_entries =
                counters.field_increment_candidate_entries.saturating_add(1);
            counters.field_increment_fixed_this_entries = counters
                .field_increment_fixed_this_entries
                .saturating_add(u64::from(fixed_this));
        }
    }

    pub(super) fn record_js_jit_field_increment_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.field_increment_fast_entries =
                counters.field_increment_fast_entries.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_numeric_find_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.numeric_find_fast_entries = counters
                .numeric_find_fast_entries
                .saturating_add(1);
        }
    }

    /// Count bytecodes reaching the general dispatcher for a tracked JS function.
    /// Threaded fast-lane instructions are excluded, so this reveals dynamic fallback
    /// work separately from the static opcode-site census.
    pub(super) fn record_js_jit_general_dispatch_opcode(
        &mut self,
        id: ProgramId,
        function: u32,
        opcode: crate::bytecode::Op,
    ) {
        let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        else {
            return;
        };
        let opcode = opcode as u16;
        if let Some((_, count)) = counters
            .general_dispatch_opcode_counts
            .iter_mut()
            .find(|(seen, _)| *seen == opcode)
        {
            *count = count.saturating_add(1);
        } else {
            counters.general_dispatch_opcode_counts.push((opcode, 1));
        }
    }

    pub(super) fn record_js_jit_fallback_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.fallback_entries = counters.fallback_entries.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_interpreter_completion(
        &mut self,
        id: ProgramId,
        function: u32,
        inclusive_ns: u64,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.interpreter_completions = counters.interpreter_completions.saturating_add(1);
            counters.interpreter_inclusive_ns = counters
                .interpreter_inclusive_ns
                .saturating_add(inclusive_ns);
        }
    }

    pub(super) fn install_js_tier(
        &mut self,
        id: ProgramId,
        function: u32,
        compiled: Option<Rc<super::compiled::CompiledJsFunction>>,
        general_fallback_after_codegen_rejection: bool,
    ) -> bool {
        let Some(entry) = self.entry(id) else {
            return false;
        };
        if entry.jit_tiers.is_wasm() {
            return false;
        }
        let compilation_succeeded = compiled.is_some();
        let compiled = compiled.filter(|compiled| {
            self.jit_code_cache
                .retain(JitArtifactKind::JsFunction, compiled.mapped_bytes())
        });
        if let Some(entry) = self.entry_mut(id) {
            let tier = match compiled {
                Some(compiled) => JsTierState::Compiled(compiled),
                None if general_fallback_after_codegen_rejection && !compilation_succeeded => {
                    JsTierState::GeneralPromotionPending { entries: 1 }
                }
                None => JsTierState::Unsupported,
            };
            if let Some(state) = entry.jit_tiers.js_mut(function) {
                *state = tier;
            } else {
                entry.jit_tiers.js_insert(function, tier);
            }
        }
        self.js_tier(id, function)
            .is_some_and(|tier| matches!(tier, JsTierState::Compiled(_)))
    }

    #[cfg(test)]
    pub(super) fn js_jit_tracked_function_count(&self) -> usize {
        self.js_jit_tracked_functions
    }

    #[cfg(test)]
    pub(super) fn js_jit_tracking_exhaustion_count(&self) -> u64 {
        self.js_jit_tracking_exhaustions
    }

    #[cfg(all(test, not(quench_profiled)))]
    pub(super) fn jit_mapped_bytes(&self) -> usize {
        self.jit_code_cache.mapped_bytes + self.jit_code_cache.baseline_mapped_bytes
    }

    /// A program's lane views by function, derived on first use.
    pub(super) fn lane_views(
        &self,
        id: ProgramId,
    ) -> Option<&[std::cell::OnceCell<super::dispatch_fast::LaneView>]> {
        let entry = self.entry(id)?;
        Some(entry.lane_views.of(&entry.residual))
    }

    /// The lane view a general-loop entry of `function` runs with. A function that may loop
    /// gets it at its first entry; any other once its deferred runs are complete
    /// (`LANE_VIEW_DEFERRED_RUNS`), so code that runs a few times without looping (a module
    /// body) runs on the general path instead of first deriving a view of every instruction,
    /// which costs more than those runs.
    pub(super) fn entry_lane_view(
        &self,
        id: ProgramId,
        function: u32,
        may_loop: impl FnOnce(&crate::bytecode::Function) -> bool,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
            Option<&[crate::compile::liveness::RegisterSet]>,
        ) -> super::dispatch_fast::LaneView,
    ) -> Option<*const super::dispatch_fast::LaneView> {
        let entry = self.entry(id)?;
        let view = entry.lane_views.of(&entry.residual).get(function as usize)?;
        if let Some(view) = view.get() {
            return Some(view);
        }
        let code = entry.residual.function_get(function as usize)?;
        if !deferred_run_completes(entry.lane_deferred.get(function as usize)?) && !may_loop(code) {
            return None;
        }
        Some(view.get_or_init(|| {
            let live = entry.residual.register_root_masks(function);
            let view = derive(code, &entry.constants, entry.residual.kind, live);
            #[cfg(feature = "profile-lane-exits")]
            {
                let (start, end) = view.record_range();
                super::lane_exits::register_view(start, end, census_label(entry, function), code.instructions().map(|instruction| instruction.op()).collect());
            }
            view
        }))
    }

    /// Whether a lane call may derive `function`'s view now: it has one, or its earlier entries
    /// and lane calls completed its deferred runs (`LANE_VIEW_DEFERRED_RUNS`), as
    /// `entry_lane_view` defers its entries. A deferred call counts its run and takes the
    /// general path.
    pub(super) fn lane_call_may_derive(&self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry(id) else {
            return false;
        };
        let derived = entry.lane_views.derived(function).is_some();
        derived
            || entry
                .lane_deferred
                .get(function as usize)
                .is_some_and(deferred_run_completes)
    }

    pub(super) fn lane_view(
        &self,
        id: ProgramId,
        function: u32,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
            Option<&[crate::compile::liveness::RegisterSet]>,
        ) -> super::dispatch_fast::LaneView,
    ) -> Option<*const super::dispatch_fast::LaneView> {
        let entry = self.entry(id)?;
        let code = entry.residual.function_get(function as usize)?;
        let view = entry.lane_views.of(&entry.residual).get(function as usize)?;
        Some(view.get_or_init(|| {
            let live = entry.residual.register_root_masks(function);
            let view = derive(code, &entry.constants, entry.residual.kind, live);
            #[cfg(feature = "profile-lane-exits")]
            {
                let (start, end) = view.record_range();
                super::lane_exits::register_view(start, end, census_label(entry, function), code.instructions().map(|instruction| instruction.op()).collect());
            }
            view
        }))
    }

    pub(crate) fn constant(&self, id: ProgramId, index: usize) -> Option<Value> {
        self.constants(id).get(index).copied()
    }

    pub(crate) fn constants(&self, id: ProgramId) -> &[Value] {
        self.entry(id)
            .map_or(&[], |entry| entry.constants.as_slice())
    }

    pub(crate) fn const_array(
        &mut self,
        id: ProgramId,
        start: usize,
        count: usize,
    ) -> Option<crate::heap::ArrayElements> {
        let entry = self.entry_mut(id)?;
        let end = start.checked_add(count)?;
        let cached = entry.const_arrays.get_mut(start)?;
        if cached.is_none() {
            *cached = Some(crate::heap::ArrayElements::from_slice(entry.constants.get(start..end)?));
        }
        cached.clone()
    }

    pub(crate) fn function_source(&self, id: ProgramId, function: u32) -> Option<Value> {
        self.entry(id)?
            .function_sources
            .get(function as usize)
            .copied()
            .flatten()
    }

    pub(crate) fn cache_function_source(
        &mut self,
        id: ProgramId,
        function: u32,
        source: Value,
    ) -> Option<Value> {
        let cached = self
            .entry_mut(id)?
            .function_sources
            .get_mut(function as usize)?;
        Some(*cached.get_or_insert(source))
    }

    pub(crate) fn regexp_literal_meta(
        &self,
        id: ProgramId,
        site: usize,
    ) -> Option<Result<Rc<crate::heap::RegExpMeta>, Rc<str>>> {
        self.entry(id)?.regexp_literals.get(site)?.as_ref().cloned()
    }

    pub(crate) fn cache_regexp_literal_meta(
        &mut self,
        id: ProgramId,
        site: usize,
        matcher: Result<Rc<crate::heap::RegExpMeta>, Rc<str>>,
    ) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let Some(cached) = entry.regexp_literals.get_mut(site) else {
            return false;
        };
        if cached.is_none() {
            *cached = Some(matcher);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn compiled_regexp_literal_count(&self, id: ProgramId) -> usize {
        self.entry(id).map_or(0, |entry| {
            entry
                .regexp_literals
                .iter()
                .filter(|site| site.is_some())
                .count()
        })
    }

    /// Appends the values the stored programs keep alive to `roots`: every program's for a
    /// full collection, only those of programs whose roots changed since the last
    /// collection for a minor one.
    pub(crate) fn extend_roots(&mut self, roots: &mut Vec<Value>, full: bool) {
        for entry in self.programs.iter_mut().flatten() {
            if !full && !entry.roots_pending {
                continue;
            }
            entry.roots_pending = false;
            roots.extend_from_slice(&entry.constants);
            roots.extend(entry.function_sources.iter().flatten());
            roots.extend(entry.module_environment);
            roots.extend(entry.import_meta);
            roots.extend(entry.module_imports.iter().map(|(_, import)| match import {
                ModuleImport::Value(value) => *value,
                ModuleImport::Binding(_, _, fallback) => *fallback,
            }));
        }
    }
}

impl ProgramStore {
    /// Whether dropping the store reports something (`QUENCH_JS_JIT_STATS`).
    pub(crate) fn reports_on_drop(&self) -> bool {
        self.js_jit_diagnostics.is_some()
    }
}

impl Drop for ProgramStore {
    fn drop(&mut self) {
        let Some(diagnostics) = &self.js_jit_diagnostics else {
            return;
        };
        let mut records: Vec<_> = diagnostics.iter().collect();
        records.sort_unstable_by_key(|(key, _)| **key);
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        for ((program, function), counters) in records {
            let entry = self
                .programs
                .get(*program as usize)
                .and_then(Option::as_ref);
            let status = entry
                .and_then(|entry| entry.jit_tiers.js(*function))
                .map(|tier| match tier {
                    JsTierState::Hot { .. } => "hot",
                    JsTierState::PromotionPending => "promotion-pending",
                    JsTierState::GeneralPromotionPending { .. } => "general-promotion-pending",
                    JsTierState::GeneralCompileDue => "general-compile-due",
                    JsTierState::Rejected { .. } => "unsupported",
                    JsTierState::Unsupported => "unsupported",
                    JsTierState::Compiled(_) => "compiled",
                })
                .unwrap_or(if counters.lane_entries != 0 {
                    "lane-only"
                } else {
                    "reclaimed"
                });
            let function_name = entry
                .and_then(|entry| entry.residual.functions.get(*function as usize).map(|f| (entry, f)))
                .and_then(|(entry, function)| {
                    let header = function.header();
                    let atom = header.name.or(header.inferred_name)? as usize;
                    (atom < entry.residual.atoms.len()).then(|| &entry.residual.atoms[atom])
                })
                .or_else(|| counters.function_name.as_deref())
                .unwrap_or("");
            let source_name = entry
                .map(|entry| entry.residual.source_name.as_str())
                .or_else(|| counters.source_name.as_deref())
                .unwrap_or("");
            let native_entry_coverage_ppm = if counters.entries == 0 {
                0
            } else {
                (u128::from(counters.native_entries) * 1_000_000
                    / u128::from(counters.entries))
                    .min(1_000_000) as u64
            };
            let total_entries = counters.entries.saturating_add(counters.lane_entries);
            let all_entry_native_coverage_ppm = if total_entries == 0 {
                0
            } else {
                (u128::from(counters.native_entries) * 1_000_000
                    / u128::from(total_entries))
                .min(1_000_000) as u64
            };
            let osr = entry
                .and_then(|entry| entry.lane_views.derived(*function))
                .map(super::dispatch_fast::LaneView::osr_diagnostics)
                .unwrap_or_default();
            let _ = write!(
                stderr,
                "QUENCH_JS_JIT_STATS {{\"program\":{program},\"function\":{function},\"function_name\":"
            );
            let _ = write_json_string(&mut stderr, function_name);
            let _ = write!(stderr, ",\"source_name\":");
            let _ = write_json_string(&mut stderr, source_name);
            let _ = write!(stderr, ",\"rejection_reason\":");
            if let Some(reason) = counters.rejection_reason {
                let _ = write_json_string(&mut stderr, reason);
            } else {
                let _ = write!(stderr, "null");
            }
            let _ = write!(
                stderr,
                ",\"status\":\"{status}\",\"parameter_count\":{},\"instruction_count\":{},\"entries\":{},\"lane_entries\":{},\"numeric_entries\":{},\"promotions\":{},\"compile_attempts\":{},\"compile_successes\":{},\"compile_ns\":{},\"native_entries\":{},\"native_entry_coverage_ppm\":{},\"all_entry_native_coverage_ppm\":{},\"field_cache_probes\":{},\"field_cache_hits\":{},\"field_store_cache_probes\":{},\"field_store_cache_hits\":{},\"field_return_candidate_entries\":{},\"field_return_fixed_this_entries\":{},\"field_return_fast_entries\":{},\"field_increment_candidate_entries\":{},\"field_increment_fixed_this_entries\":{},\"field_increment_fast_entries\":{},\"numeric_find_fast_entries\":{},\"fallback_entries\":{},\"interpreter_completions\":{},\"interpreter_inclusive_ns\":{},\"guard_census\":{{\"conditional_branch_sites\":{},\"typeof_operations\":{},\"strict_comparison_sites\":{},\"relational_comparison_sites\":{}}},\"osr\":{{\"plain_backward_jump_sites\":{},\"admitted_sites\":{},\"compile_attempts\":{},\"compile_successes\":{},\"currently_compiled_sites\":{},\"native_entry_attempts\":{},\"native_region_runs\":{},\"entry_guard_misses\":{},\"exits\":{},\"widenings\":{}}},\"opcodes\":[",
                counters.parameter_count,
                counters.instruction_count,
                counters.entries,
                counters.lane_entries,
                counters.numeric_entries,
                counters.promotions,
                counters.compile_attempts,
                counters.compile_successes,
                counters.compile_ns,
                counters.native_entries,
                native_entry_coverage_ppm,
                all_entry_native_coverage_ppm,
                counters.field_cache_probes,
                counters.field_cache_hits,
                counters.field_store_cache_probes,
                counters.field_store_cache_hits,
                counters.field_return_candidate_entries,
                counters.field_return_fixed_this_entries,
                counters.field_return_fast_entries,
                counters.field_increment_candidate_entries,
                counters.field_increment_fixed_this_entries,
                counters.field_increment_fast_entries,
                counters.numeric_find_fast_entries,
                counters.fallback_entries,
                counters.interpreter_completions,
                counters.interpreter_inclusive_ns,
                counters.guard_conditional_branches,
                counters.guard_typeof_ops,
                counters.guard_strict_comparisons,
                counters.guard_relational_comparisons,
                osr.plain_backward_jump_sites,
                osr.admitted_sites,
                osr.compile_attempts,
                osr.compile_successes,
                osr.currently_compiled_sites,
                osr.native_entry_attempts,
                osr.native_region_runs,
                osr.entry_guard_misses,
                osr.exits,
                osr.widenings,
            );
            for (index, (opcode, count)) in counters.opcode_counts.iter().enumerate() {
                if index != 0 {
                    let _ = write!(stderr, ",");
                }
                let name = crate::bytecode::Op::NAMES
                    .get(usize::from(*opcode))
                    .copied()
                    .unwrap_or("unknown");
                let _ = write!(stderr, "[");
                let _ = write_json_string(&mut stderr, name);
                let _ = write!(stderr, ",{count}]");
            }
            let _ = write!(stderr, "],\"general_dispatch_opcodes\":[");
            for (index, (opcode, count)) in counters
                .general_dispatch_opcode_counts
                .iter()
                .enumerate()
            {
                if index != 0 {
                    let _ = write!(stderr, ",");
                }
                let name = crate::bytecode::Op::NAMES
                    .get(usize::from(*opcode))
                    .copied()
                    .unwrap_or("unknown");
                let _ = write!(stderr, "[");
                let _ = write_json_string(&mut stderr, name);
                let _ = write!(stderr, ",{count}]");
            }
            let _ = write!(stderr, "],\"osr_analysis_rejections\":[");
            for (index, (reason, count)) in osr.analysis_rejections.iter().enumerate() {
                if index != 0 {
                    let _ = write!(stderr, ",");
                }
                let name = match reason {
                    super::compiled::RegionRejection::RegionTooLarge => "region-too-large",
                    super::compiled::RegionRejection::UnsupportedInstruction(op) => {
                        crate::bytecode::Op::NAMES
                            .get(*op as usize)
                            .copied()
                            .unwrap_or("unknown-opcode")
                    }
                    super::compiled::RegionRejection::RegisterLimit => "register-limit",
                    super::compiled::RegionRejection::CaptureLimit => "capture-limit",
                    super::compiled::RegionRejection::ArrayLimit => "array-limit",
                    super::compiled::RegionRejection::FieldLimit => "field-limit",
                    super::compiled::RegionRejection::FieldMutation => "field-mutation",
                };
                let _ = write!(stderr, "[");
                let _ = write_json_string(&mut stderr, name);
                let _ = write!(stderr, ",{count}]");
            }
            let _ = writeln!(
                stderr,
                "],\"code_bytes\":{},\"guard_census_kind\":\"bytecode-sites-not-proven-redundancies\",\"dynamic_opcode_count_scope\":\"general-dispatch-only; fast-lane-and-native-instructions-excluded\"}}",
                counters.code_bytes
            );
        }
        let _ = writeln!(
            stderr,
            "QUENCH_JS_JIT_DATAVIEW_FLOAT64 {{\"enabled\":{},\"get_entries\":{},\"set_entries\":{}}}",
            self.js_jit_dataview_float64_enabled,
            self.js_jit_dataview_float64_get_entries,
            self.js_jit_dataview_float64_set_entries
        );
        let _ = writeln!(
            stderr,
            "QUENCH_JS_JIT_TIER_TRACKING {{\"tracked_functions\":{},\"tracking_limit\":{},\"exhaustion_events\":{}}}",
            self.js_jit_tracked_functions,
            JS_JIT_MAX_TRACKED_FUNCTIONS,
            self.js_jit_tracking_exhaustions
        );
    }
}

fn write_json_string(output: &mut impl std::io::Write, value: &str) -> std::io::Result<()> {
    write!(output, "\"")?;
    for character in value.chars() {
        match character {
            '"' => write!(output, "\\\"")?,
            '\\' => write!(output, "\\\\")?,
            '\n' => write!(output, "\\n")?,
            '\r' => write!(output, "\\r")?,
            '\t' => write!(output, "\\t")?,
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", character as u32)?
            }
            character => write!(output, "{character}")?,
        }
    }
    write!(output, "\"")
}

fn numeric_promotion_decision(numeric: u8, observations: u8) -> Option<bool> {
    let minimum = JS_JIT_PROMOTION_LOOKS
        .iter()
        .find_map(|(look, minimum)| (*look == observations).then_some(*minimum))?;
    Some(numeric >= minimum)
}

/// The label the `profile-lane-exits` function census gives `function` of `entry`'s program.
#[cfg(feature = "profile-lane-exits")]
fn census_label(entry: &ProgramEntry, function: u32) -> String {
    let code = &entry.residual;
    let body = code.function_get(function as usize);
    format!(
        "{} f{function} {} @{:?}",
        code.source_name,
        body.and_then(|body| body.name).map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
        body.and_then(|body| body.source_span).map(|span| span.start),
    )
}

/// The runs a function makes on the general path before it derives its lane view, which costs
/// about a thousand instructions per bytecode instruction, so a function that runs fewer times
/// costs no view. Measured in instructions against deriving at the second run: four takes 3.3%
/// from the jiti npm scenario (Babel's many functions that run a few times) and 0.1-0.6% from
/// semver, ajv, Babel, color-convert and typescript-eslint; eight adds at most 0.3%.
const LANE_VIEW_DEFERRED_RUNS: u8 = 4;

/// Count one view-less run of a function; true once its deferred runs are complete.
fn deferred_run_completes(runs: &std::cell::Cell<u8>) -> bool {
    let done = runs.get();
    if done >= LANE_VIEW_DEFERRED_RUNS {
        return true;
    }
    runs.set(done + 1);
    false
}

/// A program's lane views by function, derived on first use (`ProgramStore::entry_lane_view`).
/// The table itself is made on the program's first lane use: most programs a process restores
/// or loads never enter the lane, and a table zeroed for each of their functions cost a
/// restored process about 54 fresh pages (2,493 functions at 88 bytes).
#[derive(Default)]
pub(super) struct LaneViews(std::cell::OnceCell<Box<[std::cell::OnceCell<super::dispatch_fast::LaneView>]>>);

impl LaneViews {
    /// The table, one cell per function of `residual`. Its address is fixed once made, as
    /// the lane context's pointer to it requires.
    fn of(&self, residual: &ResidualProgram) -> &[std::cell::OnceCell<super::dispatch_fast::LaneView>] {
        self.0.get_or_init(|| residual.functions.iter().map(|_| std::cell::OnceCell::new()).collect())
    }

    /// `function`'s view if it has one, without making the table.
    fn derived(&self, function: u32) -> Option<&super::dispatch_fast::LaneView> {
        self.0.get()?.get(function as usize)?.get()
    }
}

#[cfg(test)]
mod general_promotion_tests {
    use super::general_promotion_due;

    #[test]
    fn a_large_body_pays_after_enough_entries_and_a_small_one_never() {
        // 400 instructions: each entry saves 400/4 - 32 = 68 interpreted instructions
        // against 16,000 + 1,200 x 400 for the compile, so 7,295 entries pay.
        assert!(!general_promotion_due(7_294, 400));
        assert!(general_promotion_due(7_295, 400));
        // Below 4 x 32 instructions an entry saves nothing.
        assert!(!general_promotion_due(u32::MAX, 128));
        assert!(!general_promotion_due(u32::MAX, 0));
    }
}

#[cfg(test)]
mod jit_code_cache_tests {
    use super::{
        JIT_MAX_ARTIFACTS, JIT_MAX_MAPPED_BYTES, JS_JIT_MAX_FUNCTIONS, WASM_JIT_MAX_BASELINE_MAPPED_BYTES,
        JS_JIT_MAX_MAPPED_BYTES, JitArtifactKind, JitCodeCacheBudget,
        JsTierState,
        WASM_JIT_MAX_TIER_MAPPED_BYTES, WASM_JIT_MAX_TIER_ROOTS,
    };

    #[test]
    fn general_dispatch_backedge_unpriced_mode_waits_for_work_floor() {
        let mut state = JsTierState::Hot { observations: 1, numeric: 0 };
        let mut work = 0;
        super::accumulate_general_dispatch_work(&mut work, 31 * 64);
        assert!(!super::promote_general_dispatch_if_paid(&mut state, work, 64, false));
        assert!(matches!(state, JsTierState::Hot { .. }));
        super::accumulate_general_dispatch_work(&mut work, 64);
        assert!(super::promote_general_dispatch_if_paid(&mut state, work, 64, false));
        assert!(matches!(state, JsTierState::GeneralCompileDue));
    }

    #[test]
    fn general_dispatch_backedge_priced_mode_uses_existing_compile_cost() {
        let mut state = JsTierState::Rejected { entries: 8, reason: "test" };
        let mut work = 0;
        // 100 instructions cost the fixed and per-instruction compile cost, which the
        // saved share of observed bytecode work must cover.
        let (saved, of) = super::JS_JIT_GENERAL_SAVED_SHARE;
        let cost = super::JS_JIT_GENERAL_COMPILE_FIXED_COST + super::JS_JIT_GENERAL_COMPILE_COST_PER_INSTRUCTION * 100;
        let paid = usize::try_from(cost * of / saved).unwrap();
        super::accumulate_general_dispatch_work(&mut work, paid - 1);
        assert!(!super::promote_general_dispatch_if_paid(&mut state, work, 100, true));
        super::accumulate_general_dispatch_work(&mut work, 1);
        assert!(super::promote_general_dispatch_if_paid(&mut state, work, 100, true));
        assert!(matches!(state, JsTierState::GeneralCompileDue));
    }

    #[test]
    fn general_dispatch_backedge_work_saturates_and_promotes_once() {
        let mut state = JsTierState::GeneralPromotionPending { entries: 1 };
        let mut work = u64::MAX - 3;
        super::accumulate_general_dispatch_work(&mut work, 10);
        assert_eq!(work, u64::MAX);
        assert!(super::promote_general_dispatch_if_paid(&mut state, work, 1, false));
        super::accumulate_general_dispatch_work(&mut work, usize::MAX);
        assert_eq!(work, u64::MAX);
        assert!(!super::promote_general_dispatch_if_paid(&mut state, work, 1, false));
        assert!(matches!(state, JsTierState::GeneralCompileDue));
    }

    #[test]
    fn general_dispatch_backedge_is_disabled_without_opt_in() {
        let mut store = super::ProgramStore::default();
        store.js_jit_general_dispatch_enabled = false;
        assert!(!store.js_jit_general_dispatch_enabled());
        assert!(!store.note_js_general_dispatch_backedge(
            super::ProgramId::MAIN,
            1,
            usize::MAX,
            usize::MAX,
        ));
        assert!(store.js_general_dispatch_work.is_none());
    }

    #[test]
    fn shared_jit_cache_budget_tracks_mixed_artifacts() {
        let mut cache = JitCodeCacheBudget::default();
        assert!(cache.retain(JitArtifactKind::JsFunction, 128 * 1024));
        assert!(cache.retain(JitArtifactKind::WasmTierRoot, 256 * 1024));
        assert_eq!(cache.artifacts, 2);
        assert_eq!(cache.mapped_bytes, 384 * 1024);

        cache.release(JitArtifactKind::WasmTierRoot, 1, 256 * 1024);
        assert_eq!(cache.artifacts, 1);
        assert_eq!(cache.mapped_bytes, 128 * 1024);

        // An eager module artifact larger than the shared ceiling is admitted against its
        // own, and leaves the tiers' allowance as it was.
        assert!(cache.retain(JitArtifactKind::BaselineWasm, JIT_MAX_MAPPED_BYTES + 1));
        assert_eq!((cache.artifacts, cache.mapped_bytes), (1, 128 * 1024));
        assert!(cache.can_retain(JitArtifactKind::JsFunction, 128 * 1024));
        assert!(!cache.can_retain(
            JitArtifactKind::BaselineWasm,
            WASM_JIT_MAX_BASELINE_MAPPED_BYTES - JIT_MAX_MAPPED_BYTES
        ));
        cache.release(JitArtifactKind::BaselineWasm, 1, JIT_MAX_MAPPED_BYTES + 1);
        assert!(cache.can_retain(JitArtifactKind::BaselineWasm, WASM_JIT_MAX_BASELINE_MAPPED_BYTES));
    }

    #[test]
    fn shared_jit_cache_enforces_each_tier_limit() {
        let mut cache = JitCodeCacheBudget::default();
        for _ in 0..JS_JIT_MAX_FUNCTIONS {
            assert!(cache.retain(JitArtifactKind::JsFunction, 0));
        }
        assert!(!cache.can_retain(JitArtifactKind::JsFunction, 0));
        assert!(!cache.can_retain(JitArtifactKind::JsFunction, JS_JIT_MAX_MAPPED_BYTES + 1));

        for _ in 0..WASM_JIT_MAX_TIER_ROOTS {
            assert!(cache.retain(JitArtifactKind::WasmTierRoot, 0));
        }
        assert!(!cache.can_retain(JitArtifactKind::WasmTierRoot, 0));
        assert!(!cache.can_retain(
            JitArtifactKind::WasmTierRoot,
            WASM_JIT_MAX_TIER_MAPPED_BYTES + 1
        ));
        assert_eq!(cache.artifacts, JIT_MAX_ARTIFACTS);
        // Full tiers do not hold back an eager module artifact.
        assert!(cache.can_retain(JitArtifactKind::BaselineWasm, 0));
    }

    #[test]
    fn jit_tier_manager_rejects_cross_frontend_state_for_one_program() {
        let mut tiers = super::JitTierManager::default();
        assert!(tiers.js_insert(3, super::JsTierState::Unsupported));
        assert!(tiers.wasm_mut_or_default(3).is_none());
        assert!(tiers.wasm_mut_or_default(4).is_none());
        assert!(matches!(tiers.js(3), Some(super::JsTierState::Unsupported)));
        assert_eq!(tiers.js_values().count(), 1);
        assert_eq!(tiers.wasm_values().count(), 0);

        let mut wasm_tiers = super::JitTierManager::default();
        assert!(wasm_tiers.wasm_mut_or_default(4).is_some());
        assert!(!wasm_tiers.js_insert(3, super::JsTierState::Unsupported));
        assert_eq!(wasm_tiers.wasm_values().count(), 1);
        assert_eq!(
            wasm_tiers.note_js_entry(3, true, true, false, 0, true),
            super::JsTierEntryDecision::default()
        );
    }

    #[test]
    fn jit_tier_manager_owns_frontend_promotion_transitions() {
        let mut js_tiers = super::JitTierManager::default();
        let first = js_tiers.note_js_entry(1, true, true, false, 0, true);
        assert_eq!(
            first,
            super::JsTierEntryDecision {
                tracked: true,
                promoted: true,
                compile: true,
            }
        );
        assert!(matches!(
            js_tiers.js(1),
            Some(super::JsTierState::PromotionPending)
        ));

        let mut wasm_tiers = super::JitTierManager::default();
        for _ in 1..super::WASM_JIT_PROMOTION_ENTRIES {
            assert!(!wasm_tiers.note_wasm_entry(2, false));
        }
        assert!(wasm_tiers.note_wasm_entry(2, false));
        assert!(!wasm_tiers.note_wasm_entry(2, false));
        assert_eq!(
            wasm_tiers.wasm(2).map(|state| state.hotness & 0x7f),
            Some(super::WASM_JIT_PROMOTION_ENTRIES)
        );
    }
}

/// A program's `JsEntryFacts` by function, the table made on the tier's first entry of one.
#[derive(Default)]
pub(super) struct JsEntryFactsTable(
    std::cell::OnceCell<Box<[std::cell::OnceCell<Rc<super::js_compiled::JsEntryFacts>>]>>,
);
