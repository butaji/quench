//! Guarded direct and bounded-polymorphic call-loop admission.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use crate::stencil_numeric_integer_loop::{
    IntegerLoopSelection, IntegerRecurrence, DIRECT_LOOP_BACKEDGE, DIRECT_LOOP_EXIT,
    DIRECT_LOOP_HEADER, DIRECT_REGION_END, LOOP_HEADER, NAMED_REGION_END, POLYMORPHIC_REGION_END,
};
use crate::stencil_numeric_integer_selection::{
    binary_operator, number_constant, undefined_constant,
};

pub(crate) fn select_call_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    select_direct_loop(code, entries, cfg)
        .or_else(|| select_bound_loop(code, entries, cfg))
        .or_else(|| {
            crate::stencil_numeric_arguments_selection::select_arguments_loop(code, entries, cfg)
        })
        .or_else(|| select_named_loop(code, cfg))
        .or_else(|| select_polymorphic_loop(code, entries, cfg))
        .or_else(|| {
            crate::stencil_numeric_receiver_selection::select_receiver_loop(code, entries, cfg)
        })
}

const BOUND_REGION_END: usize = crate::stencil_numeric_integer_loop::BOUND_REGION_END;

fn select_bound_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, BOUND_REGION_END)?;
    let i = bound_operation_window(entries)?;
    validate_bound_prefix(code, &i)?;
    validate_bound_loop(code, &i)?;
    Some(IntegerLoopSelection {
        state_slot: i[0].b,
        value_slot: i[8].a,
        index_slot: i[11].a,
        seed_pc: 7,
        bound_pc: 15,
        multiplier: 0,
        recurrence: IntegerRecurrence::BoundCallee(metadata_name(code, 1)?),
    })
}

fn validate_bound_prefix(code: CodeView<'_>, i: &[Instruction; BOUND_REGION_END]) -> Option<()> {
    (i[1].b == i[0].a && metadata_name(code, 1)?.as_ref() == "f").then_some(())?;
    (i[2].b == i[1].a && metadata_name(code, 2)?.as_ref() == "bind").then_some(())?;
    matches!(code.constant(i[3].b), Some(crate::ops::Constant::Null)).then_some(())?;
    (i[4].flags == 1 && i[4].b == i[1].a && i[4].c == i[2].a).then_some(())?;
    (code.operand_window_at(4)? == [i[3].a] && i[5].b == i[4].a).then_some(())?;
    (i[7].b == i[6].a && i[8].b == i[7].a).then_some(())?;
    undefined_constant(code, i[9])?;
    number_constant(code, i[10], 0.0)?;
    (i[11].b == i[10].a).then_some(())?;
    undefined_constant(code, i[12])
}

fn validate_bound_loop(code: CodeView<'_>, i: &[Instruction; BOUND_REGION_END]) -> Option<()> {
    (i[13].b == i[11].a && i[15].b == i[14].a).then_some(())?;
    binary_operator(i[16], crate::ops::BinaryOp::LessThan)?;
    (i[16].b == i[13].a && i[16].c == i[15].a && i[17].a == i[16].a).then_some(())?;
    (usize::from(i[17].b) == 29 && i[18].b == i[5].a && i[19].b == i[8].a).then_some(())?;
    (i[20].flags == 1 && i[20].b == i[18].a && i[20].c == i[19].a).then_some(())?;
    (i[21].a == i[8].a && i[21].b == i[20].a && i[22].b == i[20].a).then_some(())?;
    validate_bound_update(code, i)
}

fn validate_bound_update(code: CodeView<'_>, i: &[Instruction; BOUND_REGION_END]) -> Option<()> {
    number_constant(code, i[24], 1.0)?;
    binary_operator(i[25], crate::ops::BinaryOp::NumericAdd)?;
    (i[23].b == i[11].a && i[25].b == i[23].a && i[25].c == i[24].a).then_some(())?;
    (i[26].a == i[11].a && i[26].b == i[25].a && i[27].b == i[23].a).then_some(())?;
    (usize::from(i[28].a) == 13 && i[29].b == i[8].a && i[30].a == i[29].a).then_some(())?;
    undefined_constant(code, i[31])?;
    (i[32].a == i[31].a).then_some(())
}

fn bound_operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; BOUND_REGION_END]> {
    let i: [Instruction; BOUND_REGION_END] = entries
        .get(..BOUND_REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::GetN,
        Opcode::LoadConst,
        Opcode::CallN,
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

fn select_direct_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, DIRECT_REGION_END)?;
    let i = direct_operation_window(entries)?;
    direct_bindings_match(code, &i)?;
    Some(IntegerLoopSelection {
        state_slot: i[0].b,
        value_slot: i[2].a,
        index_slot: i[8].a,
        seed_pc: 1,
        bound_pc: 12,
        multiplier: 0,
        recurrence: IntegerRecurrence::DirectCallee(metadata_name(code, 4)?),
    })
}

fn direct_operation_window(entries: &[BaselineEntry]) -> Option<[Instruction; DIRECT_REGION_END]> {
    let i: [Instruction; DIRECT_REGION_END] = entries
        .get(..DIRECT_REGION_END)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let expected = [
        Opcode::LoadLocal,
        Opcode::GetN,
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
        .all(|(actual, expected)| {
            actual.opcode == expected
                || (expected == Opcode::GetN && actual.opcode == Opcode::GetNQuickened)
        })
        .then_some(i)
}

fn direct_bindings_match(code: CodeView<'_>, i: &[Instruction; DIRECT_REGION_END]) -> Option<()> {
    let (state, value, callee, index) = (i[0].b, i[2].a, i[5].a, i[8].a);
    all_distinct([state, value, callee, index])?;
    (i[1].b == i[0].a && i[2].b == i[1].a).then_some(())?;
    (i[3].b == state && i[4].b == i[3].a && i[5].b == i[4].a).then_some(())?;
    undefined_constant(code, i[6])?;
    number_constant(code, i[7], 0.0)?;
    (i[8].b == i[7].a && i[10].b == index && i[11].b == state).then_some(())?;
    undefined_constant(code, i[9])?;
    direct_test_and_call(code, i, value, callee)?;
    direct_update_and_exit(code, i, value, index)
}

fn direct_test_and_call(
    code: CodeView<'_>,
    i: &[Instruction; DIRECT_REGION_END],
    value: u16,
    callee: u16,
) -> Option<()> {
    (i[12].b == i[11].a && metadata_name(code, 12).is_some()).then_some(())?;
    binary_operator(i[13], crate::ops::BinaryOp::LessThan)?;
    (i[13].b == i[10].a && i[13].c == i[12].a && i[14].a == i[13].a).then_some(())?;
    (usize::from(i[14].b) == DIRECT_LOOP_EXIT && i[15].b == callee && i[16].b == value)
        .then_some(())?;
    (i[17].flags == 1 && i[17].b == i[15].a && i[17].c == i[16].a).then_some(())?;
    (i[18].a == value && i[18].b == i[17].a && i[19].b == i[17].a).then_some(())
}

fn direct_update_and_exit(
    code: CodeView<'_>,
    i: &[Instruction; DIRECT_REGION_END],
    value: u16,
    index: u16,
) -> Option<()> {
    number_constant(code, i[21], 1.0)?;
    binary_operator(i[22], crate::ops::BinaryOp::NumericAdd)?;
    (i[20].b == index && i[22].b == i[20].a && i[22].c == i[21].a).then_some(())?;
    (i[23].a == index && i[23].b == i[22].a && i[24].b == i[20].a).then_some(())?;
    (usize::from(i[DIRECT_LOOP_BACKEDGE].a) == DIRECT_LOOP_HEADER
        && i[DIRECT_LOOP_EXIT].b == value
        && i[27].a == i[DIRECT_LOOP_EXIT].a)
        .then_some(())?;
    undefined_constant(code, i[28])?;
    (i[29].a == i[28].a).then_some(())
}

fn all_distinct(values: [u16; 4]) -> Option<()> {
    values
        .iter()
        .enumerate()
        .all(|(index, value)| !values[..index].contains(value))
        .then_some(())
}

fn select_polymorphic_loop(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, POLYMORPHIC_REGION_END)?;
    let i = polymorphic_operation_window(entries)?;
    polymorphic_bindings_match(code, &i)?;
    Some(IntegerLoopSelection {
        state_slot: i[0].b,
        value_slot: i[2].a,
        index_slot: i[5].a,
        seed_pc: 1,
        bound_pc: 9,
        multiplier: 0,
        recurrence: IntegerRecurrence::EquivalentCallees([
            metadata_name(code, 17)?,
            metadata_name(code, 21)?,
        ]),
    })
}

fn polymorphic_operation_window(
    entries: &[BaselineEntry],
) -> Option<[Instruction; POLYMORPHIC_REGION_END]> {
    let i: [Instruction; POLYMORPHIC_REGION_END] = entries
        .get(..POLYMORPHIC_REGION_END)?
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
        Opcode::Binary,
        Opcode::JumpIfFalse,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::Move,
        Opcode::Jump,
        Opcode::LoadLocal,
        Opcode::GetN,
        Opcode::Move,
        Opcode::LoadLocal,
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
        .all(|(actual, expected)| {
            actual.opcode == expected
                || (expected == Opcode::GetN && actual.opcode == Opcode::GetNQuickened)
        })
        .then_some(i)
}

fn polymorphic_bindings_match(
    code: CodeView<'_>,
    i: &[Instruction; POLYMORPHIC_REGION_END],
) -> Option<()> {
    let (state, value, index) = (i[0].b, i[2].a, i[5].a);
    (state != value && state != index && value != index).then_some(())?;
    common_prefix_matches(code, i, state, value, index)?;
    number_constant(code, i[13], 7.0)?;
    binary_operator(i[14], crate::ops::BinaryOp::Remainder)?;
    (i[14].b == i[12].a && i[15].a == i[14].a && usize::from(i[15].b) == 20).then_some(())?;
    (i[17].b == i[16].a && i[18].b == i[17].a && usize::from(i[19].a) == 23).then_some(())?;
    (i[21].b == i[20].a && i[22].b == i[21].a && i[22].a == i[18].a).then_some(())?;
    (i[23].b == value && i[24].b == i[18].a && i[24].c == i[23].a).then_some(())?;
    (i[25].a == value && i[25].b == i[24].a && i[26].b == i[24].a).then_some(())?;
    update_and_exit_match(code, i, value, index)
}

fn common_prefix_matches(
    code: CodeView<'_>,
    i: &[Instruction],
    state: u16,
    value: u16,
    index: u16,
) -> Option<()> {
    (i[1].b == i[0].a && i[2].b == i[1].a && i[5].b == i[4].a).then_some(())?;
    undefined_constant(code, i[3])?;
    number_constant(code, i[4], 0.0)?;
    undefined_constant(code, i[6])?;
    (i[7].b == index && i[8].b == state && i[9].b == i[8].a).then_some(())?;
    binary_operator(i[10], crate::ops::BinaryOp::LessThan)?;
    (i[10].b == i[7].a && i[10].c == i[9].a && i[11].a == i[10].a).then_some(())?;
    (usize::from(i[11].b) == 33 && i[12].b == index && i[16].b == state && i[20].b == state)
        .then_some(())
}

fn update_and_exit_match(
    code: CodeView<'_>,
    i: &[Instruction],
    value: u16,
    index: u16,
) -> Option<()> {
    number_constant(code, i[28], 1.0)?;
    binary_operator(i[29], crate::ops::BinaryOp::NumericAdd)?;
    (i[27].b == index && i[29].b == i[27].a && i[29].c == i[28].a).then_some(())?;
    (i[30].a == index && i[30].b == i[29].a && i[31].b == i[27].a).then_some(())?;
    (usize::from(i[32].a) == LOOP_HEADER && i[33].b == value && i[34].a == i[33].a).then_some(())?;
    undefined_constant(code, i[35])?;
    (i[36].a == i[35].a).then_some(())
}

fn metadata_name(code: CodeView<'_>, pc: usize) -> Option<std::rc::Rc<str>> {
    code.metadata_at(pc)?.name.clone()
}

fn select_named_loop(
    code: CodeView<'_>,
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<IntegerLoopSelection> {
    cfg.region_control(0, NAMED_REGION_END)?;
    let fact = crate::function_physical::numeric_affine_named_loop(code)?;
    Some(IntegerLoopSelection {
        state_slot: fact.parameter_slot,
        value_slot: fact.value_slot,
        index_slot: fact.index_slot,
        seed_pc: 1,
        bound_pc: 9,
        multiplier: 0,
        recurrence: IntegerRecurrence::NamedCallee(fact.method_key),
    })
}
