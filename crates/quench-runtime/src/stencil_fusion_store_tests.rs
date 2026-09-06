use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::stencil_plan::{LocalNumericInputs, NumericSource};
use crate::value::Value;

fn install_inputs(
    environment: &crate::environment::Environment,
    inputs: LocalNumericInputs,
    values: [f64; 2],
) {
    let sources = match inputs {
        LocalNumericInputs::Sources(sources) => sources,
        _ => return,
    };
    let mut assigned = Vec::new();
    for source in sources {
        let NumericSource::Local(slot) = source else {
            continue;
        };
        if !assigned.contains(&slot) {
            environment.set(slot, Value::Number(values[assigned.len()]));
            assigned.push(slot);
        }
    }
}

fn exercise_stored_view(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some((pc, selection)) = stored_selection(view, &plan) else {
        return false;
    };
    execute_stored_selection(view, &plan, pc, selection);
    true
}

fn stored_selection(
    view: CodeView<'_>,
    plan: &BaselinePlan,
) -> Option<(usize, crate::stencil_plan::LocalBinarySelection)> {
    (0..view.len()).find_map(|pc| {
        let selection = plan.native_local_binary_at(pc)?.borrow().selection();
        selection.store_slot.map(|_| (pc, selection))
    })
}

fn execute_stored_selection(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    selection: crate::stencil_plan::LocalBinarySelection,
) {
    let environment = crate::environment::Environment::new();
    install_inputs(&environment, selection.inputs, [-0.0, 2.0]);
    let slot = selection.store_slot.expect("selected store");
    environment.set(slot, Value::Undefined);
    let store_pc = pc + usize::from(selection.span) - 1;
    let store = plan
        .native_store_local_at(store_pc)
        .expect("store leaf plan");
    let store_entries = store.borrow().native_entry_count();
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    let result = crate::vm::execute_baseline_code_from(
        view,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment.clone(),
    )
    .expect("fused local store");
    assert_stored_outcome(result.0, &environment, slot);
    assert_eq!(
        plan.native_local_binary_at(pc)
            .unwrap()
            .borrow()
            .native_entry_count(),
        1
    );
    assert_eq!(store_entries, store.borrow().native_entry_count());
}

fn assert_stored_outcome(
    completion: Completion,
    environment: &crate::environment::Environment,
    slot: u16,
) {
    assert!(
        matches!(completion, Completion::Return(Value::Number(value)) if value.to_bits() == (-0.0f64).to_bits())
    );
    assert!(
        matches!(environment.get(slot), Value::Number(value) if value.to_bits() == (-0.0f64).to_bits())
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_fuses_numeric_result_into_following_local_store() {
    let source = "function f(x,z){var y=x*z;return y} f(-0,2)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut executed = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        executed |= exercise_stored_view(view);
    });
    assert!(
        executed,
        "ordinary lowering must select the store-extended span"
    );
}

fn stored_binary_function() -> crate::machine::FunctionCode {
    crate::machine::FunctionCode::from_ops(vec![
        crate::ops::Op::LoadLocal { dst: 1, slot: 5 },
        crate::ops::Op::LoadLocal { dst: 2, slot: 6 },
        crate::ops::Op::Binary {
            dst: 3,
            operator: crate::ops::BinaryOp::Add,
            lhs: 1,
            rhs: 2,
        },
        crate::ops::Op::StoreLocal { slot: 5, src: 3 },
        crate::ops::Op::LoadLocal { dst: 4, slot: 5 },
        crate::ops::Op::Return { src: 4 },
    ])
}

fn execute_stored_binary(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    environment: std::rc::Rc<crate::environment::Environment>,
) -> (Completion, std::rc::Rc<crate::environment::Environment>) {
    let mut registers = crate::register_file::RegisterFile::with_undefined(8);
    let result = crate::vm::execute_baseline_code_from(
        view,
        plan,
        0,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment.clone(),
    )
    .expect("stored binary execution");
    (result.0, environment)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn fused_store_preserves_input_alias_order_and_skips_store_leaf() {
    let function = stored_binary_function();
    let view = function.code().expect("compact code");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let environment = crate::environment::Environment::new();
    environment.set(5, Value::Number(1.0));
    environment.set(6, Value::Number(2.0));
    let (completion, environment) = execute_stored_binary(view, &plan, environment);
    assert_eq!(completion, Completion::Return(Value::Number(3.0)));
    assert_eq!(environment.get(5), Value::Number(3.0));
    assert_eq!(
        plan.native_local_binary_at(0)
            .unwrap()
            .borrow()
            .native_entry_count(),
        1
    );
    assert_eq!(
        plan.native_store_local_at(3)
            .unwrap()
            .borrow()
            .native_entry_count(),
        0
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn cell_backed_store_rejects_fusion_before_entry_and_runs_once() {
    let function = stored_binary_function();
    let view = function.code().expect("compact code");
    let plan = BaselinePlan::compile_for_test(
        view,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let native = plan.native_local_binary_at(0).expect("stored binary plan");
    let environment = crate::environment::Environment::new();
    environment.set(5, Value::Number(1.0));
    environment.set(6, Value::Number(2.0));
    let cell = environment.slot_cell(5);
    let (completion, _) = execute_stored_binary(view, &plan, environment);
    assert_eq!(completion, Completion::Return(Value::Number(3.0)));
    assert_eq!(cell.load(), Value::Number(3.0));
    assert_eq!(native.borrow().native_entry_count(), 0);
    assert_eq!(
        plan.native_store_local_at(3)
            .unwrap()
            .borrow()
            .native_entry_count(),
        0
    );
}
