use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::Value;

struct BranchResult {
    completion: Completion,
    comparison: Value,
    compare_entries: u64,
    truthiness_entries: u64,
    generated: bool,
}

fn execute_compare_branch(view: CodeView<'_>, lhs: Value, rhs: Value) -> Option<BranchResult> {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let pc = (0..view.len()).find(|pc| has_compare_branch(&plan, *pc))?;
    let instruction = view.instruction(pc)?;
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.frame_register_count()).max(8),
    );
    registers.write(usize::from(instruction.b), lhs);
    registers.write(usize::from(instruction.c), rhs);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        view,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .ok()?;
    let compare = plan.native_binary_at(pc)?;
    let branch_pc = pc + compare.borrow().compare_branch_span()?.checked_sub(1)?;
    let truthiness = plan.native_truthiness_at(branch_pc)?;
    let compare_entries = compare.borrow().native_entry_count();
    let truthiness_entries = truthiness.borrow().native_entry_count();
    let generated = compare
        .borrow()
        .last_native_view()
        .is_some_and(|view| view.generated);
    Some(BranchResult {
        completion,
        comparison: registers.read(usize::from(instruction.a))?,
        compare_entries,
        truthiness_entries,
        generated,
    })
}

fn has_compare_branch(plan: &BaselinePlan, pc: usize) -> bool {
    plan.native_binary_at(pc)
        .is_some_and(|native| native.borrow().has_compare_branch())
}

fn baseline_entries(instructions: &[crate::ir::Instruction]) -> Vec<crate::machine::BaselineEntry> {
    instructions
        .iter()
        .copied()
        .map(|instruction| crate::machine::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        })
        .collect()
}

#[test]
fn compare_branch_rejects_live_effectful_and_side_entry_interiors() {
    let compare = crate::ir::Instruction::binary_operator(
        2,
        crate::ops::BinaryOp::LessThan,
        0,
        1,
    );
    let dead = crate::ir::Instruction::load_const(3, 0);
    let branch = crate::ir::Instruction::jump_if_false(2, 3);
    let base = baseline_entries(&[compare, dead, branch, crate::ir::Instruction::ret(2)]);
    let mut liveness = vec![std::collections::BTreeSet::new(); base.len()];
    assert!(crate::machine::compare_branch(&base, &liveness, 0, compare).is_some());
    liveness[1].insert(3);
    assert!(crate::machine::compare_branch(&base, &liveness, 0, compare).is_none());

    liveness[1].clear();
    let effectful = crate::ir::Instruction::binary(crate::ir::Opcode::AGetI, 3, 4, 5);
    let entries = baseline_entries(&[compare, effectful, branch, crate::ir::Instruction::ret(2)]);
    assert!(crate::machine::compare_branch(&entries, &liveness, 0, compare).is_none());
    let side_entry = baseline_entries(&[
        compare,
        dead,
        branch,
        crate::ir::Instruction::jump(1),
    ]);
    assert!(crate::machine::compare_branch(&side_entry, &liveness, 0, compare).is_none());
}

#[test]
fn ordinary_source_compare_branch_fuses_dispatch_and_preserves_live_boolean() {
    let source = "function f(a,b){if(a<b)return 11;return 22} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let Some(taken) = execute_compare_branch(view, Value::Number(1.0), Value::Number(2.0))
        else {
            return;
        };
        assert_branch(taken, 11.0, true, 1, 0);
        let fallthrough = execute_compare_branch(view, Value::Number(3.0), Value::Number(2.0))
            .expect("same admitted comparison");
        assert_branch(fallthrough, 22.0, false, 1, 0);
        let nan = execute_compare_branch(view, Value::Number(f64::NAN), Value::Number(2.0))
            .expect("NaN comparison remains ordered");
        assert_branch(nan, 22.0, false, 1, 0);
        let signed_zero = execute_compare_branch(view, Value::Number(-0.0), Value::Number(0.0))
            .expect("signed-zero comparison remains ordered");
        assert_branch(signed_zero, 22.0, false, 1, 0);
        let guarded =
            execute_compare_branch(view, Value::String("z".into()), Value::String("a".into()))
                .expect("guarded ordinary fallback");
        assert_branch(guarded, 22.0, false, 0, 1);
        checked = true;
    });
    assert!(checked, "ordinary lowering must admit compare then branch");
}

#[test]
fn ordinary_source_identity_branch_uses_numeric_and_tagged_bodies() {
    let source = "function f(a,b){if(a===b)return 11;return 22} f(1,1)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let Some(numeric) = execute_compare_branch(view, Value::Number(2.0), Value::Number(2.0))
        else {
            return;
        };
        assert_branch(numeric, 11.0, true, 1, 0);
        let tagged = execute_compare_branch(view, Value::Boolean(true), Value::Boolean(true))
            .expect("tagged identity body");
        assert_branch(tagged, 11.0, true, 1, 0);
        let string = execute_compare_branch(
            view,
            Value::String("same".into()),
            Value::String("same".into()),
        )
        .expect("string identity fallback");
        assert_branch(string, 11.0, true, 0, 1);
        checked = true;
    });
    assert!(checked, "strict equality must admit compare then branch");
}

#[test]
fn warm_compare_branch_reuses_code_and_removes_truthiness_dispatch() {
    let source = "function f(a,b){if(a<b)return 11;return 22} f(1,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut checked = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
        let plan = BaselinePlan::compile_for_test(view, policy);
        let Some(pc) = (0..view.len()).find(|pc| has_compare_branch(&plan, *pc)) else {
            return;
        };
        let instruction = view.instruction(pc).expect("comparison instruction");
        let mut registers = crate::register_file::RegisterFile::with_undefined(8);
        run_warm_comparisons(view, &plan, pc, instruction, &mut registers);
        checked = true;
    });
    assert!(checked, "ordinary lowering must expose comparison fusion");
}

fn run_warm_comparisons(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
) {
    run_comparison_once(view, plan, pc, instruction, registers);
    let storage = plan.native_storage_for_test();
    for _ in 1..32 {
        run_comparison_once(view, plan, pc, instruction, registers);
        assert_eq!(plan.native_storage_for_test(), storage);
    }
    assert_eq!(plan.native_binary_at(pc).unwrap().borrow().native_entry_count(), 32);
    let branch_pc = pc + plan.native_binary_at(pc).unwrap().borrow().compare_branch_span().unwrap() - 1;
    assert_eq!(plan.native_truthiness_at(branch_pc).unwrap().borrow().native_entry_count(), 0);
}

fn run_comparison_once(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    instruction: crate::ir::Instruction,
    registers: &mut crate::register_file::RegisterFile,
) {
    registers.write(usize::from(instruction.b), Value::Number(1.0));
    registers.write(usize::from(instruction.c), Value::Number(2.0));
    let result = crate::vm::execute_baseline_code_from(
        view,
        plan,
        pc,
        registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    );
    assert_eq!(result.unwrap().0, Completion::Return(Value::Number(11.0)));
}

fn assert_branch(result: BranchResult, returned: f64, comparison: bool, native: u64, truthy: u64) {
    assert_eq!(
        result.completion,
        Completion::Return(Value::Number(returned))
    );
    assert_eq!(result.comparison, Value::Boolean(comparison));
    assert_eq!(result.compare_entries, native);
    assert_eq!(result.truthiness_entries, truthy);
    assert_eq!(result.generated, native != 0 && cfg!(quench_generated_stencil_artifacts));
}
