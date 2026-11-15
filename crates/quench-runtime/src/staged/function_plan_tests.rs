use super::*;
use crate::bytecode::ResidualProgram;

fn plans(source: &str) -> (ResidualProgram, Vec<(Vec<WideInstruction>, FunctionPlan)>) {
    let program = crate::Engine::specialize(source, "function-plan.js").unwrap();
    let plans = program
        .compiled_functions()
        .map(|function| {
            let instructions: Vec<WideInstruction> = function.instructions().collect();
            let plan = FunctionPlan::of(&instructions, DecodeContext::of(&program));
            (instructions, plan)
        })
        .collect();
    (program, plans)
}

#[test]
fn every_pc_of_every_function_has_a_plan_operation() {
    let (_, plans) = plans(
        "function* g(n) { for (let i = 0; i < n; i++) yield i; }
         async function a(p) { return await p; }
         class C { #x = 1; m() { return this.#x; } }
         function f(o, k) { const r = /a+/g; return [o[k], { k }, `${k}`, r, new C().m(), ...g(2)]; }",
    );
    for (instructions, plan) in &plans {
        assert_eq!(plan.ops.len(), instructions.len());
    }
}

#[test]
fn a_numeric_loop_is_planned_without_a_miss() {
    let (_, plans) = plans("function sum(a, n) { let s = 0; for (let i = 0; i < n; i++) s = (s + a[i]) | 0; return s; }");
    let (instructions, plan) = &plans[1];
    assert_eq!(plan.misses().map(|pc| instructions[pc].op()).collect::<Vec<_>>(), []);
    assert!(instructions.iter().enumerate().any(|(pc, _)| matches!(plan.at(pc), PlanOp::Staged(_))));
}

#[test]
fn suspension_points_are_suspend_exits() {
    let (_, plans) = plans("function* g() { yield 1; yield* [2]; } async function a(p) { await p; }");
    let suspensions = plans
        .iter()
        .flat_map(|(instructions, plan)| (0..instructions.len()).filter(|pc| matches!(plan.at(*pc), PlanOp::Exit(Exit::Suspend))).map(|pc| instructions[pc].op()))
        .count();
    assert!(suspensions >= 3, "yield, yield* and await suspend: {suspensions}");
}
