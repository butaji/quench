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
