//! The one differential oracle: a program runs in the interpreter and in a tier, and the two
//! must end alike (printed lines, completion or exception, the `answer` global). The tier's own
//! `TierEvents` must also show that it engaged: a tier that never ran agrees with anything.

use crate::vm::tier_policy::TierRows;
use super::compiled::forced_miss;
use super::tier_events::{Event, TierEvents};
use super::Vm;
use crate::test_host::TestHost;
use crate::{Engine, ExecutionMode, JitMode, Value};
use std::{cell::RefCell, rc::Rc};

/// The tier a run is held to.
#[derive(Clone, Copy, Debug)]
pub(super) enum Tier {
    /// `--b-jit`: stencil bodies.
    BJit,
    /// `--u-jit`: Cranelift bodies.
    UJit,
    /// Both tiers, as a process runs by default.
    Full,
    /// Both tiers, with the guard of this index forced to leave (`forced_miss`).
    Forced(usize),
}

impl Tier {
    pub(super) fn jit_mode(self) -> JitMode {
        match self {
            Self::BJit => JitMode::BaseOnly,
            Self::UJit => JitMode::UltraOnly,
            Self::Full | Self::Forced(_) => JitMode::Full,
        }
    }
}

/// What the tier must have done: at least `completed` bodies ending their activation, and at
/// most `leaves` runs leaving a body.
#[derive(Clone, Copy, Debug)]
pub(super) struct Expect {
    pub(super) completed: u64,
    pub(super) leaves: u64,
}

impl Expect {
    /// Engaged, however its runs ended.
    pub(super) const ENGAGED: Self = Self { completed: 0, leaves: u64::MAX };
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

/// A run's outcome and what its VM's tiers did.
pub(super) struct Run {
    pub(super) outcome: Outcome,
    pub(super) events: TierEvents,
    /// The guards the run's compiles emitted (`forced_miss::emitted`).
    pub(super) guards: usize,
}

impl Run {
    /// Loop regions that ran published code, entered or left.
    pub(super) fn regions(&self) -> u64 { self.events.count(Event::OsrEntered) + self.events.count(Event::OsrLeft) }
}

/// `source` run after the global scripts `scripts` (source and name) in one realm, in `mode`
/// under `jit`, over the floor rows, which admit every eligible compile.
pub(super) fn run(source: &str, scripts: &[(&str, &str)], mode: ExecutionMode, jit: JitMode) -> Run {
    let program = Engine::specialize(source, "oracle.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::with_tier_rows(TestHost::writing_to(output.clone()), TierRows::Floor);
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(mode);
    vm.programs.enable_js_lane_methods();
    let completion = crate::jit_mode::with_mode(jit, || if scripts.is_empty() {
        vm.execute(&program).map(drop)
    } else {
        vm.execute_after_scripts(scripts, &program).map(drop)
    }).map_err(|error| format!("{error:?}"));
    let atom = vm.intern_atom("answer");
    let answer = vm.own_property(vm.realm.globals, atom).map(Value::to_bits);
    let printed = output.borrow().clone();
    Run { outcome: Outcome { printed, completion, answer }, events: vm.tier_events().clone(), guards: forced_miss::emitted() }
}

/// Runs `source` after `scripts` in the interpreter and under `tier`, asserts that the outcomes
/// agree and that the tier engaged as `expect` says; the tier's run.
pub(super) fn assert_tier_agrees_after(scripts: &[(&str, &str)], source: &str, tier: Tier, expect: Expect) -> Run {
    let compiled = assert_outcomes_agree_after(scripts, source, tier);
    let events = &compiled.events;
    assert!(events.count(Event::Entered) > 0 || compiled.regions() > 0, "{tier:?} did not engage: no body or region ran: {events:?}");
    assert!(events.count(Event::Completed) >= expect.completed, "{tier:?}: {expect:?} not met: {events:?}");
    assert!(events.leaves() <= expect.leaves, "{tier:?}: {expect:?} not met: {events:?}");
    compiled
}

/// Runs `source` after `scripts` in the interpreter and under `tier`, and asserts that the
/// outcomes agree, whatever the tier ran; the tier's run. A program whose point is its semantics,
/// not a tier's engagement, is held to this alone.
pub(super) fn assert_outcomes_agree_after(scripts: &[(&str, &str)], source: &str, tier: Tier) -> Run {
    let interpreted = run(source, scripts, ExecutionMode::Interpreted, JitMode::Full);
    forced_miss::arm(match tier { Tier::Forced(guard) => Some(guard), _ => None });
    let compiled = run(source, scripts, ExecutionMode::Compiled, tier.jit_mode());
    forced_miss::arm(None);
    assert_eq!(compiled.outcome, interpreted.outcome, "{tier:?} ended unlike the interpreter");
    compiled
}

/// `assert_tier_agrees_after` of `source` alone.
pub(super) fn assert_tier_agrees(source: &str, tier: Tier, expect: Expect) -> Run {
    assert_tier_agrees_after(&[], source, tier, expect)
}
