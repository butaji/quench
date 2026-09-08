//! Bounded facts for pure decrementing int32 recurrences.

use crate::{ir::Opcode, machine::CodeView};

const BODY_LEN: usize = 24;
pub(super) const MAX_ITERATIONS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct I32CounterRecurrence {
    pub(crate) value_parameter: u16,
    pub(crate) counter_parameter: u16,
    multiplier: f64,
    addend: f64,
    threshold: f64,
    decrement: f64,
}

impl I32CounterRecurrence {
    pub(crate) fn execute(self, arguments: &[crate::value::Value]) -> Option<i32> {
        let mut value = f64::from(exact_i32(number(arguments.get(0)?)?)?);
        let mut counter = number(arguments.get(1)?)?;
        let iterations = bounded_iterations(counter, self.threshold, self.decrement)?;
        for _ in 0..iterations {
            counter -= self.decrement;
            let next = value * self.multiplier + counter + self.addend;
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
) -> Option<(u16, f64, f64)> {
    let counter = ops[1];
    (counter.opcode == Opcode::LoadLocal).then_some(())?;
    let decrement = constant_at(code, ops[2])?;
    is_binary(
        ops[3],
        crate::ops::BinaryOp::NumericSubtract,
        counter.a,
        ops[2].a,
    )?;
    (ops[4].opcode == Opcode::StoreLocal && ops[4].a == counter.b && ops[4].b == ops[3].a)
        .then_some(())?;
    is_unary(ops[5], crate::ops::UnaryOp::ToNumeric, counter.a)?;
    let threshold = constant_at(code, ops[6])?;
    is_binary(
        ops[7],
        crate::ops::BinaryOp::GreaterThan,
        ops[5].a,
        ops[6].a,
    )?;
    (ops[8] == crate::ir::Instruction::jump_if_false(ops[7].a, 20)).then_some(())?;
    (decrement > 0.0).then_some((counter.b, decrement, threshold))
}

fn select_body(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; BODY_LEN],
    counter_slot: u16,
) -> Option<(u16, f64, f64)> {
    (ops[9].opcode == Opcode::LoadLocal).then_some(())?;
    let multiplier = constant_at(code, ops[10])?;
    is_numeric(ops[11], Opcode::Mul, ops[9].a, ops[10].a)?;
    (ops[12].opcode == Opcode::LoadLocal && ops[12].b == counter_slot).then_some(())?;
    is_numeric(ops[13], Opcode::Add, ops[11].a, ops[12].a)?;
    let addend = add_const(code, ops[14], ops[13].a)?;
    let zero = constant_at(code, ops[15])?;
    (zero == 0.0).then_some(())?;
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

fn add_const(code: CodeView<'_>, instruction: crate::ir::Instruction, source: u16) -> Option<f64> {
    (instruction.opcode == Opcode::AddConst && instruction.b == source).then_some(())?;
    (!instruction.add_const_is_left()).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.c)? else {
        return None;
    };
    Some(*value)
}

fn is_binary(
    instruction: crate::ir::Instruction,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.opcode == Opcode::Binary
        && crate::ir::compact_binary_operator(instruction.flags) == Some(operator)
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

fn bounded_iterations(counter: f64, threshold: f64, decrement: f64) -> Option<usize> {
    (counter.is_finite() && threshold.is_finite() && decrement.is_finite()).then_some(())?;
    let mut counter = counter;
    for count in 0..=MAX_ITERATIONS {
        if counter <= threshold {
            return Some(count);
        }
        counter -= decrement;
    }
    None
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
