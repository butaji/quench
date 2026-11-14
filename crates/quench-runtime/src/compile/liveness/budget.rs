//! The compile budget of the liveness-based finishing passes: move coalescing and register
//! roots.
//!
//! These passes cost about 250 instructions of compiler work per bytecode instruction each,
//! and a function's finishing runs about a dozen of them (a 17K-instruction function took
//! 120M instructions to finish, 7K per bytecode instruction, x86_64 `experiment`,
//! 2026-11-13). Coalescing saves a few percent of a body's dispatches, and a body without a
//! backward jump runs each instruction once per call, so on a body this long it repays its
//! cost only after a few hundred calls: such a body is generated straight-line code, called a
//! handful of times. A body with a loop is not bounded this way: the loop is what repeats.
//! Past the budget the passes decline, which every consumer already reads as "no liveness":
//! the code keeps its moves and its root map is the conservative all-live one. The lowering
//! of locals to registers is not budgeted: it saves a dispatch's work per local access, and
//! skipping it made the four 8K-17K-instruction functions of an Air run 25% dearer to run
//! (warm, 1943M to 2054M instructions).

use crate::bytecode::{Function, ImmediateRole};

/// The instructions a function without a backward jump may have before its optional
/// finishing passes decline.
pub(crate) const STRAIGHT_LINE_LIMIT: usize = 4096;

/// Whether `function` is within the budget: short, or looping.
pub(crate) fn admits(function: &Function) -> bool {
    function.code.len() <= STRAIGHT_LINE_LIMIT || has_backward_jump(function)
}

fn has_backward_jump(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, &packed)| {
        if packed.is_wide() {
            let wide = function.wide[packed.wide_index()];
            wide.op().immediate_role() == ImmediateRole::JumpTarget && wide.jump_target() as usize <= pc
        } else {
            packed.op().immediate_role() == ImmediateRole::JumpTarget && packed.jump_target() as usize <= pc
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{admits, STRAIGHT_LINE_LIMIT};
    use crate::bytecode::{Instr, Op};
    use crate::compile::liveness::RegisterMasks;
    use crate::compile::MethodSiteSpec;
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use crate::{Engine, ResidualProgram};
    use std::{cell::RefCell, rc::Rc};

    /// A program whose root function runs `length` no-ops, optionally one jump, then returns.
    fn program_of(length: usize, jump: Option<(usize, u32)>) -> ResidualProgram {
        let mut program = Engine::specialize_unspecialized("", "budget.js").unwrap();
        let function = &mut program.functions[0];
        let mut code = vec![Instr::new(Op::Nop, 0, 0, 0, 0); length];
        if let Some((pc, target)) = jump {
            code[pc] = Instr::new(Op::Jump, 0, 0, 0, target);
        }
        code.push(Instr::new(Op::Return, 0, 0, 0, 0));
        function.code = code;
        function.registers = 4;
        program
    }

    #[test]
    fn a_long_body_without_a_backward_jump_is_over_budget() {
        assert!(admits(&program_of(STRAIGHT_LINE_LIMIT - 1, None).functions[0]));
        assert!(!admits(&program_of(STRAIGHT_LINE_LIMIT + 1, None).functions[0]));
        assert!(!admits(&program_of(STRAIGHT_LINE_LIMIT + 1, Some((0, 2))).functions[0]), "a forward jump is not a loop");
    }

    #[test]
    fn a_long_looping_body_stays_in_budget() {
        assert!(admits(&program_of(STRAIGHT_LINE_LIMIT + 1, Some((STRAIGHT_LINE_LIMIT, 1))).functions[0]));
    }

    #[test]
    fn over_budget_root_maps_are_conservative_and_transfers_decline() {
        let program = program_of(STRAIGHT_LINE_LIMIT + 1, None);
        let function = &program.functions[0];
        assert!(super::super::transfers(function, &[] as &[MethodSiteSpec], &[], &[]).is_none());
        let masks = RegisterMasks::of(function, &[] as &[MethodSiteSpec], &[], &[]).unwrap();
        assert!(masks.get(0).is_none() && masks.live_at(0, 0).is_none());
    }

    fn run(source: &str) -> Vec<String> {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&Engine::specialize(source, "budget-run.js").unwrap()).unwrap();
        let lines = output.borrow().clone();
        lines
    }

    #[test]
    fn long_functions_run_the_same_over_and_within_budget() {
        // A straight-line body past the budget and the same body in a loop (inside it), each
        // doing calls and copies between temporaries, compute what the plain arithmetic does.
        let rounds = 2000;
        let long: String = (0..rounds).map(|i| format!("t = o.f({i}, a); a = t + {i}; b = a - t; ")).collect();
        let source = format!(
            "const o = {{ f(x, y) {{ return x ^ y; }} }};\
             function straight(a) {{ let t, b; {long} return a + b; }}\
             function looping(a) {{ let t, b; for (let k = 0; k < 2; k++) {{ {long} }} return a + b; }}\
             print(straight(1) + ',' + looping(1));"
        );
        let advance = |a: &mut i64, b: &mut i64| {
            for i in 0..rounds as i64 {
                let t = i ^ *a;
                *a = t + i;
                *b = *a - t;
            }
        };
        let (mut a, mut b) = (1i64, 0);
        advance(&mut a, &mut b);
        let straight = a + b;
        advance(&mut a, &mut b);
        assert_eq!(run(&source), [format!("{straight},{}", a + b)]);
    }
}
