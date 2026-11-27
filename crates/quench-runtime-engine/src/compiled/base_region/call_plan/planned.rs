//! The call plan of each call site of a body (`compile::call_plan::call_plans`), read per site by the
//! stitcher: the callees a chained call reaches (`Callees`), whether a one-arm site inlines, and whether
//! the last arm of a closed site needs no callee compare. The b-jit classifies no site's callees itself;
//! it maps the plan's arms onto its rows.

use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::compile::aot_facts::AotFacts;
use quench_runtime::compile::call_plan::call_plans;

pub(crate) use quench_runtime::compile::call_plan::{CallForm as Lowering, CallPlan as SitePlan};

/// The plan of each call site of a function, by PC: none where the site has no plan.
pub(crate) type SitePlans = Vec<Option<SitePlan>>;

/// Whether the plan inlines its site: one arm whose body the plan's form inlines. A class switch over inlined
/// bodies has no row yet, so a multi-arm site is chained.
pub(crate) fn inlines(plan: &SitePlan) -> bool {
    matches!(plan.form, Lowering::Inline) && plan.arms.len() == 1
}

/// The plans of the call sites of `function`, whose code has `len` instructions. None where the program's facts or
/// their signatures are not settled, so every site keeps only its helper row. A callee is inlinable where `inlinable`
/// admits it (the inlining rule, `tier_policy::inlines`).
pub(crate) fn of(program: &ResidualProgram, facts: Option<&AotFacts>, function: u32, len: usize, inlinable: &dyn Fn(u32) -> bool) -> SitePlans {
    let mut plans: SitePlans = vec![None; len];
    if let Some(facts) = facts && let Some(signatures) = facts.signatures() {
        for (pc, plan) in call_plans(program, facts, signatures, function, inlinable) {
            if let Some(slot) = plans.get_mut(pc as usize) {
                *slot = Some(plan);
            }
        }
    }
    plans
}
