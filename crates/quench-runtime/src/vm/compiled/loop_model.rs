//! The tier decision rule for a loop region (`tier-decision-rule-v1`, as `tier_model` has it for
//! a lane body): the compile is admitted where its net gain in nanoseconds is positive under
//! the credible lower bound of the iterations to come, and the site stops entering native code
//! where its entries cost more than the iterations they ran save.
//!
//! One iteration of a loop `span` bytecodes long saves `span x interpreter_ns x (1 - body_share)`
//! (`saved_per_iteration`); one entry costs `region_entry_ns`. The compile costs the learned
//! nanoseconds per IR instruction (`tier_model`) over the `ir_per_bytecode` of the span, at the
//! wall share of its time. The iterations to come are `taken / remaining_divisor` (Lindy).

use super::tier_model::{compile_wall_permille, learned_ns_per_ir};
use super::tier_policy::{admits_every_compile, Loops, Priors, PER_MILLE, TIERS};

/// What one iteration of a loop `span` bytecodes long saves, in ns, over the interpreter.
fn saved_per_iteration(priors: &Priors, span: u64) -> u64 {
    span * priors.interpreter_ns * (PER_MILLE - priors.body_share) / PER_MILLE
}

/// What the compile of a loop `span` bytecodes long costs the running program, plus the one
/// entry the compiled loop is entered by, in ns.
fn outlay_ns(priors: &Priors, loops: &Loops, span: u64, compile_ns_per_ir: u64, wall_permille: u64) -> u64 {
    priors.region_entry_ns + span * loops.ir_per_bytecode * compile_ns_per_ir * wall_permille / PER_MILLE
}

/// The net gain in ns of compiling the loop after `taken` backward jumps.
#[cfg(test)]
fn net_ns(priors: &Priors, loops: &Loops, span: u64, taken: u64, compile_ns_per_ir: u64, wall_permille: u64) -> i64 {
    let future = (taken / priors.remaining_divisor) as i64;
    future * saved_per_iteration(priors, span) as i64 - outlay_ns(priors, loops, span, compile_ns_per_ir, wall_permille) as i64
}

/// The fewest backward jumps after which `net_ns` is positive, and never fewer than the
/// warm-up: its call sites have linked and its entry facts are observed.
fn required_taken(priors: &Priors, loops: &Loops, span: u64, compile_ns_per_ir: u64, wall_permille: u64) -> u32 {
    let outlay = outlay_ns(priors, loops, span, compile_ns_per_ir, wall_permille);
    let future = outlay.checked_div(saved_per_iteration(priors, span)).map_or(u64::MAX, |paid| paid + 1);
    let taken = future.saturating_mul(priors.remaining_divisor);
    u32::try_from(taken).unwrap_or(u32::MAX).max(loops.warmup_taken)
}

/// The backward jumps a loop `span` bytecodes long takes in the interpreter before its compile
/// pays, at the compile cost this process has learned; under `tier-stress` the warm-up alone.
pub(in crate::vm) fn admission_taken(span: u64) -> u32 {
    if admits_every_compile() { return TIERS.loops.warmup_taken; }
    required_taken(&TIERS.priors, &TIERS.loops, span, learned_ns_per_ir(), compile_wall_permille())
}

/// Whether `iterations`, over `entries` of the loop's native code (runs and declined entries),
/// repay the entries: the prior credits `prior_runs` entries that repaid themselves, and the
/// measurements outweigh it as the entries mount.
fn repays_entries(priors: &Priors, loops: &Loops, span: u64, entries: u64, iterations: u64) -> bool {
    let credit = loops.prior_runs * priors.region_entry_ns;
    let saved = iterations.saturating_mul(saved_per_iteration(priors, span)).saturating_add(credit);
    saved >= entries.saturating_mul(priors.region_entry_ns)
}

/// `repays_entries` under the process's priors.
pub(in crate::vm) fn repays(span: u64, entries: u64, iterations: u64) -> bool {
    repays_entries(&TIERS.priors, &TIERS.loops, span, entries, iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIORS: Priors = Priors { interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 3000, compile_wall_permille: 1000,
        region_entry_ns: 140, body_entry_ns: 330, remaining_divisor: 4, compile_forgetting_permille: 990, compile_prior_ir: 800 };
    const LOOPS: Loops = Loops { warmup_taken: 64, ir_per_bytecode: 8, prior_runs: 100, widen_after_leaves: 4, max_compilations: 8 };
    const COST: u64 = PRIORS.compile_ns_per_ir;

    fn required(span: u64, wall: u64) -> u32 { required_taken(&PRIORS, &LOOPS, span, COST, wall) }

    #[test]
    fn the_requirement_is_where_the_net_turns_positive() {
        // 10 bytecodes save 70 ns an iteration; the compile costs 10*8*3000 = 240 us plus 140 ns.
        let taken = u64::from(required(10, PER_MILLE));
        assert!(net_ns(&PRIORS, &LOOPS, 10, taken, COST, PER_MILLE) > 0);
        assert!(net_ns(&PRIORS, &LOOPS, 10, taken - 4, COST, PER_MILLE) <= 0);
    }

    #[test]
    fn a_longer_body_saves_more_per_iteration_and_compile_cost_grows_with_it_alike() {
        // Both scale with the span: the requirement stays about one figure, the compile's price in iterations.
        assert!(required(10, PER_MILLE).abs_diff(required(40, PER_MILLE)) < PRIORS.remaining_divisor as u32 * 2);
    }

    #[test]
    fn a_shared_core_asks_more_iterations_and_a_second_core_fewer() {
        assert!(required(10, 100) < required(10, PER_MILLE));
    }

    #[test]
    fn a_cheap_compile_asks_the_warm_up_alone() {
        assert_eq!(required_taken(&PRIORS, &LOOPS, 10, 1, 100), LOOPS.warmup_taken);
    }

    #[test]
    fn a_loop_that_saves_nothing_is_never_compiled() {
        let free = Priors { body_share: PER_MILLE, ..PRIORS };
        assert_eq!(required_taken(&free, &LOOPS, 10, COST, PER_MILLE), u32::MAX);
    }

    #[test]
    fn runs_of_many_iterations_repay_their_entries() {
        // 70 ns saved an iteration against a 140 ns entry: two iterations a run break even.
        assert!(repays_entries(&PRIORS, &LOOPS, 10, 1_000_000, 2_000_000));
        assert!(repays_entries(&PRIORS, &LOOPS, 10, 1_000_000, 5_000_000));
    }

    #[test]
    fn runs_that_leave_before_their_backward_jump_exhaust_the_prior() {
        // The prior credits 100 entries; a site entered without iterations pays for the 101st.
        assert!(repays_entries(&PRIORS, &LOOPS, 10, 100, 0));
        assert!(!repays_entries(&PRIORS, &LOOPS, 10, 101, 0));
    }

    #[test]
    fn declined_entries_count_as_runs_without_iterations() {
        // Richards' list walk: 6M entries at 0.02 iterations an entry.
        assert!(!repays_entries(&PRIORS, &LOOPS, 10, 6_000_000, 120_000));
    }

    #[test]
    fn a_site_whose_runs_pay_keeps_entering_however_many_entries_it_makes() {
        assert!(repays_entries(&PRIORS, &LOOPS, 10, u64::from(u32::MAX), u64::from(u32::MAX) * 3));
    }
}
