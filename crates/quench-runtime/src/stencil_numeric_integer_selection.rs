//! Pure admission over canonical lowered integer-loop operations.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::stencil_numeric_integer_loop::{
    IntegerLoopSelection, IntegerRecurrence, CONSTANT_LOOP_BACKEDGE, CONSTANT_LOOP_EXIT,
    CONSTANT_REGION_END, INDEX_LOOP_BACKEDGE, INDEX_LOOP_EXIT, INDEX_REGION_END,
    LOCAL_BODY_LOOP_EXIT, LOCAL_BODY_REGION_END, LOCAL_LOOP_EXIT, LOCAL_LOOP_HEADER,
    LOCAL_REGION_END, LOOP_HEADER,
};

const MAX_EXACT_INTEGER: i128 = 1_i128 << 53;

pub(crate) fn select_integer_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IntegerLoopSelection> {
    // The pure local/index/constant recipes are relocatable within the immutable
    // CFG. Call-recurring recipes still depend on their legacy whole-function
    // metadata layout and remain start-zero candidates until their call/exit
    // contracts are generalized by the cross-tier work.
    select_local_loop(code, entries, cfg, start)
        .or_else(|| select_local_loop_body(code, entries, cfg, start))
        .or_else(|| select_index_loop(code, entries, cfg, start))
        .or_else(|| select_constant_loop(code, entries, cfg, start))
        .or_else(|| {
            (start == 0).then_some(())?;
            crate::stencil_numeric_call_selection::select_call_loop(code, entries, cfg)
        })
}

fn select_local_loop_body(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IntegerLoopSelection> {
    let end = start.checked_add(LOCAL_BODY_REGION_END)?;
    cfg.region_control(start, end)?;
    let i: [Instruction; LOCAL_BODY_REGION_END] = entries
        .get(start..end)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::LessThan,
        Opcode::JumpIfFalse,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::AddConst,
        Opcode::LoadConst,
        Opcode::BitwiseOr,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::NumericAdd,
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
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(())?;
    local_loop_body_bindings(code, &i, start)?;
    let multiplier = number_i32(code, i[6])?;
    let addend = add_const_i32(code, i[8])?;
    IntegerLoopSelection::at(
        start,
        i[1].b,
        i[11].a,
        i[16].a,
        5,
        2,
        multiplier,
        IntegerRecurrence::LocalConstantBody(addend),
    )
}

fn local_loop_body_bindings(
    code: CodeView<'_>,
    i: &[Instruction; LOCAL_BODY_REGION_END],
    start: usize,
) -> Option<()> {
    let (value, index, state) = (i[11].a, i[0].b, i[1].b);
    (value != index && value != state && index != state).then_some(())?;
    (i[2].b == i[1].a && i[3].b == i[0].a && i[3].c == i[2].a).then_some(())?;
    binary_operator(i[3], crate::ops::BinaryOp::LessThan)?;
    (i[4].a == i[3].a && usize::from(i[4].b) == start.checked_add(LOCAL_BODY_LOOP_EXIT)?)
        .then_some(())?;
    (i[5].b == value && i[7].b == i[5].a && i[7].c == i[6].a).then_some(())?;
    (!i[8].add_const_is_left() && i[8].b == i[7].a).then_some(())?;
    number_constant(code, i[9], 0.0)?;
    binary_operator(i[10], crate::ops::BinaryOp::BitwiseOr)?;
    (i[10].b == i[8].a && i[10].c == i[9].a && i[11].b == i[10].a).then_some(())?;
    (i[12].b == i[10].a && i[13].b == index).then_some(())?;
    number_constant(code, i[14], 1.0)?;
    binary_operator(i[15], crate::ops::BinaryOp::NumericAdd)?;
    (i[15].b == i[13].a && i[15].c == i[14].a && i[16].a == index && i[16].b == i[15].a)
        .then_some(())?;
    (crate::ir::compact_unary_operator(i[17].flags) == Some(crate::ops::UnaryOp::ToNumeric)
        && i[17].b == i[13].a)
        .then_some(())?;
    (usize::from(i[18].a) == start).then_some(())?;
    (i[19].b == value && i[20].a == i[19].a && i[22].a == i[21].a).then_some(())?;
    undefined_constant(code, i[21])?;
    code.metadata_at(start.checked_add(2)?)?.name.as_deref()?;
    Some(())
}

fn select_local_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IntegerLoopSelection> {
    let end = start.checked_add(LOCAL_REGION_END)?;
    cfg.region_control(start, end)?;
    let i: [Instruction; LOCAL_REGION_END] = entries
        .get(start..end)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadConst,
        Opcode::StoreLocal,
        Opcode::LoadConst,
        Opcode::LoadLocal,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::LessThan,
        Opcode::JumpIfFalse,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::AddConst,
        Opcode::LoadConst,
        Opcode::BitwiseOr,
        Opcode::StoreLocal,
        Opcode::Move,
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::NumericAdd,
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
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(())?;
    local_loop_bindings(code, &i, start)?;
    let multiplier = number_i32(code, i[12])?;
    let addend = add_const_i32(code, i[14])?;
    IntegerLoopSelection::at(
        start,
        i[7].b,
        i[1].a,
        i[4].a,
        0,
        8,
        multiplier,
        IntegerRecurrence::LocalConstant(addend),
    )
}

fn local_loop_bindings(
    code: CodeView<'_>,
    i: &[Instruction; LOCAL_REGION_END],
    start: usize,
) -> Option<()> {
    let (value, index, state) = (i[1].a, i[4].a, i[7].b);
    (i[1].b == i[0].a && i[4].b == i[3].a).then_some(())?;
    undefined_constant(code, i[2])?;
    number_constant(code, i[3], 0.0)?;
    undefined_constant(code, i[5])?;
    (i[6].b == index && i[7].b == state && i[8].b == i[7].a).then_some(())?;
    binary_operator(i[9], crate::ops::BinaryOp::LessThan)?;
    (i[9].b == i[6].a && i[9].c == i[8].a && i[10].a == i[9].a).then_some(())?;
    (usize::from(i[10].b) == start.checked_add(LOCAL_LOOP_EXIT)?).then_some(())?;
    (i[11].b == value && i[13].b == i[11].a && i[13].c == i[12].a).then_some(())?;
    (!i[14].add_const_is_left() && i[14].b == i[13].a).then_some(())?;
    number_constant(code, i[15], 0.0)?;
    binary_operator(i[16], crate::ops::BinaryOp::BitwiseOr)?;
    (i[16].b == i[14].a && i[16].c == i[15].a && i[17].a == value && i[17].b == i[16].a)
        .then_some(())?;
    (i[18].b == i[16].a && i[19].b == index).then_some(())?;
    number_constant(code, i[20], 1.0)?;
    binary_operator(i[21], crate::ops::BinaryOp::NumericAdd)?;
    (i[21].b == i[19].a && i[21].c == i[20].a && i[22].a == index && i[22].b == i[21].a)
        .then_some(())?;
    (crate::ir::compact_unary_operator(i[23].flags) == Some(crate::ops::UnaryOp::ToNumeric)
        && i[23].b == i[19].a)
        .then_some(())?;
    (usize::from(i[24].a) == start.checked_add(LOCAL_LOOP_HEADER)?).then_some(())?;
    (i[25].b == value && i[26].a == i[25].a && i[27].a == i[0].a).then_some(())?;
    undefined_constant(code, i[27])?;
    Some(())
}

fn select_index_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(start, start.checked_add(INDEX_REGION_END)?)?;
    let instructions = index_operation_window(entries, start)?;
    index_constants_and_operators(code, &instructions)?;
    index_bindings_match(code, &instructions, start)?;
    IntegerLoopSelection::at(
        start,
        instructions[0].b,
        instructions[2].a,
        instructions[5].a,
        1,
        9,
        number_i32(code, instructions[13])?,
        IntegerRecurrence::Index,
    )
}

fn index_operation_window(
    entries: &[BaselineEntry],
    start: usize,
) -> Option<[Instruction; INDEX_REGION_END]> {
    let instructions: [Instruction; INDEX_REGION_END] = entries
        .get(start..start.checked_add(INDEX_REGION_END)?)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
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
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::LoadLocal,
        Opcode::Add,
        Opcode::LoadConst,
        Opcode::Binary,
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
    instructions
        .iter()
        .zip(expected)
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(instructions)
}

fn index_constants_and_operators(
    code: CodeView<'_>,
    i: &[Instruction; INDEX_REGION_END],
) -> Option<()> {
    undefined_constant(code, i[3])?;
    number_constant(code, i[4], 0.0)?;
    undefined_constant(code, i[6])?;
    number_i32(code, i[13])?;
    number_constant(code, i[17], 0.0)?;
    number_constant(code, i[22], 1.0)?;
    undefined_constant(code, i[29])?;
    binary_operator(i[10], crate::ops::BinaryOp::LessThan)?;
    binary_operator(i[18], crate::ops::BinaryOp::BitwiseOr)?;
    binary_operator(i[23], crate::ops::BinaryOp::NumericAdd)?;
    (crate::ir::compact_unary_operator(i[25].flags) == Some(crate::ops::UnaryOp::ToNumeric))
        .then_some(())
}

fn index_bindings_match(
    code: CodeView<'_>,
    i: &[Instruction; INDEX_REGION_END],
    start: usize,
) -> Option<()> {
    let state = i[0].b;
    let value = i[2].a;
    let index = i[5].a;
    (state != value && state != index && value != index).then_some(())?;
    (i[1].b == i[0].a && i[2].b == i[1].a && i[5].b == i[4].a).then_some(())?;
    (i[7].b == index && i[8].b == state && i[9].b == i[8].a).then_some(())?;
    (i[10].b == i[7].a && i[10].c == i[9].a && i[11].a == i[10].a).then_some(())?;
    (usize::from(i[11].b) == start.checked_add(INDEX_LOOP_EXIT)?
        && usize::from(i[INDEX_LOOP_BACKEDGE].a) == start.checked_add(LOOP_HEADER)?)
    .then_some(())?;
    (i[12].b == value && i[14].b == i[12].a && i[14].c == i[13].a).then_some(())?;
    (i[15].b == index && i[16].b == i[14].a && i[16].c == i[15].a).then_some(())?;
    (i[18].b == i[16].a && i[18].c == i[17].a && i[19].a == value).then_some(())?;
    (i[19].b == i[18].a && i[20].b == i[18].a).then_some(())?;
    (i[21].b == index && i[23].b == i[21].a && i[23].c == i[22].a).then_some(())?;
    (i[24].a == index && i[24].b == i[23].a && i[25].b == i[21].a).then_some(())?;
    (i[27].b == value && i[28].a == i[27].a && i[30].a == i[29].a).then_some(())?;
    code.metadata_at(start.checked_add(1)?)?.name.as_deref()?;
    code.metadata_at(start.checked_add(9)?)?.name.as_deref()?;
    Some(())
}

fn select_constant_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(start, start.checked_add(CONSTANT_REGION_END)?)?;
    let i = constant_operation_window(entries, start)?;
    constant_bindings_match(code, &i, start)?;
    let multiplier = number_i32(code, i[13])?;
    let addend = add_const_i32(code, i[15])?;
    constant_operators(code, &i)?;
    IntegerLoopSelection::at(
        start,
        i[0].b,
        i[2].a,
        i[5].a,
        1,
        9,
        multiplier,
        IntegerRecurrence::Constant(addend),
    )
}

fn constant_operation_window(
    entries: &[BaselineEntry],
    start: usize,
) -> Option<[Instruction; CONSTANT_REGION_END]> {
    let instructions: [Instruction; CONSTANT_REGION_END] = entries
        .get(start..start.checked_add(CONSTANT_REGION_END)?)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
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
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::AddConst,
        Opcode::LoadConst,
        Opcode::Binary,
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
    instructions
        .iter()
        .zip(expected)
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
        .then_some(instructions)
}

fn constant_operators(code: CodeView<'_>, i: &[Instruction; CONSTANT_REGION_END]) -> Option<()> {
    undefined_constant(code, i[3])?;
    number_constant(code, i[4], 0.0)?;
    undefined_constant(code, i[6])?;
    number_constant(code, i[16], 0.0)?;
    number_constant(code, i[21], 1.0)?;
    undefined_constant(code, i[28])?;
    binary_operator(i[10], crate::ops::BinaryOp::LessThan)?;
    binary_operator(i[17], crate::ops::BinaryOp::BitwiseOr)?;
    binary_operator(i[22], crate::ops::BinaryOp::NumericAdd)?;
    (crate::ir::compact_unary_operator(i[24].flags) == Some(crate::ops::UnaryOp::ToNumeric))
        .then_some(())
}

fn constant_bindings_match(
    code: CodeView<'_>,
    i: &[Instruction; CONSTANT_REGION_END],
    start: usize,
) -> Option<()> {
    let (state, value, index) = (i[0].b, i[2].a, i[5].a);
    (state != value && state != index && value != index).then_some(())?;
    (i[1].b == i[0].a && i[2].b == i[1].a && i[5].b == i[4].a).then_some(())?;
    (i[7].b == index && i[8].b == state && i[9].b == i[8].a).then_some(())?;
    (i[10].b == i[7].a && i[10].c == i[9].a && i[11].a == i[10].a).then_some(())?;
    (usize::from(i[11].b) == start.checked_add(CONSTANT_LOOP_EXIT)?
        && usize::from(i[CONSTANT_LOOP_BACKEDGE].a) == start.checked_add(LOOP_HEADER)?)
    .then_some(())?;
    (i[12].b == value && i[14].b == i[12].a && i[14].c == i[13].a).then_some(())?;
    (!i[15].add_const_is_left() && i[15].b == i[14].a).then_some(())?;
    (i[17].b == i[15].a && i[17].c == i[16].a && i[18].a == value).then_some(())?;
    (i[18].b == i[17].a && i[19].b == i[17].a).then_some(())?;
    (i[20].b == index && i[22].b == i[20].a && i[22].c == i[21].a).then_some(())?;
    (i[23].a == index && i[23].b == i[22].a && i[24].b == i[20].a).then_some(())?;
    (i[26].b == value && i[27].a == i[26].a && i[29].a == i[28].a).then_some(())?;
    code.metadata_at(start.checked_add(1)?)?.name.as_deref()?;
    code.metadata_at(start.checked_add(9)?)?.name.as_deref()?;
    Some(())
}

fn add_const_i32(code: CodeView<'_>, instruction: Instruction) -> Option<i32> {
    (instruction.opcode == Opcode::AddConst && !instruction.add_const_is_left()).then_some(())?;
    let crate::ops::Constant::Number(value) = code.constant(instruction.c)? else {
        return None;
    };
    exact_i32(*value)
}

pub(crate) fn binary_operator(
    instruction: Instruction,
    expected: crate::ops::BinaryOp,
) -> Option<()> {
    (instruction.opcode.binary_operator(instruction.flags) == Some(expected)).then_some(())
}

pub(crate) fn undefined_constant(code: CodeView<'_>, instruction: Instruction) -> Option<()> {
    matches!(
        code.constant(instruction.b),
        Some(crate::ops::Constant::Undefined)
    )
    .then_some(())
}

pub(crate) fn number_constant(
    code: CodeView<'_>,
    instruction: Instruction,
    expected: f64,
) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn number_i32(code: CodeView<'_>, instruction: Instruction) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    exact_i32(*value)
}

pub(crate) fn exact_i32(value: f64) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return None;
    }
    let result = value as i32;
    (f64::from(result).to_bits() == value.to_bits()).then_some(result)
}

pub(crate) fn exact_bound(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value < usize::MAX as f64 && value.fract() == 0.0)
        .then_some(())?;
    let integer = value as usize;
    (integer as f64 == value).then_some(integer)
}

pub(crate) fn exact_for_all_iterations(
    seed: i32,
    multiplier: i32,
    addend: i32,
    end: usize,
) -> Option<()> {
    let product =
        i128::from(seed).abs().max(i128::from(i32::MIN).abs()) * i128::from(multiplier).abs();
    let maximum_addend = i128::from(addend).abs().max(end as i128);
    (product + maximum_addend <= MAX_EXACT_INTEGER).then_some(())
}
