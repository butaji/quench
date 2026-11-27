//! The lane body's static tier decision (L8): no counts and no clock. A function compiles at its first
//! entry where the program's call graph or its own loops make its compile pay (`static_benefit`), and is
//! compiled at the quick level where its static residue is large (`compiles_quick`). The numbers it reads
//! are the VM's `Tiers` constants. The loop tier's cost model (`loop_model`) reads the helpers below.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tier_policy::{Priors, PER_MILLE};
use quench_runtime::vm::tier_policy::{BODY_STATIC_RESIDUE, QUICK_RESIDUE};

/// The part per thousand of a compile's time that waits on the running program: the prior's
/// where the process has a second core for the code generator's thread, all of it where it
/// has one (the compile then takes the program's own time).
const SINGLE_CORE: usize = 1;

pub(crate) fn compile_wall_permille(priors: &Priors) -> u64 {
    static SECOND_CORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let second_core = *SECOND_CORE.get_or_init(|| std::thread::available_parallelism().is_ok_and(|cores| cores.get() > SINGLE_CORE));
    if second_core { priors.compile_wall_permille } else { PER_MILLE }
}

/// What a body of `ir` instructions costs the running program before it runs: its plan
/// (`plan_ns`, all on the program's thread; 0 once the plan is made) and the wall share of its compile.
pub(crate) fn outlay_ns(ir: usize, plan_ns_per_ir: u64, compile_ns_per_ir: u64, wall_permille: u64) -> u64 {
    ir as u64 * (plan_ns_per_ir + compile_ns_per_ir * wall_permille / PER_MILLE)
}

/// The interpreter's time in ns over `work` bytecodes that code taking `share` per mille of it
/// per bytecode leaves to the interpreter: the work at `interpreter_ns`, less the share the code
/// still takes. Loop regions (`loop_model`) save this much per iteration.
pub(crate) fn interpreted_ns(interpreter_ns: u64, work: u64, share: u64) -> u64 {
    work * interpreter_ns * (PER_MILLE - share) / PER_MILLE
}

/// The future uses of a loop region after `count` past ones (Lindy): `count / remaining_divisor`.
pub(crate) fn future_count(priors: &Priors, count: u64) -> u64 {
    count / priors.remaining_divisor
}

/// The net gain in ns of a compile: `future` uses to come, each saving `saved` ns, less the
/// `outlay` the compile and its entry cost the program. Exact: no product saturates.
pub(crate) fn net_gain_ns(future: u64, saved: u64, outlay: u64) -> i128 {
    i128::from(future) * i128::from(saved) - i128::from(outlay)
}

/// What the static benefit reads of a function: whether a loop or a call cycle reaches it, its own back edge
/// included (`in_loop_or_cycle`), and its static residue (`lane_facts::static_residue`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StaticFacts {
    pub(crate) in_loop_or_cycle: bool,
    pub(crate) residue: u64,
}

/// Whether a function's lane body compiles at its first entry: one a loop or a call cycle reaches
/// (`StaticFacts::in_loop_or_cycle`), or one whose residue reaches `BODY_STATIC_RESIDUE`. The floor row (`admits_all`)
/// admits every function. Any other runs in the interpreter. Pure: it reads no count and no clock.
pub(crate) fn static_benefit(admits_all: bool, facts: StaticFacts) -> bool {
    admits_all || facts.in_loop_or_cycle || facts.residue >= BODY_STATIC_RESIDUE
}

/// The tier the static benefit picks for a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// No loop or call cycle reaches it and its residue is small: the interpreter runs it.
    Interpreter,
    /// The u-jit body compiles at its first entry (`static_benefit`).
    Ultra,
}

/// The pick `static_benefit` makes for `facts`.
pub(crate) fn pick(admits_all: bool, facts: StaticFacts) -> Pick {
    if static_benefit(admits_all, facts) { Pick::Ultra } else { Pick::Interpreter }
}

/// Whether a lane body of `residue` compiles at the quick level (`CompileLevel::Quick`): one at or past `QUICK_RESIDUE`,
/// whose optimizing compile costs more than its code saves. The floor row (`admits_all`) compiles every body optimized.
pub(crate) fn compiles_quick(admits_all: bool, residue: u64) -> bool {
    !admits_all && residue >= QUICK_RESIDUE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_function_a_loop_or_a_cycle_reaches_compiles_at_its_first_entry() {
        assert!(static_benefit(false, StaticFacts { in_loop_or_cycle: true, residue: 0 }));
    }

    #[test]
    fn a_large_residue_compiles_without_a_loop_or_a_cycle() {
        assert!(static_benefit(false, StaticFacts { in_loop_or_cycle: false, residue: BODY_STATIC_RESIDUE }));
        assert!(!static_benefit(false, StaticFacts { in_loop_or_cycle: false, residue: BODY_STATIC_RESIDUE - 1 }));
    }

    #[test]
    fn a_small_straight_function_runs_in_the_interpreter() {
        assert_eq!(pick(false, StaticFacts::default()), Pick::Interpreter);
        assert_eq!(pick(true, StaticFacts::default()), Pick::Ultra);
    }

    #[test]
    fn the_quick_level_takes_only_large_bodies() {
        assert!(compiles_quick(false, QUICK_RESIDUE));
        assert!(!compiles_quick(false, QUICK_RESIDUE - 1));
        assert!(!compiles_quick(true, QUICK_RESIDUE));
    }
}
