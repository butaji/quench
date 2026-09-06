use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView, FunctionCode};
use crate::value::{ObjectData, Value};
use std::rc::Rc;

struct PrototypeChain {
    owner: Rc<ObjectData>,
    middle: Rc<ObjectData>,
    receiver: Rc<ObjectData>,
}

fn prototype_chain(value: f64) -> PrototypeChain {
    let owner = Rc::new(ObjectData::new(vec![(
        "value".into(),
        Value::Number(value),
    )]));
    let middle = Rc::new(ObjectData::new(vec![(
        "\0prototype".into(),
        Value::Object(Rc::clone(&owner)),
    )]));
    let receiver = Rc::new(ObjectData::new(vec![(
        "\0prototype".into(),
        Value::Object(Rc::clone(&middle)),
    )]));
    PrototypeChain {
        owner,
        middle,
        receiver,
    }
}

fn source_named_get_body(root: CodeView<'_>) -> FunctionCode {
    let mut pending = Vec::new();
    collect_nested_bodies(root, &mut pending);
    while let Some(body) = pending.pop() {
        let is_match = body.code().is_some_and(|code| named_get_pc(code).is_some());
        if is_match {
            return body;
        }
        if let Some(code) = body.code() {
            collect_nested_bodies(code, &mut pending);
        }
    }
    panic!("ordinary source function must contain GetN")
}

fn collect_nested_bodies(view: CodeView<'_>, output: &mut Vec<FunctionCode>) {
    view.cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| output.push(body.clone()));
    });
}

fn named_get_pc(code: CodeView<'_>) -> Option<usize> {
    (0..code.len()).find(|pc| {
        code.instruction(*pc)
            .is_some_and(|instruction| instruction.opcode == crate::ir::Opcode::GetN)
    })
}

fn run_get(code: CodeView<'_>, plan: &BaselinePlan, pc: usize, receiver: &Rc<ObjectData>) -> Value {
    let instruction = code.instruction(pc).expect("GetN instruction");
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(8),
    );
    registers.write(
        usize::from(instruction.b),
        Value::Object(Rc::clone(receiver)),
    );
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .expect("named get execution");
    let Completion::Return(value) = completion else {
        panic!("named get must return")
    };
    value
}

fn native_count(plan: &BaselinePlan, pc: usize) -> u64 {
    plan.native_property_at(pc)
        .map(|native| native.borrow().native_entry_count())
        .unwrap_or(0)
}

fn replace_prototype(container: &ObjectData, replacement: &Rc<ObjectData>) {
    let slot = container
        .hot_properties()
        .position_rev("\0prototype")
        .and_then(|slot| container.hot_properties().slot_word(slot))
        .expect("prototype slot");
    slot.store_object_or_null(Some(replacement));
}

#[cfg(quench_generated_stencil_artifacts)]
fn assert_generated_prototype_entry(plan: &BaselinePlan, pc: usize) {
    let expected = crate::stencil_select::select_physical(
        crate::stencil_select::prototype_property_region_key(),
    )
    .expect("generated prototype property view");
    let witness = plan
        .native_property_at(pc)
        .and_then(|native| native.borrow().last_native_view())
        .expect("invoked prototype property view");
    assert!(expected.generated && witness.generated && witness.matches(&expected));
}

#[cfg(not(quench_generated_stencil_artifacts))]
fn assert_generated_prototype_entry(_plan: &BaselinePlan, _pc: usize) {}

fn source_plan() -> (FunctionCode, BaselinePlan, usize) {
    let program = crate::reduce::reduce_source("function read(o){return o.value}")
        .expect("ordinary prototype get lowers");
    let body = source_named_get_body(program.code());
    let code = body.code().expect("linked source function");
    let pc = named_get_pc(code).expect("GetN instruction");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(code, policy);
    (body, plan, pc)
}

fn run_local_property(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    receiver: Value,
) -> Value {
    let native = plan
        .native_local_property_at(pc)
        .expect("local property plan");
    let slot = native.borrow().selection().receiver_slot;
    let environment = crate::environment::Environment::new();
    environment.set(slot, receiver);
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(8),
    );
    let (completion, _) = crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("local property execution");
    let Completion::Return(value) = completion else {
        panic!("local property function must return")
    };
    value
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_fuses_local_load_with_guarded_property_get() {
    let (body, plan, get_pc) = source_plan();
    let code = body.code().unwrap();
    let pc = (0..get_pc)
        .find(|pc| plan.native_local_property_at(*pc).is_some())
        .expect("source producer window is admitted");
    let chain = prototype_chain(11.0);
    let receiver = || Value::Object(Rc::clone(&chain.receiver));
    assert_eq!(
        run_local_property(code, &plan, pc, receiver()),
        Value::Number(11.0)
    );
    let load_entries = plan
        .native_load_local_at(pc)
        .map(|native| native.borrow().native_entry_count())
        .unwrap_or(0);
    assert_eq!(
        run_local_property(code, &plan, pc, receiver()),
        Value::Number(11.0)
    );
    let native = plan.native_local_property_at(pc).unwrap().borrow();
    assert_eq!(native.native_entry_count(), 1);
    assert_eq!(native.local_read_count(), 2);
    drop(native);
    assert_eq!(
        plan.native_load_local_at(pc)
            .map(|native| native.borrow().native_entry_count())
            .unwrap_or(0),
        load_entries,
        "warm fusion must skip the scalar local-load entry"
    );
    assert_eq!(
        run_local_property(code, &plan, pc, Value::String("x".into())),
        Value::Undefined
    );
    assert_eq!(
        plan.native_local_property_at(pc)
            .unwrap()
            .borrow()
            .native_entry_count(),
        1
    );
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_prototype_get_executes_native_and_invalidates_chain() {
    let (body, plan, pc) = source_plan();
    let code = body.code().unwrap();
    let chain = prototype_chain(11.0);
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    assert_eq!(native_count(&plan, pc), 0, "cold lookup installs the IC");
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    assert_eq!(native_count(&plan, pc), 1);
    assert_generated_prototype_entry(&plan, pc);

    let replacement = Rc::new(ObjectData::new(vec![("value".into(), Value::Number(13.0))]));
    replace_prototype(&chain.middle, &replacement);
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(13.0)
    );
    assert_eq!(
        native_count(&plan, pc),
        2,
        "native guard observes chain mutation"
    );
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_prototype_shadow_exits_before_native_entry() {
    let (body, plan, pc) = source_plan();
    let code = body.code().unwrap();
    let chain = prototype_chain(11.0);
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    let before = native_count(&plan, pc);
    let receiver = Value::Object(Rc::clone(&chain.receiver));
    assert!(crate::execute::set_property_in_place(
        &receiver,
        "value",
        Value::Number(17.0)
    ));
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(17.0)
    );
    assert_eq!(
        native_count(&plan, pc),
        before,
        "shadowing rejects before entry"
    );
}

#[cfg(target_arch = "aarch64")]
#[test]
fn ordinary_source_prototype_accessor_exits_before_native_entry() {
    let (body, plan, pc) = source_plan();
    let code = body.code().unwrap();
    let chain = prototype_chain(11.0);
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    assert_eq!(
        run_get(code, &plan, pc, &chain.receiver),
        Value::Number(11.0)
    );
    let before = native_count(&plan, pc);
    let descriptor = Value::Object(Rc::new(ObjectData::new(vec![(
        "get".into(),
        Value::Undefined,
    )])));
    let owner = Value::Object(Rc::clone(&chain.owner));
    let key = crate::builtins::descriptor_key("value");
    assert!(crate::execute::set_property_in_place(
        &owner, &key, descriptor
    ));
    assert_eq!(run_get(code, &plan, pc, &chain.receiver), Value::Undefined);
    assert_eq!(
        native_count(&plan, pc),
        before,
        "accessor rejects before entry"
    );
}
