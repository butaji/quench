//! The one admission policy for compiles: every question that decides whether a callee or a
//! body compiles, and every reason it does not, lives here. A refused compile records its
//! `Refusal`; `QUENCH_REFUSALS=1` prints the counts at exit (`refusal_report`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_unit::CalleeBinding;
use super::tier_model::{Observed, Pick};
use super::tier_policy::Tiers;
use quench_runtime::bytecode::{Function, ResidualProgram};
use crate::dispatch_fast::lane_body_stats::{miss, Miss};
use crate::dispatch_fast::LaneView;
use quench_runtime::vm::jit_backend::CompileLevel;
use quench_runtime::vm::Vm;

/// Why a compile or a callee's admission is refused. Each variant is one refusal site, counted
/// by `record` and named in `refusal_report`. The OSR region reasons (`RegionRejection`) share
/// this enum through `From`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The compile does not repay its entries (`tier_model::Pick::Interpreter`, `PlanNotAmortized`).
    NotAmortized,
    /// The callee may not run inline (`CalleeUse::Inlined`) or frameless (`CalleeUse::Frameless`).
    CalleeNotEligible,
    /// The unit does not plan from the function's body (`lane_unit::with_trips`).
    NoUnit,
    /// The unit's call bindings are not the ones the plan keeps.
    InvalidBindings,
    /// An inlined frame of the unit is not admitted.
    FrameNotAdmitted,
    /// The unit has no instruction at the entry header.
    NoEntry,
    /// The compiles left for the site, or the compile bound, are spent.
    CompileBound,
    /// The lowering produced no native code.
    NothingNative,
    /// The compiled body's dependencies would not subscribe.
    Dependencies,
    // OSR region refusals (`osr_loop::RegionRejection`, converted by `From`).
    RegionTooLarge,
    UnsupportedInstruction(u16),
    RegisterLimit,
    CaptureLimit,
    ArrayLimit,
    FieldLimit,
    FieldStoreLimit,
    FieldMutation,
    ArrayRedefined,
}

impl Refusal {
    /// The report's names, in `slot` order.
    const NAMES: [&'static str; 18] = [
        "not-amortized", "callee-not-eligible", "no-unit", "invalid-bindings", "frame-not-admitted",
        "no-entry", "compile-bound", "nothing-native", "dependencies",
        "region-too-large", "unsupported-instruction", "register-limit", "capture-limit", "array-limit",
        "field-limit", "field-store-limit", "field-mutation", "array-redefined",
    ];

    /// The counter this refusal counts in (`NAMES`).
    fn slot(self) -> usize {
        match self {
            Self::NotAmortized => 0,
            Self::CalleeNotEligible => 1,
            Self::NoUnit => 2,
            Self::InvalidBindings => 3,
            Self::FrameNotAdmitted => 4,
            Self::NoEntry => 5,
            Self::CompileBound => 6,
            Self::NothingNative => 7,
            Self::Dependencies => 8,
            Self::RegionTooLarge => 9,
            Self::UnsupportedInstruction(_) => 10,
            Self::RegisterLimit => 11,
            Self::CaptureLimit => 12,
            Self::ArrayLimit => 13,
            Self::FieldLimit => 14,
            Self::FieldStoreLimit => 15,
            Self::FieldMutation => 16,
            Self::ArrayRedefined => 17,
        }
    }

    /// The plan-stage miss the lane diagnostics count this refusal as, if it has one.
    fn miss(self) -> Option<Miss> {
        Some(match self {
            Self::NotAmortized => Miss::PlanNotAmortized,
            Self::NoUnit => Miss::PlanNoUnit,
            Self::InvalidBindings => Miss::PlanInvalidBindings,
            Self::FrameNotAdmitted => Miss::PlanFrameNotAdmitted,
            Self::NoEntry => Miss::PlanNoEntry,
            Self::CompileBound => Miss::PlanCompileBound,
            Self::NothingNative => Miss::PlanNothingNative,
            Self::Dependencies => Miss::PlanDependencies,
            _ => return None,
        })
    }

    /// Whether the refusal is an expensive fallback (`quench_runtime::fallback`): a unit that wanted
    /// to compile and could not. The tier model's pricing and the compile bound decide that a
    /// compile does not pay, which is policy, not a fallback.
    fn is_fallback(self) -> bool {
        !matches!(self, Self::NotAmortized | Self::CompileBound)
    }

    /// The refusal's report name, with the operation an unsupported instruction names.
    pub(crate) fn describe(self) -> String {
        let name = Self::NAMES[self.slot()];
        match self {
            Self::UnsupportedInstruction(op) => match quench_runtime::bytecode::Op::from_index(usize::from(op)) {
                Some(op) => format!("{name} {op:?}"),
                None => format!("{name} {op}"),
            },
            _ => name.to_owned(),
        }
    }
}

/// Refusals counted since the process started, one per `Refusal::slot`. Atomics rather than
/// thread-local cells, so the exit report sees the compiles of every thread.
static COUNTS: [std::sync::atomic::AtomicU64; Refusal::NAMES.len()] = [const { std::sync::atomic::AtomicU64::new(0) }; Refusal::NAMES.len()];
static REPORT: std::sync::Once = std::sync::Once::new();

/// Whether refusals print at exit (`QUENCH_REFUSALS=1`).
fn reporting() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("QUENCH_REFUSALS").is_ok_and(|value| value == "1"))
}

/// Counts `refusal`, and registers the exit report on the first count when reporting is on.
pub(crate) fn record(refusal: Refusal) {
    COUNTS[refusal.slot()].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if quench_runtime::fallback::on() && refusal.is_fallback() {
        quench_runtime::fallback::note_in_focus(quench_runtime::fallback::Fallback::JitReject, format_args!("{}", refusal.describe()));
    }
    if reporting() {
        REPORT.call_once(|| {
            // SAFETY: `report` is a plain function that outlives the process.
            unsafe { libc::atexit(report) };
        });
    }
}

/// Counts `refusal` and, where the lane diagnostics name it as a plan miss, counts that too
/// (`lane_body_stats::miss`, which also names the decline reason).
pub(crate) fn refuse(vm: &Vm, refusal: Refusal) {
    record(refusal);
    if let Some(reason) = refusal.miss() { miss(vm, reason); }
}

/// The refusals counted so far, `QUENCH_REFUSALS {...}`, most frequent first; only the nonzero ones.
pub fn refusal_report() -> String {
    let mut counts: Vec<(&str, u64)> = Refusal::NAMES.iter().zip(COUNTS.iter())
        .map(|(name, count)| (*name, count.load(std::sync::atomic::Ordering::Relaxed)))
        .filter(|(_, count)| *count != 0).collect();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let fields = counts.iter().map(|(name, count)| format!("\"{name}\":{count}")).collect::<Vec<_>>().join(",");
    format!("QUENCH_REFUSALS {{{fields}}}")
}

/// Prints `refusal_report` at process exit.
extern "C" fn report() {
    eprintln!("{}", refusal_report());
}

/// The kinds of unit whose tier gate `may_compile` decides.
#[derive(Clone, Copy)]
pub(crate) enum UnitKind {
    /// A function's lane body: u-jit under the Cranelift tiers, or b-jit stencils under `--b-jit`.
    Body,
    /// A loop region, under its memory policy's loop tier.
    Region(quench_runtime::MemoryPolicy),
    /// An ahead image's body of this function, lowered at build time or installed at run time: the one
    /// AOT kind. No JIT mode or memory policy gates it, only the `AotBodies` trace range.
    Ahead(u32),
    /// A b-jit (stencil) body: its facts decide it (`specialize`).
    Base,
    /// A Wasm function's specialization (its lowered body and entry): always cleared.
    Wasm,
    /// A Wasm module compiled whole at its first entry, when the runtime's first-entry flag allows it.
    WasmEager,
    /// A hot regexp program's native search (`regexp_native::start`): always cleared, since only a VM
    /// under a compiling mode counts its searches toward one (`regexp_native::enable`).
    Regexp,
    /// A JavaScript function's numeric or lane compile at an entry, when the Cranelift tiers run here.
    Entry,
    /// A Wasm program's module compile at a root (`Vm::wasm_module_unit`): the program is Wasm, and a
    /// root-only compile names its root.
    WasmModule,
    /// A lane body's plan from an entry, before planning (phase A): the compile bound, the pick the entries
    /// price, the looped iterations' payment. Its unit is the proof every later lane gate needs.
    LanePlan,
    /// A lane body's entry gate during planning (phase B, inside the plan): the plan's unit, the amortization
    /// of its entry, and the callers that want it anyway.
    LaneEntry(CompiledUnit),
    /// A lane body's install gate after planning (phase B, before the plan binds its callees): the plan's
    /// unit, the callers of regions, the observed and looped entries, and the amortization of its entry.
    LaneInstall(CompiledUnit),
    /// A JavaScript function claimed by a tier at an entry: the Cranelift tiers, or lane bodies on the
    /// stencil tier, for a function that is not Wasm.
    Claim,
}

/// What the gate's decisions read of the unit, computed by the entry that asks, and carried by the
/// request so proven facts can flow through the same path.
#[derive(Clone, Copy, Default)]
pub(crate) struct UnitFacts {
    /// The body's rows cover the stencil tier (`base_region::covers`). A loop with an interpreter
    /// kernel runs the kernel on its entry edge (`base_region::kernel_entries`).
    pub(crate) covers: bool,
    /// The body amortizes its b-jit entry (`body_may_amortize` for `Tier::Base`).
    pub(crate) amortizes: bool,
    /// The caller is native code or a body that calls the function (`LaneView::called_by_bodies`).
    pub(crate) called_by_bodies: bool,
    /// The body indexes a parameter that is a typed array (`indexes_typed_parameter`).
    pub(crate) indexes_typed: bool,
    /// The Cranelift function tiers run in this VM (`Vm::tier_may_run`), for `UnitKind::Entry`.
    pub(crate) cranelift_allowed: bool,
    /// The runtime compiles Wasm modules whole at their first entry (`wasm_modules_compile_at_first_entry`).
    pub(crate) wasm_first_entry: bool,
    /// The program is a Wasm program (`ProgramKind::Wasm`), for `UnitKind::WasmModule`.
    pub(crate) wasm_program: bool,
    /// A root-only compile names its root (`WasmModule`).
    pub(crate) root_ok: bool,
    /// Lane phase A: the compile bound leaves room for a compile from this entry (`compiles_left_in_lane`).
    pub(crate) compiles_left: bool,
    /// Lane phase A: the pick the entries price (`pays_compile`), where the entries were observed and the
    /// body is not optimizing.
    pub(crate) pick: Option<Pick>,
    /// The caller is a loop region, which calls the body from native code (`LaneView::called_by_regions`).
    pub(crate) called_by_regions: bool,
    /// Lane phase A: the looped iterations pay for the body at its planned size (`loop_model::body_taken`).
    pub(crate) looped_taken: bool,
    /// The compile is from the function's own entry, not a loop's (`entry.is_none()`).
    pub(crate) entry_none: bool,
    /// The function calls itself back through other functions (`recursive`).
    pub(crate) recursive: bool,
    /// Lane phase B: the entry counts were not observed (`observed.is_none()`).
    pub(crate) observed_none: bool,
    /// Lane phase B: no looped iterations were observed (`looped.is_none()`).
    pub(crate) looped_none: bool,
    /// The body's entry amortizes its work (`lane_facts::amortizes_entry`), or the gate does not ask.
    pub(crate) amortizes_entry: bool,
    /// A lane body may run on the stencil tier here: lane methods are enabled and b-jit runs (`UnitKind::Claim`).
    pub(crate) base_lane_allowed: bool,
}

/// Whether a unit of `kind` may compile at all, under the process's JIT mode and, for a loop
/// region, its memory policy. Only `specialize` asks this.
fn may_compile(kind: UnitKind) -> bool {
    match kind {
        UnitKind::Body => quench_runtime::jit_mode::tiers_allowed(quench_runtime::jit_mode::Tier::CRANELIFT) || quench_runtime::jit_mode::base_only(),
        UnitKind::Region(policy) => quench_runtime::vm::tier_policy::by_policy(policy).loop_tier,
        UnitKind::Ahead(function) => within_trace_range(quench_runtime::trace::Dev::AotBodies, function),
        UnitKind::Wasm | UnitKind::Base | UnitKind::Regexp => true,
        UnitKind::WasmEager | UnitKind::Entry | UnitKind::WasmModule | UnitKind::LanePlan | UnitKind::LaneEntry(_) | UnitKind::LaneInstall(_) | UnitKind::Claim =>
            unreachable!("facts decide these kinds, in `specialize`"),
    }
}

/// Whether the trace setting `setting` (a `LOOP_RANGE` style `low-high` range, or off) admits
/// `function`. Off admits every function.
pub(crate) fn within_trace_range(setting: quench_runtime::trace::Dev, function: u32) -> bool {
    !setting.on() || setting.value()
        .and_then(|range| range.split_once('-'))
        .and_then(|(low, high)| Some((low.parse::<u32>().ok()?, high.parse::<u32>().ok()?)))
        .is_some_and(|(low, high)| (low..=high).contains(&function))
}

/// The unit of an ahead image's body of `function`, where the trace range admits it.
pub(crate) fn ahead_unit(function: u32) -> Option<CompiledUnit> {
    specialize(UnitRequest { kind: UnitKind::Ahead(function), facts: UnitFacts::default() }).compiled()
}

/// One request to specialize a unit (`specialize`): what it is and the facts its decisions read.
#[derive(Clone, Copy)]
pub(crate) struct UnitRequest {
    pub(crate) kind: UnitKind,
    pub(crate) facts: UnitFacts,
}

/// The outcome of `specialize`: a unit cleared to compile, a body too short to amortize its
/// entry (`Short`, refused as `NotAmortized`), or declined by the mode, the policy or its facts.
pub(crate) enum Decision<T> {
    Compile(T),
    Short,
    Declined,
    /// The body does not repay its compile yet (`LaneBodyOutcome::Unpaid`).
    Unpaid,
    /// The stencil tier serves the body (`LaneBodyOutcome::Base`).
    Base,
}

impl<T> Decision<T> {
    /// The unit, where the decision cleared it.
    pub(crate) fn compiled(self) -> Option<T> {
        match self { Self::Compile(unit) => Some(unit), _ => None }
    }
}

/// A unit cleared to compile. Its fields are private to this module, so `specialize` is the
/// only way to obtain one; copies of it are the same proof.
#[derive(Clone, Copy)]
pub(crate) struct CompiledUnit {
    /// The most machine code the unit may compile to (`code_ceiling`).
    ceiling: usize,
}

impl CompiledUnit {
    /// The most machine code the unit may compile to: the specializer's one code-size policy,
    /// by the kind of unit, for every language's compile (`compile_function`'s budget).
    pub(crate) fn code_ceiling(self) -> usize { self.ceiling }
}

/// The code-size policy of a unit of `kind`: a JavaScript function's numeric entry, a regexp
/// program's search, or a lane body or region (every other kind).
fn code_ceiling(kind: UnitKind) -> usize {
    use quench_runtime::vm::tier_policy::{AHEAD_BODY_MAX_CODE_BYTES, JS_COMPILED_MAX_CODE_BYTES, LANE_BODY_MAX_CODE_BYTES, REGEXP_NATIVE_MAX_CODE_BYTES};
    match kind {
        UnitKind::Entry => JS_COMPILED_MAX_CODE_BYTES,
        UnitKind::Regexp => REGEXP_NATIVE_MAX_CODE_BYTES,
        // An image body compiles once, offline: the JIT lane body's warm-up bound is not its cost.
        UnitKind::Ahead(_) => AHEAD_BODY_MAX_CODE_BYTES,
        _ => LANE_BODY_MAX_CODE_BYTES,
    }
}

/// A unit minted for a test, through `specialize` (an AOT unit of the root, which no mode gates).
#[cfg(test)]
pub(crate) fn test_unit() -> CompiledUnit {
    ahead_unit(0).expect("an AOT unit always compiles")
}

/// The one specializer entry: whether the request's unit may compile, under the process's JIT
/// mode and memory policy. Every compile entry obtains its unit here.
pub(crate) fn specialize(request: UnitRequest) -> Decision<CompiledUnit> {
    let UnitRequest { kind, facts } = request;
    // Each kind's gates run in the order its entry always ran them; the first that refuses decides.
    let cleared = match kind {
        UnitKind::Base => {
            if !facts.covers { return Decision::Declined; }
            if !facts.amortizes && !facts.called_by_bodies { return Decision::Short; }
            !facts.indexes_typed
        }
        UnitKind::WasmEager => facts.wasm_first_entry,
        UnitKind::Claim => (facts.cranelift_allowed || facts.base_lane_allowed) && !facts.wasm_program,
        UnitKind::Entry => facts.cranelift_allowed,
        UnitKind::WasmModule => facts.wasm_program && facts.root_ok,
        // Phase A: the compile bound, then the pick, then the looped iterations.
        UnitKind::LanePlan => {
            if !facts.compiles_left { return Decision::Declined; }
            match facts.pick {
                Some(Pick::Interpreter) if !facts.called_by_regions => { record(Refusal::NotAmortized); return Decision::Unpaid; }
                Some(Pick::Base) => return Decision::Base,
                _ => {}
            }
            facts.looped_taken
        }
        // Phase B, during planning: the entry gate.
        UnitKind::LaneEntry(_) => {
            if facts.entry_none && !facts.called_by_bodies && !facts.recursive && !facts.amortizes_entry { return Decision::Short; }
            true
        }
        // Phase B, after planning: the install gate.
        UnitKind::LaneInstall(_) => {
            if facts.entry_none && !facts.called_by_regions && facts.observed_none && facts.looped_none && !facts.recursive && !facts.amortizes_entry { return Decision::Short; }
            true
        }
        UnitKind::Body | UnitKind::Region(_) | UnitKind::Ahead(_) | UnitKind::Wasm | UnitKind::Regexp => may_compile(kind),
    };
    if cleared && bisected_out() { return Decision::Declined; }
    if cleared { Decision::Compile(CompiledUnit { ceiling: code_ceiling(kind) }) } else { Decision::Declined }
}

/// Whether the `unit-bisect` range leaves out the unit `specialize` is about to clear: each cleared
/// unit takes the next number, and only numbers in the range compile.
fn bisected_out() -> bool {
    use quench_runtime::trace::Dev::UnitBisect;
    if !UnitBisect.on() { return false; }
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if UnitBisect.value().is_none_or(str::is_empty) {
        eprintln!("unit-bisect {number}");
        return false;
    }
    !within_trace_range(UnitBisect, number)
}

/// The tiers of the JIT mode, as the specializer names them.
pub(crate) use quench_runtime::jit_mode::Tier;

/// Whether the process's JIT mode lets `tiers` run: the specializer's one reading of the mode,
/// which every other tier decision of the engine takes from here.
#[inline]
pub(crate) fn tier_runs(tiers: &[Tier]) -> bool { quench_runtime::jit_mode::tiers_allowed(tiers) }

/// Whether the stencil tier is the only tier (`--b-jit`).
#[inline]
pub(crate) fn stencil_only() -> bool { quench_runtime::jit_mode::base_only() }

/// Whether loop regions take the stencil tier in place of Cranelift (`jit_mode::base_regions`).
#[inline]
pub(crate) fn stencil_regions() -> bool { quench_runtime::jit_mode::base_regions() }

/// How a function's interpreter records specialize (`dispatch_fast::derive_lane_view`): the
/// interpreter's own projection of the function, decided here as every compiled unit is.
#[derive(Clone, Copy)]
pub(crate) struct RecordPlan {
    /// Loop records keep tier sites (`install_loop_sites`) and their jumps stay visible to them.
    pub(crate) loop_sites: bool,
    /// The int32 kinds may speculate: the interpreter retires a speculation whose `IncDec`
    /// overflows, and the lane retires them once native code, whose updates retire nothing,
    /// hands it an activation (`retire_after_native`).
    pub(crate) speculates: bool,
}

/// The record plan of a function whose tiers may run (`tiers_run`: `Vm::tier_may_run`, any tier).
pub(crate) fn record_plan(tiers_run: bool) -> RecordPlan {
    RecordPlan { loop_sites: tiers_run, speculates: crate::dispatch_fast::int_kinds::speculation_allowed() }
}

/// Retires the speculative int32 kinds (`RecordPlan::speculates`) after native code ran an activation
/// the lane goes on in (`exit` names a record of it or a callee it made, or `frames_left` native frames
/// it left): a native tier steps a register as a double without the interpreter's retirement (a widened
/// stencil row, a Cranelift float increment), so the records reading it take their checked forms first.
/// A run that completed its activation and left no native frame hands back nothing.
#[inline(always)]
pub(crate) fn retire_after_native(vm: &quench_runtime::vm::Vm, exit: crate::compiled::LaneExit, frames_left: impl FnOnce() -> bool) {
    if crate::dispatch_fast::int_kinds::speculation_allowed() && (!matches!(exit, crate::compiled::LaneExit::Returned { .. }) || frames_left()) {
        vm.retire_int_speculation();
    }
}

/// Where a function's lane body comes from (`route_for_lane`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LaneRoute {
    /// The program's loaded AOT image serves it (`install_ahead_lane_body`): no warm-up, no runtime tier.
    Ahead,
    /// The stencil tier stitches it at once (`--b-jit`).
    Base,
    /// It waits for callees that still compile, to call their bodies by their entries.
    Deferred,
    /// The lane body tier plans it (`LanePlan` and the gates after it).
    Lane,
}

/// The facts `route_for_lane` reads.
#[derive(Clone, Copy, Default)]
pub(crate) struct LaneRouteFacts {
    /// The function's program has a loaded AOT image.
    pub(crate) ahead_image: bool,
    /// The process runs the stencil tier only (`jit_mode::base_only`).
    pub(crate) base_only: bool,
    /// A start whose direct callees still compile (`body_tier_in_lane::callees_compiling`).
    pub(crate) callees_compiling: bool,
}

/// The route of a function's lane body: the specializer decides it, as it decides each unit after.
pub(crate) fn route_for_lane(facts: LaneRouteFacts) -> LaneRoute {
    if facts.ahead_image { LaneRoute::Ahead }
    else if facts.base_only { LaneRoute::Base }
    else if facts.callees_compiling { LaneRoute::Deferred }
    else { LaneRoute::Lane }
}

/// How a callee would run: inlined into a lane unit under the call's `CalleeBinding`, or called
/// frameless.
#[derive(Clone, Copy)]
pub(crate) enum CalleeUse {
    Inlined(CalleeBinding),
    Frameless,
}

/// Whether `callee` may run as `how`. An inlined callee bound by identity is admitted by its
/// call's guard; a function-bound one must be a plain activation the compiled tier takes. A
/// frameless callee runs in a loop with registers only, and takes no rest parameters. A refusal
/// is counted.
pub(crate) fn admit_callee(program: &ResidualProgram, callee: &Function, how: CalleeUse) -> Result<(), Refusal> {
    let admitted = match how {
        CalleeUse::Inlined(binding) => binding != CalleeBinding::Function
            || (callee.plain_activation(program) && crate::js_compiled::eligible(callee, program)),
        CalleeUse::Frameless => {
            let plan = callee.call_plan(program);
            plan.in_loop && plan.registers_only && !callee.rest
        }
    };
    if admitted { Ok(()) } else {
        record(Refusal::CalleeNotEligible);
        Err(Refusal::CalleeNotEligible)
    }
}

/// The compile level of `function`'s u-jit body, admitted at `entries` with `ir` planned
/// instructions: `Quick` where the entries to come gain more from its cheaper compile than they
/// lose to its slower code (`tier_model::compiles_quick`), else `Optimized`.
pub(crate) fn compile_level(tiers: &Tiers, program: &ResidualProgram, function: u32, entries: u32, ir: usize, caller: &LaneView) -> CompileLevel {
    let length = program.function_get(function as usize).map_or(0, |body| body.code.len()) as u64;
    let observed = Observed { length, loop_work: caller.loop_work(), entries, region_runs: caller.region_runs(), ir };
    if super::tier_model::compiles_quick(tiers, &observed, true) { CompileLevel::Quick } else { CompileLevel::Optimized }
}

impl From<super::osr_loop::RegionRejection> for Refusal {
    fn from(reason: super::osr_loop::RegionRejection) -> Self {
        use super::osr_loop::RegionRejection as R;
        match reason {
            R::RegionTooLarge => Self::RegionTooLarge,
            R::UnsupportedInstruction(op) => Self::UnsupportedInstruction(op),
            R::RegisterLimit => Self::RegisterLimit,
            R::CaptureLimit => Self::CaptureLimit,
            R::ArrayLimit => Self::ArrayLimit,
            R::FieldLimit => Self::FieldLimit,
            R::FieldStoreLimit => Self::FieldStoreLimit,
            R::FieldMutation => Self::FieldMutation,
            R::ArrayRedefined => Self::ArrayRedefined,
        }
    }
}
