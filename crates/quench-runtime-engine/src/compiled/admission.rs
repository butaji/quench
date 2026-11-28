//! The one admission policy for compiles: every question that decides whether a callee or a
//! body compiles, and every reason it does not, lives here. A refused compile records its
//! `Refusal`.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_unit::CalleeBinding;
use quench_runtime::vm::tier_policy::{lane_body_tier, PredictedTier};
use quench_runtime::bytecode::{Function, ResidualProgram};
use crate::dispatch_fast::lane_body_stats::{miss, Miss};
use crate::dispatch_fast::LaneView;
use quench_runtime::vm::Vm;

/// Why a compile or a callee's admission is refused. Each variant is one refusal site. The OSR region reasons (`RegionRejection`) share this enum
/// through `From`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The predicted tier is the interpreter (`PredictedTier::Interpreter`, `PlanNotAmortized`).
    NotAmortized,
    /// The callee may not run inline (`CalleeUse::Inlined`) or frameless (`CalleeUse::Frameless`).
    CalleeNotEligible,
    /// The unit does not plan from the function's body (`lane_unit::unit`).
    NoUnit,
    /// An inlined frame of the unit is not admitted.
    FrameNotAdmitted,
    /// The unit has no instruction at the entry header.
    NoEntry,
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
    /// The plan-stage miss the lane diagnostics count this refusal as, if it has one.
    fn miss(self) -> Option<Miss> {
        Some(match self {
            Self::NotAmortized => Miss::PlanNotAmortized,
            Self::NoUnit => Miss::PlanNoUnit,
            Self::FrameNotAdmitted => Miss::PlanFrameNotAdmitted,
            Self::NoEntry => Miss::PlanNoEntry,
            Self::NothingNative => Miss::PlanNothingNative,
            Self::Dependencies => Miss::PlanDependencies,
            _ => return None,
        })
    }

}

/// Where the lane diagnostics name `refusal` as a plan miss, counts it (`lane_body_stats::miss`,
/// which also names the decline reason).
pub(crate) fn refuse(vm: &Vm, refusal: Refusal) {
    if let Some(reason) = refusal.miss() { miss(vm, reason); }
}

/// The kinds of unit whose tier gate `may_compile` decides.
#[derive(Clone, Copy)]
pub(crate) enum UnitKind {
    /// A function's lane body: u-jit under the Cranelift tiers, or b-jit stencils under `--b-jit`.
    Body,
    /// A loop region, under its memory policy's loop tier.
    Region(quench_runtime::MemoryPolicy),
    /// An ahead image's body of this function, lowered at build time or installed at run time: the one
    /// AOT kind. No JIT mode or memory policy gates it.
    Ahead,
    /// A b-jit (stencil) body: its facts decide it (`specialize`).
    Base,
    /// A Wasm function's specialization (its lowered body and entry): always cleared.
    Wasm,
    /// A hot regexp program's native search (`regexp_native::start`): always cleared, since only a VM
    /// under a compiling mode counts its searches toward one (`regexp_native::enable`).
    Regexp,
    /// A JavaScript function's numeric or lane compile at an entry, when the Cranelift tiers run here.
    Entry,
    /// A Wasm program's module compile (`Vm::wasm_module_unit`): the program is Wasm.
    WasmModule,
    /// A lane body's plan from an entry, before planning (phase A): the compile bound, the tier predicted
    /// makes, the compile bound. Its unit is the proof every later lane gate needs.
    LanePlan,
    /// A lane body's entry gate during planning (phase B, inside the plan): the plan's unit, the amortization
    /// of its entry, and the callers that want it anyway.
    LaneEntry(CompiledUnit),
    /// A lane body's install gate after planning (phase B, before the plan binds its callees): the plan's
    /// unit, the callers of regions, and the amortization of its entry.
    LaneInstall(CompiledUnit),
    /// A JavaScript function claimed by a tier at an entry: the Cranelift tiers, or lane bodies on the
    /// stencil tier, for a function that is not Wasm.
    Claim,
}

/// What the gate's decisions read of the unit, computed by the entry that asks, and carried by the
/// request so proven facts can flow through the same path.
#[derive(Clone, Copy, Default)]
pub(crate) struct UnitFacts {
    /// The body's rows cover the stencil tier (`base_region::covers`).
    pub(crate) covers: bool,
    /// The body amortizes its b-jit entry (`body_may_amortize` for `Tier::Base`).
    pub(crate) amortizes: bool,
    /// The caller is native code or a body that calls the function (`LaneView::called_by_bodies`).
    pub(crate) called_by_bodies: bool,
    /// The body indexes a parameter that is a typed array (`indexes_typed_parameter`).
    pub(crate) indexes_typed: bool,
    /// The Cranelift function tiers run in this VM (`tier_policy::tiers_run`), for `UnitKind::Entry`.
    pub(crate) cranelift_allowed: bool,
    /// The program is a Wasm program (`ProgramKind::Wasm`), for `UnitKind::WasmModule`.
    pub(crate) wasm_program: bool,
    /// The caller is a loop region, which calls the body from native code (`LaneView::called_by_regions`).
    pub(crate) called_by_regions: bool,
    /// The compile is from the function's own entry, not a loop's (`entry.is_none()`).
    pub(crate) entry_none: bool,
    /// The function calls itself back through other functions (`recursive`).
    pub(crate) recursive: bool,
    /// The body's entry amortizes its work (`lane_facts::amortizes_entry`), or the gate does not ask.
    pub(crate) amortizes_entry: bool,
    /// A lane body may run on the stencil tier here: lane methods are enabled and b-jit runs (`UnitKind::Claim`).
    pub(crate) base_lane_allowed: bool,
}

/// Whether a unit of `kind` may compile at all, under the process's JIT mode and, for a loop
/// region, its memory policy. Only `specialize` asks this.
fn may_compile(kind: UnitKind) -> bool {
    match kind {
        UnitKind::Body => quench_runtime::vm::tier_policy::may_compile(),
        UnitKind::Region(policy) => quench_runtime::vm::tier_policy::by_policy(policy).loop_tier,
        UnitKind::Ahead | UnitKind::Wasm | UnitKind::Base | UnitKind::Regexp => true,
        UnitKind::Entry | UnitKind::WasmModule | UnitKind::LanePlan | UnitKind::LaneEntry(_) | UnitKind::LaneInstall(_) | UnitKind::Claim =>
            unreachable!("facts decide these kinds, in `specialize`"),
    }
}

/// The unit of an ahead image's body: the AOT kind is always cleared, so every image body may compile.
pub(crate) fn ahead_unit() -> CompiledUnit {
    CompiledUnit { ceiling: code_ceiling(UnitKind::Ahead) }
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
        UnitKind::Ahead => AHEAD_BODY_MAX_CODE_BYTES,
        _ => LANE_BODY_MAX_CODE_BYTES,
    }
}

/// A unit minted for a test, through `specialize` (an AOT unit of the root, which no mode gates).
#[cfg(test)]
pub(crate) fn test_unit() -> CompiledUnit {
    ahead_unit()
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
        UnitKind::Claim => (facts.cranelift_allowed || facts.base_lane_allowed) && !facts.wasm_program,
        UnitKind::Entry => facts.cranelift_allowed,
        UnitKind::WasmModule => facts.wasm_program,
        // Phase A: the pick decided the tier before this gate.
        UnitKind::LanePlan => true,
        // Phase B, during planning: the entry gate.
        UnitKind::LaneEntry(_) => {
            if facts.entry_none && !facts.called_by_bodies && !facts.recursive && !facts.amortizes_entry { return Decision::Short; }
            true
        }
        // Phase B, after planning: the install gate.
        UnitKind::LaneInstall(_) => {
            if facts.entry_none && !facts.called_by_regions && !facts.recursive && !facts.amortizes_entry { return Decision::Short; }
            true
        }
        UnitKind::Body | UnitKind::Region(_) | UnitKind::Ahead | UnitKind::Wasm | UnitKind::Regexp => may_compile(kind),
    };
    if cleared { Decision::Compile(CompiledUnit { ceiling: code_ceiling(kind) }) } else { Decision::Declined }
}

/// The tiers of the JIT mode, as the specializer names them.
pub(crate) use quench_runtime::vm::tier_policy::Tier;

/// Whether the lane-body tier is enrolled in this VM (`ProgramStore::js_jit_lane_methods_enabled`).
#[inline]
pub(crate) fn lane_bodies_enrolled(vm: &Vm) -> bool { vm.programs.js_jit_lane_methods_enabled() }

/// Whether the JIT diagnostics (`QUENCH_TRACE=js-jit`) record their counters. Diagnostics only: no tier
/// decision reads it.
#[inline]
pub(crate) fn diagnosing(vm: &Vm) -> bool { vm.programs.js_jit_diagnostics_enabled() }

/// The unit of a loop region that `policy` lets compile: the policy's loop tier, through `specialize`.
pub(crate) fn region_unit_under(policy: quench_runtime::MemoryPolicy) -> Option<CompiledUnit> {
    specialize(UnitRequest { kind: UnitKind::Region(policy), facts: Default::default() }).compiled()
}

/// The unit of a loop region this VM may compile (`region_unit_under` of its memory policy).
pub(crate) fn region_unit(vm: &Vm) -> Option<CompiledUnit> { region_unit_under(vm.memory_policy()) }

/// Whether a loop region compiles in this VM (`region_unit`).
#[inline]
pub(crate) fn loop_regions_compile(vm: &Vm) -> bool { region_unit(vm).is_some() }

/// How a function's interpreter records specialize (`dispatch_fast::derive_lane_view`): the
/// interpreter's own projection of the function, decided here as every compiled unit is.
#[derive(Clone, Copy)]
pub(crate) struct RecordPlan {
    /// Loop records keep tier sites (`install_loop_sites`) and their jumps stay visible to them.
    pub(crate) loop_sites: bool,
}

/// The record plan of a function whose tiers may run (`tier_policy::tiers_run`, any tier).
pub(crate) fn record_plan(tiers_run: bool) -> RecordPlan {
    RecordPlan { loop_sites: tiers_run }
}

/// Where a function's lane body comes from (`route_for_lane`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LaneRoute {
    /// The program's loaded AOT image serves it (`install_ahead_lane_body`): no warm-up, no runtime tier.
    Ahead,
    /// Its predicted tier is the interpreter: no body is compiled.
    Interpreted,
    /// The stencil tier stitches it at once (its predicted tier is b-jit).
    Base,
    /// It waits for callees that still compile, to call their bodies by their entries.
    Deferred,
    /// The lane body tier plans it (`LanePlan` and the gates after it).
    Lane,
}

/// The facts `route_for_lane` reads.
#[derive(Clone, Copy)]
pub(crate) struct LaneRouteFacts {
    /// The function's program has a loaded AOT image.
    pub(crate) ahead_image: bool,
    /// The function's predicted tier (`lane_facts::body_pick`).
    pub(crate) pick: PredictedTier,
    /// A start whose direct callees still compile (`body_tier_in_lane::callees_compiling`).
    pub(crate) callees_compiling: bool,
}

/// The route of a function's lane body, a pure function of its facts: an image body is served by the image; otherwise
/// its predicted tier, through the policy (`lane_body_tier`), picks the stencil route, the lane tier after its callees
/// still compiling, or none.
pub(crate) fn route_for_lane(facts: LaneRouteFacts) -> LaneRoute {
    if facts.ahead_image { return LaneRoute::Ahead; }
    match lane_body_tier(facts.pick) {
        None => LaneRoute::Interpreted,
        Some(Tier::Base) => LaneRoute::Base,
        Some(_) if facts.callees_compiling => LaneRoute::Deferred,
        Some(_) => LaneRoute::Lane,
    }
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
        Err(Refusal::CalleeNotEligible)
    }
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
