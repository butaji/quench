use crate::completion::Completion;
use crate::ir::Opcode;
use crate::machine::{BaselinePlan, CodeView};
use crate::value::Value;

fn numeric_body(source: &str, opcode: Opcode) -> (crate::machine::FunctionCode, usize) {
    let program = crate::reduce::reduce_source(source).expect("ordinary arithmetic source");
    let mut pending = nested_bodies(program.code());
    while let Some(body) = pending.pop() {
        let Some(code) = body.code() else { continue };
        if let Some(pc) = (0..code.len()).find(|pc| {
            code.instruction(*pc)
                .is_some_and(|instruction| instruction.opcode == opcode)
        }) {
            return (body, pc);
        }
        pending.extend(nested_bodies(code));
    }
    panic!("lowered arithmetic body")
}

fn nested_bodies(code: CodeView<'_>) -> Vec<crate::machine::FunctionCode> {
    let mut bodies = Vec::new();
    code.cold_ops()
        .for_each(|(_, op)| op.visit_bodies(&mut |body| bodies.push(body.clone())));
    bodies
}

fn execute_from(
    code: CodeView<'_>,
    plan: &BaselinePlan,
    pc: usize,
    lhs: Value,
    rhs: Value,
) -> Completion {
    let instruction = code.instruction(pc).expect("arithmetic instruction");
    let mut registers = crate::register_file::RegisterFile::with_undefined(
        usize::from(code.register_count()).max(8),
    );
    registers.write(usize::from(instruction.b), lhs);
    registers.write(usize::from(instruction.c), rhs);
    crate::vm::execute_baseline_code_from(
        code,
        plan,
        pc,
        &mut registers,
        &crate::vm::current_context_or_default(),
        crate::environment::Environment::new(),
    )
    .expect("arithmetic execution")
    .0
}

fn returned_number(completion: Completion) -> f64 {
    let Completion::Return(Value::Number(number)) = completion else {
        panic!("arithmetic body must return a Number")
    };
    number
}

fn assert_number(actual: f64, expected: f64) {
    if expected.is_nan() {
        assert!(actual.is_nan());
    } else {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

macro_rules! arithmetic_driver_case {
    ($test:ident, $source:literal, $opcode:ident, $cases:expr, $hostile:expr) => {
        #[test]
        fn $test() {
            let (body, pc) = numeric_body($source, Opcode::$opcode);
            let code = body.code().expect("linked arithmetic body");
            let plan = BaselinePlan::compile_for_test(
                code,
                crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
            );
            let native = plan.native_binary_at(pc).expect("native arithmetic plan");
            for (lhs, rhs, expected) in $cases {
                let actual = returned_number(execute_from(
                    code,
                    &plan,
                    pc,
                    Value::Number(lhs),
                    Value::Number(rhs),
                ));
                assert_number(actual, expected);
            }
            let native_entries = native.borrow().native_entry_count();
            assert!(
                native_entries >= $cases.len() as u64,
                "actual native witness"
            );
            let (lhs, rhs, expected) = $hostile;
            let actual = returned_number(execute_from(code, &plan, pc, lhs, rhs));
            assert_number(actual, expected);
            assert_eq!(native.borrow().native_entry_count(), native_entries);
            #[cfg(quench_generated_stencil_artifacts)]
            assert!(
                native
                    .borrow()
                    .last_native_view()
                    .expect("physical view")
                    .generated
            );
        }
    };
}

arithmetic_driver_case!(
    ordinary_source_multiply_executes_native_and_guards_coercion,
    "function f(x,y){return x*y}",
    Mul,
    [
        (6.0, 7.0, 42.0),
        (-0.0, 3.0, -0.0),
        (f64::INFINITY, 0.0, f64::NAN),
    ],
    (Value::String("6".into()), Value::Number(7.0), 42.0)
);

arithmetic_driver_case!(
    ordinary_source_divide_executes_native_and_guards_coercion,
    "function f(x,y){return x/y}",
    Div,
    [
        (7.0, 2.0, 3.5),
        (0.0, -1.0, -0.0),
        (1.0, 0.0, f64::INFINITY),
    ],
    (Value::String("7".into()), Value::Number(2.0), 3.5)
);
