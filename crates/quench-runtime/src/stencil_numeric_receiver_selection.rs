//! Admission for a fresh nonescaping receiver with one affine method.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::{FunctionKind, Op};
use crate::stencil_numeric_integer_loop::{
    IntegerLoopSelection, IntegerRecurrence, RECEIVER_LOOP_BACKEDGE, RECEIVER_REGION_END,
};
use crate::stencil_numeric_integer_selection::{
    binary_operator, exact_i32, number_constant, undefined_constant,
};

const METHOD_LEN: usize = 11;
const LOOP_HEADER: usize = 12;
const LOOP_EXIT: usize = 29;

pub(crate) fn select_receiver_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, RECEIVER_REGION_END)?;
    let ops = operation_window(entries)?;
    let receiver = receiver_fact(code, &ops)?;
    validate_loop(code, &ops, receiver.object, receiver.function)?;
    IntegerLoopSelection::at(
        0,
        ops[5].b,
        ops[7].a,
        ops[10].a,
        6,
        14,
        receiver.multiplier,
        IntegerRecurrence::ReceiverConstant(receiver.addend),
    )
}

struct ReceiverFact {
    object: u16,
    function: u16,
    multiplier: i32,
    addend: i32,
}

fn receiver_fact(
    code: CodeView<'_>,
    ops: &[Instruction; RECEIVER_REGION_END],
) -> Option<ReceiverFact> {
    let Op::MakeFunctionWithKind {
        dst: function,
        body,
        params: 1,
        kind: FunctionKind::Ordinary,
        is_async: false,
        ..
    } = code.cold_at(1)?
    else {
        return None;
    };
    let Op::SetFunctionName {
        function: named,
        name,
    } = code.cold_at(2)?
    else {
        return None;
    };
    let Op::MakeObject {
        dst: object,
        properties,
    } = code.cold_at(3)?
    else {
        return None;
    };
    (*named == *function && name == "f").then_some(())?;
    let (bias_source, _method_source) = literal_sources(properties, *function)?;
    (ops[0].a == bias_source).then_some(())?;
    let addend = number_i32(code, ops[0])?;
    let multiplier = affine_receiver_method(body.code()?)?;
    Some(ReceiverFact {
        object: *object,
        function: *function,
        multiplier,
        addend,
    })
}

fn literal_sources(
    properties: &[(crate::value::PropertyName, u16)],
    function: u16,
) -> Option<(u16, u16)> {
    let [(bias, bias_source), (method, method_source)] = properties else {
        return None;
    };
    (bias == "bias" && method == "f" && *method_source == function)
        .then_some((*bias_source, *method_source))
}

fn affine_receiver_method(code: CodeView<'_>) -> Option<i32> {
    let i: [Instruction; METHOD_LEN] = (code.len() == METHOD_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).unwrap()))?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::LoadConst,
        Opcode::Mul,
        Opcode::LoadLocalChecked,
        Opcode::GetN,
        Opcode::Add,
        Opcode::LoadConst,
        Opcode::Binary,
        Opcode::Return,
        Opcode::LoadConst,
        Opcode::Return,
    ];
    i.iter()
        .zip(expected)
        .all(|(a, b)| {
            b.matches_physical_contract(a.opcode)
                || (b == Opcode::GetN && a.opcode == Opcode::GetNQuickened)
        })
        .then_some(())?;
    validate_method_flow(code, &i)?;
    number_i32(code, i[1])
}

fn validate_method_flow(code: CodeView<'_>, i: &[Instruction; METHOD_LEN]) -> Option<()> {
    (i[2].b == i[0].a && i[2].c == i[1].a && i[4].b == i[3].a).then_some(())?;
    (code.metadata_at(4)?.name.as_deref() == Some("bias")).then_some(())?;
    (i[5].b == i[2].a && i[5].c == i[4].a && i[7].b == i[5].a).then_some(())?;
    number_constant(code, i[6], 0.0)?;
    binary_operator(i[7], crate::ops::BinaryOp::BitwiseOr)?;
    (i[7].c == i[6].a && i[8].a == i[7].a).then_some(())?;
    undefined_constant(code, i[9])?;
    (i[10].a == i[9].a).then_some(())
}

fn validate_loop(
    code: CodeView<'_>,
    i: &[Instruction; RECEIVER_REGION_END],
    object: u16,
    function: u16,
) -> Option<()> {
    validate_prefix(code, i, object)?;
    validate_test(code, i)?;
    validate_body(code, i, object, function)?;
    validate_update_and_exit(code, i)
}

fn validate_prefix(
    code: CodeView<'_>,
    i: &[Instruction; RECEIVER_REGION_END],
    object: u16,
) -> Option<()> {
    (i[4].a != i[7].a && i[4].a != i[10].a && i[7].a != i[10].a).then_some(())?;
    (i[4].b == object).then_some(())?;
    (i[6].b == i[5].a && i[7].b == i[6].a).then_some(())?;
    undefined_constant(code, i[8])?;
    number_constant(code, i[9], 0.0)?;
    (i[10].b == i[9].a).then_some(())?;
    undefined_constant(code, i[11])
}

fn validate_test(code: CodeView<'_>, i: &[Instruction; RECEIVER_REGION_END]) -> Option<()> {
    (i[12].b == i[10].a && i[14].b == i[13].a).then_some(())?;
    (code.metadata_at(14)?.name.as_deref() == Some("n")).then_some(())?;
    binary_operator(i[15], crate::ops::BinaryOp::LessThan)?;
    (i[15].b == i[12].a && i[15].c == i[14].a && i[16].a == i[15].a).then_some(())?;
    (usize::from(i[16].b) == LOOP_EXIT).then_some(())
}

fn validate_body(
    code: CodeView<'_>,
    i: &[Instruction; RECEIVER_REGION_END],
    object: u16,
    function: u16,
) -> Option<()> {
    (i[17].b == i[4].a && i[18].b == i[17].a).then_some(())?;
    (code.metadata_at(18)?.name.as_deref() == Some("f")).then_some(())?;
    (i[19].b == i[7].a && i[20].flags == 1).then_some(())?;
    (i[20].b == i[17].a && i[20].c == i[18].a).then_some(())?;
    (code.operand_window_at(20)? == [i[19].a]).then_some(())?;
    (function != object).then_some(())?;
    (i[21].a == i[7].a && i[21].b == i[20].a && i[22].b == i[20].a).then_some(())
}

fn validate_update_and_exit(
    code: CodeView<'_>,
    i: &[Instruction; RECEIVER_REGION_END],
) -> Option<()> {
    number_constant(code, i[24], 1.0)?;
    binary_operator(i[25], crate::ops::BinaryOp::NumericAdd)?;
    (i[23].b == i[10].a && i[25].b == i[23].a && i[25].c == i[24].a).then_some(())?;
    (i[26].a == i[10].a && i[26].b == i[25].a && i[27].b == i[23].a).then_some(())?;
    (usize::from(i[RECEIVER_LOOP_BACKEDGE].a) == LOOP_HEADER).then_some(())?;
    (i[29].b == i[7].a && i[30].a == i[29].a).then_some(())?;
    undefined_constant(code, i[31])?;
    (i[32].a == i[31].a).then_some(())
}

fn operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; RECEIVER_REGION_END]> {
    let i: [Instruction; RECEIVER_REGION_END] = entries
        .get(..RECEIVER_REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadConst,
        Opcode::Slow,
        Opcode::Slow,
        Opcode::Slow,
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
        Opcode::GetN,
        Opcode::LoadLocal,
        Opcode::CallN,
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
        .all(|(a, b)| {
            b.matches_physical_contract(a.opcode)
                || (b == Opcode::GetN && a.opcode == Opcode::GetNQuickened)
        })
        .then_some(i)
}

fn number_i32(code: CodeView<'_>, instruction: Instruction) -> Option<i32> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    exact_i32(*value)
}
