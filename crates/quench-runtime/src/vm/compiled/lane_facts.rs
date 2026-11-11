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
/// the lane's own run of a short leaf never pays: a loop, an inlined call, or length
/// (`TIERS.lane_body.min_instructions`).
pub(super) fn amortizes_entry(unit: &lane_unit::Unit) -> bool {
    let loops = unit.instructions.iter().enumerate().any(|(pc, instruction)|
        instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget && (instruction.imm() as usize) <= pc);
    loops || !unit.frames.is_empty() || unit.instructions.len() >= TIERS.lane_body.min_instructions
}

/// What a compile of a plan comes to, as one number: its unit, the leaves and entry it was
/// planned from, its field proofs and its direct callees. Equal keys make equal code.
pub(super) fn plan_key(unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug,
    direct: usize, generic: &[LaneExit], entry: Option<u32>, budget: usize) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    format!("{:?}{:?}{fields:?}{generic:?}{entry:?}{budget}{direct}", unit.instructions, unit.frames)
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

/// `repays_compile` for `function`'s `entries` so far, each running its whole body.
pub(super) fn pays_compile(program: &ResidualProgram, function: u32, entries: u32, ir: usize) -> bool {
    let length = program.function_get(function as usize).map_or(0, |body| body.code.len()) as u64;
    repays_compile(u64::from(entries) * length, ir)
}
