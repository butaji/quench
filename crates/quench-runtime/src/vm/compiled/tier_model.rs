//! The tier decision rule for a lane body's compile (`tier-decision-rule-v1`, sections 1, 3, 5):
//! a compile is admitted where the model's net gain in nanoseconds is positive under the
//! credible lower bound of the entries to come. The thresholds it replaced are gone; the
//! numbers it reads are `TIERS.priors`, and the compile cost is learned from this process's
//! own compiles.

use super::tier_policy::{Priors, Tiers, PER_MILLE};
use std::sync::Mutex;

/// What the model knows of one function at a decision point.
pub(super) struct Observed {
    /// Entries so far.
    pub(super) entries: u32,
    /// Nanoseconds the function has been interpreted for: the time since its warm-up began,
    /// which is how long it has run (`LaneView::interpreted_ns`).
    pub(super) interpreted_ns: u64,
    /// IR instructions of the planned body.
    pub(super) ir: usize,
}

/// The part per thousand of a compile's time that waits on the running program: the prior's
/// where the process has a second core for the code generator's thread, all of it where it
/// has one (the compile then takes the program's own time).
const SINGLE_CORE: usize = 1;

pub(super) fn compile_wall_permille(priors: &Priors) -> u64 {
    static SECOND_CORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let second_core = *SECOND_CORE.get_or_init(|| std::thread::available_parallelism().is_ok_and(|cores| cores.get() > SINGLE_CORE));
    if second_core { priors.compile_wall_permille } else { PER_MILLE }
}

/// Compile cost per IR instruction as a forgetting running ratio of the compiles this process
/// has made (equal to the least-squares slope through the origin, which the rule's RLS gives
/// with the intercept prior at zero), with the prior as `compile_prior_ir` pseudo-instructions.
pub(super) struct CompileCost { ns: f64, ir: f64 }

static COMPILES: Mutex<CompileCost> = Mutex::new(CompileCost { ns: 0.0, ir: 0.0 });

impl CompileCost {
    /// Folds one compile's census pair in, the older ones weighted down by the forgetting factor.
    pub(super) fn record(&mut self, priors: &Priors, took_ns: u64, ir: usize) {
        let forget = priors.compile_forgetting_permille as f64 / PER_MILLE as f64;
        self.ns = self.ns * forget + took_ns as f64;
        self.ir = self.ir * forget + ir as f64;
    }

    /// The estimated nanoseconds per IR instruction.
    pub(super) fn ns_per_ir(&self, priors: &Priors) -> u64 {
        let prior = priors.compile_prior_ir as f64;
        ((self.ns + prior * priors.compile_ns_per_ir as f64) / (self.ir + prior)) as u64
    }
}

/// Records a finished compile of `ir` instructions that took `took`.
pub(super) fn note_compile(priors: &Priors, took: std::time::Duration, ir: usize) {
    COMPILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).record(priors, took.as_nanos() as u64, ir);
}

pub(super) fn learned_ns_per_ir(priors: &Priors) -> u64 {
    COMPILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ns_per_ir(priors)
}

/// The net gain in ns of compiling `observed` into code that takes `share` per mille of the
/// interpreter's time, compiled at `compile_ns_per_ir`. A function that has run for a time is
/// expected to run on for as long (Lindy; the credible lower bound divides it by
/// `remaining_divisor`): that time to come saves what the body does not take, less the
/// entries to come crossing into it, less the wall share of the compile.
pub(super) fn net_ns(priors: &Priors, share: u64, compile_ns_per_ir: u64, wall_permille: u64, observed: &Observed) -> i64 {
    let saved = observed.interpreted_ns / priors.remaining_divisor * (PER_MILLE - share) / PER_MILLE;
    let crossings = u64::from(observed.entries) / priors.remaining_divisor * priors.body_entry_ns;
    let compile = observed.ir as u64 * compile_ns_per_ir * wall_permille / PER_MILLE;
    saved as i64 - crossings as i64 - compile as i64
}

/// Whether the model admits the compile of `observed`; under `tier-stress` every eligible
/// compile is admitted (`TIERS` floor row).
pub(super) fn admits(tiers: &Tiers, observed: &Observed, diagnostics: bool) -> bool {
    let priors = &tiers.priors;
    let net = net_ns(priors, priors.body_share, learned_ns_per_ir(priors), compile_wall_permille(priors), observed);
    if diagnostics { shadow_log(priors, observed, net); }
    tiers.admits_all || net > 0
}

/// The decision under `QUENCH_TRACE=js-jit`.
fn shadow_log(priors: &Priors, observed: &Observed, net: i64) {
    eprintln!("QUENCH_TIER_MODEL entries={} interpreted_ns={} ir={} compile_ns_per_ir={} net_ns={net} pick={}",
        observed.entries, observed.interpreted_ns, observed.ir, learned_ns_per_ir(priors), if net > 0 { "u-jit" } else { "interp" });
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tier_policy::TEST_PRIORS as PRIORS;

    const LONG_RUN_NS: u64 = 1_000_000_000;
    const LEAF_ENTRY_NS: u64 = 100;

    fn observed(entries: u32, interpreted_ns: u64, ir: usize) -> Observed { Observed { entries, interpreted_ns, ir } }
    fn net(observed: &Observed) -> i64 { net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, PER_MILLE, observed) }

    #[test]
    fn a_short_leaf_never_repays_its_entry() {
        // An entry that runs 100 ns saves 70 ns, under the 330 ns crossing: no entry count pays.
        let entries = 1_000_000;
        assert!(net(&observed(entries, u64::from(entries) * LEAF_ENTRY_NS, 100)) < 0);
    }

    #[test]
    fn a_function_that_has_run_long_repays_its_compile() {
        // 3 ms of compile: a quarter of the time so far, 70% of it saved, pays past 17 ms.
        let spent = |ns| net(&observed(400, ns, 1000));
        assert!(spent(4_000_000) < 0 && spent(40_000_000) > 0);
    }

    #[test]
    fn the_time_so_far_prices_a_loop_body_whatever_its_entries() {
        // A function entered 400 times that ran a second is worth compiling at any size of plan.
        assert!(net(&observed(400, LONG_RUN_NS, 10_000)) > 0);
        assert!(net(&observed(400, 0, 10)) < 0);
    }

    #[test]
    fn a_shared_core_charges_the_whole_compile() {
        let work = observed(400, 40_000_000, 1000);
        assert!(net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, 100, &work) > net(&work));
    }

    #[test]
    fn the_compile_cost_moves_from_the_prior_to_the_measurements() {
        let mut cost = CompileCost { ns: 0.0, ir: 0.0 };
        assert_eq!(cost.ns_per_ir(&PRIORS), PRIORS.compile_ns_per_ir);
        for _ in 0..1000 { cost.record(&PRIORS, 1_000_000, 1000); }
        let learned = cost.ns_per_ir(&PRIORS);
        assert!(learned < 1100 && learned > 1000, "{learned}");
    }
}
