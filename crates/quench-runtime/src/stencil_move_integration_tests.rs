use crate::completion::Completion;
use crate::machine::{BaselinePlan, CodeView, FunctionCode};
use crate::value::Value;

fn nested_functions(root: CodeView<'_>) -> Vec<FunctionCode> {
    let mut output = Vec::new();
    root.cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| output.push(body.clone()));
    });
    output
}

fn move_return_body(root: CodeView<'_>) -> (FunctionCode, usize) {
    let mut pending = nested_functions(root);
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else {
            continue;
        };
        if let Some(pc) = move_return_pc(code) {
            return (body, pc);
        }
        pending.extend(nested_functions(code));
    }
    panic!("ordinary conditional source must lower a Move/Return edge")
}

fn move_return_pc(code: CodeView<'_>) -> Option<usize> {
    (0..code.len().saturating_sub(1)).find(|pc| {
        let Some(moved) = code.instruction(*pc) else {
            return false;
        };
        let Some(returned) = code.instruction(*pc + 1) else {
            return false;
        };
        moved.opcode == crate::ir::Opcode::Move
            && returned.opcode == crate::ir::Opcode::Return
            && returned.a == moved.a
    })
}

fn body_with_returning_opcode(
    root: CodeView<'_>,
    opcode: crate::ir::Opcode,
) -> (FunctionCode, usize) {
    let mut pending = nested_functions(root);
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else { continue };
        if let Some(pc) = (0..code.len().saturating_sub(1)).find(|pc| {
            let Some(value) = code.instruction(*pc) else {
                return false;
            };
            let Some(ret) = code.instruction(*pc + 1) else {
                return false;
            };
            value.opcode == opcode && ret.opcode == crate::ir::Opcode::Return && ret.a == value.a
        }) {
            return (body, pc);
        }
        pending.extend(nested_functions(code));
    }
    panic!("ordinary source must lower returning {opcode:?}")
}

fn body_with_opcode(root: CodeView<'_>, opcode: crate::ir::Opcode) -> (FunctionCode, usize) {
    let mut pending = nested_functions(root);
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else { continue };
        if let Some(pc) = (0..code.len()).rev().find(|pc| {
            code.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == opcode)
        }) {
            return (body, pc);
        }
        pending.extend(nested_functions(code));
    }
    panic!("ordinary source must lower {opcode:?}")
}

fn body_with_number_constant(root: CodeView<'_>, expected: f64) -> (FunctionCode, usize) {
    let mut pending = nested_functions(root);
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else { continue };
        if let Some(pc) = (0..code.len().saturating_sub(1)).find(|pc| {
            let Some((dst, crate::ops::Constant::Number(value))) = code.constant_at(*pc) else {
                return false;
            };
            let Some(ret) = code.instruction(*pc + 1) else {
                return false;
            };
            value.to_bits() == expected.to_bits()
                && ret.opcode == crate::ir::Opcode::Return
                && ret.a == dst
        }) {
            return (body, pc);
        }
        pending.extend(nested_functions(code));
    }
    panic!("ordinary source must retain Number constant")
}

fn leaf_policy() -> crate::stencil_policy::ExecutionPolicy {
    crate::stencil_policy::ExecutionPolicy {
        native_leaves: true,
        native_dispatch: false,
        fused_regions: false,
        composed_regions: false,
        optimizing_view: false,
    }
}

fn disabled_policy() -> crate::stencil_policy::ExecutionPolicy {
    crate::stencil_policy::ExecutionPolicy {
        native_leaves: false,
        native_dispatch: false,
        fused_regions: false,
        composed_regions: false,
        optimizing_view: false,
    }
}

fn execute_store_state(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    instruction: crate::ir::Instruction,
    value: Value,
) -> (Completion, Value) {
    let environment = crate::environment::Environment::new();
    environment.set(instruction.a, Value::Number(0.0));
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(4),
    );
    registers.write(usize::from(instruction.b), value);
    let completion = crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment.clone(),
    )
    .expect("local-store execution")
    .0;
    (completion, environment.get(instruction.a))
}

fn assert_owned_store(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    instruction: crate::ir::Instruction,
) {
    let object = std::rc::Rc::new(crate::value::ObjectData::new(vec![]));
    let weak = std::rc::Rc::downgrade(&object);
    let (_, stored) =
        execute_store_state(code, plan, pc, instruction, Value::Object(object.clone()));
    let Value::Object(stored) = stored else {
        panic!("stored owner must remain an object")
    };
    assert!(std::rc::Rc::ptr_eq(&object, &stored));
    drop(object);
    assert!(weak.upgrade().is_some(), "environment must retain owner");
}

fn execute_move(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    registers: &mut crate::register_file::RegisterFile,
) -> Completion {
    crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .expect("Move/Return execution")
    .0
}

fn assert_owned_move(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    source: usize,
    registers: &mut crate::register_file::RegisterFile,
) {
    let object = std::rc::Rc::new(crate::value::ObjectData::new(vec![]));
    let weak = std::rc::Rc::downgrade(&object);
    registers.write(source, Value::Object(std::rc::Rc::clone(&object)));
    let Completion::Return(Value::Object(result)) = execute_move(code, plan, pc, registers) else {
        panic!("tagged Move must preserve the object result")
    };
    assert!(std::rc::Rc::ptr_eq(&object, &result));
    drop(object);
    assert!(weak.upgrade().is_some(), "moved owner must stay rooted");
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_move_executes_typed_native_body() {
    let program = crate::reduce::reduce_source("function choose(x){return x ? 17 : 23}")
        .expect("ordinary source lowers");
    let (body, pc) = move_return_body(program.code());
    let code = body.code().expect("linked conditional body");
    let instruction = code.instruction(pc).expect("Move instruction");
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    let plan = BaselinePlan::compile_for_test(code, policy);
    let native = plan.native_move_at(pc).expect("normal Move admission");
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(8),
    );
    registers.write(usize::from(instruction.b), Value::Number(23.0));
    assert_eq!(
        execute_move(code, &plan, pc, &mut registers),
        Completion::Return(Value::Number(23.0))
    );

    assert_owned_move(code, &plan, pc, usize::from(instruction.b), &mut registers);

    let native = native.borrow();
    assert_eq!(native.native_entry_count(), 2);
    assert_eq!(
        native.last_native_view().expect("native witness").abi,
        crate::stencil_select::RegionAbi::TaggedWord
    );
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(native.last_native_view().expect("native witness").generated);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_constant_executes_typed_native_body() {
    let program = crate::reduce::reduce_source("function value(){return 42.5}")
        .expect("ordinary constant source lowers");
    let (body, pc) = body_with_number_constant(program.code(), 42.5);
    let code = body.code().expect("linked constant body");
    let plan = BaselinePlan::compile_for_test(code, leaf_policy());
    let native = plan.native_load_const_at(pc).expect("constant admission");
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(4),
    );
    let completion = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .expect("constant execution")
    .0;
    assert_eq!(completion, Completion::Return(Value::Number(42.5)));
    assert_eq!(native.borrow().native_entry_count(), 1);
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(
        crate::stencil_select::select_physical(crate::stencil_select::load_const_region_key())
            .expect("generated constant physical view")
            .generated
    );
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_local_load_executes_typed_native_body() {
    let program = crate::reduce::reduce_source("function value(x){return x}")
        .expect("ordinary local source lowers");
    let (body, pc) = body_with_returning_opcode(program.code(), crate::ir::Opcode::LoadLocal);
    let code = body.code().expect("linked local body");
    let instruction = code.instruction(pc).expect("LoadLocal instruction");
    let plan = BaselinePlan::compile_for_test(code, leaf_policy());
    let native = plan.native_load_local_at(pc).expect("local admission");
    let environment = crate::environment::Environment::new();
    environment.set(instruction.b, Value::Number(37.5));
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(4),
    );
    let completion = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    )
    .expect("local execution")
    .0;
    assert_eq!(completion, Completion::Return(Value::Number(37.5)));
    assert_eq!(native.borrow().native_entry_count(), 1);
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ordinary_source_local_store_executes_native_and_guards_binding() {
    let program = crate::reduce::reduce_source("function value(x){let y=0;y=x;return y}")
        .expect("ordinary local-store source lowers");
    let (body, pc) = body_with_opcode(program.code(), crate::ir::Opcode::StoreLocal);
    let code = body.code().expect("linked local-store body");
    let instruction = code.instruction(pc).expect("StoreLocal instruction");
    let plan = BaselinePlan::compile_for_test(code, leaf_policy());
    let fallback = BaselinePlan::compile_for_test(code, disabled_policy());
    let native = plan
        .native_store_local_at(pc)
        .expect("local-store admission");
    let expected = execute_store_state(code, &fallback, pc, instruction, Value::Number(37.5));
    let actual = execute_store_state(code, &plan, pc, instruction, Value::Number(37.5));
    assert_eq!(actual, expected);
    assert_eq!(actual.1, Value::Number(37.5));
    assert_owned_store(code, &plan, pc, instruction);
    assert_eq!(native.borrow().native_entry_count(), 2);
    #[cfg(quench_generated_stencil_artifacts)]
    assert!(native.borrow().last_native_view().unwrap().generated);

    let environment = crate::environment::Environment::new();
    environment.set(instruction.a, Value::Number(0.0));
    environment.mark_immutable_slot(instruction.a);
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(4),
    );
    registers.write(usize::from(instruction.b), Value::Number(41.0));
    let result = crate::vm::execute_baseline_code_from(
        code,
        &plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        environment,
    );
    assert!(result.is_err(), "immutable binding must use ordinary throw");
    assert_eq!(native.borrow().native_entry_count(), 2);
}
