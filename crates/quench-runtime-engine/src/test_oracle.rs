//! The one differential oracle: a program runs in the interpreter and in a tier, and the two
//! must end alike (printed lines, completion or exception, the `answer` global). The load-time
//! policy must also pick a compiled tier for some function of the program (`predicted_tiers`):
//! a tier the policy never picks agrees with anything.

use crate::compiled::forced_miss;
use quench_runtime::test_host::TestHost;
use quench_runtime::vm::tier_policy::PredictedTier;
use quench_runtime::vm::ProgramId;
use quench_runtime::{Engine, ExecutionMode, JitMode, Value};
use std::{cell::RefCell, rc::Rc};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The tier a run is held to.
#[derive(Clone, Copy, Debug)]
pub(super) enum Tier {
    /// Every tier, as a process runs by default: the policy picks each function's tier.
    Full,
    /// Every tier, with the guard of this index forced to leave (`forced_miss`).
    Forced(usize),
}

/// How a run ended, which two tiers must share.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub(super) printed: Vec<String>,
    pub(super) completion: Result<(), String>,
    pub(super) answer: Option<u64>,
}

impl Outcome {
    /// The `answer` global as a Number, whatever its representation (an int32 or a double).
    pub(super) fn answer_number(&self) -> Option<f64> {
        self.answer.map(Value::from_bits).and_then(Value::as_number)
    }
}

/// A run's outcome and the tiers the load-time policy picked for its program.
pub(super) struct Run {
    pub(super) outcome: Outcome,
    /// The predicted tier of each function (`ProgramStore::predicted_tiers`).
    pub(super) predicted: Vec<PredictedTier>,
    /// The guards the run's compiles emitted (`forced_miss::emitted`).
    pub(super) guards: usize,
}

impl Run {
    /// Whether the policy picks a compiled tier for some function of the program.
    pub(super) fn predicts_compiled(&self) -> bool { self.predicted.iter().any(|tier| !matches!(tier, PredictedTier::Interpreter)) }
}

/// `source` run after the global scripts `scripts` (source and name) in one realm, in `mode`
/// under `jit`.
pub(super) fn run(source: &str, scripts: &[(&str, &str)], mode: ExecutionMode) -> Run {
    let program = Engine::specialize(source, "oracle.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, mode);
    vm.programs.enable_js_lane_methods();
    let completion = quench_runtime::jit_mode::with_mode(JitMode::Full, || if scripts.is_empty() {
        quench_runtime::test_support::execute(&mut vm, &program).map(drop)
    } else {
        quench_runtime::test_support::execute_after_scripts(&mut vm, scripts, &program).map(drop)
    }).map_err(|error| format!("{error:?}"));
    let atom = vm.intern_atom("answer");
    let answer = vm.own_property(vm.realm.globals, atom).map(Value::to_bits);
    let printed = output.borrow().clone();
    let predicted = vm.programs.predicted_tiers(ProgramId::MAIN).map(|tiers| tiers.to_vec()).unwrap_or_default();
    Run { outcome: Outcome { printed, completion, answer }, predicted, guards: forced_miss::emitted() }
}

/// Runs `source` after `scripts` in the interpreter and under `tier`, asserts that the outcomes
/// agree and that the policy predicts a compiled tier for it; the tier's run.
pub(super) fn assert_tier_agrees_after(scripts: &[(&str, &str)], source: &str, tier: Tier) -> Run {
    let compiled = assert_outcomes_agree_after(scripts, source, tier);
    assert!(compiled.predicts_compiled(), "{tier:?} did not engage: the policy predicts the interpreter for every function: {:?}", compiled.predicted);
    compiled
}

/// Runs `source` after `scripts` in the interpreter and under `tier`, and asserts that the
/// outcomes agree, whatever the tier ran; the tier's run. A program whose point is its semantics,
/// not a tier's engagement, is held to this alone.
pub(super) fn assert_outcomes_agree_after(scripts: &[(&str, &str)], source: &str, tier: Tier) -> Run {
    let interpreted = run(source, scripts, ExecutionMode::Interpreted);
    forced_miss::arm(match tier { Tier::Forced(guard) => Some(guard), _ => None });
    let compiled = run(source, scripts, ExecutionMode::Compiled);
    forced_miss::arm(None);
    assert_eq!(compiled.outcome, interpreted.outcome, "{tier:?} ended unlike the interpreter");
    compiled
}

/// `assert_tier_agrees_after` of `source` alone.
pub(super) fn assert_tier_agrees(source: &str, tier: Tier) -> Run {
    assert_tier_agrees_after(&[], source, tier)
}

/// The loop sites the load installed in the lane view of function `function` of `vm`'s main
/// program (`install_loop_sites`); none where it has no view.
pub(crate) fn main_loops(vm: &quench_runtime::vm::Vm, function: usize) -> &[quench_runtime::vm::engine_types::LoopSite] {
    let views = vm.programs.views_in_lane(ProgramId::MAIN).expect("main program lane views");
    views.get(function).and_then(quench_runtime::vm::program_store::derived_lane_view).map_or(&[], |view| &view.loops)
}

/// Asserts the load's decision for the loops of function `function` of `vm`'s main program:
/// they hold their entering rows exactly where its predicted tier compiles it. Returns whether it does.
pub(crate) fn assert_loop_decision(vm: &quench_runtime::vm::Vm, function: usize) -> bool {
    let predicted = vm.programs.compiled_tier(ProgramId::MAIN, function as u32);
    let sites = main_loops(vm, function).len();
    assert_eq!(sites > 0, predicted, "function {function}: its loops hold their entering rows iff it is predicted compiled ({sites} sites)");
    predicted
}
