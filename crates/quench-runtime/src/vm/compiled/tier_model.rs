//! The tier decision rule for a lane body's compile (`tier-decision-rule-v1`, sections 1, 3, 5):
//! a compile is admitted where the model's net gain in nanoseconds is positive under the
//! credible lower bound of the entries to come. The thresholds it replaced are gone; the
//! numbers it reads are the VM's `Tiers::priors`. The compile and the plan are priced by their work (IR
//! instructions), not timed: what is compiled then repeats from run to run, under callgrind and
//! the virtual clock alike (policies follow work, not wall time).

use super::tier_policy::{Priors, Tiers, PER_MILLE};

/// What the model knows of one function at a decision point.
pub(super) struct Observed {
    /// Bytecodes of straight-line work per entry.
    pub(super) length: u64,
    /// Bytecodes per entry inside its loops that the interpreter still runs (trip means times spans).
    pub(super) loop_work: u64,
    /// Entries so far.
    pub(super) entries: u32,
    /// Native runs of its loops that run in a region entered from the lane, over all entries:
    /// each a crossing the body removes, while the loop's own work is native already.
    pub(super) region_runs: u64,
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

/// What a body of `ir` instructions costs the running program before it runs: its plan
/// (`plan_ns`, all on the program's thread; 0 once the plan is made) and the wall share of its compile.
pub(super) fn outlay_ns(ir: usize, plan_ns_per_ir: u64, compile_ns_per_ir: u64, wall_permille: u64) -> u64 {
    ir as u64 * (plan_ns_per_ir + compile_ns_per_ir * wall_permille / PER_MILLE)
}

/// The net gain in ns of compiling `observed` into code that takes `share` per mille of the
/// interpreter's time per bytecode, compiled at `compile_ns_per_ir`: each future entry
/// (Lindy, `entries / remaining_divisor`) saves the interpreter's work outside native regions
/// less the share the body still takes, and the region crossings an entry makes, less its own
/// entry crossing; the compile costs the wall share of its time.
#[cfg(test)]
pub(super) fn net_ns(priors: &Priors, share: u64, compile_ns_per_ir: u64, wall_permille: u64, observed: &Observed) -> i64 {
    net_planned_ns(priors, share, (0, compile_ns_per_ir), wall_permille, observed)
}

/// `net_ns` where the body's plan is still to make, at `plan_ns_per_ir`, besides its compile.
pub(super) fn net_planned_ns(priors: &Priors, share: u64, (plan_ns_per_ir, compile_ns_per_ir): (u64, u64), wall_permille: u64, observed: &Observed) -> i64 {
    let interpreted = (observed.length + observed.loop_work) * priors.interpreter_ns * (PER_MILLE - share) / PER_MILLE;
    let crossings = observed.region_runs * priors.region_entry_ns / u64::from(observed.entries.max(1));
    let saved = (interpreted + crossings).saturating_sub(priors.body_entry_ns) as i64;
    let future = (u64::from(observed.entries) / priors.remaining_divisor) as i64;
    future * saved - outlay_ns(observed.ir, plan_ns_per_ir, compile_ns_per_ir, wall_permille) as i64
}

/// Whether the model admits the compile of `observed`, whose plan is made already unless
/// `unplanned` (its plan is then charged too); where `tiers` admits every eligible compile
/// (`Tiers::admits_all`, the floor row), it is admitted.
pub(super) fn admits(tiers: &Tiers, observed: &Observed, unplanned: bool, diagnostics: bool) -> bool {
    let priors = &tiers.priors;
    let plan = if unplanned { priors.plan_ns_per_ir } else { 0 };
    let net = net_planned_ns(priors, priors.body_share, (plan, priors.compile_ns_per_ir), compile_wall_permille(priors), observed);
    if diagnostics { shadow_log(priors, observed, net); }
    tiers.admits_all || net > 0
}

/// The decision under `QUENCH_TRACE=js-jit`.
fn shadow_log(priors: &Priors, observed: &Observed, net: i64) {
    eprintln!("QUENCH_TIER_MODEL entries={} ir={} region_runs={} compile_ns_per_ir={} net_ns={net} pick={}",
        observed.entries, observed.ir, observed.region_runs, priors.compile_ns_per_ir, if net > 0 { "u-jit" } else { "interp" });
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tier_policy::TEST_PRIORS as PRIORS;


    fn observed(length: u64, loop_work: u64, entries: u32, region_runs: u64, ir: usize) -> Observed { Observed { length, loop_work, entries, region_runs, ir } }
    fn net(observed: &Observed) -> i64 { net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, PER_MILLE, observed) }

    #[test]
    fn a_short_leaf_never_repays_its_entry() {
        // 40 bytecodes save 280 ns an entry, under the 330 ns crossing: no entry count pays.
        assert!(net(&observed(40, 0, u32::MAX, 0, 100)) < 0);
    }

    #[test]
    fn a_long_body_pays_once_enough_entries_were_seen() {
        let long = |entries| net(&observed(400, 0, entries, 0, 1000));
        // Saves 400*10*0.7 - 330 = 2470 ns an entry; the compile costs 3 ms.
        assert!(long(4000) < 0 && long(8000) > 0);
    }

    #[test]
    fn a_loop_body_counts_its_trip_work() {
        assert!(net(&observed(20, 0, 4000, 0, 200)) < 0);
        assert!(net(&observed(20, 5000, 4000, 0, 200)) > 0);
    }

    #[test]
    fn a_region_inside_the_function_leaves_the_bodys_other_work_and_its_crossings_to_save() {
        // A 10-bytecode driver whose loop runs natively saves 70 ns of its own work an entry,
        // under the 330 ns body crossing, unless each entry also crosses into the region often.
        let driver = |region_runs| net(&observed(10, 0, 4000, region_runs, 10));
        assert!(driver(4000) < 0, "one 140 ns crossing an entry does not repay the body's");
        assert!(driver(4 * 4000) > 0, "four crossings an entry do");
        // The straight-line work around the region still counts: 400 bytecodes pay as without one.
        assert!(net(&observed(400, 0, 8000, 8000, 1000)) > net(&observed(400, 0, 8000, 0, 1000)));
    }

    #[test]
    fn a_shared_core_charges_the_whole_compile() {
        let work = observed(400, 0, 8000, 0, 1000);
        assert!(net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, 100, &work) > net(&work));
    }

    #[test]
    fn a_plan_still_to_make_is_charged_on_the_programs_thread() {
        // 400 bytecodes save 2470 ns an entry: 6000 entries (1500 to come, 3.7 ms) repay the
        // 3 ms compile of 1000 instructions, not the 1.5 ms plan besides it.
        let work = observed(400, 0, 6000, 0, 1000);
        assert!(net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, PER_MILLE, &work) > 0);
        assert!(net_planned_ns(&PRIORS, PRIORS.body_share, (PRIORS.plan_ns_per_ir, PRIORS.compile_ns_per_ir), PER_MILLE, &work) < 0);
    }
}
