//! The one admission policy for compiles: every question that decides whether a callee or a
//! body compiles, and every reason it does not, lives here. A refused compile records its
//! `Refusal`; `QUENCH_REFUSALS=1` prints the counts at exit (`refusal_report`).

use super::lane_unit::CalleeBinding;
use super::tier_model::Observed;
use super::tier_policy::Tiers;
use crate::bytecode::{Function, ResidualProgram};
use crate::vm::dispatch_fast::lane_body_stats::{miss, Miss};
use crate::vm::dispatch_fast::LaneView;
use crate::vm::jit_backend::CompileLevel;
use crate::vm::{Host, Vm};

/// Why a compile or a callee's admission is refused. Each variant is one refusal site, counted
/// by `record` and named in `refusal_report`. The OSR region reasons (`RegionRejection`) share
/// this enum through `From`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum Refusal {
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
pub(in crate::vm) fn record(refusal: Refusal) {
    COUNTS[refusal.slot()].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if reporting() {
        REPORT.call_once(|| {
            // SAFETY: `report` is a plain function that outlives the process.
            unsafe { libc::atexit(report) };
        });
    }
}

/// Counts `refusal` and, where the lane diagnostics name it as a plan miss, counts that too
/// (`lane_body_stats::miss`, which also names the decline reason).
pub(in crate::vm) fn refuse(vm: &Vm, refusal: Refusal) {
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

/// How a callee would run: inlined into a lane unit under the call's `CalleeBinding`, or called
/// frameless.
#[derive(Clone, Copy)]
pub(in crate::vm) enum CalleeUse {
    Inlined(CalleeBinding),
    Frameless,
}

/// Whether `callee` may run as `how`. An inlined callee bound by identity is admitted by its
/// call's guard; a function-bound one must be a plain activation the compiled tier takes. A
/// frameless callee runs in a loop with registers only, and takes no rest parameters. A refusal
/// is counted.
pub(in crate::vm) fn admit_callee(program: &ResidualProgram, callee: &Function, how: CalleeUse) -> Result<(), Refusal> {
    let admitted = match how {
        CalleeUse::Inlined(binding) => binding != CalleeBinding::Function
            || (callee.plain_activation(program) && crate::vm::js_compiled::eligible(callee, program)),
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
pub(in crate::vm) fn compile_level(tiers: &Tiers, program: &ResidualProgram, function: u32, entries: u32, ir: usize, caller: &LaneView) -> CompileLevel {
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
