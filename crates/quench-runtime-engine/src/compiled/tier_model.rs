//! The lane body's cost arithmetic: no counts and no clock. The tier a function runs in is the
//! argmin of `quench_runtime::vm::tier_policy::predicted_tier` (design decision 4.9). The loop tier's
//! cost model (`loop_model`) reads the helpers below.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tier_policy::{Priors, PER_MILLE};

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
