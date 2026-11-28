//! The planning cost of a loop's lane body (`LoopSite::body`). A loop region's tier is no longer
//! decided here: a loop region enters native code iff its function's predicted tier is not the
//! interpreter (`tier_policy::predicted_tier`, design decision 4.9), with no iteration counts.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tier_model::interpreted_ns;
use super::tier_policy::{Loops, Priors, Tiers, PER_MILLE};

/// What one iteration of a loop `span` bytecodes long saves, in ns, over the interpreter.
fn saved_per_iteration(priors: &Priors, span: u64) -> u64 {
    interpreted_ns(priors.interpreter_ns, span, priors.body_share)
}

/// What one iteration of a loop `span` bytecodes long saves, in ns, in stencil code (b-jit).
fn base_saved_per_iteration(priors: &Priors, span: u64) -> u64 {
    interpreted_ns(priors.interpreter_ns, span, priors.base_body_share)
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
    span.max(ir as u64 / loops.ir_per_bytecode)
}

/// `body_required_taken` at the costs of `tiers`.
pub(crate) fn body_taken(tiers: &Tiers, span: u64, ir: usize, unplanned: bool) -> u64 {
    let priors = &tiers.priors;
    let plan = if unplanned { priors.plan_ns_per_ir } else { 0 };
    // The compile charged whole, as `promotion_taken`'s, against what an iteration saves over the tier below: the
    // b-jit region where the default mode runs one, the interpreter otherwise (TOD-2194).
    let outlay = priors.region_entry_ns + super::tier_model::outlay_ns(ir, plan, priors.compile_ns_per_ir, PER_MILLE);
    let work = body_work(&tiers.loops, span, ir);
    let below = if quench_runtime::vm::tier_policy::base_runs() { base_saved_per_iteration(priors, work) } else { 0 };
    body_required_taken(priors, saved_per_iteration(priors, work).saturating_sub(below), outlay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::vm::tier_policy::TEST_PRIORS as PRIORS;

    const LOOPS: Loops = Loops { warmup_taken: 64, ir_per_bytecode: 8, prior_runs: 100, widen_after_leaves: 4, max_compilations: 8 };
    const COST: u64 = PRIORS.compile_ns_per_ir;

    #[test]
    fn a_body_that_inlines_its_calls_asks_iterations_for_its_own_size() {
        // 8,000 instructions planned (12 ms) and compiled (24 ms at full wall) work through 1,000
        // bytecodes an iteration, 7 us saved: some 20,000 iterations, though the span is 10.
        let outlay = crate::compiled::tier_model::outlay_ns(8_000, PRIORS.plan_ns_per_ir, COST, PER_MILLE);
        let taken = body_required_taken(&PRIORS, saved_per_iteration(&PRIORS, body_work(&LOOPS, 10, 8_000)), outlay);
        assert!(taken > 19_000 && taken < 22_000, "{taken}");
        assert_eq!(body_work(&LOOPS, 10, 40), 10);
    }
}
