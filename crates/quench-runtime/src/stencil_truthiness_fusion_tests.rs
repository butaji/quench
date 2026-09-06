use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::Value;
use std::collections::BTreeSet;

fn baseline_entry(instruction: crate::ir::Instruction) -> crate::machine::BaselineEntry {
    crate::machine::BaselineEntry {
        instruction,
        handler: instruction.opcode.handler(),
        control: instruction.opcode.control_operands(instruction),
    }
}

#[test]
fn truthiness_selection_skips_only_dead_values() {
    let instructions = [
        crate::ir::Instruction::load_local(1, 5),
        crate::ir::Instruction::load_const(2, 0),
        crate::ir::Instruction::jump_if_false(1, 4),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::ret(0),
    ];
    let entries = instructions.map(baseline_entry);
    let cfg = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 5]);
    let control = cfg.region_control(0, 3).expect("branch region");
    let discarded = [Some(1), Some(2), None, None, None, None, None, None];
    let select = |live| {
        crate::stencil_plan::select_local_truthiness(
            instructions[0],
            instructions[2],
            live,
            control,
            discarded,
        )
    };
    let dead = BTreeSet::new();
    let live = BTreeSet::from([1]);
    assert!(select(&dead).is_some());
    assert!(select(&live).is_none());
}

fn execute_case(
    view: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    value: Value,
) -> (Completion, u64, u64) {
    let fused = plan
        .native_local_truthiness_at(pc)
        .expect("local truthiness plan");
    let selection = fused.borrow().selection();
    let load = plan.native_load_local_at(pc).expect("load leaf");
    let branch = plan
        .native_truthiness_at(selection.true_pc - 1)
        .expect("truthiness leaf");
    let before = (
        load.borrow().native_entry_count(),
        branch.borrow().native_entry_count(),
    );
    let environment = crate::environment::Environment::new();
    environment.set(selection.source_slot, value);
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(view.register_count()).max(8),
    );
    let stale: Vec<_> = selection
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
    .expect("local truthiness execution");
    assert!(stale.iter().all(|value| value.upgrade().is_none()));
    let skipped = load.borrow().native_entry_count() - before.0
        + branch.borrow().native_entry_count()
        - before.1;
    (completion, fused.borrow().native_entry_count(), skipped)
}

fn with_source_plan(mut test: impl FnMut(CodeView<'_>, &BaselinePlan, usize)) {
    let source = "function f(x){if(x)return 11;return 22} f(1)";
    let program = crate::reduce::reduce_source(source).expect("ordinary source lowers");
    let mut found = false;
    crate::stencil_test_support::visit_code_views(program.code(), &mut |view| {
        let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
        let plan = BaselinePlan::compile_for_test(view, policy);
        if let Some(pc) = (0..view.len()).find(|pc| plan.native_local_truthiness_at(*pc).is_some())
        {
            test(view, &plan, pc);
            found = true;
        }
    });
    assert!(found, "ordinary source did not admit local truthiness");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_fuses_local_load_with_truthiness_branch() {
    with_source_plan(|view, plan, pc| {
        let object = std::rc::Rc::new(crate::value::ObjectData::new(vec![]));
        let cases = [
            (Value::Number(3.0), 11.0),
            (Value::Number(-0.0), 22.0),
            (Value::Number(f64::NAN), 22.0),
            (Value::Boolean(true), 11.0),
            (Value::Boolean(false), 22.0),
            (Value::Null, 22.0),
            (Value::Undefined, 22.0),
            (Value::Object(object), 11.0),
        ];
        for (value, expected) in cases {
            let result = execute_case(view, plan, pc, value);
            assert_eq!(result.0, Completion::Return(Value::Number(expected)));
            assert_eq!(result.2, 0, "fused path must skip both scalar leaves");
        }
        let count = plan
            .native_local_truthiness_at(pc)
            .unwrap()
            .borrow()
            .native_entry_count();
        assert_eq!(count, 8);
    });
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn heap_primitive_breaks_fact_and_runs_complete_branch_fallback() {
    with_source_plan(|view, plan, pc| {
        let native = plan.native_local_truthiness_at(pc).unwrap();
        let before = native.borrow().native_entry_count();
        let result = execute_case(view, plan, pc, Value::String(String::new()));
        assert_eq!(result.0, Completion::Return(Value::Number(22.0)));
        assert_eq!(native.borrow().native_entry_count(), before);
    });
}
