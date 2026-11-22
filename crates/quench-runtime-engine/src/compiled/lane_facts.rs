//! What a lane body's plan reads of its unit alone: its leaves, direct sites, entry and the
//! facts about its callees.

use super::*;
use std::collections::BTreeMap;

/// The unit PCs of a previous body's leaves: the specialization's feedback. Only the table's
/// places are read here, so no leave's stored values are asked for.
pub(super) fn unit_leaves(unit: &lane_unit::Unit, generic: &[LaneExit]) -> Vec<usize> {
    let exits = ExitTable::of_unit(unit, |_, _| Vec::new());
    generic.iter().filter_map(|exit| exits.site(*exit).map(|site| site.unit_pc as usize)).collect()
}

/// The calls a unit still makes (none it inlines) whose callees have bodies. Each candidate
/// owns one contiguous stable slice, which the code names by address. A site with several
/// callees binds every candidate the unit does not inline, in the order the candidates list them
/// (a chain of identity tests takes them, `Lowering::polymorphic_call`); a construction binds only
/// the first, and a site whose candidates are all inlined binds the first of them.
pub(super) fn unit_direct_calls(unit: &lane_unit::Unit, direct: &[DirectCandidates])
    -> (Box<[DirectCallee]>, BTreeMap<usize, Vec<*const DirectCallee>>) {
    let chosen: Vec<(usize, Vec<DirectCallee>)> = unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Call) || quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Construct))
        .filter_map(|(pc, instruction)| match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => {
                let inlined = |callee: &DirectCallee| unit.frames.iter()
                    .any(|frame| frame.parent.is_none() && frame.call == origin && frame.target.function == callee.target.function);
                let candidates = direct.get(origin as usize)?;
                let mut bound: Vec<DirectCallee> = candidates.iter().filter(|callee| !inlined(callee)).cloned().collect();
                if bound.is_empty() { bound.extend(candidates.first().cloned()); }
                if quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::Construct) { bound.truncate(1); }
                (!bound.is_empty()).then_some((pc, bound))
            }
            lane_unit::Origin::Inlined { .. } => None,
        })
        .collect();
    let mut owned: Vec<DirectCallee> = Vec::new();
    let mut spans: Vec<(usize, std::ops::Range<usize>)> = Vec::with_capacity(chosen.len());
    for (pc, callees) in chosen {
        let start = owned.len();
        owned.extend(callees);
        spans.push((pc, start..owned.len()));
    }
    let direct: Box<[DirectCallee]> = owned.into_boxed_slice();
    // The pointers are taken once the slice is boxed, so they name its final elements.
    let sites = spans.into_iter().map(|(pc, span)| (pc, direct[span].iter().map(std::ptr::from_ref).collect())).collect();
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
        lane_region::restartable_numeric_function(program, body, body.instructions()))
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

/// Whether `code` jumps backward: it has a loop, whose iterations repay any entry.
pub(super) fn closes_loop(code: &[quench_runtime::bytecode::WideInstruction]) -> bool {
    code.iter().enumerate().any(|(pc, instruction)|
        instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget && (instruction.imm() as usize) <= pc)
}

/// Whether one entry of `unit` repays the body's entry cost (`Priors::body_entry_ns`): the one
/// such predicate, shared by every compile gate and by the published method. A body has work
/// that amortizes an entry, which costs a few hundred instructions the lane's own run of a short
/// leaf never pays: a loop, an inlined call, a call of itself (its recursion runs natively inside
/// the entry), or length (`Tiers::amortizing_instructions`, which counts the inlined callees'
/// instructions too, as they are in `unit.instructions`).
pub(super) fn amortizes_entry(tiers: &Tiers, unit: &lane_unit::Unit, function: u32, direct: &[DirectCallee]) -> bool {
    closes_loop(&unit.instructions) || !unit.frames.is_empty() || calls_itself(direct, function) || unit.instructions.len() >= tiers.amortizing_instructions(tier_policy::body_tier_in_lane())
}

use tier_policy::RECURSION_DEPTH;

/// The functions that call `function` back through the targets their call sites linked
/// (`targets`), each within `RECURSION_DEPTH` calls of it: `function` itself where it recurses
/// directly, and the other members of a mutual recursion (`f` calling `g` calling `f`). Empty
/// where `function` does not recurse.
pub(super) fn recursion_cycle(program: &ResidualProgram, function: u32, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>) -> std::collections::BTreeSet<u32> {
    let callees = |caller: u32| -> Vec<u32> {
        let Some(body) = program.function_get(caller as usize) else { return Vec::new() };
        (0..body.code.len() as u32).flat_map(|pc| targets(caller, pc)).map(|target| target.function).collect()
    };
    // The call graph out of `function`, as far as `RECURSION_DEPTH` calls.
    let mut edges: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut frontier = std::collections::BTreeSet::from([function]);
    for _ in 0..RECURSION_DEPTH {
        let mut next = std::collections::BTreeSet::new();
        for caller in frontier {
            let std::collections::btree_map::Entry::Vacant(slot) = edges.entry(caller) else { continue };
            let out = slot.insert(callees(caller));
            next.extend(out.iter().copied());
        }
        frontier = next;
    }
    // Those of its nodes from which `function` is reached again.
    let mut cycle = std::collections::BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (caller, out) in &edges {
            if !cycle.contains(caller) && out.iter().any(|callee| *callee == function || cycle.contains(callee)) {
                cycle.insert(*caller);
                changed = true;
            }
        }
    }
    cycle
}

/// Whether a body of `function` calls itself directly: one entry then runs a native call tree
/// rather than one straight line, as a loop does. Recursion through other functions is
/// `recursion_cycle`'s.
fn calls_itself(direct: &[DirectCallee], function: u32) -> bool {
    direct.iter().any(|callee| callee.target.function == function)
}

/// What a compile of a plan comes to, as one number: its unit, the leaves and entry it was
/// planned from, its field proofs and its direct callees. Equal keys make equal code.
pub(super) fn plan_key(unit: &lane_unit::Unit, fields: &dyn std::fmt::Debug,
    direct: usize, generic: &[LaneExit], entry: Option<u32>, budget: usize,
    arrays: &[(quench_runtime::bytecode::Register, quench_runtime::typed_ir::Type)]) -> u64 {
    use std::fmt::Write;
    // The instructions, most of the key, fold as their words; the rest as its debug text,
    // written into the hash without a string between.
    let mut key = Fnv::default();
    for instruction in &unit.instructions {
        let (word, imm) = instruction.words();
        key.word(word);
        key.word(u64::from(imm));
    }
    let _ = write!(key, "{:?}{fields:?}{generic:?}{entry:?}{budget}{direct}{arrays:?}", unit.frames);
    key.0
}

/// A 64-bit FNV-1a hash.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self { Self(0xcbf2_9ce4_8422_2325) }
}

impl Fnv {
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn word(&mut self, word: u64) {
        self.0 = (self.0 ^ word).wrapping_mul(Self::PRIME);
    }
}

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 = text.bytes().fold(self.0, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(Self::PRIME));
        Ok(())
    }
}

/// Whether the tier model admits compiling `function`'s body for the `entries` it has made
/// (`tier_model::admits`), its plan charged too where `unplanned`: the gate every lane-body
/// compile request passes.
pub(super) fn pays_compile(tiers: &Tiers, program: &ResidualProgram, function: u32, entries: u32, ir: usize, caller: &crate::dispatch_fast::LaneView, unplanned: bool, diagnostics: bool) -> super::tier_model::Pick {
    let length = program.function_get(function as usize).map_or(0, |body| body.code.len()) as u64;
    super::tier_model::decide(tiers, &super::tier_model::Observed { length, loop_work: caller.loop_work(), entries, region_runs: caller.region_runs(), ir }, unplanned, diagnostics)
}

/// Whether `caller`'s body, compiled quick from a plan of `ir` instructions, moves to an
/// optimizing compile now that it has run `entries` times (`tier_model::optimizes`).
pub(crate) fn optimizes_quick_body(tiers: &Tiers, caller: &crate::dispatch_fast::LaneView, length: u64, entries: u32, ir: usize) -> bool {
    super::tier_model::optimizes(tiers, &super::tier_model::Observed { length, loop_work: caller.loop_work(), entries, region_runs: caller.region_runs(), ir })
}

/// The calls a plan keeps are its caller's facts and the demand for their callees' bodies: a
/// declined short callee warms again while the caller publishes at once, calling it through
/// its entry cell. A callee the plan inlines has no call of it to bind.
pub(crate) fn bind_callees(direct: &[DirectCallee], caller: &crate::dispatch_fast::LaneView) {
    for callee in direct {
        // SAFETY: a resolved target's view outlives its program.
        if let Some(view) = unsafe { callee.target.view.as_ref() } { view.note_caller(caller); view.demand_body(); }
    }
}

/// What a lane compile reports under `QUENCH_TRACE=js-jit` (`QUENCH_LANE_COMPILE`).
pub(super) struct Census {
    pub(super) function: u32, pub(super) osr: bool, pub(super) unit: usize, pub(super) ir: usize, pub(super) key: u64,
    pub(super) generic: usize, pub(super) direct: usize, pub(super) budget: usize, pub(super) observed: Option<u32>, pub(super) closed: bool,
    /// The planning, proofs and lowering on the VM's own thread, which the background `us`
    /// excludes and the measured window pays.
    pub(super) plan_us: u64,
}

impl Census {
    pub(super) fn report(&self, took: std::time::Duration, bytes: usize) -> String {
        let Self { function, osr, unit, ir, key, generic, direct, budget, observed, closed, plan_us } = self;
        format!("QUENCH_LANE_COMPILE {{\"function\":{function},\"osr\":{osr},\"unit\":{unit},\"ir\":{ir},\"generic\":{generic},\"direct\":{direct},\"budget\":{budget},\"observed\":{},\"key\":\"{key:016x}\",\"us\":{},\"closed\":{closed},\"bytes\":{bytes},\"plan_us\":{plan_us}}}",
            observed.map_or(-1, i64::from), took.as_micros())
    }
}

/// Whether every call of a unit that has run is direct (`LaneMethod::closed`). A call that
/// recorded no callee has not run; where it first does it leaves, as any helper call does
/// under a direct caller's frames.
pub(super) fn unit_closed(unit: &lane_unit::Unit, function: u32, direct: impl Fn(usize) -> bool,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>) -> bool {
    unit.instructions.iter().enumerate().all(|(pc, instruction)| !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call)
        || direct(pc)
        || match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => targets(function, origin),
            lane_unit::Origin::Inlined { frame, pc } => targets(unit.frames[frame].target.function, pc),
        }.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_line_unit_repays_its_entry_from_the_tier_length() {
        // The length threshold is `Tiers::amortizing_instructions`, checked per tier by
        // `tier_policy`'s `each_lane_body_tier_has_its_own_amortizing_length`.
        let priors = &tier_policy::TEST_PRIORS;
        assert_eq!(priors.repaying_length(priors.body_share), 48);
    }
}
