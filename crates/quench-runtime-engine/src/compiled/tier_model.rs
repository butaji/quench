//! The tier decision rule for a lane body's compile (`tier-decision-rule-v1`, sections 1, 3, 5):
//! a compile is admitted where the model's net gain in nanoseconds is positive under the
//! credible lower bound of the entries to come. The thresholds it replaced are gone; the
//! numbers it reads are the VM's `Tiers::priors`. The compile and the plan are priced by their work (IR
//! instructions), not timed: what is compiled then repeats from run to run, under callgrind and
//! the virtual clock alike (policies follow work, not wall time).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::tier_policy::{Priors, Tiers, PER_MILLE};

/// What the model knows of one function at a decision point.
pub(crate) struct Observed {
    /// Bytecodes of straight-line work per entry.
    pub(crate) length: u64,
    /// Bytecodes per entry inside its loops that the interpreter still runs (trip means times spans).
    pub(crate) loop_work: u64,
    /// Entries so far.
    pub(crate) entries: u32,
    /// Native runs of its loops that run in a region entered from the lane, over all entries:
    /// each a crossing the body removes, while the loop's own work is native already.
    pub(crate) region_runs: u64,
    /// IR instructions of the planned body.
    pub(crate) ir: usize,
}

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
/// still takes. Loop regions (`loop_model`) and lane bodies both save this much per iteration or entry.
pub(crate) fn interpreted_ns(interpreter_ns: u64, work: u64, share: u64) -> u64 {
    work * interpreter_ns * (PER_MILLE - share) / PER_MILLE
}

/// The future uses of a compile after `count` past ones (Lindy): `count / remaining_divisor`.
pub(crate) fn future_count(priors: &Priors, count: u64) -> u64 {
    count / priors.remaining_divisor
}

/// The net gain in ns of a compile: `future` uses to come, each saving `saved` ns, less the
/// `outlay` the compile and its entry cost the program. Exact: no product saturates.
pub(crate) fn net_gain_ns(future: u64, saved: u64, outlay: u64) -> i128 {
    i128::from(future) * i128::from(saved) - i128::from(outlay)
}

/// What one entry of `observed` takes from the interpreter in code taking `share` per mille of
/// its time per bytecode, and the region crossings it makes: the work it leaves to native code
/// is saved, and each crossing is paid by the entry.
fn entry_work_ns(priors: &Priors, share: u64, observed: &Observed) -> u64 {
    let interpreted = interpreted_ns(priors.interpreter_ns, observed.length + observed.loop_work, share);
    let crossings = observed.region_runs * priors.region_entry_ns / u64::from(observed.entries.max(1));
    interpreted + crossings
}

/// What one entry of `observed` saves in ns in code taking `share` per mille of the interpreter's
/// time per bytecode behind an `entry_ns` crossing: `entry_work_ns`, less its own entry.
fn saved_per_entry(priors: &Priors, share: u64, entry_ns: u64, observed: &Observed) -> i64 {
    entry_work_ns(priors, share, observed) as i64 - entry_ns as i64
}

/// The net gain in ns of compiling `observed` into code that takes `share` per mille of the
/// interpreter's time per bytecode, compiled at `compile_ns_per_ir`: each future entry
/// (Lindy, `entries / remaining_divisor`) saves the interpreter's work outside native regions
/// less the share the body still takes, and the region crossings an entry makes, less its own
/// entry crossing; the compile costs the wall share of its time.
#[cfg(test)]
pub(crate) fn net_ns(priors: &Priors, share: u64, compile_ns_per_ir: u64, wall_permille: u64, observed: &Observed) -> i64 {
    net_planned_ns(priors, share, (0, compile_ns_per_ir), wall_permille, observed)
}

/// `net_ns` where the body's plan is still to make, at `plan_ns_per_ir`, besides its compile.
pub(crate) fn net_planned_ns(priors: &Priors, share: u64, (plan_ns_per_ir, compile_ns_per_ir): (u64, u64), wall_permille: u64, observed: &Observed) -> i64 {
    let saved = entry_work_ns(priors, share, observed).saturating_sub(priors.body_entry_ns);
    let outlay = outlay_ns(observed.ir, plan_ns_per_ir, compile_ns_per_ir, wall_permille);
    net_gain_ns(future_count(priors, u64::from(observed.entries)), saved, outlay) as i64
}

/// The net gain in ns of a b-jit body of `observed`: stencil code at `base_body_share` per
/// bytecode behind the same `body_entry_ns` entry a u-jit body pays, stitched in microseconds on the program's thread,
/// so neither a plan nor a compile is charged (`Tiers::amortizing_instructions`' `Tier::Base`).
pub(crate) fn base_net_ns(priors: &Priors, observed: &Observed) -> i64 {
    let saved = entry_work_ns(priors, priors.base_body_share, observed).saturating_sub(priors.body_entry_ns);
    net_gain_ns(future_count(priors, u64::from(observed.entries)), saved, 0) as i64
}

/// The tier the model picks for a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// Neither tier pays yet: it counts on.
    Interpreter,
    /// b-jit, for good: each entry of a u-jit body would save no more than a b-jit body's, so
    /// no number of entries repays a u-jit compile over it.
    Base,
    /// u-jit: its net gain is positive, and it saves more per entry than a b-jit body would.
    Ultra,
}

/// The pick from the u-jit body's net gain `ultra` and, where the mode runs b-jit too, the
/// b-jit body's net gain and each body's saving per entry (`base`: net, b-jit saving, u-jit
/// saving). A function whose u-jit body would save no more per entry than its b-jit body takes
/// b-jit for good, once that pays. Any other weighs u-jit against the interpreter alone: a body,
/// unlike a loop region (`loop_model::promotes`), is never promoted from b-jit to u-jit, so the
/// b-jit body it would wait beside is never built, and waiting for u-jit to get ahead of it only
/// leaves the function interpreted the longer.
pub(crate) fn pick(ultra: i64, base: Option<(i64, i64, i64)>) -> Pick {
    match base {
        Some((net, base_saving, ultra_saving)) if base_saving >= ultra_saving => if net > 0 { Pick::Base } else { Pick::Interpreter },
        _ if ultra > 0 => Pick::Ultra,
        _ => Pick::Interpreter,
    }
}

/// What the model picks for `observed`, whose u-jit plan is made already unless `unplanned` (its
/// plan is then charged too). Where `tiers` admits every eligible compile (`Tiers::admits_all`,
/// the floor row), it picks u-jit.
pub(crate) fn decide(tiers: &Tiers, observed: &Observed, unplanned: bool, diagnostics: bool) -> Pick {
    let priors = &tiers.priors;
    let plan = if unplanned { priors.plan_ns_per_ir } else { 0 };
    let net = net_planned_ns(priors, priors.body_share, (plan, priors.compile_ns_per_ir), compile_wall_permille(priors), observed);
    // In the default mode b-jit is the tier below u-jit: a function u-jit would not beat per entry takes it.
    let base = quench_runtime::jit_mode::tiers_allowed(&[quench_runtime::jit_mode::Tier::Base]).then(|| (base_net_ns(priors, observed),
        saved_per_entry(priors, priors.base_body_share, priors.body_entry_ns, observed),
        saved_per_entry(priors, priors.body_share, priors.body_entry_ns, observed)));
    let picked = if tiers.admits_all { Pick::Ultra } else { pick(net, base) };
    if diagnostics { shadow_log(priors, observed, net, base.map(|(net, ..)| net), picked); }
    picked
}

/// Whether the u-jit body `decide` admitted for `observed` compiles quick (`CompileLevel::Quick`):
/// where the entries to come gain more from its cheaper compile than they lose to its slower code
/// (`Priors::quick_body_share`, `quick_compile_permille`). A body entered many times takes the
/// optimizing compile; one compiled quick moves to it once its entries repay that (`optimizes`).
pub(crate) fn compiles_quick(tiers: &Tiers, observed: &Observed, unplanned: bool) -> bool {
    !tiers.admits_all && quick_ahead(&tiers.priors, observed, unplanned)
}

/// `compiles_quick` under `priors`.
fn quick_ahead(priors: &Priors, observed: &Observed, unplanned: bool) -> bool {
    let plan = if unplanned { priors.plan_ns_per_ir } else { 0 };
    let wall = compile_wall_permille(priors);
    let optimized = net_planned_ns(priors, priors.body_share, (plan, priors.compile_ns_per_ir), wall, observed);
    let quick = net_planned_ns(priors, priors.quick_body_share, (plan, priors.compile_ns_per_ir * priors.quick_compile_permille / PER_MILLE), wall, observed);
    quick > optimized
}

/// Whether a body compiled quick moves to an optimizing compile after `observed.entries` entries
/// in all: the entries to come (`remaining_divisor`) at the optimized body's extra saving per
/// entry repay its plan and compile.
pub(crate) fn optimizes(tiers: &Tiers, observed: &Observed) -> bool {
    optimizes_under(&tiers.priors, observed)
}

fn optimizes_under(priors: &Priors, observed: &Observed) -> bool {
    let work = (observed.length + observed.loop_work) * priors.interpreter_ns;
    let gain = work * priors.quick_body_share.saturating_sub(priors.body_share) / PER_MILLE;
    let outlay = outlay_ns(observed.ir, priors.plan_ns_per_ir, priors.compile_ns_per_ir, compile_wall_permille(priors));
    net_gain_ns(future_count(priors, u64::from(observed.entries)), gain, outlay) > 0
}

/// Whether the model admits the u-jit compile of `observed` (`decide` picks u-jit).
#[cfg(test)]
pub(crate) fn admits(tiers: &Tiers, observed: &Observed, unplanned: bool, diagnostics: bool) -> bool {
    decide(tiers, observed, unplanned, diagnostics) == Pick::Ultra
}

/// The decision under `QUENCH_TRACE=js-jit`.
fn shadow_log(priors: &Priors, observed: &Observed, net: i64, base: Option<i64>, picked: Pick) {
    let pick = match picked { Pick::Ultra => "u-jit", Pick::Base => "b-jit", Pick::Interpreter => "interp" };
    eprintln!("QUENCH_TIER_MODEL entries={} ir={} region_runs={} compile_ns_per_ir={} net_ns={net} base_net_ns={} pick={pick}",
        observed.entries, observed.ir, observed.region_runs, priors.compile_ns_per_ir, base.map_or(String::from("-"), |base| base.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u_jit_is_weighed_against_the_interpreter_where_it_saves_more() {
        // u-jit saves more per entry: no b-jit body is built beside it, so it compiles once it pays.
        assert_eq!(pick(100, Some((50, 5, 9))), Pick::Ultra);
        assert_eq!(pick(50, Some((100, 5, 9))), Pick::Ultra);
        assert_eq!(pick(-1, Some((100, 5, 9))), Pick::Interpreter);
        // u-jit saves no more per entry: b-jit is the tier for good.
        assert_eq!(pick(100, Some((60, 9, 9))), Pick::Base);
        assert_eq!(pick(-1, Some((-5, 9, 3))), Pick::Interpreter);
        assert_eq!(pick(100, None), Pick::Ultra);
        assert_eq!(pick(-1, None), Pick::Interpreter);
    }

    #[test]
    fn a_b_jit_body_charges_no_compile() {
        // The same entries pay a b-jit body before a u-jit one: it charges no compile.
        let observed = observed(40, 0, 2000, 0, 5000);
        assert!(base_net_ns(&PRIORS, &observed) > net(&observed));
    }
    use quench_runtime::vm::tier_policy::TEST_PRIORS as PRIORS;


    fn observed(length: u64, loop_work: u64, entries: u32, region_runs: u64, ir: usize) -> Observed { Observed { length, loop_work, entries, region_runs, ir } }
    fn net(observed: &Observed) -> i64 { net_ns(&PRIORS, PRIORS.body_share, PRIORS.compile_ns_per_ir, PER_MILLE, observed) }

    #[test]
    fn a_body_entered_rarely_compiles_quick_and_one_entered_often_optimized() {
        let rare = observed(400, 0, 6000, 0, 4000);
        let hot = observed(400, 0, 4_000_000, 0, 4000);
        assert!(quick_ahead(&PRIORS, &rare, false));
        assert!(!quick_ahead(&PRIORS, &hot, false));
        assert!(!optimizes_under(&PRIORS, &rare) && optimizes_under(&PRIORS, &hot));
    }

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
