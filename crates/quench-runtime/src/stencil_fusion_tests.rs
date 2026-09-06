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
) -> (Completion, u64, u64) {
    let native = plan.native_local_binary_at(pc).expect("local binary plan");
    let selection = native.borrow().selection();
    let environment = crate::environment::Environment::new();
    match selection.inputs {
        crate::stencil_plan::LocalNumericInputs::Sources(sources) => {
            let mut value_index = 0;
            let mut assigned = [None; 2];
            for source in sources {
                let crate::stencil_plan::NumericSource::Local(slot) = source else {
                    continue;
                };
                if assigned.contains(&Some(slot)) {
                    continue;
                }
                environment.set(slot, values[value_index].clone());
                assigned[value_index] = Some(slot);
                value_index += 1;
            }
        }
        crate::stencil_plan::LocalNumericInputs::SlotConstant { slot, .. } => {
            environment.set(slot, values[0].clone());
        }
        crate::stencil_plan::LocalNumericInputs::Folded { .. } => {}
    }
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    let stale_roots: Vec<_> = selection
        .discarded
        .into_iter()
        .flatten()
        .map(|register| {
            let owner = std::rc::Rc::new(crate::value::ObjectData::new(vec![]));
            let weak = std::rc::Rc::downgrade(&owner);
            registers.write(usize::from(register), Value::Object(owner));
            weak
        })
        .collect();
    let (completion, _) = crate::vm::execute_baseline_code_from(
        view,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("local binary execution");
    assert!(stale_roots.iter().all(|root| root.upgrade().is_none()));
    let native = native.borrow();
    (
        completion,
        native.native_entry_count(),
        native.local_read_count(),
    )
}

fn exercise_constant_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| {
        plan.native_local_binary_at(*pc).is_some_and(|native| {
            matches!(
                native.borrow().selection().inputs,
                crate::stencil_plan::LocalNumericInputs::SlotConstant { .. }
            )
        })
    }) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(1.25), Value::Undefined]);
    assert_eq!(numeric, (Completion::Return(Value::Number(3.75)), 1, 1));
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Undefined],
    );
    assert_eq!(
        hostile,
        (Completion::Return(Value::String("x2.5".into())), 1, 2)
    );
    true
}

fn exercise_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| plan.native_local_binary_at(*pc).is_some()) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(1.25), Value::Number(2.5)]);
    assert_eq!(numeric, (Completion::Return(Value::Number(3.75)), 1, 2));
    let storage = plan.native_storage_for_test();
    let warm = execute_case(view, &plan, pc, [Value::Number(-4.0), Value::Number(1.5)]);
    assert_eq!(warm, (Completion::Return(Value::Number(-2.5)), 2, 4));
    assert_eq!(plan.native_storage_for_test(), storage);
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Number(2.0)],
    );
    assert_eq!(
        hostile,
        (Completion::Return(Value::String("x2".into())), 2, 5)
    );
    true
}

fn exercise_repeated_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| {
        plan.native_local_binary_at(*pc).is_some_and(|native| {
            matches!(
                native.borrow().selection().inputs,
                crate::stencil_plan::LocalNumericInputs::Sources([
                    crate::stencil_plan::NumericSource::Local(first),
                    crate::stencil_plan::NumericSource::Local(second),
                ]) if first == second
            )
        })
    }) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(2.25), Value::Undefined]);
    assert_eq!(numeric, (Completion::Return(Value::Number(4.5)), 1, 1));
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Undefined],
    );
    assert_eq!(
        hostile,
        (Completion::Return(Value::String("xx".into())), 1, 2)
    );
    true
}

fn exercise_constant_left_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| {
        plan.native_local_binary_at(*pc).is_some_and(|native| {
            matches!(
                native.borrow().selection().inputs,
                crate::stencil_plan::LocalNumericInputs::Sources([
                    crate::stencil_plan::NumericSource::Constant(_),
                    crate::stencil_plan::NumericSource::Local(_),
                ])
            )
        })
    }) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(1.25), Value::Undefined]);
    assert_eq!(numeric, (Completion::Return(Value::Number(1.25)), 1, 1));
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Undefined],
    );
    assert!(matches!(hostile.0, Completion::Return(Value::Number(value)) if value.is_nan()));
    assert_eq!((hostile.1, hostile.2), (1, 2));
    true
}

fn exercise_constant_left_add_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = constant_local_admission(&plan, view, crate::ir::Opcode::Add) else {
        return false;
    };
    let numeric = execute_case(view, &plan, pc, [Value::Number(-0.0), Value::Undefined]);
    assert_eq!(numeric, (Completion::Return(Value::Number(2.5)), 1, 1));
    let hostile = execute_case(
        view,
        &plan,
        pc,
        [Value::String("x".into()), Value::Undefined],
    );
    assert_eq!(
        hostile,
        (Completion::Return(Value::String("2.5x".into())), 1, 2)
    );
    true
}

fn exercise_folded_source_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some(pc) = (0..view.len()).find(|pc| {
        plan.native_local_binary_at(*pc).is_some_and(|native| {
            matches!(
                native.borrow().selection().inputs,
                crate::stencil_plan::LocalNumericInputs::Folded { .. }
            )
        })
    }) else {
        return false;
    };
    let result = execute_case(view, &plan, pc, [Value::Undefined, Value::Undefined]);
    assert_eq!(result, (Completion::Return(Value::Number(4.0)), 0, 0));
    assert_eq!(plan.native_storage_for_test(), (0, 0, 0));
    true
}

fn constant_local_admission(
    plan: &BaselinePlan,
    view: CodeView<'_>,
    opcode: crate::ir::Opcode,
) -> Option<usize> {
    (0..view.len()).find(|pc| {
        plan.native_local_binary_at(*pc).is_some_and(|native| {
            let selection = native.borrow().selection();
            selection.operation.opcode == opcode
                && matches!(
                    selection.inputs,
                    crate::stencil_plan::LocalNumericInputs::Sources([
                        crate::stencil_plan::NumericSource::Constant(_),
                        crate::stencil_plan::NumericSource::Local(_),
                    ])
                )
        })
    })
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
fn ordinary_source_reuses_repeated_local_value() {
    let program = crate::reduce::reduce_source("function f(x){return x+x} f(1)")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_repeated_source_view(view);
    });
    assert!(executed, "source must execute the repeated-slot entry");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_fuses_local_load_with_numeric_constant() {
    let program = crate::reduce::reduce_source("function f(x){return x+2.5} f(1)")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_constant_source_view(view);
    });
    assert!(executed, "source must execute the constant physical entry");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_fuses_constant_left_add_without_changing_coercion_order() {
    let program = crate::reduce::reduce_source("function f(x){return 2.5+x} f(1)")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_constant_left_add_view(view);
    });
    assert!(executed, "source must execute the constant-left Add entry");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_constant_fold_skips_render_and_native_entry() {
    let program = crate::reduce::reduce_source("function f(){return 2.5+1.5} f()")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_folded_source_view(view);
    });
    assert!(
        executed,
        "source must execute the folded physical selection"
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn canonical_driver_eliminates_dead_move_in_numeric_window() {
    let function = crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::LoadLocal { dst: 1, slot: 0 },
        crate::ops::Op::Move { dst: 2, src: 1 },
        crate::ops::Op::LoadLocal { dst: 3, slot: 1 },
        crate::ops::Op::Binary {
            dst: 4,
            operator: crate::ops::BinaryOp::Multiply,
            lhs: 2,
            rhs: 3,
        },
        crate::ops::Op::Return { src: 4 },
    ]);
    let view = function.code().unwrap();
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let result = execute_case(view, &plan, 0, [Value::Number(6.0), Value::Number(7.0)]);
    assert_eq!(result, (Completion::Return(Value::Number(42.0)), 1, 2));
    assert_eq!(
        plan.native_local_binary_at(0)
            .unwrap()
            .borrow()
            .selection()
            .span,
        4
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn canonical_driver_eliminates_bounded_dead_pure_producers() {
    let function = crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::LoadLocal { dst: 1, slot: 0 },
        crate::ops::Op::LoadLocal { dst: 2, slot: 1 },
        crate::ops::Op::LoadLocal { dst: 3, slot: 2 },
        crate::ops::Op::LoadLocal { dst: 4, slot: 3 },
        crate::ops::Op::LoadLocal { dst: 5, slot: 4 },
        crate::ops::Op::Binary {
            dst: 6,
            operator: crate::ops::BinaryOp::Multiply,
            lhs: 2,
            rhs: 5,
        },
        crate::ops::Op::Return { src: 6 },
    ]);
    let view = function.code().unwrap();
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let result = execute_case(view, &plan, 0, [Value::Number(6.0), Value::Number(7.0)]);
    assert_eq!(result, (Completion::Return(Value::Number(42.0)), 1, 2));
    assert_eq!(
        plan.native_local_binary_at(0)
            .unwrap()
            .borrow()
            .selection()
            .span,
        6
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_propagates_left_constant_without_swapping_operands() {
    let program = crate::reduce::reduce_source("function f(x){return 2.5-x} f(1)")
        .expect("ordinary source lowers");
    let mut executed = false;
    visit_views(program.code(), &mut |view| {
        executed |= exercise_constant_left_source_view(view);
    });
    assert!(
        executed,
        "source must execute the constant-left physical entry"
    );
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
