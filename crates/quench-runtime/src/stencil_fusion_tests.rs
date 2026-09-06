use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::Value;

fn visit_views(view: CodeView<'_>, visit: &mut impl FnMut(CodeView<'_>)) {
    visit(view);
    view.cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(nested) = body.code() {
                visit_views(nested, visit);
            }
        });
    });
}

fn execute_case(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    values: [Value; 2],
) -> (Completion, u64) {
    let native = plan.native_local_binary_at(pc).expect("local binary plan");
    let selection = native.borrow().selection();
    let environment = crate::environment::Environment::new();
    environment.set(selection.slots[0], values[0].clone());
    environment.set(selection.slots[1], values[1].clone());
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    let (completion, _) = crate::vm::execute_baseline_code_from(
        view,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("local binary execution");
    (completion, native.borrow().native_entry_count())
}

fn exercise_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| plan.native_local_binary_at(*pc).is_some()) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(1.25), Value::Number(2.5)]);
    assert_eq!(numeric, (Completion::Return(Value::Number(3.75)), 1));
    let storage = plan.native_storage_for_test();
    let warm = execute_case(view, &plan, pc, [Value::Number(-4.0), Value::Number(1.5)]);
    assert_eq!(warm, (Completion::Return(Value::Number(-2.5)), 2));
    assert_eq!(plan.native_storage_for_test(), storage);
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Number(2.0)],
    );
    assert_eq!(hostile, (Completion::Return(Value::String("x2".into())), 2));
    true
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_fuses_two_local_loads_with_numeric_operation() {
    let program = crate::reduce::reduce_source("function f(x,z){var y=x; return y+z} f(1,2)")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_source_view(view);
    });
    assert!(executed, "source must execute the fused physical entry");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn optimizing_driver_reuses_the_same_local_binary_plan() {
    let function = crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::LoadLocal { dst: 1, slot: 0 },
        crate::ops::Op::LoadLocal { dst: 2, slot: 1 },
        crate::ops::Op::Binary {
            dst: 3,
            operator: crate::ops::BinaryOp::Multiply,
            lhs: 1,
            rhs: 2,
        },
        crate::ops::Op::Return { src: 3 },
    ]);
    let code = function.code().unwrap();
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let baseline = BaselinePlan::compile_for_test(code, policy);
    let optimizing = crate::machine::OptimizingPlan::compile_for_test(&baseline, policy);
    let environment = crate::environment::Environment::new();
    environment.set(0, Value::Number(6.0));
    environment.set(1, Value::Number(7.0));
    let _guard = crate::locals::EnvironmentGuard::install(environment);
    let mut registers = crate::register_file::RegisterFile::with_undefined(4);
    let result = crate::vm::execute_optimized_code_step_from(
        code,
        &optimizing,
        &baseline,
        0,
        &mut registers,
        &crate::vm::current_context_or_default(),
    )
    .unwrap();
    assert_eq!(result, (Completion::Normal, 3));
    assert_eq!(registers.read(3), Some(Value::Number(42.0)));
    let entry = optimizing.entry(0).unwrap();
    let native = entry.native_local_binary().unwrap();
    assert_eq!(native.borrow().native_entry_count(), 1);
}
