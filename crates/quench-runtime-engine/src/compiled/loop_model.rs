//! The tier decision rule for a loop region (`tier-decision-rule-v1`, as `tier_model` has it for
//! a lane body): the compile is admitted where its net gain in nanoseconds is positive under
//! the credible lower bound of the iterations to come, and the site stops entering native code
//! where its entries cost more than the iterations they ran save.
//!
//! One iteration of a loop `span` bytecodes long saves `span x interpreter_ns x (1 - body_share)`
//! (`saved_per_iteration`); one entry costs `region_entry_ns`. The compile costs the learned
//! nanoseconds per IR instruction (`tier_model`) over the `ir_per_bytecode` of the span, at the
//! wall share of its time. The iterations to come are `taken / remaining_divisor` (Lindy).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tier_model::{compile_wall_permille, future_count, interpreted_ns, net_gain_ns};
use super::tier_policy::{Loops, Priors, Tiers, PER_MILLE};

/// What one iteration of a loop `span` bytecodes long saves, in ns, over the interpreter.
fn saved_per_iteration(priors: &Priors, span: u64) -> u64 {
    interpreted_ns(priors.interpreter_ns, span, priors.body_share)
}

/// What the compile of a loop `span` bytecodes long costs the running program, plus the one
/// entry the compiled loop is entered by, in ns.
fn outlay_ns(priors: &Priors, loops: &Loops, span: u64, compile_ns_per_ir: u64, wall_permille: u64) -> u64 {
    priors.region_entry_ns + span * loops.ir_per_bytecode * compile_ns_per_ir * wall_permille / PER_MILLE
}

/// The net gain in ns of compiling the loop after `taken` backward jumps.
#[cfg(test)]
fn net_ns(priors: &Priors, loops: &Loops, span: u64, taken: u64, compile_ns_per_ir: u64, wall_permille: u64) -> i64 {
    let future = future_count(priors, taken);
    net_gain_ns(future, saved_per_iteration(priors, span), outlay_ns(priors, loops, span, compile_ns_per_ir, wall_permille)) as i64
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
/// pays, at the compile cost of the priors; under `tier-stress` the warm-up alone.
pub(crate) fn admission_taken(tiers: &Tiers, span: u64) -> u32 {
    if tiers.admits_all { return tiers.loops.warmup_taken; }
    required_taken(&tiers.priors, &tiers.loops, span, tiers.priors.compile_ns_per_ir, compile_wall_permille(&tiers.priors))
}

/// What `iterations` over `entries` of the loop's native code (runs and declined entries) save
/// beyond the entries' region entries, in ns, the prior's `prior_runs` entries credited as having
/// repaid themselves. Negative where the entries cost more than the iterations save.
fn spare_ns(priors: &Priors, loops: &Loops, span: u64, entries: u64, iterations: u64) -> i128 {
    let entry_ns = i128::from(priors.region_entry_ns);
    let credit = i128::from(loops.prior_runs) * entry_ns;
    i128::from(iterations) * i128::from(saved_per_iteration(priors, span)) + credit - i128::from(entries) * entry_ns
}

/// The further entries the loop's native code repays after `entries` and `iterations`, each
/// costing one region entry from the spare (`spare_ns`), or `None` where the entries already cost
/// more. Iterations only add, so until the entries pass this count the site keeps entering.
pub(crate) fn repaid_entries(tiers: &Tiers, span: u64, entries: u64, iterations: u64) -> Option<u64> {
    let spare = u64::try_from(spare_ns(&tiers.priors, &tiers.loops, span, entries, iterations)).ok()?;
    Some(spare.checked_div(tiers.priors.region_entry_ns).unwrap_or(u64::MAX))
}

/// The iterations after which a loop body whose iterations work through `work` bytecodes pays
/// its `outlay` (`Vm::start_lane_method`): its plan, still to make where `unplanned`, takes the
/// program's own time. The iterations to come are `taken / remaining_divisor` as for a region.
fn body_required_taken(priors: &Priors, saved: u64, outlay: u64) -> u64 {
    outlay.checked_div(saved).map_or(u64::MAX, |paid| paid + 1).saturating_mul(priors.remaining_divisor)
}

/// The bytecodes one iteration of a loop body of `ir` instructions works through: the body
/// inlines the loop's calls, so an iteration does the work of its instructions
/// (`ir_per_bytecode` each), and of the loop's span at least.
fn body_work(loops: &Loops, span: u64, ir: usize) -> u64 {
    span.max(ir as u64 / super::lane_facts::measured_ir_per_bytecode(loops, super::lane_facts::Lowers::Body))
}

/// `body_required_taken` at the costs of `tiers`; under `tier-stress` none.
pub(crate) fn body_taken(tiers: &Tiers, span: u64, ir: usize, unplanned: bool) -> u64 {
    if tiers.admits_all { return 0; }
    let priors = &tiers.priors;
    let plan = if unplanned { priors.plan_ns_per_ir } else { 0 };
    // The compile charged whole, as `promotion_taken`'s, against what an iteration saves over the tier below: the
    // b-jit region where the default mode runs one, the interpreter otherwise (TOD-2194).
    let outlay = priors.region_entry_ns + super::tier_model::outlay_ns(ir, plan, priors.compile_ns_per_ir, PER_MILLE);
    let work = body_work(&tiers.loops, span, ir);
    let below = if crate::compiled::admission::tier_runs(&[crate::compiled::admission::Tier::Base]) { base_saved_per_iteration(priors, work) } else { 0 };
    body_required_taken(priors, saved_per_iteration(priors, work).saturating_sub(below), outlay)
}

/// What one iteration of a loop `span` bytecodes long saves, in ns, in stencil code (b-jit).
fn base_saved_per_iteration(priors: &Priors, span: u64) -> u64 {
    interpreted_ns(priors.interpreter_ns, span, priors.base_body_share)
}

/// Whether a u-jit region of a loop `span` bytecodes long is ahead of a b-jit region after
/// `taken` iterations: each region's iterations to come (`taken / remaining_divisor`) at its own
/// saving, the u-jit region's less its compile and entry (`outlay_ns`), the b-jit region's less
/// its entry alone, as its stencils stitch in microseconds.
pub(crate) fn ultra_ahead_of_base(priors: &Priors, loops: &Loops, span: u64, taken: u64, compile_ns_per_ir: u64, wall_permille: u64) -> bool {
    let future = future_count(priors, taken);
    let ultra = net_gain_ns(future, saved_per_iteration(priors, span), outlay_ns(priors, loops, span, compile_ns_per_ir, wall_permille));
    let base = net_gain_ns(future, base_saved_per_iteration(priors, span), priors.region_entry_ns);
    ultra > base
}

/// Whether a loop `span` bytecodes long whose regions took `native` iterations compiles to a b-jit
/// region (stencils) rather than a u-jit one: under `--b-jit` (or `QUENCH_DEV=bjit`) always, under
/// `--u-jit`/`--q-jit` never. In the default mode the b-jit region is the loop's first tier, by
/// construction, so the default never costs more than b-jit: the loop takes u-jit only once its
/// native iterations reach the point where the u-jit compile is repaid over the stencil region
/// (`promotion_taken`), counted in compiled code, not guessed from the interpreter's jumps.
pub(crate) fn base_region(tiers: &Tiers, span: u64, native: u64) -> bool {
    use crate::compiled::admission::{stencil_regions as base_regions, tier_runs as tiers_allowed, Tier};
    if base_regions() { return true; }
    if !tiers_allowed(&[Tier::Base]) || tiers.admits_all { return false; }
    promotion_taken(tiers, span).is_none_or(|at| native < at)
}

/// The fewest iterations in all after which a u-jit region of a loop `span` bytecodes long is
/// ahead of its b-jit region (`ultra_ahead_of_base`), or `None` where it never is.
pub(crate) fn promotion_taken(tiers: &Tiers, span: u64) -> Option<u64> {
    let priors = &tiers.priors;
    let gain = saved_per_iteration(priors, span).checked_sub(base_saved_per_iteration(priors, span)).filter(|gain| *gain > 0)?;
    // The b-jit region already runs, so the u-jit compile is all extra work, charged whole whichever thread runs it
    // (TOD-2194: savings over the remaining runs against the whole compile), not at the second core's wall share.
    // Its IR is the region compiles' measured ratio over the span (`lane_facts::measured_ir_per_bytecode`).
    let outlay = span.saturating_mul(super::lane_facts::measured_ir_per_bytecode(&tiers.loops, super::lane_facts::Lowers::Region)).saturating_mul(priors.compile_ns_per_ir);
    Some((outlay / gain + 1).saturating_mul(priors.remaining_divisor))
}

/// Whether a loop running a b-jit region in the default mode moves to a u-jit region after
/// `taken` iterations in all: where `base_region` no longer picks b-jit for it.
pub(crate) fn promotes(tiers: &Tiers, span: u64, taken: u64) -> bool {
    !crate::compiled::admission::stencil_regions() && !base_region(tiers, span, taken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::vm::tier_policy::TEST_PRIORS as PRIORS;

    const LOOPS: Loops = Loops { warmup_taken: 64, ir_per_bytecode: 8, prior_runs: 100, widen_after_leaves: 4, max_compilations: 8 };
    const COST: u64 = PRIORS.compile_ns_per_ir;

    fn required(span: u64, wall: u64) -> u32 { required_taken(&PRIORS, &LOOPS, span, COST, wall) }

    fn repays_entries(priors: &Priors, loops: &Loops, span: u64, entries: u64, iterations: u64) -> bool {
        spare_ns(priors, loops, span, entries, iterations) >= 0
    }

    #[test]
    fn the_repaid_entries_are_exactly_the_entries_the_spare_covers() {
        let tiers = quench_runtime::vm::tier_policy::STATIC_ROWS;
        let (entries, iterations) = (1_000, 1_000_000);
        let spare = repaid_entries(tiers, 10, entries, iterations).expect("the iterations repay the entries");
        assert!(repays_entries(&tiers.priors, &tiers.loops, 10, entries + spare, iterations));
        assert!(!repays_entries(&tiers.priors, &tiers.loops, 10, entries + spare + 1, iterations));
        assert_eq!(repaid_entries(tiers, 10, 1_000_000_000, 0), None);
    }

    #[test]
    fn promotion_is_where_u_jit_gets_ahead() {
        let tiers = quench_runtime::vm::tier_policy::STATIC_ROWS;
        let at = promotion_taken(tiers, 10).expect("u-jit saves more per iteration");
        let ahead = |taken| ultra_ahead_of_base(&tiers.priors, &tiers.loops, 10, taken, tiers.priors.compile_ns_per_ir, PER_MILLE);
        assert!(ahead(at));
    }

    #[test]
    fn a_u_jit_region_gets_ahead_of_a_b_jit_one_with_iterations() {
        // Few iterations: the stencil region, which costs no compile, is ahead.
        assert!(!ultra_ahead_of_base(&PRIORS, &LOOPS, 10, 100, COST, PER_MILLE));
        // Enough iterations repay the u-jit compile over the stencil region.
        assert!(ultra_ahead_of_base(&PRIORS, &LOOPS, 10, 1 << 30, COST, PER_MILLE));
    }

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
    fn a_body_that_inlines_its_calls_asks_iterations_for_its_own_size() {
        // 8,000 instructions planned (12 ms) and compiled (24 ms at full wall) work through 1,000
        // bytecodes an iteration, 7 us saved: some 20,000 iterations, though the span is 10.
        let outlay = crate::compiled::tier_model::outlay_ns(8_000, PRIORS.plan_ns_per_ir, COST, PER_MILLE);
        let taken = body_required_taken(&PRIORS, saved_per_iteration(&PRIORS, body_work(&LOOPS, 10, 8_000)), outlay);
        assert!(taken > 19_000 && taken < 22_000, "{taken}");
        assert_eq!(body_work(&LOOPS, 10, 40), 10);
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
