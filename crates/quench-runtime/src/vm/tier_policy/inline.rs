//! Which call sites a compiled body inlines: the one rule a callee inlines at all (`inlines`), the growth a unit may add
//! (`INLINE_GROWTH_PERCENT`), the lane's scope of callees (`lane_callee`), and the decision for every call site of one
//! host in site order (`inline_plan`). The lane planner (`lane_unit`) and the tier cost model (`cost`) both read
//! `inline_plan`; the planner keeps only the lowering of the sites it decided.

use std::collections::BTreeMap;

use super::PERCENT;
use crate::bytecode::{FieldBase, FieldLookup, Function, ImmediateLayout, ImmediateRole, ResidualProgram, WideInstruction, RETURN_REGISTER};
use crate::compile::call_graph::CallGraph;
use crate::staged::meaning::{means, Meaning};
use crate::staged::{calls, Operation, OperationOperationRecordsExt};

/// The longest body a callee of the lane's inlined view may have (`lane_callee`): a leaf, which the lane's own
/// unit carries whole at each site.
pub const LANE_CALLEE_MAX_INSTRUCTIONS: usize = 16;

/// The growth a unit's inlining may add, in percent of its root body's instructions (design decision 4.6): the callee
/// instructions one unit copies in all are at most this share of the root's own size, spent on the call sites in
/// program order (`lane_unit`'s planner). A callee with exactly one call site adds no growth (`charges_growth`).
pub const INLINE_GROWTH_PERCENT: usize = 10;

/// The longest body a callee with no call in it inlines: an accessor, an arithmetic helper. Its copy costs code at
/// each site, and its call costs most relative to its work (design decision 4.6: 64).
pub const INLINE_LEAF_MAX_INSTRUCTIONS: usize = 64;

/// The longest body a callee with exactly one call site in the closed world inlines. That one site takes the one copy
/// the body makes, so its bound is the compile time a unit may spend on one body, not the code its copies cost.
pub const INLINE_SINGLE_SITE_MAX_INSTRUCTIONS: usize = 400;

/// The longest body an accessor-shaped callee inlines (`accessor_shaped`): a field read, compare or store of `this`
/// and the moves and returns around them, whose call is the whole of its cost.
pub const INLINE_ACCESSOR_MAX_INSTRUCTIONS: usize = 12;

/// Whether inlining `callee` adds code to its unit: a callee with one call site in the closed world moves its one body
/// rather than copying it, so it adds no growth and is not charged to `INLINE_GROWTH_PERCENT`.
pub fn charges_growth(graph: &CallGraph, callee: u32) -> bool {
    !graph.single_call_site(callee)
}

/// Whether the callee `callee` of `program` inlines at a call site the planner has proved a single target for
/// (an arm of its call plan). The one rule every tier asks, and it reads only static
/// facts: the body's size and shape, and the program's call graph (`CallGraph::single_call_site`). A callee inlines
/// where (a) it is a leaf, with no call, under `INLINE_LEAF_MAX_INSTRUCTIONS`; or (b) it has exactly one call site in
/// the closed world, under `INLINE_SINGLE_SITE_MAX_INSTRUCTIONS`; or (c) it is accessor-shaped, under
/// `INLINE_ACCESSOR_MAX_INSTRUCTIONS`. Size is the instruction count of the callee's body.
pub fn inlines(graph: &CallGraph, program: &ResidualProgram, callee: u32) -> bool {
    let Some(body) = program.function_get(callee as usize) else { return false };
    let size = body.instructions().count();
    let leaf = !body.instructions().any(|instruction| means(instruction, Meaning::Call) || means(instruction, Meaning::Construct));
    (leaf && size <= INLINE_LEAF_MAX_INSTRUCTIONS)
        || (graph.single_call_site(callee) && size <= INLINE_SINGLE_SITE_MAX_INSTRUCTIONS)
        || (size <= INLINE_ACCESSOR_MAX_INSTRUCTIONS && accessor_shaped(body))
}

/// Whether `body` is accessor-shaped: it reads or writes a field of `this` (a `FieldLookup::Atom` on `FieldBase::THIS`),
/// and every other instruction is a number operation, a move, a constant, a `this` load, or a `Return`.
fn accessor_shaped(body: &Function) -> bool {
    let mut touches_this = false;
    let shaped = body.instructions().all(|instruction| match Operation::<u64, ()>::of(instruction) {
        Some(Operation::GetField { .. } | Operation::SetField { .. }) => match instruction.field_lookup() {
            Some(FieldLookup::Atom { base, .. }) if base == FieldBase::THIS => {
                touches_this = true;
                true
            }
            _ => false,
        },
        Some(Operation::Number(_) | Operation::Move { .. } | Operation::LoadConst { .. } | Operation::LoadThis { .. } | Operation::Return { .. }) => true,
        _ => false,
    });
    shaped && touches_this
}

/// Whether a closed-set call's plan inlines its arms' bodies, so the call is lowered as the class-layout compare chain
/// (`closed_set_by_layout`) rather than the indirect call through the class's method vector (`closed_set_indirect`):
/// at most three arms, each of which the rule `inlines` admits.
pub fn arms_inline(plan: &crate::compile::call_plan::CallPlan, graph: &CallGraph, program: &ResidualProgram) -> bool {
    plan.arms.len() <= 3 && plan.arms.iter().all(|arm| inlines(graph, program, arm.function))
}


/// Whether the lane's inlined view carries `callee` at all (`lane_unit`'s scope over the rule `inlines`): a leaf of at
/// most `LANE_CALLEE_MAX_INSTRUCTIONS` instructions, with no handler, no generator or async body, no call, and every
/// jump forward. Pure over the body.
pub fn lane_callee(program: &ResidualProgram, callee: u32) -> bool {
    let Some(body) = program.function_get(callee as usize) else { return false };
    body.handlers.is_empty() && !body.is_generator && !body.is_async && body.code.len() <= LANE_CALLEE_MAX_INSTRUCTIONS
        && body.instructions().enumerate().all(|(pc, instruction)| !calls(instruction.op())
            && (instruction.op().immediate_role() != ImmediateRole::JumpTarget || instruction.imm() as usize > pc))
}

/// Whether a call keeps its result in its register or returns it: the two a unit inlines (`RETURN_REGISTER`, or none).
pub fn returns_or_keeps(instruction: WideInstruction) -> bool {
    matches!(instruction.result_flags(), 0 | RETURN_REGISTER)
}

/// Whether `body` ends in a `Return` of its own result, the canonical completion a terminal call may use without
/// replaying the call (`terminal_return`).
fn ends_in_return(body: &Function) -> bool {
    body.instructions().last().is_some_and(|instruction| {
        matches!(Operation::<u64, ()>::of(instruction), Some(Operation::Return { .. })) && instruction.result_flags() == 0
    })
}

/// The decision for each named call site of `host`, in site order: one `(edge index, inlined)` per entry of
/// `graph.call_edges(host)`, where `inlined` says the call of that edge's callee is inlined at its site. A site is
/// considered only where it is a plain call the frame may inline (not a construction, not an eval, not a strict or
/// non-terminal return) and every callee it names passes the rule `inlines`. Of those, a callee inlines where the lane's
/// scope admits it (`lane_callee`), it is not in the host's own call cycle, and it is not the host (root exclusion). Each
/// callee charged to the growth cap (`charges_growth`) takes its size from the host's budget (`INLINE_GROWTH_PERCENT` of
/// the host's instructions) in site order, and a callee that does not fit keeps its call; so does one whose arguments
/// outnumber its registers. Pure: the lowering's own refusals (engine side) are applied by the planner after this.
pub fn inline_plan(graph: &CallGraph, program: &ResidualProgram, host: u32) -> Vec<(usize, bool)> {
    let edges = graph.call_edges(host);
    let mut inlined = vec![false; edges.len()];
    if let Some(body) = program.function_get(host as usize) {
        let mut budget = body.instructions().count() * INLINE_GROWTH_PERCENT / PERCENT as usize;
        let mut sites: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, edge) in edges.iter().enumerate() {
            sites.entry(edge.pc).or_default().push(index);
        }
        for (pc, instruction) in body.instructions().enumerate() {
            let Some(indexes) = sites.get(&(pc as u32)) else { continue };
            if !Operation::<u64, ()>::is_plain_call(instruction) || !returns_or_keeps(instruction)
                || instruction.direct_eval() || instruction.parameter_eval() {
                continue;
            }
            if instruction.returns_from_frame() && (body.strict || !ends_in_return(body)) {
                continue;
            }
            if !indexes.iter().all(|&index| inlines(graph, program, edges[index].callee)) {
                continue;
            }
            let argc = ImmediateLayout::argument_count(instruction.imm());
            for &index in indexes {
                let callee = edges[index].callee;
                if callee == host || !lane_callee(program, callee) || graph.cycle_of(host).contains(&callee) {
                    continue;
                }
                let Some(callee_body) = program.function_get(callee as usize) else { continue };
                if argc > callee_body.registers {
                    continue;
                }
                let size = callee_body.instructions().count();
                if charges_growth(graph, callee) {
                    if size > budget {
                        continue;
                    }
                    budget -= size;
                }
                inlined[index] = true;
            }
        }
    }
    inlined.into_iter().enumerate().collect()
}
