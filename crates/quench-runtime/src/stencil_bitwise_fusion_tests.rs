use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView, FunctionCode};
use crate::ops::{BinaryOp, Op};
use crate::stencil_plan::{LocalNumericInputs, NumericSource};
use crate::value::Value;

fn bitwise_function(operator: BinaryOp) -> FunctionCode {
    FunctionCode::from_ops(vec![
        Op::LoadLocal { dst: 1, slot: 5 },
        Op::LoadLocal { dst: 2, slot: 6 },
        Op::Binary {
            dst: 3,
            operator,
            lhs: 1,
            rhs: 2,
        },
        Op::StoreLocal { slot: 7, src: 3 },
        Op::LoadLocal { dst: 4, slot: 7 },
        Op::Return { src: 4 },
    ])
}

fn install_sources(
    environment: &crate::environment::Environment,
    inputs: LocalNumericInputs,
    values: [Value; 2],
) {
    let LocalNumericInputs::Sources(sources) = inputs else {
        panic!("bitwise region must retain its two sources");
    };
    for (index, source) in sources.into_iter().enumerate() {
        let NumericSource::Local(slot) = source else {
            panic!("bitwise test expects local sources");
        };
        environment.set(slot, values[index].clone());
    }
}

fn execute_case(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    values: [Value; 2],
) -> (Completion, u64, u64) {
    let native = plan.native_local_binary_at(0).expect("fused bitwise plan");
    let selection = native.borrow().selection();
    let environment = crate::environment::Environment::new();
    install_sources(&environment, selection.inputs, values);
    let store_slot = selection.result.store_slot.expect("fused local store");
    environment.set(store_slot, Value::Undefined);
    let store_pc = usize::from(selection.span) - 1;
    let store = plan.native_store_local_at(store_pc).expect("store leaf");
    let store_before = store.borrow().native_entry_count();
    let mut registers = crate::register_file::RegisterFile::with_undefined(8);
    let (completion, _) = crate::vm::execute_baseline_code_from(
        view,
        plan,
        0,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("bitwise region execution");
    let native_entries = native.borrow().native_entry_count();
    let skipped_stores = store.borrow().native_entry_count() - store_before;
    (completion, native_entries, skipped_stores)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn bitwise_result_regions_preserve_js_number_conversions() {
    let cases = [
        (BinaryOp::BitwiseAnd, f64::NAN, 7.0, 0.0),
        (BinaryOp::BitwiseOr, f64::INFINITY, 5.0, 5.0),
        (BinaryOp::BitwiseXor, 5.9, -2.1, -5.0),
        (BinaryOp::ShiftLeft, 1.9, 33.2, 2.0),
        (BinaryOp::ShiftRight, -8.9, 34.0, -2.0),
        (BinaryOp::ShiftRightZeroFill, -1.0, 0.0, 4_294_967_295.0),
    ];
    for (operator, lhs, rhs, expected) in cases {
        let function = bitwise_function(operator);
        let view = function.code().expect("bitwise code");
        let plan = BaselinePlan::compile_for_test(
            view,
            crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
        );
        let result = execute_case(view, &plan, [Value::Number(lhs), Value::Number(rhs)]);
        assert_eq!(result, (Completion::Return(Value::Number(expected)), 1, 0));
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn broken_numeric_fact_runs_complete_bitwise_fallback() {
    let function = bitwise_function(BinaryOp::ShiftLeft);
    let view = function.code().expect("bitwise code");
    let plan = BaselinePlan::compile_for_test(
        view,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let result = execute_case(
        view,
        &plan,
        [Value::String("3".into()), Value::Number(33.0)],
    );
    assert_eq!(result.0, Completion::Return(Value::Number(6.0)));
    assert_eq!(result.1, 0, "guard miss must not enter native code");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_reaches_fused_unsigned_shift_and_store() {
    let source = "function f(a,b){var y=a>>>b;return y} f(-1,0)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut admitted = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
        let plan = BaselinePlan::compile_for_test(view, policy);
        let Some(native) = plan.native_local_binary_at(0) else {
            return;
        };
        let selection = native.borrow().selection();
        if selection.result.store_slot.is_none()
            || crate::ir::compact_binary_operator(selection.operation.flags)
                != Some(BinaryOp::ShiftRightZeroFill)
        {
            return;
        }
        let result = execute_case(view, &plan, [Value::Number(-1.0), Value::Number(0.0)]);
        assert_eq!(
            result,
            (Completion::Return(Value::Number(4_294_967_295.0)), 1, 0)
        );
        admitted = true;
    });
    assert!(admitted, "ordinary lowering did not reach bitwise fusion");
}
