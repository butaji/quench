//! Guarded facts for pure decrementing int32 recurrences.

use crate::{ir::Opcode, machine::CodeView};

const BODY_LEN: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct I32CounterRecurrence {
    pub(crate) value_parameter: u16,
    pub(crate) counter_parameter: u16,
    pub(crate) multiplier: i32,
    pub(crate) addend: i32,
    pub(crate) threshold: i32,
    pub(crate) decrement: i32,
}

impl I32CounterRecurrence {
    pub(crate) fn execute(self, arguments: &[crate::value::Value]) -> Option<i32> {
        let mut value = f64::from(exact_i32(number(arguments.first()?)?)?);
        let mut counter = number(arguments.get(1)?)?;
        let iterations = bounded_f64_iterations(counter, self.threshold, self.decrement)?;
        for _ in 0..iterations {
            counter -= f64::from(self.decrement);
            let next = value * f64::from(self.multiplier) + counter + f64::from(self.addend);
            value = f64::from(crate::vm::numeric_to_int32(next));
        }
        Some(crate::vm::numeric_to_int32(value))
    }
}

pub(crate) fn select(code: CodeView<'_>) -> Option<I32CounterRecurrence> {
    let ops = instructions(code)?;
    let (counter_parameter, decrement, threshold) = select_test(code, &ops)?;
    let (value_parameter, multiplier, addend) = select_body(code, &ops, counter_parameter)?;
    select_exit(code, &ops, value_parameter)?;
    Some(I32CounterRecurrence {
        value_parameter,
        counter_parameter,
        multiplier,
        addend,
        threshold,
        decrement,
    })
}

fn select_test(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
) -> Option<(u16, i32, i32)> {
    let counter = ops[1];
    (counter.opcode == Opcode::LoadLocal).then_some(())?;
    is_unary(ops[2], crate::ops::UnaryOp::ToNumeric, counter.a)?;
    let decrement = integer_constant(code, ops[3])?;
    is_binary(
        ops[4],
        crate::ops::BinaryOp::NumericSubtract,
        ops[2].a,
        ops[3].a,
    )?;
    (ops[5].opcode == Opcode::StoreLocal && ops[5].a == counter.b && ops[5].b == ops[4].a)
        .then_some(())?;
    let threshold = integer_constant(code, ops[6])?;
    is_binary(
        ops[7],
        crate::ops::BinaryOp::GreaterThan,
        ops[2].a,
        ops[6].a,
    )?;
    (ops[8] == crate::ir::Instruction::jump_if_false(ops[7].a, 20)).then_some(())?;
    (decrement > 0).then_some((counter.b, decrement, threshold))
}

fn select_body(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
    counter_slot: u16,
) -> Option<(u16, i32, i32)> {
    (ops[9].opcode == Opcode::LoadLocal).then_some(())?;
    let multiplier = integer_constant(code, ops[10])?;
    is_numeric(ops[11], Opcode::Mul, ops[9].a, ops[10].a)?;
    (ops[12].opcode == Opcode::LoadLocal && ops[12].b == counter_slot).then_some(())?;
    is_numeric(ops[13], Opcode::Add, ops[11].a, ops[12].a)?;
    let addend = add_const(code, ops[14], ops[13].a)?;
    let zero = integer_constant(code, ops[15])?;
    (zero == 0).then_some(())?;
    is_binary(
        ops[16],
        crate::ops::BinaryOp::BitwiseOr,
        ops[14].a,
        ops[15].a,
    )?;
    (ops[17].opcode == Opcode::StoreLocal && ops[17].a == ops[9].b && ops[17].b == ops[16].a)
        .then_some(())?;
    (ops[18].opcode == Opcode::Move && ops[18].b == ops[16].a).then_some(())?;
    (ops[19] == crate::ir::Instruction::jump(1)).then_some(())?;
    Some((ops[9].b, multiplier, addend))
}

fn select_exit(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
    value_slot: u16,
) -> Option<()> {
    (ops[20].opcode == Opcode::LoadLocal && ops[20].b == value_slot).then_some(())?;
    (ops[21].opcode == Opcode::Return && ops[21].a == ops[20].a).then_some(())?;
    matches!(
        code.constant_at(22),
        Some((_, crate::ops::Constant::Undefined))
    )
    .then_some(())?;
    (ops[23].opcode == Opcode::Return).then_some(())
}

fn instructions(code: CodeView<'_>) -> Option<[crate::ir::Instruction; BODY_LEN]> {
    (code.len() == BODY_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).expect("bounded code")))
}

fn constant_at(code: CodeView<'_>, instruction: crate::ir::Instruction) -> Option<f64> {
    (instruction.opcode == Opcode::LoadConst).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    Some(*value)
}

fn add_const(code: CodeView<'_>, instruction: crate::ir::Instruction, source: u16) -> Option<i32> {
    (instruction.opcode == Opcode::AddConst && instruction.b == source).then_some(())?;
    (!instruction.add_const_is_left()).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.c)? else {
        return None;
    };
    exact_i32(*value)
}

fn is_binary(
    instruction: crate::ir::Instruction,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode.binary_operator(instruction.flags) == Some(operator)
        && instruction.b == lhs
        && instruction.c == rhs)
        .then_some(())
}

fn is_numeric(
    instruction: crate::ir::Instruction,
    opcode: Opcode,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode == opcode && instruction.b == lhs && instruction.c == rhs).then_some(())
}

fn is_unary(
    instruction: crate::ir::Instruction,
    operator: crate::ops::UnaryOp,
    source: u16,
) -> Option<()> {
    (instruction.opcode == Opcode::Unary
        && crate::ir::compact_unary_operator(instruction.flags) == Some(operator)
        && instruction.b == source)
        .then_some(())
}

fn bounded_f64_iterations(counter: f64, threshold: i32, decrement: i32) -> Option<usize> {
    counter.is_finite().then_some(())?;
    let decrement = f64::from(decrement);
    (decrement > 0.0).then_some(())?;
    let mut current = counter;
    let mut iterations = 0usize;
    while current > f64::from(threshold) {
        let next = current - decrement;
        // At large magnitudes an integer decrement can round away entirely.
        // The canonical loop would not make progress either, so reject this
        // specialization rather than spinning in an uninterruptible helper.
        (next != current).then_some(())?;
        current = next;
        iterations = iterations.checked_add(1)?;
    }
    Some(iterations)
}

fn integer_constant(code: CodeView<'_>, instruction: crate::ir::Instruction) -> Option<i32> {
    exact_i32(constant_at(code, instruction)?)
}

fn number(value: &crate::value::Value) -> Option<f64> {
    let crate::value::Value::Number(value) = value else {
        return None;
    };
    Some(*value)
}

fn exact_i32(value: f64) -> Option<i32> {
    crate::stencil_numeric_integer_selection::exact_i32(value)
}

#[cfg(test)]
mod tests {
    use crate::value::Value;

    const COUNTER: &str = r#"
        var recurrence = function(value, counter) {
            while (counter-- > 0) value = (value * 33 + counter + 7) | 0;
            return value;
        };
    "#;

    fn assert_counter_result(body: &str, expected: &str) {
        let _scope = crate::with_scope::FunctionGuard::isolate();
        let expected = serde_json::to_string(expected).expect("expected result is JSON text");
        let source = format!("{COUNTER}\n{body}\nif (result !== {expected}) throw result;");
        let program = crate::reduce::reduce_source(&source).expect("counter source lowers");
        let mut selected = 0;
        crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
            selected += usize::from(super::select(code).is_some());
        });
        assert_eq!(selected, 1, "the portable counter fact remains available");
        assert_eq!(
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("counter source executes with the Node result"),
            Value::Undefined,
        );
    }

    #[test]
    fn regression_portable_counter_keeps_numeric_boundaries_and_guard_fallbacks() {
        assert_counter_result(
            r#"
                var result = [recurrence(1, 4), recurrence(2147483647, 3),
                 recurrence(-2147483648, 2), recurrence(1, 2.5),
                 Object.is(recurrence(-0, 0), -0), recurrence(3, NaN)].join(',');
            "#,
            "1555363,2147457783,-2147483377,44886,true,3",
        );
    }

    #[test]
    fn regression_portable_counter_preserves_coercion_order_and_thrown_identity() {
        assert_counter_result(
            r#"
                var calls = [];
                var value = { valueOf: function() { calls.push('value'); return 1; } };
                var counter = { valueOf: function() { calls.push('counter'); return 3; } };
                var valueResult = recurrence(value, 2);
                var counterResult = recurrence(1, counter);
                var thrown = {}, sameError = false;
                try { recurrence({ valueOf: function() { throw thrown; } }, 1); }
                catch (error) { sameError = error === thrown; }
                var result = [valueResult, counterResult, calls.join('/'), sameError].join(',');
            "#,
            "1360,46009,value/counter,true",
        );
    }
}
