//! Where a unit spends its inlining budget: by call-site weight per inlined instruction.
//!
//! A site's weight is the executions it is expected to make per entry of the caller: the product
//! of the trip counts of the loops around it, in its function and in each callee inlined above
//! it (`loop_weights`). A loop's trips are its measured mean (`LoopTier::mean_trip`) where a
//! native run took it, else `ASSUMED_LOOP_TRIP`. The caller's own entry count scales every site
//! of the unit alike, so it cannot reorder them. The plan first runs unbudgeted, noting each
//! site it inlines (`Candidate`); where they fit the budget that plan is the unit, else the
//! sites are chosen greedily by weight per instruction (`select`) and the plan runs again
//! admitting only them: a knapsack over the sites, each callee after the call that reaches it.

use super::*;

/// The calls from the caller down to a site: each call's PC in its function and its callee.
pub(super) type SitePath = Vec<(u32, u32)>;

/// A call site the unplanned unit would inline.
pub(super) struct Candidate {
    path: SitePath,
    weight: u64,
    /// The callee instructions it takes of the budget.
    cost: usize,
}

/// What the planner does at a call site that would inline: note it, or admit only the chosen.
pub(super) enum Choice {
    Survey(Vec<Candidate>),
    /// The chosen sites' paths, sorted.
    Admit(Vec<SitePath>),
}

impl Default for Choice {
    fn default() -> Self { Self::Admit(Vec::new()) }
}

/// Per PC of `code`, the executions per entry of its function: the product of the trip counts
/// of the loops (backward jumps) holding it. `trip` gives a loop's measured mean by its header.
pub(super) fn loop_weights(code: &[WideInstruction], trip: &dyn Fn(u32) -> Option<u64>) -> Vec<u64> {
    let loops: Vec<(usize, usize, u64)> = code.iter().enumerate()
        .filter(|(pc, instruction)| instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget && instruction.imm() as usize <= *pc)
        .map(|(pc, instruction)| (instruction.imm() as usize, pc, trip(instruction.imm()).unwrap_or(super::super::tier_policy::ASSUMED_LOOP_TRIP).max(1)))
        .collect();
    (0..code.len()).map(|pc| loops.iter().filter(|(header, backedge, _)| (*header..=*backedge).contains(&pc))
        .fold(1, |weight: u64, (_, _, trips)| weight.saturating_mul(*trips))).collect()
}

/// Orders by weight per instruction, heavier first (cross-multiplied: no division), a caller's
/// own call before its callees' and, equally dense, the earlier site first.
fn denser(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    (u128::from(b.weight) * a.cost as u128).cmp(&(u128::from(a.weight) * b.cost as u128))
        .then(a.path.len().cmp(&b.path.len()))
}

/// The sites to inline within `budget` instructions: greedy by density, a callee's own sites
/// only once the call that reaches it is chosen, and a site that no longer fits skipped.
pub(super) fn select(mut candidates: Vec<Candidate>, budget: usize) -> Vec<SitePath> {
    candidates.sort_by(denser);
    let (mut left, mut chosen) = (budget, Vec::<SitePath>::new());
    loop {
        let before = chosen.len();
        candidates.retain(|site| {
            let reachable = site.path.len() == 1 || chosen.iter().any(|path| path[..] == site.path[..site.path.len() - 1]);
            if reachable && site.cost <= left {
                left -= site.cost;
                chosen.push(site.path.clone());
            }
            !reachable
        });
        if chosen.len() == before { break; }
    }
    chosen.sort();
    chosen
}

impl Planner<'_, '_> {
    /// The plan of `function` (`function_id`) under `budget`, its call sites chosen by weight.
    pub(super) fn plan_weighted(&mut self, function_id: u32, function: &Function, budget: usize) -> Plan {
        let surveys = budget != 0;
        let (registers, strict) = (u32::from(function.registers), function.strict);
        self.restart(registers, if surveys { usize::MAX } else { 0 }, if surveys { Choice::Survey(Vec::new()) } else { Choice::default() });
        let plan = self.plan(function_id, decoded(function), strict, 0);
        let Choice::Survey(candidates) = std::mem::take(&mut self.choice) else { return plan };
        if usize::MAX - self.budget <= budget { return plan; }
        self.restart(registers, budget, Choice::Admit(select(candidates, budget)));
        self.plan(function_id, decoded(function), strict, 0)
    }

    fn restart(&mut self, registers: u32, budget: usize, choice: Choice) {
        (self.registers, self.budget, self.weight, self.choice) = (registers, budget, 1, choice);
        self.path.clear();
    }

    /// The target of the call at `pc`, inlined if the choice admits it: its site is noted where
    /// the plan surveys, and forgotten again with the callee's sites where it does not inline.
    pub(super) fn inline_target(&mut self, target: CallTarget, pc: u32, instruction: WideInstruction, depth: usize, weight: u64) -> Option<InlinedPlan> {
        self.path.push((pc, target.function));
        let mark = self.survey_len();
        let above = std::mem::replace(&mut self.weight, weight);
        let inlined = self.admitted().then(|| self.inline_body(target, instruction, depth)).flatten();
        self.weight = above;
        match &inlined {
            Some(callee) => self.note(weight, callee.plan.code.len()),
            None => self.forget_after(mark),
        }
        self.path.pop();
        inlined
    }

    fn admitted(&self) -> bool {
        match &self.choice {
            Choice::Survey(_) => true,
            Choice::Admit(chosen) => chosen.binary_search(&self.path).is_ok(),
        }
    }

    fn note(&mut self, weight: u64, cost: usize) {
        if let Choice::Survey(sites) = &mut self.choice {
            sites.push(Candidate { path: self.path.clone(), weight, cost });
        }
    }

    pub(super) fn survey_len(&self) -> usize {
        match &self.choice { Choice::Survey(sites) => sites.len(), Choice::Admit(_) => 0 }
    }

    pub(super) fn forget_after(&mut self, mark: usize) {
        if let Choice::Survey(sites) = &mut self.choice { sites.truncate(mark); }
    }
}
