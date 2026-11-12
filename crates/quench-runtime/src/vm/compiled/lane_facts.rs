//! What a lane body's plan reads of its unit alone: its leaves, direct sites, entry and the
//! facts about its callees.

use super::*;
use std::collections::BTreeMap;

/// The unit PCs of a previous body's leaves: the specialization's feedback.
pub(super) fn unit_leaves(unit: &lane_unit::Unit, generic: &[LaneExit]) -> Vec<usize> {
    generic.iter().filter_map(|exit| unit.origins.iter().position(|origin| match *exit {
        LaneExit::At(pc) => *origin == lane_unit::Origin::Caller(pc as u32),
        LaneExit::Inlined { frame, pc } => *origin == lane_unit::Origin::Inlined { frame, pc },
        LaneExit::Callee | LaneExit::Returned { .. } | LaneExit::Terminated | LaneExit::Entry => false,
    })).collect()
}

/// The calls a unit still makes (none it inlines) whose callees have bodies. Each candidate
/// owns one contiguous stable slice, which the code names by address. A site with several
/// callees binds the first the unit does not inline: the calls its alternatives miss.
pub(super) fn unit_direct_calls(unit: &lane_unit::Unit, direct: &[DirectCandidates])
    -> (Box<[DirectCallee]>, BTreeMap<usize, *const DirectCallee>) {
    let (pcs, direct): (Vec<usize>, Vec<DirectCallee>) = unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| instruction.op().is_call() || instruction.op().class().eq(crate::bytecode::Class::Construct))
        .filter_map(|(pc, _)| match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => {
                let inlined = |callee: &DirectCallee| unit.frames.iter()
                    .any(|frame| frame.parent.is_none() && frame.call == origin && frame.target.function == callee.target.function);
                let candidates = direct.get(origin as usize)?;
                candidates.iter().find(|callee| !inlined(callee)).or_else(|| candidates.first()).cloned().map(|callee| (pc, callee))
            }
            lane_unit::Origin::Inlined { .. } => None,
        })
        .unzip();
    let direct: Box<[DirectCallee]> = direct.into_boxed_slice();
    let sites = pcs.iter().zip(direct.iter()).map(|(pc, callee)| (*pc, std::ptr::from_ref(callee))).collect();
    (direct, sites)
}

/// The unit PC a running loop enters at: its `header`'s own instruction; `Some(None)` for a
/// body that starts at its first, `None` where the unit has no such instruction.
pub(super) fn entry_pc(unit: &lane_unit::Unit, header: Option<u32>) -> Option<Option<usize>> {
    let Some(header) = header else { return Some(None) };
    unit.origins.iter().position(|origin| *origin == lane_unit::Origin::Caller(header)).map(Some)
}

/// Whether a function is pure numeric, observing neither closure environment nor identity.
pub(super) fn numeric_callee(program: &ResidualProgram, function: u32) -> bool {
    program.function_get(function as usize).is_some_and(|body| body.plain_activation(program))
        && numeric_restartable(program, function)
}

/// Whether a callee bound by its definition restarts its call on a leave, as a pure numeric
/// function does.
pub(super) fn numeric_restartable(program: &ResidualProgram, function: u32) -> bool {
    program.function_get(function as usize).is_some_and(|body|
        lane_region::restartable_numeric_function(program, body, &body.instructions().collect::<Vec<_>>()))
}

/// `targets`, each once, in first-seen order.
pub(super) fn each_once(mut targets: Vec<CallTarget>) -> Vec<CallTarget> {
    let mut kept = 0;
    for index in 0..targets.len() {
        let target = targets[index];
        if !targets[..kept].contains(&target) { targets[kept] = target; kept += 1; }
    }
    targets.truncate(kept);
    targets
}

/// Whether a body has work that amortizes an entry, which costs a few hundred instructions
/// the lane's own run of a short leaf never pays: a loop, an inlined call, a call of itself
/// (its recursion runs natively inside the entry), or length (`TIERS.lane_body.min_instructions`).
pub(super) fn amortizes_entry(unit: &lane_unit::Unit, function: u32, direct: &[DirectCallee]) -> bool {
    let loops = unit.instructions.iter().enumerate().any(|(pc, instruction)|
        instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget && (instruction.imm() as usize) <= pc);
    loops || !unit.frames.is_empty() || direct.iter().any(|callee| callee.target.function == function) || unit.instructions.len() >= TIERS.lane_body.min_instructions
}

/// What a compile of a plan comes to, as one number: its unit, the leaves and entry it was
/// planned from, its field proofs and its direct callees. Equal keys make equal code.
pub(super) fn plan_key(unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug,
    direct: usize, generic: &[LaneExit], entry: Option<u32>, budget: usize,
    arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    format!("{:?}{:?}{fields:?}{generic:?}{entry:?}{budget}{direct}{arrays:?}", unit.instructions, unit.frames)
        .bytes().fold(FNV_OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME))
}

/// Whether `interpreted` instructions of work so far, which a compiled body would run, repay
/// the compile of `ir` instructions at the tier's margin (`TIERS.lane_body`): the gate every
/// compile request passes, bytecode units and native engines alike.
pub(in crate::vm) fn repays_compile(interpreted: u64, ir: usize) -> bool {
    let lane = &TIERS.lane_body;
    let (saved, of) = lane.saved_share;
    interpreted * saved >= lane.benefit_over_cost * lane.compile_cost_per_ir * ir as u64 * of
}

/// `repays_compile` for `function`'s `entries` so far, each running its whole body; where its
/// loop runs in a region that enters native code, each saves that entry alone
/// (`TIERS.loops.region_entry_work`).
pub(super) fn pays_compile(program: &ResidualProgram, function: u32, entries: u32, ir: usize, caller: &super::super::dispatch_fast::LaneView, diagnostics: bool) -> bool {
    let length = program.function_get(function as usize).map_or(0, |body| body.code.len()) as u64;
    let region = caller.enters_region();
    let today = repays_compile(u64::from(entries) * if region { TIERS.loops.region_entry_work } else { length }, ir);
    if diagnostics { let work = caller.loop_work(); shadow_log(function, entries, ir, region, today, shadow_net_ns(length, entries, ir, region, work, false), shadow_net_ns(length, entries, ir, region, work, true)); }
    today
}

/// The tier model's net gain in ns for `function` at `entries` so far (shadow: logged beside
/// `pays_compile`, deciding nothing): each future entry (Lindy: as many as seen, over the
/// prior's divisor) saves the interpreter's share of the work the body takes over, less the
/// body's own entry, minus the compile of `ir` instructions.
fn shadow_net_ns(length: u64, entries: u32, ir: usize, entering_region: bool, loop_work: u64, quick: bool) -> i64 {
    let priors = &TIERS.priors;
    let (share, compile) = if quick { (priors.quick_body_share, priors.quick_compile_ns_per_ir) } else { (priors.body_share, priors.compile_ns_per_ir) };
    let per_entry = if entering_region { priors.region_entry_ns } else { (length + loop_work) * priors.interpreter_ns * (super::tier_policy::PER_MILLE - share) / super::tier_policy::PER_MILLE };
    let saved = per_entry.saturating_sub(priors.body_entry_ns) as i64;
    let future = (u64::from(entries) / priors.remaining_divisor) as i64;
    future * saved - (ir as u64 * compile) as i64
}

fn shadow_log(function: u32, entries: u32, ir: usize, region: bool, today: bool, net: i64, quick_net: i64) {
    let pick = if net.max(quick_net) <= 0 { "interp" } else if quick_net >= net { "q-jit" } else { "u-jit" };
    eprintln!("QUENCH_TIER_SHADOW function={function} entries={entries} ir={ir} region={region} today={today} model={} net_ns={net} quick_net_ns={quick_net} pick={pick}{}", net > 0, if today != (net > 0) { " DISAGREE" } else { "" });
}

/// The calls a plan keeps are its caller's facts and the demand for their callees' bodies: a
/// declined short callee warms again while the caller publishes at once, calling it through
/// its entry cell. A callee the plan inlines has no call of it to bind.
pub(super) fn bind_callees(direct: &[DirectCallee], caller: &super::super::dispatch_fast::LaneView) {
    for callee in direct {
        // SAFETY: a resolved target's view outlives its program.
        if let Some(view) = unsafe { callee.target.view.as_ref() } { view.note_caller(caller); view.demand_body(); }
    }
}

/// What a lane compile reports under `QUENCH_TRACE=js-jit` (`QUENCH_LANE_COMPILE`).
pub(super) struct Census {
    pub(super) function: u32, pub(super) osr: bool, pub(super) unit: usize, pub(super) ir: usize, pub(super) key: u64,
    pub(super) generic: usize, pub(super) direct: usize, pub(super) budget: usize, pub(super) observed: Option<u32>, pub(super) closed: bool,
}

impl Census {
    pub(super) fn report(&self, took: std::time::Duration, bytes: usize) -> String {
        let Self { function, osr, unit, ir, key, generic, direct, budget, observed, closed } = self;
        format!("QUENCH_LANE_COMPILE {{\"function\":{function},\"osr\":{osr},\"unit\":{unit},\"ir\":{ir},\"generic\":{generic},\"direct\":{direct},\"budget\":{budget},\"observed\":{},\"key\":\"{key:016x}\",\"us\":{},\"closed\":{closed},\"bytes\":{bytes}}}",
            observed.map_or(-1, i64::from), took.as_micros())
    }
}

/// Whether every call of a unit that has run is direct (`LaneMethod::closed`). A call that
/// recorded no callee has not run; where it first does it leaves, as any helper call does
/// under a direct caller's frames.
pub(super) fn unit_closed(unit: &lane_unit::Unit, function: u32, direct: impl Fn(usize) -> bool,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>) -> bool {
    unit.instructions.iter().enumerate().all(|(pc, instruction)| !instruction.op().is_call()
        || direct(pc)
        || match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => targets(function, origin),
            lane_unit::Origin::Inlined { frame, pc } => targets(unit.frames[frame].target.function, pc),
        }.is_empty())
}
