use crate::vm::EngineLaneView;
use crate::vm::tier_policy::{TierRows, Tiers};
use super::wtf16::JsString;
use crate::Value;
use crate::bytecode::ResidualProgram;
use std::rc::{Rc, Weak};
use crate::heap::{PromotionAge, WeakHandle};
use rustc_hash::FxHashMap;
use residual::{EncodedResidual, Residual};

pub mod lane_publication;
mod chunks;
mod eval_cache;
mod js_jit_report;
mod loops;
mod residual;
mod root_constants;
mod snapshot;
mod tier_census;
mod wasm_tier;
pub(super) use wasm_tier::WasmTierState;

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
    native_entries: u64,
    field_cache_probes: u64,
    field_cache_hits: u64,
    field_cache_sites: FxHashMap<u16, (u64, u64)>,
    field_store_cache_probes: u64,
    field_store_cache_hits: u64,
    field_return_candidate_entries: u64,
    field_return_fixed_this_entries: u64,
    field_return_fast_entries: u64,
    field_increment_candidate_entries: u64,
    field_increment_fixed_this_entries: u64,
    field_increment_fast_entries: u64,
    captured_store_fast_entries: u64,
    numeric_find_fast_entries: u64,
    fallback_entries: u64,
    opcode_counts: Vec<(u16, u32)>,
    call_sites: Vec<JsJitCallSiteCounters>,
    code_bytes: usize,
}

#[derive(Default)]
struct JsJitCallSiteCounters {
    pc: u32,
    opcode: u16,
    attempts: u64,
    direct_target: Option<(u32, u32)>,
    direct_target_calls: u64,
    alternate_target_calls: u64,
    other_target_calls: u64,
    outcomes: [u64; JsJitCallOutcome::COUNT],
}

#[cfg(feature = "native-execution")]
impl JsJitCallSiteCounters {
    /// Counts one call by where it went: the site's first direct target, a later call to it,
    /// a call to another direct target, or a target that is not a direct callee.
    fn note_target(&mut self, direct_target: Option<(u32, u32)>) {
        match direct_target {
            Some(target) => match self.direct_target {
                None => {
                    self.direct_target = Some(target);
                    self.direct_target_calls = self.direct_target_calls.saturating_add(1);
                }
                Some(first) if first == target => {
                    self.direct_target_calls = self.direct_target_calls.saturating_add(1);
                }
                Some(_) => {
                    self.alternate_target_calls = self.alternate_target_calls.saturating_add(1);
                }
            },
            None => {
                self.other_target_calls = self.other_target_calls.saturating_add(1);
            }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum JsJitCallOutcome {
    Continue = 0,
    TailCall = 1,
    PushFrame = 2,
    Return = 3,
    Await = 4,
    Yield = 5,
    Error = 6,
}

impl JsJitCallOutcome {
    const COUNT: usize = Self::Error as usize + 1;

    const ALL: [Self; Self::COUNT] = [
        Self::Continue,
        Self::TailCall,
        Self::PushFrame,
        Self::Return,
        Self::Await,
        Self::Yield,
        Self::Error,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::TailCall => "tail_call",
            Self::PushFrame => "push_frame",
            Self::Return => "return",
            Self::Await => "await",
            Self::Yield => "yield",
            Self::Error => "error",
        }
    }
}

use crate::vm::tier_policy::{JS_JIT_MAX_FUNCTIONS, JS_JIT_MAX_MAPPED_BYTES, WASM_JIT_MAX_TIER_ROOTS, WASM_JIT_MAX_TIER_MAPPED_BYTES, JIT_MAX_ARTIFACTS, JIT_MAX_MAPPED_BYTES, LANE_VIEW_DEFERRED_RUNS};
// Tier observations include dynamically generated functions across all retained
// programs. Web-SSR exposes more than 1,300 functions in one VM; keep a fixed
// metadata bound large enough to observe that workload without changing the
// independent compiled-artifact or executable-memory budgets.
const JS_JIT_MAX_TRACKED_FUNCTIONS: usize = 2048;
const JS_JIT_MAX_DIAGNOSTIC_FUNCTIONS: usize = 4096;
const JS_JIT_MAX_REJECTED_FUNCTIONS: usize = 4096;
const JS_JIT_REJECTION_DIAGNOSTIC_ENTRIES: u64 = 32;
/// The least an artifact holds in the code zone.
const JIT_MIN_ARTIFACT_BYTES: usize = crate::vm::engine_types::CODE_GRANULE;

#[derive(Clone, Copy)]
enum JitArtifactKind {
    JsFunction,
    WasmTierRoot,
}

/// One VM-local executable-code budget for the JavaScript and Wasm root artifacts. Tier-kind
/// limits and the JS and Wasm tiers' aggregate capacity are admitted and accounted together here.
#[derive(Default)]
struct JitCodeCacheBudget {
    /// The JavaScript and Wasm tier artifacts, which share `JIT_MAX_ARTIFACTS` and
    /// `JIT_MAX_MAPPED_BYTES`.
    artifacts: usize,
    mapped_bytes: usize,
    js_functions: usize,
    js_mapped_bytes: usize,
    wasm_tier_roots: usize,
    wasm_tier_mapped_bytes: usize,
}

impl JitCodeCacheBudget {
    fn can_retain(&self, kind: JitArtifactKind, mapped_bytes: usize) -> bool {
        let (count, bytes, max_count, max_bytes) = match kind {
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
        self.artifacts += 1;
        self.mapped_bytes += mapped_bytes;
        match kind {
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
        self.artifacts = self.artifacts.saturating_sub(artifacts);
        self.mapped_bytes = self.mapped_bytes.saturating_sub(mapped_bytes);
        match kind {
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
pub enum JsTierState {
    Rejected {
        entries: u64,
        reason: &'static str,
    },
    /// Terminal: no tier compiles the function again. Its rejections are deterministic (the same
    /// bytecode lowers to the same code), so a retry only repeats the compile.
    Unsupported,
    Compiled(Rc<crate::vm::engine_types::CompiledJsFunction>),
}

/// One sparse tier-state table per program. Frontend-specific transitions and
/// artifacts stay typed, while JS and Wasm share the same state owner and
/// avoid carrying a second empty hash-map header in every program entry.
#[derive(Default)]
enum JitTierManager {
    #[default]
    Empty,
    JavaScript(FxHashMap<u32, JsTierState>),
    Wasm(WasmTiers),
}

/// A Wasm program's tier state: each function's compile attempt and its root artifacts.
#[derive(Default)]
struct WasmTiers {
    functions: FxHashMap<u32, WasmTierState>,
    /// The root whose artifact holds each function one holds: a function's own, else the first
    /// installed. A call finds its code without visiting every artifact.
    holders: FxHashMap<u32, u32>,
}

impl WasmTiers {
    /// Keeps `root`'s artifact, the holder of each function it holds that none held.
    fn install(&mut self, root: u32, compiled: Rc<crate::vm::engine_types::CompiledProgram>) {
        for function in compiled.held_functions() { self.holders.entry(function as u32).or_insert(root); }
        self.holders.insert(root, root);
        self.functions.insert(root, WasmTierState::Compiled(compiled));
    }
}

impl JitTierManager {
    /// Whether an entry of `function` compiles it now: the model picked a compiled tier for it
    /// (`predicted`), the entry is numeric (the code is specialized to numbers), and no state
    /// yet records the function. Compiles are made on the first such entry; no entry counts.
    fn note_js_entry(&mut self, function: u32, numeric: bool, predicted: bool) -> bool {
        !self.is_wasm() && self.js(function).is_none() && predicted && numeric
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
            Self::Wasm(tiers) => tiers.functions.get(&function),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    /// The root artifact holding `function` (`WasmTiers::holders`).
    fn wasm_holding(&self, function: u32) -> Option<&Rc<crate::vm::engine_types::CompiledProgram>> {
        let Self::Wasm(tiers) = self else { return None };
        tiers.functions.get(tiers.holders.get(&function)?)?.compiled()
    }

    fn wasm_values(&self) -> impl Iterator<Item = &WasmTierState> {
        let functions = match self {
            Self::Wasm(tiers) => Some(&tiers.functions),
            Self::Empty | Self::JavaScript(_) => None,
        };
        functions.into_iter().flat_map(|functions| functions.values())
    }

    fn wasm_tiers_mut(&mut self) -> Option<&mut WasmTiers> {
        if matches!(self, Self::Empty) {
            *self = Self::Wasm(WasmTiers::default());
        }
        match self {
            Self::Wasm(tiers) => Some(tiers),
            Self::Empty | Self::JavaScript(_) => None,
        }
    }

    /// Whether `function` has no compile attempt yet: a function with no state has not been attempted.
    fn wasm_unattempted(&self, function: u32) -> bool {
        match self {
            Self::Wasm(tiers) => !tiers.functions.contains_key(&function),
            Self::Empty => true,
            Self::JavaScript(_) => false,
        }
    }

    /// Claims `function`'s one compile attempt, where it is unattempted (`wasm_unattempted`). A claimed
    /// attempt is never repeated, whether or not it compiled.
    fn claim_wasm_root(&mut self, function: u32) -> bool {
        if !self.wasm_unattempted(function) {
            return false;
        }
        let Some(tiers) = self.wasm_tiers_mut() else {
            return false;
        };
        tiers.functions.insert(function, WasmTierState::Attempted);
        true
    }

    fn is_javascript(&self) -> bool {
        matches!(self, Self::JavaScript(_))
    }

    fn is_wasm(&self) -> bool {
        matches!(self, Self::Wasm(_))
    }
}

/// The one object a callee-argument RegExp literal site shares (`Vm::regexp_literal_argument_of`),
/// and what keeps it unobservable: it is answered again only in `frame`, the frame of the
/// evaluation that made it (`lane_allocations::regexp_argument_key`), while the current realm is
/// `realm`, `watch` (on the values of `RegExp.prototype`) is published and the `@@species` fuse of
/// `constructor` holds.
pub(crate) struct RegExpArgumentShare {
    pub(crate) object: Value,
    pub(crate) frame: usize,
    pub(crate) realm: Value,
    pub(crate) constructor: Value,
    pub(crate) watch: crate::heap::PrototypeValueWatch,
}

/// Whether a planned arm enters its function `function` of the program `code` as a framed method call with `argc`
/// arguments (`planned_call_sites`): the derivation's check on the callee's body, which the engine decides.
pub type PlannedArmAdmission = fn(&ResidualProgram, u32, u16) -> bool;

struct ProgramEntry {
    residual: Residual,
    wasm_signatures: Option<Rc<crate::wasm::WasmSignatures>>,
    /// Its materialized constants, with room for those its first-call chunks append, which
    /// never move them (`AppendVec`).
    constants: crate::bytecode::append_vec::AppendVec<Value>,
    /// The elements of the constant array literals that ran, by their first constant: few
    /// programs run any, so a table dense over every constant touched a page per 256 of them
    /// for nothing (an indirect eval of a 2 MB bundle, every load).
    const_arrays: FxHashMap<u32, crate::heap::ArrayElements>,
    /// The source texts `Function.prototype.toString` made, by function: few functions are
    /// printed, so the table holds those alone, not a slot for each function and its room.
    function_sources: FxHashMap<u32, Value>,
    /// Each RegExp literal site's record, shared by every object it evaluates to.
    regexp_literals: Vec<Option<Rc<crate::heap::RegExpMeta>>>,
    /// The one object each receiver-only literal site evaluates to while it cannot escape
    /// (`RegExpLiteralSite::receiver`, `Vm::regexp_literal_receiver`).
    regexp_receivers: FxHashMap<u32, Value>,
    /// The one object each callee-argument literal site shares while it cannot escape
    /// (`Vm::regexp_literal_argument_of`).
    regexp_arguments: FxHashMap<u32, RegExpArgumentShare>,
    module_environment: Option<Value>,
    import_meta: Option<Value>,
    module_imports: Vec<(u16, ModuleImport)>,
    /// Each root slot's rule, derived from the residual and `module_imports` on first read.
    root_slots: std::cell::OnceCell<Box<[crate::bytecode::RootSlotRule]>>,
    /// Whether every root slot is the binding itself (`ProgramStore::root_slots_are_bindings`),
    /// derived with `root_slots`.
    root_bindings: std::cell::OnceCell<bool>,
    /// Its root slots written once, the read records that keep their values, and how many
    /// runs its root has made.
    root_constants: root_constants::RootConstants,
    module: bool,
    /// Per-function lane views, derived from the residual on first lane entry.
    views_in_lane: LaneViews,
    /// Per Wasm function a collection found on the stack: the live registers holding raw
    /// words at each pc (`raw_webs::raw_live`), `None` for a function with no raw op.
    raw_masks: std::cell::RefCell<FxHashMap<u32, Option<Rc<crate::compile::liveness::RegisterMasks>>>>,
    /// Per function: what the JavaScript tier found at its entries (`js_entry_facts`).
    js_entry_facts: JsEntryFactsTable,
    /// The program's AOT facts, solved once on the first lowering that reads them (`aot_facts`).
    aot_facts: std::cell::OnceCell<std::sync::Arc<crate::compile::aot_facts::AotFacts>>,
    /// The program's call graph and its loop-or-cycle facts, solved once beside `aot_facts` (`call_graph`).
    call_graph: std::cell::OnceCell<std::sync::Arc<crate::compile::call_graph::CallGraph>>,
    /// Per function, its predicted tier as a call of its body from a loop's region or not (`[called outside a loop,
    /// called by a loop]`), solved once beside `call_graph` (`predicted_tiers`).
    predicted_tiers: std::cell::OnceCell<std::sync::Arc<[[crate::vm::tier_policy::PredictedTier; 2]]>>,
    /// The program's method selectors (`compile::call_plan::Selectors`), solved once beside `call_graph` (`selectors`).
    selectors: std::cell::OnceCell<std::sync::Arc<crate::compile::call_plan::Selectors>>,
    /// Per class layout: the method vector of the class's `CallRecord` addresses by selector, allocated on first write
    /// (`set_class_method`), each of the selectors' count; `layouts::set_class_methods` holds its address.
    class_method_vectors: std::cell::RefCell<FxHashMap<u32, Box<[usize]>>>,
    /// Per class (a constructor's function id): the layout id its constructed instances hold, 0 until the first
    /// construction records it (`Vm::record_class_header`). Allocated on first use (`class_header_cell`).
    class_headers: std::cell::OnceCell<Box<[std::cell::Cell<u32>]>>,
    /// Per recorded class: the prototype its first instance had, which the class-header edge resolves the planned arms
    /// from (`Vm::record_class_edge`). It is the snapshot's record of the class, and a root while the program lives.
    class_prototypes: std::cell::RefCell<std::collections::BTreeMap<u32, Value>>,
    /// The planned arms' targets by (class, method, function), each boxed so its address is the arm's for the program's
    /// life (`planned_target`). Entries are added, never removed.
    planned_targets: std::cell::RefCell<std::collections::BTreeMap<(u32, u32, u32), Box<crate::vm::engine_types::PlannedTarget>>>,
    /// Whether every function's planned sites have registered their arms' targets (`register_planned_targets`).
    planned_registered: std::cell::Cell<bool>,
    /// Per function: the runs it has made without its lane view (`entry_lane_view`,
    /// `call_may_derive_in_lane`), up to `LANE_VIEW_DEFERRED_RUNS`.
    lane_deferred: Vec<std::cell::Cell<u8>>,
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
    /// can run it, with the footprint the collector was charged for it
    /// (`ProgramFootprint`). The main program and modules stay for the realm's life.
    reclaimable: Option<usize>,
    /// The reclamation pass current at insertion.
    pass: u64,
    /// The minor collections that must still trace its root values, which changed since a
    /// collection last traced them: one per minor collection a young value survives before
    /// it is old (`PromotionAge`). A minor collection traces only such programs: what one
    /// promoted stays marked until the next full collection, which traces every program.
    minor_root_traces: u8,
}

pub(super) const ANONYMOUS_EVAL_ORIGIN: &str = "eval at <anonymous>";

#[derive(Clone)]
pub(super) enum CompilationOrigin {
    Source,
    /// Execution state in another realm of one logical host Script.
    SharedScript {
        source: ProgramId,
    },
    Eval {
        caller: Option<JsString>,
    },
    FunctionConstructor {
        caller: JsString,
    },
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
    /// A first-call chunk of a function of code whose names resolve through its environments:
    /// its own root declares nothing, and its by-name loads reach what its stub's do.
    Enclosed,
}

#[derive(Clone, Copy)]
pub enum ModuleImport {
    Value(Value),
    Binding(ProgramId, u16, Value),
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProgramId(u32);

impl ProgramId {
    pub const MAIN: Self = Self(0);

    fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    #[inline(always)]
    fn index(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

struct OwnedScript {
    owner: Weak<crate::api::CompiledScriptState>,
    instances: Vec<(WeakHandle, ProgramId)>,
}

pub struct ProgramStore {
    /// Weak native-call proofs: the published bodies that call callees, whose publications root those callees.
    native_call_bindings: std::cell::RefCell<Vec<lane_publication::NativeCallBinding>>,
    /// The bodies published by what they compile from (`lane_publication::LaneKey`).
    /// The compiles each body or loop entry has had (`lane_publication::LaneKey`'s first three).
    /// The IR size of the last lane plan from each entry of each function (`Vm::start_lane_method`):
    /// what deciding the next compile's worth from costs, before planning it again.
    lane_installed: std::cell::RefCell<std::collections::BTreeMap<lane_publication::LaneKey, Weak<crate::vm::engine_types::PublishedLaneMethod>>>,
    /// The ahead-of-time bodies the loaded files hold.
    ahead: std::cell::RefCell<Vec<Rc<crate::vm::engine_types::AheadImages>>>,
    /// Indexed by `ProgramId`; a reclaimed program leaves an empty slot, so an
    /// id never names another program.
    programs: Vec<Option<Box<ProgramEntry>>>,
    /// The classes a restored snapshot recorded (program, class, header, prototype), which `Vm::install_restored_main`
    /// records again at the class-header edge once the main program is installed; empty otherwise.
    restored_classes: Vec<(ProgramId, u32, u32, Value)>,
    /// The id of each program `insert_shared` holds, by its code's address: `find_shared` is one
    /// probe, not a scan of every program (the Wasm boundary asks it per call).
    shared_ids: FxHashMap<usize, ProgramId>,
    /// Advanced by every change to a program the store holds or to the set it holds, and by
    /// the VM's changes to cache layouts and tables (`bump_context_epoch`): a program context
    /// derived at another epoch is derived again (`Vm::program_context`).
    context_epoch: u64,
    /// Weak compiled-script identities for global eval; contexts are supplied per call.
    global_eval_programs: FxHashMap<super::eval::eval_source::EvalSource, ProgramId>,
    /// Compiled direct evals by text and the caller facts the compilation reads.
    direct_eval_programs: FxHashMap<super::eval::eval_source::DirectEvalKey, ProgramId>,
    /// Strict direct evals by text and caller facts: a strict eval's program is its own Script
    /// per evaluation (its eval origin is not shared), so a later eval of the same text makes a
    /// new program over the latest one's code (`eval_cache.rs`).
    strict_direct_eval_code: FxHashMap<super::eval::eval_source::DirectEvalKey, ProgramId>,
    owned_scripts: FxHashMap<ProgramId, OwnedScript>,
    /// Completed reclamation passes; see `reclaim_unreferenced`.
    reclaim_passes: u64,
    /// Reclaimable programs inserted since the last reclamation pass.
    unchecked_reclaimable: usize,
    /// `settled_reclaimable_bytes`, as the last reclamation pass found it.
    settled_reclaimable_bytes: usize,
    /// The entry facts every function shares whose facts are all unset
    /// (`JsEntryFacts::is_unset`): most functions the tier
    /// watches, which had each held a copy (V8-v7 Crypto `--jit`: 32 -> 21 KiB at the heap's peak).
    no_entry_facts: Rc<crate::vm::engine_types::JsEntryFacts>,
    /// VM-local bounds for the optional JavaScript tier cache.
    js_jit_tracked_functions: usize,
    js_jit_tracking_exhaustions: u64,
    js_jit_rejected_functions: usize,
    /// Shared JS/Wasm executable-code cache admission and accounting.
    jit_code_cache: JitCodeCacheBudget,
    /// Allocated only when opt-in JavaScript JIT diagnostics are requested.
    js_jit_diagnostics: Option<FxHashMap<(u32, u32), JsJitCounters>>,
    /// Whether the lane prologue specializes a declined function into a frameless lane
    /// body (`Vm::compile_lane_method`).
    js_jit_lane_methods_enabled: bool,
    /// The warming-entry clock a lane body's `recurrent` reads: one tick per entry a warming
    /// body counts, program-wide. A plain cell of the store the lane already reaches, where a
    /// thread-local read costs a `tlv_get_addr` call on every entry.
    warming_ticks: std::cell::Cell<u32>,
    #[cfg(any(test, feature = "test-support"))]
    js_entry_specializations_enabled: bool,
    js_jit_dataview_float64_get_entries: u64,
    js_jit_dataview_float64_set_entries: u64,
    /// What the tiers did in this VM (`Vm::tier_events`).
    tier_events: super::tier_events::TierEvents,
    /// The thresholds this VM's tier controllers read (`TierRows`), fixed at its construction.
    tiers: &'static Tiers,
}

impl Default for ProgramStore {
    fn default() -> Self {
        crate::jit_coverage::sample_this_thread();
        let tiers = TierRows::made();
        Self {
            native_call_bindings: Default::default(),
            lane_installed: Default::default(),
            ahead: Default::default(),
            programs: Vec::new(),
            shared_ids: FxHashMap::default(),
            // Each store's epochs start apart from every other's, so a context derived from
            // a store a snapshot or reset replaced never matches its successor's.
            context_epoch: {
                static STORES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                STORES.fetch_add(1 << 40, std::sync::atomic::Ordering::Relaxed)
            },
            global_eval_programs: FxHashMap::default(),
            direct_eval_programs: FxHashMap::default(),
            strict_direct_eval_code: FxHashMap::default(),
            owned_scripts: FxHashMap::default(),
            reclaim_passes: 0,
            unchecked_reclaimable: 0,
            settled_reclaimable_bytes: 0,
            no_entry_facts: Rc::default(),
            js_jit_tracked_functions: 0,
            js_jit_tracking_exhaustions: 0,
            js_jit_rejected_functions: 0,
            jit_code_cache: JitCodeCacheBudget::default(),
            // TOD-191's correctness oracle may force hot candidates past the payback model so
            // guarded exits can be tested independently of profitability (diagnostic only).
            js_jit_diagnostics: (crate::trace::Channel::JsJit.on() || crate::trace::Channel::Census.on()).then(FxHashMap::default),
            js_jit_lane_methods_enabled: false,
            warming_ticks: std::cell::Cell::new(0),
            #[cfg(any(test, feature = "test-support"))]
            js_entry_specializations_enabled: true,
            js_jit_dataview_float64_get_entries: 0,
            js_jit_dataview_float64_set_entries: 0,
            tier_events: Default::default(),
            tiers,
        }
    }
}

impl ProgramStore {
    /// What the JavaScript tier finds at the entries of `function` (`JsEntryFacts`), derived on
    /// its first entry.
    pub fn js_entry_facts(&self, id: ProgramId, function: u32) -> Option<Rc<crate::vm::engine_types::JsEntryFacts>> {
        let entry = self.entry(id)?;
        let body = entry.residual.function_get(function as usize)?;
        let cells = entry.js_entry_facts.0.get_or_init(|| zeroed_entry_facts(entry.residual.functions.capacity()));
        Some(Rc::clone(cells.get(function as usize)?.get_or_init(|| {
            let facts = crate::vm::engine_types::JsEntryFacts::of(&entry.residual, body, !cfg!(test)); // tests pin tiny fixtures
            #[cfg(any(test, feature = "test-support"))]
            let facts = if self.js_entry_specializations_enabled { facts } else { Default::default() };
            if facts.is_unset() {
                Rc::clone(&self.no_entry_facts)
            } else {
                Rc::new(facts)
            }
        })))
    }

    /// The AOT facts of program `id` (`compile::aot_facts::analyze`), solved on its first call and
    /// shared by every lowering of the program after it.
    pub fn aot_facts(&self, id: ProgramId) -> Option<std::sync::Arc<crate::compile::aot_facts::AotFacts>> {
        let entry = self.entry(id)?;
        Some(std::sync::Arc::clone(entry.aot_facts.get_or_init(|| std::sync::Arc::new(crate::compile::aot_facts::analyze(&entry.residual)))))
    }

    /// The method selectors of program `id` (`compile::call_plan::Selectors`), solved on its first call from its AOT facts.
    pub fn selectors(&self, id: ProgramId) -> Option<std::sync::Arc<crate::compile::call_plan::Selectors>> {
        let entry = self.entry(id)?;
        Some(std::sync::Arc::clone(entry.selectors.get_or_init(|| {
            let facts = self.aot_facts(id).expect("a program with an entry has AOT facts");
            std::sync::Arc::new(crate::compile::call_plan::Selectors::of(&entry.residual, &facts))
        })))
    }

    /// Writes `record` (a `CalleeRecord` address) as the entry of method `selector` in the method vector of class layout
    /// `layout` of program `id`, allocating the vector zeroed on its first write, and publishes its address to
    /// `layouts::set_class_methods`. Nothing is written where the program has no selectors.
    pub fn set_class_method(&self, id: ProgramId, layout: u32, selector: u32, record: usize) {
        let (Some(count), Some(entry)) = (self.selectors(id).map(|selectors| selectors.count()), self.entry(id)) else { return };
        let mut vectors = entry.class_method_vectors.borrow_mut();
        let vector = vectors.entry(layout).or_insert_with(|| vec![0_usize; count].into_boxed_slice());
        if let Some(slot) = vector.get_mut(selector as usize) {
            *slot = record;
        }
        crate::heap::layouts::set_class_methods(layout, vector.as_ptr() as usize);
    }

    /// The call graph of program `id` and its loop-or-cycle facts (`compile::call_graph::CallGraph`), solved on its first
    /// call from the program's AOT facts and shared by every lane body that reads them after it.
    pub fn call_graph(&self, id: ProgramId) -> Option<std::sync::Arc<crate::compile::call_graph::CallGraph>> {
        let entry = self.entry(id)?;
        Some(std::sync::Arc::clone(entry.call_graph.get_or_init(|| {
            let facts = self.aot_facts(id).expect("a program with an entry has AOT facts");
            std::sync::Arc::new(crate::compile::call_graph::CallGraph::of(&entry.residual, &facts))
        })))
    }

    /// The predicted tier of each function of program `id`, per whether a loop region calls it (`tier_policy::predicted_tier`),
    /// solved on its first call from `call_graph`. The one table the lane's tier picks read (`lane_facts::body_pick`).
    pub fn predicted_tiers(&self, id: ProgramId) -> Option<std::sync::Arc<[[crate::vm::tier_policy::PredictedTier; 2]]>> {
        let entry = self.entry(id)?;
        let graph = self.call_graph(id)?;
        Some(std::sync::Arc::clone(entry.predicted_tiers.get_or_init(|| {
            (0..entry.residual.functions.len() as u32)
                .map(|function| [false, true].map(|called_by_loop| crate::vm::tier_policy::predicted_tier(&graph, &entry.residual, function, called_by_loop)))
                .collect()
        })))
    }

    /// The header cell of class `class` (a class id, `AotFacts::constructed_classes`) in program `id`: the layout id its
    /// constructed instances hold, recorded once by the first construction (`Vm::record_class_header`), 0 until then. A
    /// synthetic class has no cell (`class_header_cell`), so it has no header.
    pub fn class_header(&self, id: ProgramId, class: u32) -> Option<&std::cell::Cell<u32>> {
        Self::class_header_cell(self.entry(id)?, class)
    }

    fn class_header_cell(entry: &ProgramEntry, class: u32) -> Option<&std::cell::Cell<u32>> {
        let headers = entry.class_headers.get_or_init(|| (0..entry.residual.functions.len()).map(|_| std::cell::Cell::new(0)).collect());
        headers.get(class as usize)
    }

    /// The planned call sites of `function` in program `id`: each call whose plan (`AotFacts::call_plans`) is a framed
    /// method call (`CallForm::Framed`) over a closed union of classes and misses on nothing (`Miss::None`), and which
    /// the facts admit. A site is admitted when every arm's class has a prototype that names the method (`prototype_layout`:
    /// an own data property of the prototype, no other writes), and `admits` (the callee's body enters a framed call
    /// with the site's argument count, `planned_calls::planned_arm_admits`) holds for each arm's function. Each arm has
    /// its class's header cell and its target slot (`planned_target`); the last class is the default arm. Plans of one
    /// callee (no class) keep their own rows. A site's record dispatches on its receiver's header (`planned_call_row`).
    pub fn planned_call_sites(&self, id: ProgramId, function: u32, admits: PlannedArmAdmission) -> Vec<crate::vm::engine_types::PlannedSite> {
        use crate::compile::call_plan::{CallForm, Miss};
        use crate::vm::engine_types::{PlannedArm, PlannedSite};
        let (Some(entry), Some(facts)) = (self.entry(id), self.aot_facts(id)) else { return Vec::new() };
        facts.call_plans(&entry.residual, function).into_iter()
            .filter(|(_, plan)| plan.miss == Miss::None && plan.form == CallForm::Framed && !plan.arms.is_empty())
            .filter_map(|(pc, plan)| {
                let method = plan.method?;
                let mut arms = Vec::with_capacity(plan.arms.len());
                for arm in plan.arms.iter() {
                    let class = arm.class?;
                    if !facts.prototype_layout(class)?.contains(&method) || !admits(&entry.residual, arm.function, plan.argc) {
                        return None;
                    }
                    let header = Self::class_header_cell(entry, class)?;
                    arms.push(PlannedArm { function: arm.function, header: std::ptr::from_ref(header), target: Self::planned_target(entry, class, arm.function, method) });
                }
                let default = arms.pop()?;
                Some(PlannedSite { pc, arms: arms.into_boxed_slice(), default })
            })
            .collect()
    }

    /// The slot of the target of class `class`'s arm `function` reading `method`, made on first use.
    fn planned_target(entry: &ProgramEntry, class: u32, function: u32, method: u32) -> *const crate::vm::engine_types::PlannedTarget {
        let mut targets = entry.planned_targets.borrow_mut();
        let slot = targets.entry((class, method, function)).or_insert_with(|| Box::new(crate::vm::engine_types::PlannedTarget {
            method,
            target: std::cell::Cell::new(crate::vm::engine_types::EMPTY_SCRIPT_CALL_TARGET),
        }));
        std::ptr::from_ref(&**slot)
    }

    /// Makes every planned arm of program `id` register its target slot (`planned_call_sites` of each function), so
    /// that a class's arms are all known when its header is recorded. Once per program.
    pub fn register_planned_targets(&self, id: ProgramId, admits: PlannedArmAdmission) {
        let Some(entry) = self.entry(id) else { return };
        if entry.planned_registered.get() { return; }
        entry.planned_registered.set(true);
        for function in 0..entry.residual.functions.len() {
            drop(self.planned_call_sites(id, function as u32, admits));
        }
    }

    /// Records class `class`'s instances' prototype in program `id` at the class-header edge (`Vm::record_class_edge`).
    pub fn record_class_prototype(&self, id: ProgramId, class: u32, prototype: Value) {
        if let Some(entry) = self.entry(id) {
            entry.class_prototypes.borrow_mut().insert(class, prototype);
        }
    }

    /// Every recorded class of every program, as (program, class, header, prototype), for the snapshot (`write_snapshot`).
    fn recorded_classes(&self) -> Vec<(ProgramId, u32, u32, Value)> {
        let mut recorded = Vec::new();
        for (index, entry) in self.programs.iter().enumerate() {
            let Some(entry) = entry else { continue };
            let id = ProgramId::from_raw(index as u32);
            for (&class, &prototype) in entry.class_prototypes.borrow().iter() {
                let header = Self::class_header_cell(entry, class).map_or(0, std::cell::Cell::get);
                recorded.push((id, class, header, prototype));
            }
        }
        recorded
    }

    /// The classes a restored snapshot recorded, taken once the main program is installed (`Vm::install_restored_main`).
    pub(crate) fn take_restored_classes(&mut self) -> Vec<(ProgramId, u32, u32, Value)> {
        std::mem::take(&mut self.restored_classes)
    }

    /// Marks program `id`'s roots pending after one of its planned arms' targets is set, so the next collection traces
    /// the target's values (`extend_roots`).
    pub fn planned_target_set(&mut self, id: ProgramId) {
        self.entry_mut(id);
    }

    /// The arms of class `class` in program `id`, registered (`register_planned_targets`): each arm's function, the
    /// property its method is read from, and its target slot.
    pub fn planned_targets_of_class(&self, id: ProgramId, class: u32) -> Vec<(u32, u32, *const crate::vm::engine_types::PlannedTarget)> {
        let Some(entry) = self.entry(id) else { return Vec::new() };
        let targets = entry.planned_targets.borrow();
        targets.range((class, 0, 0)..=(class, u32::MAX, u32::MAX))
            .map(|(&(_, _, function), slot)| (function, slot.method, std::ptr::from_ref(&**slot)))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.programs.len()
    }

    #[inline(always)]
    fn entry(&self, id: ProgramId) -> Option<&ProgramEntry> {
        self.programs.get(id.index())?.as_deref()
    }

    #[inline]
    fn entry_in_lane(&self, id: ProgramId) -> Option<&ProgramEntry> {
        self.entry(id)
    }

    /// Every mutation goes through here, so it also marks the program's roots pending.
    fn entry_mut(&mut self, id: ProgramId) -> Option<&mut ProgramEntry> {
        self.context_epoch += 1;
        let entry = self.programs.get_mut(id.index())?.as_deref_mut()?;
        entry.minor_root_traces = PromotionAge::OLDEST.minors();
        Some(entry)
    }

    /// Inserts dynamic code that `reclaim_unreferenced` may later drop.
    pub(crate) fn insert_reclaimable_shared(&mut self, program: Rc<ResidualProgram>, footprint: usize) -> Option<ProgramId> {
        let id = self.insert_shared(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.reclaimable = Some(footprint);
        }
        self.unchecked_reclaimable += 1;
        Some(id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn live_count(&self) -> usize {
        self.programs.iter().flatten().count()
    }

    /// The footprints of the reclaimable programs the last reclamation pass found still
    /// referenced after a pass before it (`ProgramEntry::reclaimable`): programs that stay.
    /// Those inserted since are still pending (`reclaim_unreferenced`) and are not counted,
    /// so a stream of short-lived programs does not raise its own threshold.
    pub(crate) fn settled_reclaimable_bytes(&self) -> usize {
        self.settled_reclaimable_bytes
    }

    /// Whether dynamic code may be reclaimable, including dropped host owners.
    pub(crate) fn has_unchecked_reclaimable(&self) -> bool {
        self.unchecked_reclaimable > 0
            || self
                .owned_scripts
                .values()
                .any(|script| script.owner.strong_count() == 0)
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
            let live_instance = script
                .instances
                .iter()
                .any(|(_, id)| referenced.get(id.index()).copied().unwrap_or(false));
            if (owned || live_instance)
                && let Some(mark) = referenced.get_mut(source.index())
            {
                *mark = true;
            }
            if owned {
                for (_, id) in &script.instances {
                    if let Some(mark) = referenced.get_mut(id.index()) {
                        *mark = true;
                    }
                }
            }
        }
        let previous_pass = self.reclaim_passes;
        self.reclaim_passes += 1;
        let mut reclaimed = Vec::new();
        let mut reclaimed_tiers = (0usize, 0usize, 0usize);
        let mut reclaimed_wasm_tiers = (0usize, 0usize);
        let mut reclaimed_rejected = 0usize;
        let mut pending = 0;
        let mut settled = 0usize;
        for (index, slot) in self.programs.iter_mut().enumerate() {
            let Some(entry) = slot else { continue };
            let Some(footprint) = entry.reclaimable else { continue };
            if referenced.get(index).copied().unwrap_or(true) {
                if entry.pass < previous_pass {
                    settled = settled.saturating_add(footprint);
                }
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
                .filter_map(WasmTierState::compiled)
                .map(|compiled| compiled.mapped_bytes())
                .sum::<usize>();
            reclaimed_wasm_tiers.0 += entry
                .jit_tiers
                .wasm_values()
                .filter(|state| state.compiled().is_some())
                .count();
            if let Some(code) = entry.residual.if_decoded()
                && let key = Rc::as_ptr(code) as usize
                && self.shared_ids.get(&key).copied() == ProgramId::from_index(index)
            {
                self.shared_ids.remove(&key);
            }
            *slot = None;
            self.context_epoch += 1;
            reclaimed.extend(ProgramId::from_index(index));
        }
        self.settled_reclaimable_bytes = settled;
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
        self.unchecked_reclaimable = pending;
        if !reclaimed.is_empty() {
            self.retain_live_eval_programs();
        }
        reclaimed
    }

    pub(crate) fn reset(&mut self, main: Rc<ResidualProgram>) -> ProgramId {
        self.global_eval_programs.clear();
        self.direct_eval_programs.clear();
        self.strict_direct_eval_code.clear();
        self.owned_scripts.clear();
        self.programs.clear();
        self.shared_ids.clear();
        self.context_epoch += 1;
        self.unchecked_reclaimable = 0;
        self.js_jit_tracked_functions = 0;
        self.js_jit_rejected_functions = 0;
        self.jit_code_cache = JitCodeCacheBudget::default();
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
        self.owned_scripts.insert(
            script.source,
            OwnedScript {
                owner: Rc::downgrade(script),
                instances: Vec::new(),
            },
        );
    }

    pub(super) fn owns_script(&self, script: &Rc<crate::api::CompiledScriptState>) -> bool {
        self.owned_scripts
            .get(&script.source)
            .is_some_and(|entry| std::ptr::eq(entry.owner.as_ptr(), Rc::as_ptr(script)))
    }

    pub(super) fn owned_script_instance(
        &self,
        source: ProgramId,
        realm: WeakHandle,
    ) -> Option<ProgramId> {
        self.owned_scripts
            .get(&source)?
            .instances
            .iter()
            .find_map(|(owner, id)| (*owner == realm).then_some(*id))
    }

    pub(super) fn bind_script_instance(
        &mut self,
        source: ProgramId,
        realm: WeakHandle,
        id: ProgramId,
    ) {
        self.set_origin(id, CompilationOrigin::SharedScript { source });
        self.owned_scripts
            .get_mut(&source)
            .expect("registered Script source")
            .instances
            .push((realm, id));
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

    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {
        if let Some(entry) = self.entry_mut(id) {
            entry.origin = origin;
        }
    }

    pub(super) fn eval_origin(&self, id: ProgramId) -> Option<JsString> {
        match &self.entry(id)?.origin {
            CompilationOrigin::Source | CompilationOrigin::SharedScript { .. } => None,
            CompilationOrigin::Eval { caller } => Some(
                caller
                    .clone()
                    .unwrap_or_else(|| JsString::from_str(ANONYMOUS_EVAL_ORIGIN)),
            ),
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

    /// Program `id`, a first-call chunk of a function of program `of`, loads names where its
    /// stub does: through environments, unless `of` runs in the global scope.
    pub(super) fn inherit_scope(&mut self, id: ProgramId, of: ProgramId) {
        if !self.runs_in_global_scope(of) {
            self.set_scope(id, ProgramScope::Enclosed);
        }
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
            ProgramScope::Caller | ProgramScope::Enclosed => false,
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

    /// Whether a by-name load the compiler left unresolved in any function of the program,
    /// its root included, can only denote a global binding, given that the function's names
    /// resolve statically: a module's environment holds only bindings the compiler resolved;
    /// a global-scope program reaches even its root's vars by name (`frame_var`), and declares
    /// them on the global object before its body runs (`general_global_get` asks the same).
    pub fn names_are_global(&self, id: ProgramId) -> bool { self.module_kind(id) || self.runs_in_global_scope(id) }
    fn module_kind(&self, id: ProgramId) -> bool { self.entry(id).is_some_and(|entry| entry.residual.kind == crate::bytecode::ProgramKind::Module) }

    /// Whether the program's root body evaluates in the global environment.
    pub(crate) fn runs_in_global_scope(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| match entry.scope {
            ProgramScope::Own => entry.residual.kind == crate::bytecode::ProgramKind::Script,
            ProgramScope::Global => true,
            ProgramScope::GlobalVariables | ProgramScope::Caller | ProgramScope::Enclosed => false,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.insert_shared(Rc::new(program))
    }

    #[quench_macros::test_visible]
    pub(crate) fn find_shared(&self, program: &Rc<ResidualProgram>) -> Option<ProgramId> {
        if let Some(&id) = self.shared_ids.get(&(Rc::as_ptr(program) as usize)) {
            return Some(id);
        }
        // A program a snapshot restored is not in the index. Only a decoded one can be the
        // caller's code: the caller's `Rc` exists only once the residual is decoded, so the scan
        // decodes nothing.
        self.programs
            .iter()
            .position(|entry| {
                entry.as_ref().and_then(|entry| entry.residual.if_decoded()).is_some_and(|code| Rc::ptr_eq(code, program))
            })
            .and_then(ProgramId::from_index)
    }

    pub(crate) fn insert_shared(&mut self, mut program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = ProgramId::from_index(self.programs.len())?;
        // A function compiles again on its first call; until then it keeps only its header.
        // A program other owners share keeps its records.
        #[cfg(feature = "profile-aggregate")]
        let retain_profile_records = crate::vm::switches::profile_retain_bytecode();
        #[cfg(not(feature = "profile-aggregate"))]
        let retain_profile_records = false;
        if !retain_profile_records && let Some(program) = Rc::get_mut(&mut program) {
            program.release_replayable_records();
        }
        // Sized by the sites the program has: a chunk's sites, appended later, grow it.
        let regexp_literals = vec![None; program.regexp_literal_sites.len()];
        let function_sources = FxHashMap::default();
        let views_in_lane = LaneViews::default();
        let lane_deferred = zeroed_counts(program.functions.capacity());
        let origin = if program.kind == crate::bytecode::ProgramKind::Eval { CompilationOrigin::Eval { caller: None } } else { CompilationOrigin::Source };
        self.context_epoch += 1;
        self.shared_ids.entry(Rc::as_ptr(&program) as usize).or_insert(id);
        self.programs.push(Some(Box::new(ProgramEntry {
            residual: Residual::decoded(program),
            wasm_signatures: None,
            constants: Vec::new().into(),
            const_arrays: FxHashMap::default(),
            function_sources,
            regexp_literals,
            regexp_receivers: FxHashMap::default(),
            regexp_arguments: FxHashMap::default(),
            module_environment: None,
            import_meta: None,
            module_imports: Vec::new(),
            root_slots: std::cell::OnceCell::new(),
            root_bindings: std::cell::OnceCell::new(),
            root_constants: root_constants::RootConstants::default(),
            module: false,
            views_in_lane,
            raw_masks: Default::default(),
            js_entry_facts: JsEntryFactsTable::default(),
            aot_facts: std::cell::OnceCell::new(),
            call_graph: std::cell::OnceCell::new(),
            predicted_tiers: std::cell::OnceCell::new(),
            selectors: std::cell::OnceCell::new(),
            class_method_vectors: Default::default(),
            class_headers: std::cell::OnceCell::new(),
            class_prototypes: Default::default(),
            planned_targets: Default::default(),
            planned_registered: std::cell::Cell::new(false),
            lane_deferred,
            jit_tiers: JitTierManager::default(),
            scope: ProgramScope::Own,
            origin,
            script_or_module: None,
            reclaimable: None,
            pass: self.reclaim_passes,
            minor_root_traces: PromotionAge::OLDEST.minors(),
        })));
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
        if entry.residual.validate_unless_proven().is_err()
            || signatures.defined_count() != entry.residual.function_count()
        {
            return false;
        }
        // A function its module lowers on its first call reads only the module's own pool.
        for function in entry.residual.functions.iter().filter_map(crate::bytecode::FunctionSlot::record) {
            for instruction in &function.code {
                let instruction = function.widened(*instruction);
                let valid = match crate::staged::wasm_record_check(instruction.op()) {
                    crate::staged::WasmRecordCheck::RefFunc => {
                        signatures.get(instruction.imm() as usize).is_some()
                    }
                    crate::staged::WasmRecordCheck::IndirectTarget => {
                        signatures.type_signature(instruction.imm()).is_some()
                    }
                    _ if matches!(
                        crate::staged::wasm_gc::GcStep::of(instruction.op()),
                        Some(crate::staged::wasm_gc::GcStep::StructNew { .. })
                    ) => {
                        let Some(crate::staged::wasm_gc::GcStep::StructNew { default, descriptor }) =
                            crate::staged::wasm_gc::GcStep::of(instruction.op())
                        else {
                            unreachable!("guarded by the struct constructor test")
                        };
                        signatures
                            .declarations
                            .struct_fields(instruction.imm())
                            .is_some_and(|fields| {
                                descriptor
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
                                                && (!default || ty.is_defaultable())
                                        }
                                    })
                                    && (default && !descriptor
                                        || usize::from(instruction.register_window().count)
                                            == if default {
                                                1
                                            } else {
                                                fields.len() + usize::from(descriptor)
                                            })
                            })
                    }
                    crate::staged::WasmRecordCheck::RefGetDesc => signatures
                        .declarations
                        .descriptor_type(instruction.imm())
                        .is_some(),
                    crate::staged::WasmRecordCheck::ArraySegment => {
                        use crate::wasm::gc::ArraySegmentInput;
                        instruction.register_window().count == ArraySegmentInput::COUNT
                            && signatures
                                .declarations
                                .array_field(instruction.imm())
                                .is_some_and(|field| {
                                    matches!(
                                        crate::staged::wasm_gc::GcStep::of(instruction.op()),
                                        Some(crate::staged::wasm_gc::GcStep::ArraySegment { kind, .. })
                                            if kind.accepts(field.element_type)
                                    ) && match field.element_type {
                                            wasmparser::StorageType::Val(ty) => signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some(),
                                            _ => true,
                                        }
                                })
                    }
                    crate::staged::WasmRecordCheck::ArrayNew => signatures
                        .declarations
                        .array_field(instruction.imm())
                        .is_some_and(|field| {
                            let valid = match field.element_type {
                                wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => true,
                                wasmparser::StorageType::Val(ty) => {
                                    signatures.declarations.callable_value_type(ty).is_some()
                                        && (!crate::staged::wasm_array_new_defaulted(instruction.op())
                                            || ty.is_defaultable())
                                }
                            };
                            valid
                                && (!crate::staged::wasm_array_new_fixed(instruction.op())
                                    || usize::from(instruction.register_b())
                                        .checked_add(usize::from(
                                            instruction.register_window().count,
                                        ))
                                        .is_some_and(|end| end <= usize::from(function.registers)))
                        }),
                    crate::staged::WasmRecordCheck::DescriptorReference => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.descriptor_type(&signatures.declarations))
                            .is_some()
                    }
                    crate::staged::WasmRecordCheck::ReferenceTest => {
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

    pub fn wasm_function_signature(
        &self,
        id: ProgramId,
        index: u32,
    ) -> Option<&crate::WasmSignature> {
        self.entry(id)?
            .wasm_signatures
            .as_ref()?
            .defined_signature(index)
    }

    #[inline] pub fn wasm_signatures(&self, id: ProgramId) -> Option<&crate::wasm::WasmSignatures> {
        self.entry(id)?.wasm_signatures.as_deref()
    }

    /// The program's signature pool, shared: a caller that runs the program keeps its
    /// signatures and declarations borrowed across the call without copying them.
    pub fn wasm_signatures_shared(&self, id: ProgramId) -> Option<Rc<crate::wasm::WasmSignatures>> {
        self.entry(id)?.wasm_signatures.clone()
    }

    pub(crate) fn insert_module_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = self.insert_shared(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.module = true;
        }
        Some(id)
    }

    /// What `slot` of `id`'s root environment holds for the reads through it
    /// (`RootSlotRule`), derived once per program.
    pub fn root_slot(&self, id: ProgramId, slot: u16) -> crate::bytecode::RootSlotRule {
        use crate::bytecode::RootSlotRule;
        let Some(entry) = self.entry(id) else {
            return RootSlotRule::Import;
        };
        let import = |slot: usize| entry.module_imports.iter().any(|(local, _)| usize::from(*local) == slot);
        let rules = entry.root_slots.get_or_init(|| {
            let locals = entry.residual.functions.first().map_or(0, |root| root.local_atoms.len());
            (0..locals)
                .map(|slot| if import(slot) { RootSlotRule::Import } else { entry.residual.root_slot_rule(slot) })
                .collect()
        });
        match rules.get(usize::from(slot)) {
            Some(rule) => *rule,
            // A slot past the root's named locals is a temporary, the binding itself.
            None if import(usize::from(slot)) => RootSlotRule::Import,
            None => RootSlotRule::Binding,
        }
    }

    /// Whether every slot of `id`'s root environment is the binding itself
    /// (`RootSlotRule::Binding`), as a strict eval's, a function-wrapped script's or an
    /// import-free module's is: its dense slots then serve a closure's captures as any
    /// function environment's do (`Vm::target_capture`, `Vm::environment_capture_slots`).
    /// A module's imports read through their records, so a module that declares any (its
    /// residual's, known before linking) has none.
    pub fn root_slots_are_bindings(&self, id: ProgramId) -> bool {
        let Some(entry) = self.entry(id) else { return false };
        *entry.root_bindings.get_or_init(|| {
            if !entry.module_imports.is_empty() || !entry.residual.module_imports.is_empty() {
                return false;
            }
            let locals = entry.residual.functions.first().map_or(0, |root| root.local_atoms.len());
            // Past the named locals every slot is a temporary, the binding itself (`root_slot`).
            (0..locals).all(|slot| self.root_slot(id, slot as u16) == crate::bytecode::RootSlotRule::Binding)
        })
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
        match self.root_slot(id, slot) {
            RootSlotRule::Binding => true,
            RootSlotRule::GlobalLexical(_) => lexical_slots_are_bindings,
            RootSlotRule::GlobalVar(_) | RootSlotRule::Import => false,
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
    pub(crate) fn residuals(&self) -> impl Iterator<Item = &ResidualProgram> {
        self.programs.iter().flatten().map(|entry| &**entry.residual)
    }

    /// The opcodes of every live program's lowered functions, which the lane's views run.
    pub(crate) fn lowered_ops(&self) -> Vec<u16> {
        let mut seen = vec![false; crate::bytecode::Op::COUNT];
        for program in self.residuals() {
            for index in 0..program.function_count() {
                for instruction in program.function(index).instructions() {
                    seen[instruction.op() as usize] = true;
                }
            }
        }
        (0..seen.len()).filter(|&op| seen[op]).map(|op| op as u16).collect()
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
            entry.root_bindings = std::cell::OnceCell::new();
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
    /// The decoded residuals of the stored programs, a shared one once per program sharing it.
    pub(crate) fn decoded_residuals(&self) -> impl Iterator<Item = &ResidualProgram> {
        self.programs.iter().flatten().filter_map(|entry| entry.residual.if_decoded().map(|program| &**program))
    }

    /// The registers of `function` of program `id` live before each pc that hold raw words,
    /// which a collection reads past (`RegisterMasks::extend_live_except`); `None` for a
    /// function with no raw op. Derived once per function, when a collection first asks.
    pub(crate) fn raw_register_masks(&self, id: ProgramId, function: u32) -> Option<Rc<crate::compile::liveness::RegisterMasks>> {
        let entry = self.entry(id)?;
        if entry.residual.kind != crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        if let Some(masks) = entry.raw_masks.borrow().get(&function) {
            return masks.clone();
        }
        let masks = entry
            .residual
            .function_get(function as usize)
            .and_then(|record| crate::compile::liveness::raw_webs::raw_live(record))
            .map(|live| Rc::new(crate::compile::liveness::RegisterMasks::new(live)));
        entry.raw_masks.borrow_mut().insert(function, masks.clone());
        masks
    }

    #[inline] pub fn residual(&self, id: ProgramId) -> Option<&ResidualProgram> {
        self.entry(id).map(|entry| &**entry.residual)
    }

    pub fn get(&self, id: ProgramId) -> Option<Rc<ResidualProgram>> {
        self.entry(id).map(|entry| entry.residual.clone())
    }

    /// Drop the lane views `id` derived (`LaneViews`); a later interpreted run derives them
    /// again. The caller ensures no lane run of `id` holds their records.
    pub fn release_lane_views(&mut self, id: ProgramId) {
        if let Some(entry) = self.entry_mut(id) {
            entry.views_in_lane = LaneViews::default();
        }
    }

    /// Sets program `id`'s materialized constants, with room for `headroom` more.
    pub(crate) fn set_constants(&mut self, id: ProgramId, constants: Vec<Value>, headroom: usize) {
        if let Some(entry) = self.entry_mut(id) {
            entry.const_arrays.clear();
            entry.constants = crate::bytecode::append_vec::AppendVec::with_headroom(constants, headroom);
            // Lane views carry constants, so they derive from the new set.
            entry.views_in_lane = LaneViews::default();
        }
    }

    /// Whether a root tier may take `function` of Wasm program `id`: the function is small enough, and the code
    /// budget could keep the artifact (a compile it could not keep is work thrown away).
    fn wasm_root_admissible(&self, id: ProgramId, function: u32) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.residual.kind == crate::bytecode::ProgramKind::Wasm
                && entry.residual.function_get(function as usize).is_some_and(|function| {
                    function.code.len() <= crate::vm::tier_policy::MAX_JIT_TIER_BYTECODES_PER_FUNCTION
                })
        }) && self.wasm_tier_has_room()
    }

    /// Whether the root compile of `function` of Wasm program `id` is due at its first call (`wasm_activation::offer_entry`):
    /// the function is unattempted, its predicted tier compiles it (`predicted_compiled`), and the root may be admitted.
    pub fn wasm_root_due(&self, id: ProgramId, function: u32) -> bool {
        self.wasm_root_admissible(id, function)
            && self.predicted_compiled(id, function)
            && self.entry(id).is_some_and(|entry| entry.jit_tiers.wasm_unattempted(function))
    }

    /// Claims the one compile attempt of `function` of Wasm program `id` when its root compile is due
    /// (`wasm_root_due`). A claimed attempt is never repeated, so a refused or failed compile does not turn later
    /// calls into work.
    pub fn claim_wasm_root(&mut self, id: ProgramId, function: u32) -> bool {
        self.wasm_root_due(id, function)
            && self.entry_mut(id).is_some_and(|entry| entry.jit_tiers.claim_wasm_root(function))
    }

    /// Whether the predicted tier of `function` of program `id` compiles it (`tier_policy::lane_body_tier`), read at
    /// the function's entry: the argmin of design 4.9 that a JavaScript function's entry reads too (`predicted_tiers`).
    pub(super) fn predicted_compiled(&self, id: ProgramId, function: u32) -> bool {
        self.predicted_tiers(id)
            .and_then(|table| table.get(function as usize).map(|row| row[0]))
            .is_some_and(|pick| crate::vm::tier_policy::lane_body_tier(pick).is_some())
    }

    pub fn wasm_tier(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<crate::vm::engine_types::CompiledProgram>> {
        self.entry(id)?
            .jit_tiers
            .wasm(function)?
            .compiled()
            .cloned()
    }

    /// Find a root artifact whose static closure includes this function.
    pub fn wasm_tier_containing(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<crate::vm::engine_types::CompiledProgram>> {
        self.entry(id)?.jit_tiers.wasm_holding(function).cloned()
    }

    /// Find a root artifact holding the requested function in the currently executing artifact.
    /// Both caller and target can sit in one closure; prefer that closure so the native call
    /// does not leave it.
    pub fn wasm_tier_for_artifact_containing(
        &self,
        id: ProgramId,
        function: usize,
        artifact: *const crate::vm::engine_types::CompiledProgram,
    ) -> Option<Rc<crate::vm::engine_types::CompiledProgram>> {
        self.entry(id)?
            .jit_tiers
            .wasm_values()
            .filter_map(WasmTierState::compiled)
            .find(|compiled| {
                std::ptr::eq(Rc::as_ptr(compiled), artifact) && compiled.has_function(function)
            })
            .cloned()
    }

    /// Whether the code budget could still retain a Wasm tier artifact: a compile it could not
    /// keep is work thrown away (sqlite3-wasm `--jit`: hundreds of root compiles after the
    /// tier budget filled, 15 s of its first iteration).
    pub fn wasm_tier_has_room(&self) -> bool {
        self.jit_code_cache.can_retain(JitArtifactKind::WasmTierRoot, JIT_MIN_ARTIFACT_BYTES)
    }

    /// Retain one compiled root closure if the VM-wide tier budget permits
    /// it. Active calls retain their own Rc through native return.
    pub fn install_wasm_tier(
        &mut self,
        id: ProgramId,
        root: u32,
        compiled: Rc<crate::vm::engine_types::CompiledProgram>,
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
            .is_some_and(|state| state.compiled().is_some())
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
        let Some(tiers) = entry.jit_tiers.wasm_tiers_mut() else {
            self.jit_code_cache
                .release(JitArtifactKind::WasmTierRoot, 1, bytes);
            return false;
        };
        tiers.install(root, compiled);
        true
    }

    pub fn js_tier(&self, id: ProgramId, function: u32) -> Option<JsTierState> {
        self.entry(id)?.jit_tiers.js(function).cloned()
    }

    pub fn js_tier_ref(&self, id: ProgramId, function: u32) -> Option<&JsTierState> {
        self.entry(id)?.jit_tiers.js(function)
    }

    pub fn enable_js_lane_methods(&mut self) {
        // AArch64's lane-body tier is exact but slower than the interpreter on this corpus until
        // its bodies stop leaving mid-body (`ty2`/`raytrace` leaves at At(5)/At(16)/At(23)), so it
        // stays out of every mode there while the loop regions compile and run. Debt (TOD-1202).
        #[cfg(target_arch = "aarch64")]
        return;
        #[cfg(not(target_arch = "aarch64"))]
        { self.js_jit_lane_methods_enabled = true; }
    }

    pub fn enable_ahead_lane_bodies(&mut self) { self.js_jit_lane_methods_enabled = true; }

    #[quench_macros::test_visible]
    /// Set before loading a test program; general/OSR coverage then owns entry.
    #[cfg(any(all(test, not(quench_profiled)), feature = "test-support"))]
    pub(super) fn disable_js_entry_specializations(&mut self) {
        self.js_entry_specializations_enabled = false;
    }

    #[quench_macros::test_visible]
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn js_entry_specializations_enabled(&self) -> bool {
        self.js_entry_specializations_enabled
    }

    pub fn js_jit_lane_methods_enabled(&self) -> bool {
        self.js_jit_lane_methods_enabled
    }

    /// Takes the next warming-entry tick (`body_tier_in_lane`): one per entry a warming body
    /// counts, which a function entered rarely but for long reads to specialize early.
    #[inline]
    pub(super) fn warming_tick(&self) -> u32 {
        let tick = self.warming_ticks.get().wrapping_add(1);
        self.warming_ticks.set(tick);
        tick
    }

    #[inline]
    pub fn tiers(&self) -> &'static Tiers { self.tiers }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn hold_tiers(&mut self, tiers: &'static Tiers) { self.tiers = tiers; }

    #[inline]
    pub fn tier_events(&self) -> &super::tier_events::TierEvents {
        &self.tier_events
    }

    #[inline]
    pub fn js_jit_diagnostics_enabled(&self) -> bool {
        self.js_jit_diagnostics.is_some()
    }

    #[quench_macros::test_visible]
    /// Records the counters `QUENCH_TRACE=js-jit` enables, for a test to read.
    #[cfg(any(all(test, not(quench_profiled)), feature = "test-support"))]
    pub(super) fn enable_js_jit_diagnostics(&mut self) {
        self.js_jit_diagnostics.get_or_insert_with(FxHashMap::default);
    }

    #[cfg(any(all(test, not(quench_profiled)), feature = "test-support"))]
    pub(super) fn js_jit_field_cache_probes(&self, id: ProgramId, function: u32) -> Option<u64> {
        self.js_jit_diagnostics
            .as_ref()?
            .get(&(id.raw(), function))
            .map(|counters| counters.field_cache_probes)
    }

    pub fn note_js_jit_dataview_float64(&mut self, setter: bool) {
        if self.js_jit_diagnostics.is_none() {
            return;
        }
        if setter {
            self.js_jit_dataview_float64_set_entries += 1;
        } else {
            self.js_jit_dataview_float64_get_entries += 1;
        }
    }

    /// Whether an entry of `function` of program `id` compiles it now (`JitTierManager::note_js_entry`).
    /// A compile is due where the model picks a compiled tier for the function
    /// (`predicted_compiled`), within the tracking and code budgets.
    pub fn note_js_function_entry(&mut self, id: ProgramId, function: u32, numeric: bool) -> bool {
        let (compiled_functions, predicted) = (self.jit_code_cache.js_functions, self.predicted_compiled(id, function));
        let can_track = self.js_jit_tracked_functions < JS_JIT_MAX_TRACKED_FUNCTIONS;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let compile = entry.jit_tiers.note_js_entry(function, numeric, predicted);
        if compile && compiled_functions >= JS_JIT_MAX_FUNCTIONS {
            entry.jit_tiers.js_insert(function, JsTierState::Unsupported);
            return false;
        }
        if compile && !can_track {
            self.js_jit_tracking_exhaustions = self.js_jit_tracking_exhaustions.saturating_add(1);
            return false;
        }
        if compile {
            self.record_js_jit_promotion(id, function);
        }
        compile
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

    /// Counts, in the diagnostics report, an entry of a function no tier path claims: its entries
    /// and, once rejected, its reason. The entry takes its general path whatever this finds.
    pub fn observe_unclaimed_entry(&mut self, program: &ResidualProgram, id: ProgramId, function: u32, parameters: &[Value]) {
        if self.js_jit_diagnostics.is_none() { return; }
        let Some(body) = program.function_get(function as usize) else { return };
        let Some(arguments) = parameters.get(..usize::from(body.params)) else { return };
        let numeric = arguments.iter().all(|value| value.as_number().is_some());
        let rejected = matches!(self.js_tier_ref(id, function), Some(JsTierState::Rejected { .. })).then(|| self.note_js_rejected_entry(id, function)).flatten();
        let reported = match rejected {
            Some(rejection) => Some(rejection),
            None if self.js_tier_ref(id, function).is_none() => {
                let reason = crate::vm::engine_calls::candidate_rejection_reason(program, function);
                self.reject_js_function(id, function, reason).map(|entries| (entries, reason))
            }
            None => None,
        };
        if let Some((entries, reason)) = reported {
            self.record_js_jit_entry(id, function, numeric, body.params);
            self.record_js_jit_rejection(id, function, reason, entries);
        }
    }

    pub fn record_js_jit_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        numeric: bool,
        parameter_count: u16,
    ) {
        self.record_js_jit_counter_entry(id, function, Some(numeric), parameter_count);
    }

    #[inline(always)]
    pub fn record_js_jit_lane_entry(&mut self, id: ProgramId, function: u32) { if self.js_jit_diagnostics.is_some() { self.record_diagnosed_lane_entry(id, function); } }

    #[inline(never)] fn record_diagnosed_lane_entry(&mut self, id: ProgramId, function: u32) {
        let params = self.entry(id).and_then(|entry| entry.residual.function_get(function as usize)).map_or(0, |body| body.params);
        self.record_js_jit_counter_entry(id, function, None, params);
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
                    .and_then(|slot| slot.header().name.or_else(|| slot.header().inferred_name))
                    .and_then(|atom| {
                        let index = atom as usize;
                        (index < entry.residual.atoms.len())
                            .then(|| entry.residual.atoms[index].to_owned())
                    })
                    .unwrap_or_default();
                let instruction_count = function_code.map_or(0, |function| {
                    u32::try_from(function.code.len()).unwrap_or(u32::MAX)
                });
                let mut opcode_counts = vec![0_u32; crate::bytecode::OPCODE_COUNT];
                if let Some(function) = function_code {
                    for instruction in &function.code {
                        let opcode = instruction.as_wide().op() as usize;
                        opcode_counts[opcode] = opcode_counts[opcode].saturating_add(1);
                    }
                }
                let opcode_counts = opcode_counts
                    .into_iter()
                    .enumerate()
                    .filter(|&(_opcode, count)| count != 0).map(|(opcode, count)| (u16::try_from(opcode).unwrap_or(u16::MAX), count))
                    .collect();
                // This is a bytecode-site census, not a claim that a site is
                // pure, redundant, or safe to remove. Predicate purity and
                // path implication need separate proofs before optimization.
                let mut guard_census = (0_u32, 0_u32, 0_u32, 0_u32);
                if let Some(function) = function_code {
                    for instruction in &function.code {
                        let instruction = instruction.as_wide();
                        let op = instruction.op();
                        // merge: the guard census counts conditional jumps (JumpFalse, JumpTrue, JumpBinaryFalse) and their comparison operator, as HEAD's JumpIf/CompareJump arms did.
                        if crate::staged::conditional_jump(op) {
                            guard_census.0 = guard_census.0.saturating_add(1);
                            if crate::staged::compare_jump(op) {
                                count_comparison_guard(&mut guard_census, instruction.binary_operator_field())
                            }
                        } else if crate::staged::computes_unary(op)
                            && instruction.unary_operator() == crate::operator::UnaryOperator::Typeof as u32
                        {
                            guard_census.1 = guard_census.1.saturating_add(1);
                        } else if crate::staged::binary(op) {
                            count_comparison_guard(&mut guard_census, instruction.binary_operator())
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

    pub fn record_js_jit_compile(
        &mut self,
        id: ProgramId,
        function: u32,
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

    pub fn record_js_jit_native_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.native_entries = counters.native_entries.saturating_add(1);
        }
    }

    #[cfg(feature = "native-execution")]
    pub fn record_js_jit_field_cache_probe(
        &mut self,
        id: ProgramId,
        function: u32,
        cache_site: u16,
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
            let site = counters.field_cache_sites.entry(cache_site).or_default();
            site.0 = site.0.saturating_add(1);
            site.1 = site.1.saturating_add(u64::from(hit));
        }
    }

    #[cfg(feature = "native-execution")]
    pub fn record_js_jit_field_store_cache_probe(
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

    /// Count call instructions handled by a compiled function's VM islands.
    /// Rows are bounded by that function's static Call/CallArray PCs and only
    /// exist when the opt-in diagnostics map has been allocated.
    #[cfg(feature = "native-execution")]
    pub fn record_js_jit_call_site(
        &mut self,
        id: ProgramId,
        function: u32,
        pc: usize,
        opcode: crate::bytecode::Op,
        direct_target: Option<(u32, u32)>,
        outcome: JsJitCallOutcome,
    ) {
        let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        else {
            return;
        };
        let Ok(pc) = u32::try_from(pc) else {
            return;
        };
        let key = (pc, opcode as u16);
        let index = match counters
            .call_sites
            .binary_search_by_key(&key, |site| (site.pc, site.opcode))
        {
            Ok(index) => index,
            Err(index) => {
                counters.call_sites.insert(
                    index,
                    JsJitCallSiteCounters {
                        pc,
                        opcode: key.1,
                        ..JsJitCallSiteCounters::default()
                    },
                );
                index
            }
        };
        let site = &mut counters.call_sites[index];
        site.attempts = site.attempts.saturating_add(1);
        site.note_target(direct_target);
        let outcome_count = &mut site.outcomes[outcome as usize];
        *outcome_count = outcome_count.saturating_add(1);
    }

    #[cfg(any(all(test, not(quench_profiled)), feature = "test-support"))]
    pub(super) fn js_jit_call_site_counts(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Vec<(
        u32,
        u16,
        u64,
        [u64; JsJitCallOutcome::COUNT],
        Option<(u32, u32)>,
        u64,
        u64,
        u64,
    )>> {
        self.js_jit_diagnostics
            .as_ref()?
            .get(&(id.raw(), function))
            .map(|counters| {
                counters
                    .call_sites
                    .iter()
                    .map(|site| {
                        (
                            site.pc,
                            site.opcode,
                            site.attempts,
                            site.outcomes,
                            site.direct_target,
                            site.direct_target_calls,
                            site.alternate_target_calls,
                            site.other_target_calls,
                        )
                    })
                    .collect()
            })
    }

    pub fn record_js_jit_fallback_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.fallback_entries = counters.fallback_entries.saturating_add(1);
        }
    }

    pub fn install_js_tier(
        &mut self,
        id: ProgramId,
        function: u32,
        compiled: Option<Rc<crate::vm::engine_types::CompiledJsFunction>>,
    ) -> bool {
        let Some(entry) = self.entry(id) else {
            return false;
        };
        if entry.jit_tiers.is_wasm() {
            return false;
        }
        let compiled = compiled.filter(|compiled| {
            self.jit_code_cache.retain(JitArtifactKind::JsFunction, compiled.mapped_bytes())
        });
        let mut tracked = false;
        if let Some(entry) = self.entry_mut(id) {
            let tier = compiled.map_or(JsTierState::Unsupported, JsTierState::Compiled);
            if let Some(state) = entry.jit_tiers.js_mut(function) {
                *state = tier;
            } else {
                tracked = entry.jit_tiers.js_insert(function, tier);
            }
        }
        self.js_jit_tracked_functions += usize::from(tracked);
        self.js_tier(id, function)
            .is_some_and(|tier| matches!(tier, JsTierState::Compiled(_)))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn js_jit_tracked_function_count(&self) -> usize {
        self.js_jit_tracked_functions
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn js_jit_tracking_exhaustion_count(&self) -> u64 {
        self.js_jit_tracking_exhaustions
    }

    pub(super) fn jit_mapped_bytes(&self) -> usize {
        self.jit_code_cache.mapped_bytes + self.jit_code_cache.baseline_mapped_bytes
    }

    /// The epoch program contexts are derived at (`context_epoch`).
    #[inline(always)]
    pub fn context_epoch(&self) -> u64 {
        self.context_epoch
    }

    /// Makes every program context derive again: the VM changed what they read beside the
    /// store (a cache layout, the field cache table).
    pub(super) fn bump_context_epoch(&mut self) {
        self.context_epoch += 1;
    }

    /// Program `id`'s lane view table if it was made (`program_in_lane` makes it).
    pub fn made_lane_views(&self, id: ProgramId) -> Option<&[LaneViewCell]> {
        self.entry(id)?.views_in_lane.table.get().map(|table| &table[..])
    }

    /// What a lane running program `id` reads of it, from one look at its entry: its
    /// residual, lane views (`views_in_lane`) and constants (`constants`).
    #[inline(always)]
    pub(super) fn program_in_lane(
        &self,
        id: ProgramId,
    ) -> Option<(&ResidualProgram, &[LaneViewCell], &[Value])> {
        let entry = self.entry(id)?;
        Some((&entry.residual, entry.views_in_lane.of(&entry.residual), &entry.constants[..]))
    }

    #[quench_macros::test_visible]
    /// Invalidates every lane view's bindings to shape IDs: compaction renumbered
    /// the IDs held by field hints and native own-field descriptors.
    pub(super) fn invalidate_shape_bindings(&self) {
        for entry in self.programs.iter().flatten() {
            entry.views_in_lane.invalidate_shape_bindings();
        }
    }

    /// Clears every lane view's global-object hints of its by-name loads
    /// (`LaneView::clear_name_hints`): the global lexical names they read past changed.
    pub(super) fn clear_global_name_hints(&self) {
        for entry in self.programs.iter().flatten() {
            for view in entry.views_in_lane.arena.borrow().iter().flatten() {
                view.clear_name_hints();
            }
        }
    }

    /// A program's lane views by function, derived on first use.
    pub fn views_in_lane(
        &self,
        id: ProgramId,
    ) -> Option<&[LaneViewCell]> {
        let entry = self.entry_in_lane(id)?;
        Some(entry.views_in_lane.of(&entry.residual))
    }

    /// What a lane run's context reads of program `id` (`Vm::lane_context`), from one look at
    /// its entry: its lane views (`lane_views`, none for a program the store does not hold) and
    /// its constants (`constants`).
    #[inline(always)]
    pub fn lane_tables(&self, id: ProgramId) -> (&[LaneViewCell], &[Value]) {
        let Some(entry) = self.entry(id) else {
            return (&[], &[]);
        };
        (entry.views_in_lane.of(&entry.residual), &entry.constants[..])
    }

    /// The lane view a general-loop entry of `function` runs with. A function that may loop
    /// gets it at its first entry; any other once its deferred runs are complete
    /// (`LANE_VIEW_DEFERRED_RUNS`), so code that runs a few times without looping (a module
    /// body) runs on the general path instead of first deriving a view of every instruction,
    /// which costs more than those runs.
    pub fn entry_lane_view<'s>(
        &'s self,
        id: ProgramId,
        function: u32,
        may_loop: impl FnOnce(&crate::bytecode::Function) -> bool,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
            &dyn Fn() -> Option<&'s std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
            &[crate::bytecode::FieldSite],
        ) -> crate::vm::engine_types::LaneView,
    ) -> Option<*const crate::vm::engine_types::LaneView> {
        let entry = self.entry_in_lane(id)?;
        let view = entry.views_in_lane.of(&entry.residual).get(function as usize)?;
        if let Some(view) = view.get() {
            return Some(view.as_ptr());
        }
        let code = entry.residual.function_get(function as usize)?;
        if !deferred_run_completes(entry.lane_deferred.get(function as usize)?) && !may_loop(code) || entry.residual.runs_tier0(function as usize) {
            return None;
        }
        Some(store_lane_view(entry, function, code, view, derive))
    }

    /// Whether a lane call may derive `function`'s view now: it has one, or its earlier entries
    /// and lane calls completed its deferred runs (`LANE_VIEW_DEFERRED_RUNS`), as
    /// `entry_lane_view` defers its entries. A deferred call counts its run and takes the
    /// general path.
    /// An unlowered function derives only where `unlowered` (`Vm::call_may_derive_in_lane`); where
    /// `now`, its deferred runs are skipped, uncounted.
    pub(super) fn call_may_derive_in_lane(&self, id: ProgramId, function: u32, unlowered: bool, now: bool) -> bool {
        let Some(entry) = self.entry_in_lane(id) else {
            return false;
        };
        let derived = entry.views_in_lane.derived(function).is_some();
        derived
            || entry
                .lane_deferred
                .get(function as usize)
                .is_some_and(|runs| now || deferred_run_completes(runs))
                && (unlowered || !entry.residual.runs_tier0(function as usize))
    }

    pub fn view_in_lane<'s>(
        &'s self,
        id: ProgramId,
        function: u32,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
            &dyn Fn() -> Option<&'s std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
            &[crate::bytecode::FieldSite],
        ) -> crate::vm::engine_types::LaneView,
    ) -> Option<*const crate::vm::engine_types::LaneView> {
        let entry = self.entry_in_lane(id)?;
        let code = entry.residual.function_get(function as usize)?;
        let view = entry.views_in_lane.of(&entry.residual).get(function as usize)?;
        Some(store_lane_view(entry, function, code, view, derive))
    }

    /// A view of `function` derived afresh from its bytecode and not stored, beside the view
    /// its runs settled (requickened records) if one was derived: the lane plan
    /// (`QUENCH_TRACE=bytecode=lanes`, `Vm::report_lane_plan`).
    pub fn plan_lane_view<'s>(
        &'s self,
        id: ProgramId,
        function: u32,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
            &dyn Fn() -> Option<&'s std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
            &[crate::bytecode::FieldSite],
        ) -> crate::vm::engine_types::LaneView,
    ) -> Option<(crate::vm::engine_types::LaneView, Option<&'s crate::vm::engine_types::LaneView>)> {
        let entry = self.entry_in_lane(id)?;
        let code = entry.residual.function_get(function as usize)?;
        let live = || entry.residual.shared_register_root_masks(function);
        let fresh = derive(code, &entry.constants, entry.residual.kind, &live, &entry.residual.field_sites);
        Some((fresh, entry.views_in_lane.derived(function)))
    }

    /// A second view of `function` that `derive` makes from its code (an inlined view, N7), stored in
    /// the program's arena beside its own view, at an address fixed while the program holds it. The
    /// function's cell keeps its own view.
    pub fn store_extra_lane_view(
        &self,
        id: ProgramId,
        function: u32,
        derive: impl FnOnce(&crate::bytecode::Function, &[Value], crate::bytecode::ProgramKind, &[crate::bytecode::FieldSite])
            -> Option<crate::vm::engine_types::LaneView>,
    ) -> Option<*const crate::vm::engine_types::LaneView> {
        let entry = self.entry_in_lane(id)?;
        let code = entry.residual.function_get(function as usize)?;
        let view = derive(code, &entry.constants, entry.residual.kind, &entry.residual.field_sites)?;
        Some(entry.views_in_lane.store(view).as_ptr().cast_const())
    }

    pub fn constant(&self, id: ProgramId, index: usize) -> Option<Value> {
        self.constants(id).get(index).copied()
    }

    pub fn constants(&self, id: ProgramId) -> &[Value] {
        self.entry(id)
            .map_or(&[], |entry| &entry.constants[..])
    }

    pub fn const_array(
        &mut self,
        id: ProgramId,
        start: usize,
        count: usize,
    ) -> Option<crate::heap::ArrayElements> {
        let entry = self.entry_mut(id)?;
        let end = start.checked_add(count)?;
        let key = u32::try_from(start).ok()?;
        if let Some(cached) = entry.const_arrays.get(&key) {
            return Some(cached.clone());
        }
        let elements = crate::heap::ArrayElements::from_slice(entry.constants.get(start..end)?);
        entry.const_arrays.insert(key, elements.clone());
        Some(elements)
    }

    pub(crate) fn function_source(&self, id: ProgramId, function: u32) -> Option<Value> {
        self.entry(id)?.function_sources.get(&function).copied()
    }

    pub(crate) fn cache_function_source(
        &mut self,
        id: ProgramId,
        function: u32,
        source: Value,
    ) -> Option<Value> {
        let entry = self.entry_mut(id)?;
        (function < entry.residual.functions.capacity() as u32).then_some(())?;
        Some(*entry.function_sources.entry(function).or_insert(source))
    }

    pub(crate) fn regexp_literal_meta(
        &self,
        id: ProgramId,
        site: usize,
    ) -> Option<Rc<crate::heap::RegExpMeta>> {
        self.entry(id)?.regexp_literals.get(site)?.clone()
    }

    /// The object receiver-only literal `site` of program `id` shares, once one was made.
    pub(crate) fn regexp_literal_receiver(&self, id: ProgramId, site: usize) -> Option<Value> {
        self.entry(id)?.regexp_receivers.get(&u32::try_from(site).ok()?).copied()
    }

    /// Keeps `object` as the one object receiver-only literal `site` evaluates to.
    pub(crate) fn cache_regexp_literal_receiver(&mut self, id: ProgramId, site: usize, object: Value) {
        if let (Some(entry), Ok(site)) = (self.entry_mut(id), u32::try_from(site)) {
            entry.regexp_receivers.insert(site, object);
        }
    }

    /// The object callee-argument literal `site` of program `id` shares, with what its sharing
    /// was proved under (`Vm::regexp_literal_argument_of`).
    pub(crate) fn regexp_literal_argument(&self, id: ProgramId, site: usize) -> Option<&RegExpArgumentShare> {
        self.entry(id)?.regexp_arguments.get(&u32::try_from(site).ok()?)
    }

    /// Keeps `share` as callee-argument literal `site`'s shared object, in place of any
    /// object it shared before, which it never answers again.
    pub(crate) fn cache_regexp_literal_argument(&mut self, id: ProgramId, site: usize, share: RegExpArgumentShare) {
        if let (Some(entry), Ok(site)) = (self.entry_mut(id), u32::try_from(site)) {
            entry.regexp_arguments.insert(site, share);
        }
    }

    /// Drops callee-argument literal `site`'s shared object, which it never answers again.
    pub(crate) fn forget_regexp_literal_argument(&mut self, id: ProgramId, site: usize) {
        let Ok(site) = u32::try_from(site) else { return };
        if self.entry(id).is_some_and(|entry| entry.regexp_arguments.contains_key(&site))
            && let Some(entry) = self.entry_mut(id)
        {
            entry.regexp_arguments.remove(&site);
        }
    }

    pub(crate) fn cache_regexp_literal_meta(
        &mut self,
        id: ProgramId,
        site: usize,
        meta: Rc<crate::heap::RegExpMeta>,
    ) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if site >= entry.regexp_literals.len() && site < entry.residual.regexp_literal_sites.capacity() {
            entry.regexp_literals.resize(site + 1, None);
        }
        let Some(cached) = entry.regexp_literals.get_mut(site) else {
            return false;
        };
        if cached.is_none() {
            *cached = Some(meta);
        }
        true
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn regexp_literal_record_count(&self, id: ProgramId) -> usize {
        self.entry(id).map_or(0, |entry| {
            entry
                .regexp_literals
                .iter()
                .filter(|site| site.is_some())
                .count()
        })
    }

    /// Appends the values the stored programs keep alive to `roots`: every program's for a
    /// full collection, for a minor one only those of programs whose roots changed since
    /// they were last traced, until as many minor collections as promote a value under `age`
    /// (or under the age of an earlier collection, if that was fewer) have traced them.
    pub(crate) fn extend_roots(&mut self, roots: &mut Vec<Value>, full: bool, age: PromotionAge) {
        for entry in self.programs.iter_mut().flatten() {
            if full {
                entry.minor_root_traces = 0;
            } else if entry.minor_root_traces == 0 {
                continue;
            } else {
                entry.minor_root_traces = entry.minor_root_traces.min(age.minors()) - 1;
            }
            roots.extend_from_slice(&entry.constants);
            roots.extend(entry.function_sources.values().copied());
            roots.extend(entry.regexp_receivers.values().copied());
            roots.extend(entry.regexp_arguments.values().flat_map(|share| [share.object, share.realm, share.constructor]));
            roots.extend(entry.module_environment);
            roots.extend(entry.root_constants.environment.get());
            // A planned arm's target holds its closure, its environment and its lexical `this`, which the arm
            // enters while the program lives (`PlannedTarget`).
            for slot in entry.planned_targets.borrow().values() {
                let target = slot.target.get();
                roots.extend([target.callee, target.env, target.lexical_this]);
            }
            roots.extend(entry.class_prototypes.borrow().values().copied());
            roots.extend(entry.import_meta);
            roots.extend(entry.module_imports.iter().map(|(_, import)| match import {
                ModuleImport::Value(value) => *value,
                ModuleImport::Binding(_, _, fallback) => *fallback,
            }));
        }
    }
}

impl ProgramStore {
    /// Whether dropping the store reports something (`QUENCH_TRACE=js-jit`).
    pub fn reports_on_drop(&self) -> bool {
        self.js_jit_diagnostics.is_some()
    }
}

/// The diagnostics report's ratios are parts per million.
const PARTS_PER_MILLION: u128 = 1_000_000;

/// `part` of `whole` in parts per million, capped at the whole; zero of nothing is zero.
fn parts_per_million(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    (u128::from(part) * PARTS_PER_MILLION / u128::from(whole)).min(PARTS_PER_MILLION) as u64
}

impl Drop for ProgramStore {
    fn drop(&mut self) {
        let census = crate::trace::Channel::Census.on();
        if census && self.js_jit_diagnostics.is_some() {
            eprintln!("{}", self.tier_census().render());
        }
        if !census || crate::trace::Channel::JsJit.on() {
            self.report_js_jit();
        }
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

/// Count one view-less run of a function; true once its deferred runs are complete.
fn deferred_run_completes(runs: &std::cell::Cell<u8>) -> bool {
    let held = runs.get();
    if held & loops::DEFERRED_RUNS >= LANE_VIEW_DEFERRED_RUNS {
        return true;
    }
    runs.set(held + 1);
    false
}

/// A program's lane views by function, derived on first use (`ProgramStore::entry_lane_view`).
/// The table itself is made on the program's first lane use: most programs a process restores
/// or loads never enter the lane, and a table zeroed for each of their functions cost a
/// restored process about 54 fresh pages (2,493 functions at 88 bytes). Its cells point into
/// `arena`, which holds the derived views in derivation order, so the table costs a pointer
/// per function: most functions never run (fastify's JSON route over HTTP, memory-first,
/// 2026-10-25: about 12,800 functions in its programs, whose 88-byte cells were 1.1 MB at
/// the peak).
#[derive(Default)]
pub(super) struct LaneViews {
    table: std::cell::OnceCell<LaneViewTable>,
    /// The derived views in chunks that never grow past their capacity, so a view's address is
    /// fixed while its program holds it. Each chunk holds as many as all earlier chunks together
    /// (`LANE_VIEW_FIRST_CHUNK` first), so a program's views take a few allocations with at
    /// most half of the arena unused, and lie together in the order the program derived them.
    arena: std::cell::RefCell<Vec<Vec<crate::vm::engine_types::LaneView>>>,
}

/// The views a program's first arena chunk holds: most programs that enter the lane derive
/// a handful of views, which this keeps in one allocation of 704 bytes.
const LANE_VIEW_FIRST_CHUNK: usize = 8;


/// A program's lane view cells, one per function slot its table can reach. A large table lies
/// on fresh zero pages, an empty cell being zero bits, so only the pages of functions that
/// derive a view become resident: each eval of first-inspector-code-load builds a program of
/// 31K functions whose top level alone derives a view, and kept a 250 KB table written whole.
enum LaneViewTable {
    Boxed(Box<[LaneViewCell]>),
    Mapped { base: std::ptr::NonNull<LaneViewCell>, len: usize, bytes: usize },
}

/// The smallest table that takes its own mapping, which costs two system calls.
const MAPPED_LANE_VIEW_TABLE_BYTES: usize = 4 * super::site_table::MAPPING_GRANULE_BYTES;

// An empty cell is zero bits, which a fresh mapping holds.
const _: () = assert!(
    // SAFETY: a cell is one pointer-sized `Option<NonNull>`, whose `None` this reads.
    unsafe { std::mem::transmute::<LaneViewCell, usize>(std::cell::OnceCell::new()) } == 0
);

impl LaneViewTable {
    fn new(len: usize) -> Self {
        let bytes = len * size_of::<LaneViewCell>();
        if bytes < MAPPED_LANE_VIEW_TABLE_BYTES {
            return Self::Boxed((0..len).map(|_| std::cell::OnceCell::new()).collect());
        }
        let bytes = bytes.next_multiple_of(super::site_table::MAPPING_GRANULE_BYTES);
        let base = super::site_table::storage::map_zeroed(bytes).cast();
        Self::Mapped { base, len, bytes }
    }
}

impl std::ops::Deref for LaneViewTable {
    type Target = [LaneViewCell];

    #[inline(always)]
    fn deref(&self) -> &[LaneViewCell] {
        match self {
            Self::Boxed(cells) => cells,
            // SAFETY: the mapping holds `len` cells, zero bits being empty ones, for the table's
            // life.
            Self::Mapped { base, len, .. } => unsafe { std::slice::from_raw_parts(base.as_ptr(), *len) },
        }
    }
}

impl Drop for LaneViewTable {
    fn drop(&mut self) {
        if let Self::Mapped { base, bytes, .. } = *self {
            // SAFETY: the mapping came from `map_zeroed` with these bytes, and the table is its
            // only reader.
            unsafe { super::site_table::storage::unmap(base.cast(), bytes) };
        }
    }
}

/// One function's derived lane view, in its program's arena (`LaneViews::arena`).
pub(super) type LaneViewCell = std::cell::OnceCell<std::ptr::NonNull<crate::vm::engine_types::LaneView>>;

/// The view a cell holds, if its function's was derived.
#[inline(always)] pub fn derived_lane_view(cell: &LaneViewCell) -> Option<&crate::vm::engine_types::LaneView> {
    // SAFETY: a cell points into its table's arena (`LaneViews::store`), which outlives it.
    cell.get().map(|view| unsafe { view.as_ref() })
}

/// `function`'s view in its cell `view`, which `derive` makes from `code` if it has none yet.
fn store_lane_view<'s>(
    entry: &'s ProgramEntry,
    function: u32,
    code: &crate::bytecode::Function,
    view: &LaneViewCell,
    derive: impl FnOnce(
        &crate::bytecode::Function,
        &[Value],
        crate::bytecode::ProgramKind,
        &dyn Fn() -> Option<&'s std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
        &[crate::bytecode::FieldSite],
    ) -> crate::vm::engine_types::LaneView,
) -> *const crate::vm::engine_types::LaneView {
    view.get_or_init(|| {
        // Only a loop tier reads the masks; a collection derives them otherwise.
        let live = || entry.residual.shared_register_root_masks(function);
        entry.residual.pin_tier0(function as usize);
        let view = derive(code, &entry.constants, entry.residual.kind, &live, &entry.residual.field_sites);
        #[cfg(feature = "profile-lane-exits")]
        {
            let (start, end) = view.record_range();
            super::lane_exits::register_view(start, end, census_label(entry, function), code.instructions().map(|instruction| instruction.op()).collect());
        }
        entry.views_in_lane.store(view)
    })
    .as_ptr()
}

impl LaneViews {
    /// Notifies every derived view that shape compaction invalidated its bindings.
    fn invalidate_shape_bindings(&self) {
        for view in self.arena.borrow().iter().flatten() {
            view.invalidate_shape_bindings();
        }
    }

    /// The table, one cell per function of `residual`. Its address is fixed once made, as
    /// the lane context's pointer to it requires.
    #[inline(always)]
    fn of(&self, residual: &ResidualProgram) -> &[LaneViewCell] {
        self.table.get().map_or_else(|| self.make_table(residual), |table| table)
    }

    #[cold]
    fn make_table(&self, residual: &ResidualProgram) -> &[LaneViewCell] {
        self.table.get_or_init(|| LaneViewTable::new(residual.functions.capacity()))
    }

    /// `function`'s view if it has one, without making the table.
    fn derived(&self, function: u32) -> Option<&crate::vm::engine_types::LaneView> {
        derived_lane_view(self.table.get()?.get(function as usize)?)
    }

    /// `view` moved into the arena, at an address fixed while the arena holds it.
    fn store(&self, view: crate::vm::engine_types::LaneView) -> std::ptr::NonNull<crate::vm::engine_types::LaneView> {
        let mut arena = self.arena.borrow_mut();
        if arena.last().is_none_or(|chunk| chunk.len() == chunk.capacity()) {
            let held: usize = arena.iter().map(Vec::len).sum();
            arena.push(Vec::with_capacity(held.max(LANE_VIEW_FIRST_CHUNK)));
        }
        let chunk = arena.last_mut().expect("the arena has a chunk with room");
        chunk.push(view);
        let stored = std::ptr::NonNull::from(chunk.last().expect("the view just stored"));
        stored
    }
}

// Profiled builds deliberately disable JS function tiers (`Vm::tier_may_run`).
#[cfg(all(test, not(quench_profiled)))]
mod lane_diagnostic_tests {
    #[test]
    fn plain_lane_calls_remain_visible_after_numeric_tier_rejection() {
        let program = crate::Engine::compile(crate::ExecutionRequest::script(
            r#"
                function label(value) { return value + '!'; }
                function drive(count) {
                    let result;
                    for (let index = 0; index < count; index++) result = label(index);
                    return result;
                }
                for (let round = 0; round < 4; round++) {
                    if (drive(2000) !== '1999!') throw Error('lane call result');
                }
            "#,
            "lane-diagnostic-entries.js",
        )).unwrap();
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.programs.enable_js_jit_diagnostics();
        vm.execute(&program).unwrap();
        let counters = vm.programs.js_jit_diagnostics.as_ref().unwrap().values()
            .find(|counters| counters.function_name.as_deref() == Some("label"))
            .expect("the called function must have diagnostic counters");
        assert!(counters.lane_entries > 1000,
            "hot plain calls must stay visible after tier rejection: {} tier entries, {} lane entries",
            counters.entries, counters.lane_entries);
    }
}

#[cfg(test)]
mod jit_code_cache_tests {
    use crate::vm::tier_policy::TierRows;
    use super::{
        JIT_MAX_ARTIFACTS, JIT_MAX_MAPPED_BYTES, JS_JIT_MAX_FUNCTIONS,
        JS_JIT_MAX_MAPPED_BYTES, JitArtifactKind, JitCodeCacheBudget,
        WASM_JIT_MAX_TIER_MAPPED_BYTES, WASM_JIT_MAX_TIER_ROOTS,
    };

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
        assert!(cache.can_retain(JitArtifactKind::JsFunction, 128 * 1024));
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
    }

    #[test]
    fn jit_tier_manager_rejects_cross_frontend_state_for_one_program() {
        let mut tiers = super::JitTierManager::default();
        assert!(tiers.js_insert(3, super::JsTierState::Unsupported));
        assert!(!tiers.wasm_unattempted(3));
        assert!(!tiers.claim_wasm_root(3));
        assert!(matches!(tiers.js(3), Some(super::JsTierState::Unsupported)));
        assert_eq!(tiers.js_values().count(), 1);
        assert_eq!(tiers.wasm_values().count(), 0);

        let mut wasm_tiers = super::JitTierManager::default();
        assert!(wasm_tiers.claim_wasm_root(4));
        assert!(!wasm_tiers.js_insert(3, super::JsTierState::Unsupported));
        assert_eq!(wasm_tiers.wasm_values().count(), 1);
        assert!(!wasm_tiers.note_js_entry(3, true, true));
    }

    #[test]
    fn jit_tier_manager_owns_frontend_promotion_transitions() {
        let mut js_tiers = super::JitTierManager::default();
        assert!(js_tiers.note_js_entry(1, true, true));
        assert!(!js_tiers.note_js_entry(1, false, true));
        assert!(!js_tiers.note_js_entry(2, true, false));

        let mut wasm_tiers = super::JitTierManager::default();
        assert!(wasm_tiers.wasm_unattempted(2));
        assert!(wasm_tiers.claim_wasm_root(2));
        assert!(!wasm_tiers.wasm_unattempted(2));
        assert!(!wasm_tiers.claim_wasm_root(2));
        assert!(matches!(
            wasm_tiers.wasm(2),
            Some(super::WasmTierState::Attempted)
        ));
    }
}

/// A program's `JsEntryFacts` by function, the table made on the tier's first entry of one.
#[derive(Default)]
pub(super) struct JsEntryFactsTable(
    std::cell::OnceCell<Box<[std::cell::OnceCell<Rc<crate::vm::engine_types::JsEntryFacts>>]>>,
);

// An empty cell is zero bits, which the zeroed table relies on.
const _: () = assert!(
    // SAFETY: a cell is one pointer-sized `Option<Rc<_>>`, whose `None` this reads.
    unsafe { std::mem::transmute::<std::cell::OnceCell<Rc<crate::vm::engine_types::JsEntryFacts>>, usize>(std::cell::OnceCell::new()) } == 0
);

/// `len` empty entry-fact cells on zero pages: a program's table takes a cell for each function
/// it may append, and a filled table would touch every page of them at the first JavaScript
/// tier entry of any function. Only the pages of functions that enter the tier become resident.
fn zeroed_entry_facts(len: usize) -> Box<[std::cell::OnceCell<Rc<crate::vm::engine_types::JsEntryFacts>>]> {
    let zeros: Box<[usize]> = vec![0; len].into_boxed_slice();
    let cells = Box::into_raw(zeros) as *mut [std::cell::OnceCell<Rc<crate::vm::engine_types::JsEntryFacts>>];
    // SAFETY: the allocation is `len` words, the layout of `len` cells of one pointer each
    // (asserted above), zeroed, so every cell is empty; the box alone owns it.
    unsafe { Box::from_raw(cells) }
}

/// Counts a comparison in the guard census: strict (in)equality in its third column, an
/// order (`<`, `<=`, `>`, `>=`) in its fourth.
fn count_comparison_guard(census: &mut (u32, u32, u32, u32), operator: u32) {
    use crate::operator::BinaryOperator;
    const STRICT: [BinaryOperator; 2] = [BinaryOperator::StrictEquality, BinaryOperator::StrictInequality];
    const ORDER: [BinaryOperator; 4] = [
        BinaryOperator::LessThan,
        BinaryOperator::LessEqualThan,
        BinaryOperator::GreaterThan,
        BinaryOperator::GreaterEqualThan,
    ];
    let named = |operators: &[BinaryOperator]| operators.iter().any(|&named| named as u32 == operator);
    if named(&STRICT) {
        census.2 = census.2.saturating_add(1);
    } else if named(&ORDER) {
        census.3 = census.3.saturating_add(1);
    }
}

/// `len` zero counts on zero pages: a program's per-function counts take a slot for each
/// function it may append, which a filled table would touch at its load.
pub(super) fn zeroed_counts(len: usize) -> Vec<std::cell::Cell<u8>> {
    let mut zeros = std::mem::ManuallyDrop::new(vec![0u8; len]);
    // SAFETY: `Cell<u8>` has the layout of `u8`, so the zeroed buffer is `len` counts of 0,
    // owned by the new vector alone.
    unsafe { Vec::from_raw_parts(zeros.as_mut_ptr().cast::<std::cell::Cell<u8>>(), zeros.len(), zeros.capacity()) }
}

/// The tiers of a Wasm entry made where no compile is (`wasm_activation::offer_entry`): the lane's.
/// It reads the root tiers, and hands a compile that is due to the general loop, which makes it at the same call
/// (`Compile::Leave`).
pub struct HandoffTiers<'a> {
    pub store: &'a mut ProgramStore,
    pub program: ProgramId,
}

impl crate::staged::wasm_activation::WasmEntryTiers for HandoffTiers<'_> {
    type Code = Rc<crate::vm::engine_types::CompiledProgram>;

    #[inline(always)]
    fn root(&mut self, function: u32) -> Option<Self::Code> {
        self.store.wasm_tier_containing(self.program, function)
    }

    #[inline(always)]
    fn root_due(&mut self, function: u32) -> bool {
        self.store.wasm_root_due(self.program, function)
    }

    fn compile_root(&mut self, _function: u32) -> crate::staged::wasm_activation::Compile {
        crate::staged::wasm_activation::Compile::Leave
    }
}
