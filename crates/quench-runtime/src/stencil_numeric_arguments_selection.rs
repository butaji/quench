//! Admission for one nonescaping affine function with constant call arguments.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{FunctionKind, Op};
use crate::stencil_numeric_integer_loop::{
    IntegerLoopSelection, IntegerRecurrence, ARGUMENTS_REGION_END,
};
use crate::stencil_numeric_integer_selection::{
    binary_operator, exact_i32, number_constant, undefined_constant,
};

const BODY_LEN: usize = 16;
const LOOP_HEADER: usize = 14;
const LOOP_EXIT: usize = 35;

pub(crate) fn select_arguments_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, ARGUMENTS_REGION_END)?;
    let i = operation_window(entries)?;
    let function = local_function(code)?;
    validate_prefix(code, &i, function)?;
    let (multiplier, addend) = call_formula(code, &i, function)?;
    validate_loop(code, &i)?;
    Some(IntegerLoopSelection {
        state_slot: i[7].b,
        value_slot: i[9].a,
        index_slot: i[12].a,
        seed_pc: 8,
        bound_pc: 16,
        multiplier,
        recurrence: IntegerRecurrence::ArgumentConstants(addend),
    })
}

fn local_function(code: CodeView<'_>) -> Option<u16> {
    let Op::MakeFunctionWithKind {
        dst,
        body,
        params: 6,
        captures,
        kind: FunctionKind::Ordinary,
        is_async: false,
        ..
    } = code.cold_at(2)?
    else {
        return None;
    };
    validate_affine_body(body.code()?, *captures)?;
    Some(*dst)
}

fn validate_prefix(
    code: CodeView<'_>,
    i: &[Instruction; ARGUMENTS_REGION_END],
    function: u16,
) -> Option<()> {
    undefined_constant(code, i[0])?;
    (i[1].b == i[0].a).then_some(())?;
    let Op::SetFunctionName {
        function: named,
        name,
    } = code.cold_at(3)?
    else {
        return None;
    };
    (*named == function && name == "f").then_some(())?;
    matches!(
        code.constant(i[4].b),
        Some(crate::ops::Constant::Boolean(true))
    )
    .then_some(())?;
    (i[5].a == function && i[5].b == i[4].a).then_some(())?;
    (code.metadata_at(5)?.name.as_deref() == Some(crate::functions::FUNCTION_SELF)).then_some(())?;
    (i[6].a == i[1].a && i[6].b == function).then_some(())
}

fn call_formula(
    code: CodeView<'_>,
    i: &[Instruction; ARGUMENTS_REGION_END],
    function: u16,
) -> Option<(i32, i32)> {
    (i[19].b == i[6].a && i[19].a != function).then_some(())?;
    let arguments = code.operand_window_at(26)?;
    (arguments.len() == 6 && arguments[0] == i[20].a).then_some(())?;
    let constants: [i32; 5] = i[21..=25]
        .iter()
        .map(|instruction| number_i32(code, *instruction))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (arguments[1..] == i[21..=25].iter().map(|op| op.a).collect::<Vec<_>>()).then_some(())?;
    let tail = &constants[1..];
    (tail.iter().all(|value| *value >= 0) || tail.iter().all(|value| *value <= 0)).then_some(())?;
    let addend = tail
        .iter()
        .try_fold(0_i32, |sum, value| sum.checked_add(*value))?;
    Some((constants[0], addend))
}

fn validate_loop(code: CodeView<'_>, i: &[Instruction; ARGUMENTS_REGION_END]) -> Option<()> {
    (i[8].b == i[7].a && i[9].b == i[8].a).then_some(())?;
    undefined_constant(code, i[10])?;
    number_constant(code, i[11], 0.0)?;
    (i[12].b == i[11].a && i[14].b == i[12].a).then_some(())?;
    undefined_constant(code, i[13])?;
    (i[16].b == i[15].a && code.metadata_at(16)?.name.as_deref() == Some("n")).then_some(())?;
    binary_operator(i[17], crate::ops::BinaryOp::LessThan)?;
    (i[17].b == i[14].a && i[17].c == i[16].a && i[18].a == i[17].a).then_some(())?;
    (usize::from(i[18].b) == LOOP_EXIT).then_some(())?;
    validate_call_and_update(code, i)
}

fn validate_call_and_update(
    code: CodeView<'_>,
    i: &[Instruction; ARGUMENTS_REGION_END],
) -> Option<()> {
    (i[20].b == i[9].a && i[26].flags == 6 && i[26].b == i[19].a).then_some(())?;
    (i[27].a == i[9].a && i[27].b == i[26].a && i[28].b == i[26].a).then_some(())?;
    number_constant(code, i[30], 1.0)?;
    binary_operator(i[31], crate::ops::BinaryOp::NumericAdd)?;
    (i[29].b == i[12].a && i[31].b == i[29].a && i[31].c == i[30].a).then_some(())?;
    (i[32].a == i[12].a && i[32].b == i[31].a && i[33].b == i[29].a).then_some(())?;
    (usize::from(i[34].a) == LOOP_HEADER && i[35].b == i[9].a && i[36].a == i[35].a)
        .then_some(())?;
    undefined_constant(code, i[37])?;
    (i[38].a == i[37].a).then_some(())
}

fn validate_affine_body(code: CodeView<'_>, captures: u16) -> Option<()> {
    let i: [Instruction; BODY_LEN] = (code.len() == BODY_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).unwrap()))?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::Mul,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    i.iter()
        .zip(expected)
        .all(|(a, b)| a.opcode == b)
        .then_some(())?;
    validate_body_flow(code, &i, captures)
}

fn validate_body_flow(code: CodeView<'_>, i: &[Instruction; BODY_LEN], base: u16) -> Option<()> {
    (i[0].b == base && i[1].b == base + 1 && i[2].b == i[0].a && i[2].c == i[1].a).then_some(())?;
    for (load, add, slot) in [(3, 4, 2), (5, 6, 3), (7, 8, 4), (9, 10, 5)] {
        (i[load].b == base + slot && i[add].b == i[add - 2].a && i[add].c == i[load].a)
            .then_some(())?;
    }
    number_constant(code, i[11], 0.0)?;
    binary_operator(i[12], crate::ops::BinaryOp::BitwiseOr)?;
    (i[12].b == i[10].a && i[12].c == i[11].a && i[13].a == i[12].a).then_some(())?;
    undefined_constant(code, i[14])?;
    (i[15].a == i[14].a).then_some(())
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; ARGUMENTS_REGION_END]> {
    let i: [Instruction; ARGUMENTS_REGION_END] = entries
        .get(..ARGUMENTS_REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::Slow,
        Opcode::Slow,
        Opcode::LoadConst,
        Opcode::SetN,
        Opcode::StoreLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::Binary,
        Opcode::JumpIfFalse,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::Call,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::StoreLocal,
        Opcode::Unary,
        Opcode::Jump,
        Opcode::LoadLocal,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    i.iter()
        .zip(expected)
        .all(|(a, b)| a.opcode == b || (b == Opcode::GetN && a.opcode == Opcode::GetNQuickened))
        .then_some(i)
}

fn number_i32(code: CodeView<'_>, instruction: Instruction) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    exact_i32(*value)
}
