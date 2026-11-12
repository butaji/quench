//! The tier decision rule for a lane body's compile (`tier-decision-rule-v1`, sections 1, 3, 5):
//! a compile is admitted where the model's net gain in nanoseconds is positive under the
//! credible lower bound of the entries to come. The thresholds it replaced are gone; the
//! numbers it reads are `TIERS.priors`, and the compile cost is learned from this process's
//! own compiles.

use super::tier_policy::{Priors, PER_MILLE, STRESS, TIERS};
use std::sync::Mutex;

/// What the model knows of one function at a decision point.
pub(super) struct Observed {
    /// Bytecodes of straight-line work per entry.
    pub(super) length: u64,
    /// Bytecodes per entry inside its loops (trip means times spans).
    pub(super) loop_work: u64,
    /// Entries so far.
    pub(super) entries: u32,
    /// Whether its loop already runs a region entered from the lane: the body then saves that
    /// entry alone.
    pub(super) region: bool,
    /// IR instructions of the planned body.
    pub(super) ir: usize,
}

/// The part per thousand of a compile's time that waits on the running program: the prior's
/// where the process has a second core for the code generator's thread, all of it where it
/// has one (the compile then takes the program's own time).
const SINGLE_CORE: usize = 1;

pub(super) fn compile_wall_permille() -> u64 {
    static SHARE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *SHARE.get_or_init(|| match std::thread::available_parallelism() {
        Ok(cores) if cores.get() > SINGLE_CORE => TIERS.priors.compile_wall_permille,
        _ => PER_MILLE,
    })
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
pub(super) fn note_compile(took: std::time::Duration, ir: usize) {
    COMPILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).record(&TIERS.priors, took.as_nanos() as u64, ir);
}

pub(super) fn learned_ns_per_ir() -> u64 {
    COMPILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ns_per_ir(&TIERS.priors)
}

/// The net gain in ns of compiling `observed` into code that takes `share` per mille of the
/// interpreter's time per bytecode, compiled at `compile_ns_per_ir`: each future entry
/// (Lindy, `entries / remaining_divisor`) saves the interpreter's work less the share the body
/// still takes and less its own entry crossing; the compile costs the wall share of its time.
pub(super) fn net_ns(priors: &Priors, share: u64, compile_ns_per_ir: u64, wall_permille: u64, observed: &Observed) -> i64 {
    let per_entry = if observed.region { priors.region_entry_ns }
        else { (observed.length + observed.loop_work) * priors.interpreter_ns * (PER_MILLE - share) / PER_MILLE };
    let saved = per_entry.saturating_sub(priors.body_entry_ns) as i64;
    let future = (u64::from(observed.entries) / priors.remaining_divisor) as i64;
    future * saved - (observed.ir as u64 * compile_ns_per_ir * wall_permille / PER_MILLE) as i64
}

/// Whether the model admits the compile of `observed`; under `tier-stress` every eligible
/// compile is admitted (`TIERS` floor row).
pub(super) fn admits(observed: &Observed, diagnostics: bool) -> bool {
    let net = net_ns(&TIERS.priors, TIERS.priors.body_share, learned_ns_per_ir(), compile_wall_permille(), observed);
    if diagnostics { shadow_log(observed, net); }
    STRESS || net > 0
}

/// The decision under `QUENCH_TRACE=js-jit`.
fn shadow_log(observed: &Observed, net: i64) {
    eprintln!("QUENCH_TIER_MODEL entries={} ir={} region={} compile_ns_per_ir={} net_ns={net} pick={}",
        observed.entries, observed.ir, observed.region, learned_ns_per_ir(), if net > 0 { "u-jit" } else { "interp" });
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIORS: Priors = Priors { interpreter_ns: 10, body_share: 300, compile_ns_per_ir: 3000, compile_wall_permille: 1000,
        region_entry_ns: 140, body_entry_ns: 330, remaining_divisor: 4, compile_forgetting_permille: 990, compile_prior_ir: 800 };

    fn observed(length: u64, loop_work: u64, entries: u32, region: bool, ir: usize) -> Observed { Observed { length, loop_work, entries, region, ir } }
    fn net(observed: &Observed) -> i64 { net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, PER_MILLE, observed) }

    #[test]
    fn a_short_leaf_never_repays_its_entry() {
        // 40 bytecodes save 280 ns an entry, under the 330 ns crossing: no entry count pays.
        assert!(net(&observed(40, 0, u32::MAX, false, 100)) < 0);
    }

    #[test]
    fn a_long_body_pays_once_enough_entries_were_seen() {
        let long = |entries| net(&observed(400, 0, entries, false, 1000));
        // Saves 400*10*0.7 - 330 = 2470 ns an entry; the compile costs 3 ms.
        assert!(long(4000) < 0 && long(8000) > 0);
    }

    #[test]
    fn a_loop_body_counts_its_trip_work() {
        assert!(net(&observed(20, 0, 4000, false, 200)) < 0);
        assert!(net(&observed(20, 5000, 4000, false, 200)) > 0);
    }

    #[test]
    fn a_region_function_saves_its_region_entry_alone() {
        // 140 ns of region entry against the 330 ns body crossing: the body loses.
        assert!(net(&observed(400, 400, u32::MAX, true, 10)) < 0);
    }

    #[test]
    fn a_shared_core_charges_the_whole_compile() {
        let work = observed(400, 0, 8000, false, 1000);
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
