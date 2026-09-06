use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::{ObjectData, Value};
use std::rc::Rc;

fn stored_property_selection(
    view: CodeView<'_>,
    plan: &BaselinePlan,
) -> Option<(usize, crate::stencil_plan::LocalPropertySelection)> {
    (0..view.len()).find_map(|pc| {
        let selection = plan.native_local_property_at(pc)?.borrow().selection();
        selection.result.store_slot.map(|_| (pc, selection))
    })
}

fn run_stored_property(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    environment: Rc<crate::environment::Environment>,
) -> Completion {
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    crate::vm::execute_baseline_code_from(
        view,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("stored property execution")
    .0
}

fn exercise_stored_property(view: CodeView<'_>) -> bool {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(view, policy);
    let Some((pc, selection)) = stored_property_selection(view, &plan) else {
        return false;
    };
    let environment = crate::environment::Environment::new();
    let receiver = Rc::new(ObjectData::new(vec![("value".into(), Value::Number(11.0))]));
    environment.set(selection.receiver_slot, Value::Object(receiver));
    let store_slot = selection.result.store_slot.expect("selected store");
    environment.set(store_slot, Value::Undefined);
    assert_property_store_runs(view, &plan, pc, store_slot, environment);
    true
}

fn assert_property_store_runs(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    slot: u16,
    environment: Rc<crate::environment::Environment>,
) {
    let store_pc =
        pc + usize::from(
            plan.native_local_property_at(pc)
                .unwrap()
                .borrow()
                .selection()
                .span,
        ) - 1;
    let store = plan
        .native_store_local_at(store_pc)
        .expect("store leaf plan");
    prime_property_cache(view, plan, pc, &environment);
    let store_entries = store.borrow().native_entry_count();
    let fused = plan.native_local_property_at(pc).unwrap();
    assert_eq!(
        run_stored_property(view, plan, pc, Rc::clone(&environment)),
        Completion::Return(Value::Number(11.0))
    );
    assert_eq!(fused.borrow().native_entry_count(), 1);
    assert_eq!(environment.get(slot), Value::Number(11.0));
    assert_eq!(store.borrow().native_entry_count(), store_entries);
    assert_cell_fallback(view, plan, pc, slot, environment, fused);
}

fn prime_property_cache(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    environment: &Rc<crate::environment::Environment>,
) {
    assert_eq!(
        run_stored_property(view, plan, pc, Rc::clone(&environment)),
        Completion::Return(Value::Number(11.0))
    );
}

fn assert_cell_fallback(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    slot: u16,
    environment: Rc<crate::environment::Environment>,
    fused: &std::cell::RefCell<crate::stencil_fusion::NativeLocalPropertyPlan>,
) {
    let cell = environment.slot_cell(slot);
    let before = fused.borrow().native_entry_count();
    assert_eq!(
        run_stored_property(view, plan, pc, environment),
        Completion::Return(Value::Number(11.0))
    );
    assert_eq!(cell.load(), Value::Number(11.0));
    assert_eq!(fused.borrow().native_entry_count(), before);
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_fuses_guarded_property_result_into_local_store() {
    let source = "function read(o){var y=o.value;return y}";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut executed = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        executed |= exercise_stored_property(view);
    });
    assert!(
        executed,
        "ordinary lowering must select property plus store"
    );
}
