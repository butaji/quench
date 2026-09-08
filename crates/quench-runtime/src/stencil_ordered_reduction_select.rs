//! Static admission for ordered dense-Number reductions.

use super::{ReductionProfile, ReductionSelection, ReductionSource};
use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};

const REGION_END: usize = 27;
const STATE_REGION_END: usize = 30;
const LOOP_HEADER: usize = 6;
const LOOP_BACKEDGE: usize = 24;
const STATE_LOOP_BACKEDGE: usize = 25;
const FOR_ARRAY_PC: usize = 13;
const FOR_BOUND_PC: usize = 8;
const WHILE_LOOP_HEADER: usize = 5;
const WHILE_ARRAY_PC: usize = 12;
const WHILE_BOUND_PC: usize = 7;
const OPERATIONS: [Opcode; REGION_END] = [
    Opcode::LoadConst,
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
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
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
];
const STATE_OPERATIONS: [Opcode; STATE_REGION_END] = [
    Opcode::LoadConst,
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
    Opcode::GetN,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
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
const WHILE_OPERATIONS: [Opcode; STATE_REGION_END] = [
    Opcode::LoadConst,
    Opcode::StoreLocal,
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
    Opcode::GetN,
    Opcode::Slow,
    Opcode::LoadLocal,
    Opcode::AGetI,
    Opcode::Add,
    Opcode::StoreLocal,
    Opcode::Move,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::StoreLocal,
    Opcode::Unary,
    Opcode::Move,
    Opcode::Jump,
    Opcode::LoadLocal,
    Opcode::Return,
    Opcode::LoadConst,
    Opcode::Return,
];

pub(crate) fn select_reduction(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<ReductionSelection> {
    (start == 0).then_some(())?;
    select_direct(code, entries, cfg)
        .or_else(|| select_for_state(code, entries, cfg))
        .or_else(|| select_while_state(code, entries, cfg))
}

fn select_direct(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<ReductionSelection> {
    cfg.region_control(0, REGION_END)?;
    let i = operation_window::<REGION_END>(entries, &OPERATIONS)?;
    direct_constants(code, &i)?;
    direct_bindings(code, &i)?;
    Some(ReductionSelection {
        source: ReductionSource::DirectArray { slot: i[7].b },
        profile: ReductionProfile::OrderedF64,
        total_slot: i[1].a,
        index_slot: i[4].a,
        region_end: REGION_END,
        loop_header: LOOP_HEADER,
        loop_backedge: LOOP_BACKEDGE,
    })
}

fn select_for_state(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<ReductionSelection> {
    cfg.region_control(0, STATE_REGION_END)?;
    let i = operation_window::<STATE_REGION_END>(entries, &STATE_OPERATIONS)?;
    state_constants(code, &i)?;
    let (state, total, index) = state_bindings(code, &i)?;
    Some(ReductionSelection {
        source: ReductionSource::StateArray {
            slot: state,
            array_pc: FOR_ARRAY_PC,
            bound_pc: FOR_BOUND_PC,
        },
        profile: ReductionProfile::ControlFor,
        total_slot: total,
        index_slot: index,
        region_end: STATE_REGION_END,
        loop_header: LOOP_HEADER,
        loop_backedge: STATE_LOOP_BACKEDGE,
    })
}

fn select_while_state(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<ReductionSelection> {
    cfg.region_control(0, STATE_REGION_END)?;
    let i = operation_window::<STATE_REGION_END>(entries, &WHILE_OPERATIONS)?;
    while_constants(code, &i)?;
    let (state, total, index) = while_bindings(code, &i)?;
    Some(ReductionSelection {
        source: ReductionSource::StateArray {
            slot: state,
            array_pc: WHILE_ARRAY_PC,
            bound_pc: WHILE_BOUND_PC,
        },
        profile: ReductionProfile::ControlWhile,
        total_slot: total,
        index_slot: index,
        region_end: STATE_REGION_END,
        loop_header: WHILE_LOOP_HEADER,
        loop_backedge: STATE_LOOP_BACKEDGE,
    })
}

fn operation_window<const N: usize>(
    entries: &[BaselineEntry],
    operations: &[Opcode; N],
) -> Option<[Instruction; N]> {
    let instructions: [Instruction; N] = entries
        .get(..N)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    instructions
        .iter()
        .zip(operations)
        .all(|(actual, expected)| {
            actual.opcode == *expected
                || (*expected == Opcode::GetN && actual.opcode == Opcode::GetNQuickened)
        })
        .then_some(instructions)
}

fn direct_constants(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    number_constant(code, i[0], 0.0)?;
    number_constant(code, i[3], 0.0)?;
    number_constant(code, i[20], 1.0)
}

fn state_constants(code: CodeView<'_>, i: &[Instruction; STATE_REGION_END]) -> Option<()> {
    number_constant(code, i[0], 0.0)?;
    number_constant(code, i[3], 0.0)?;
    number_constant(code, i[21], 1.0)?;
    undefined_constant(code, i[2])?;
    undefined_constant(code, i[5])?;
    undefined_constant(code, i[28])
}

fn while_constants(code: CodeView<'_>, i: &[Instruction; STATE_REGION_END]) -> Option<()> {
    number_constant(code, i[0], 0.0)?;
    number_constant(code, i[2], 0.0)?;
    number_constant(code, i[20], 1.0)?;
    undefined_constant(code, i[4])?;
    undefined_constant(code, i[28])
}

fn number_constant(code: CodeView<'_>, op: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(op.b)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn undefined_constant(code: CodeView<'_>, op: Instruction) -> Option<()> {
    matches!(code.constant(op.b), Some(crate::ops::Constant::Undefined)).then_some(())
}

fn direct_bindings(code: CodeView<'_>, i: &[Instruction; REGION_END]) -> Option<()> {
    let (total, index, array) = (i[1].a, i[4].a, i[7].b);
    (i[1].b == i[0].a && i[4].b == i[3].a && i[6].b == index).then_some(())?;
    (i[7].a == i[8].b && i[8].flags == crate::ir::GETN_LENGTH_FLAG).then_some(())?;
    (i[9].b == i[6].a && i[9].c == i[8].a && i[10].a == i[9].a).then_some(())?;
    (usize::from(i[10].b) == 25 && usize::from(i[24].a) == LOOP_HEADER).then_some(())?;
    (i[11].b == total && i[12].b == array && i[14].b == index).then_some(())?;
    require_object(code, 13, i[12].a)?;
    direct_body_bindings(i, total, index)
}

fn direct_body_bindings(i: &[Instruction; REGION_END], total: u16, index: u16) -> Option<()> {
    (i[15].b == i[12].a && i[15].c == i[14].a).then_some(())?;
    (i[16].b == i[11].a && i[16].c == i[15].a).then_some(())?;
    (i[17].a == total && i[17].b == i[16].a && i[19].b == index).then_some(())?;
    (i[21].b == i[19].a && i[21].c == i[20].a).then_some(())?;
    (i[22].a == index && i[22].b == i[21].a).then_some(())?;
    (i[25].b == total && i[26].a == i[25].a).then_some(())
}

fn state_bindings(
    code: CodeView<'_>,
    i: &[Instruction; STATE_REGION_END],
) -> Option<(u16, u16, u16)> {
    let (total, index, state) = (i[1].a, i[4].a, i[7].b);
    (total != index && total != state && index != state).then_some(())?;
    (i[1].b == i[0].a && i[4].b == i[3].a && i[6].b == index).then_some(())?;
    (i[7].b == i[12].b && i[8].b == i[7].a && i[13].b == i[12].a).then_some(())?;
    named_property(code, 8, "n")?;
    named_property(code, 13, "a")?;
    state_control_bindings(i, total, index)?;
    state_body_bindings(code, i, total, index)?;
    Some((state, total, index))
}

fn state_control_bindings(
    i: &[Instruction; STATE_REGION_END],
    total: u16,
    index: u16,
) -> Option<()> {
    (i[9].b == i[6].a && i[9].c == i[8].a && i[10].a == i[9].a).then_some(())?;
    (usize::from(i[10].b) == 26 && i[11].b == total && i[15].b == index).then_some(())?;
    (i[20].b == index && i[23].a == index && i[24].b == i[20].a).then_some(())?;
    (usize::from(i[25].a) == LOOP_HEADER && i[26].b == total).then_some(())?;
    (i[27].a == i[26].a && i[29].a == i[28].a).then_some(())
}

fn state_body_bindings(
    code: CodeView<'_>,
    i: &[Instruction; STATE_REGION_END],
    total: u16,
    index: u16,
) -> Option<()> {
    require_object(code, 14, i[13].a)?;
    (i[16].b == i[13].a && i[16].c == i[15].a).then_some(())?;
    (i[17].b == i[11].a && i[17].c == i[16].a).then_some(())?;
    (i[18].a == total && i[18].b == i[17].a && i[19].b == i[17].a).then_some(())?;
    (i[22].b == i[20].a && i[22].c == i[21].a && i[23].b == i[22].a).then_some(())
}

fn while_bindings(
    code: CodeView<'_>,
    i: &[Instruction; STATE_REGION_END],
) -> Option<(u16, u16, u16)> {
    let (total, index, state) = (i[1].a, i[3].a, i[6].b);
    (total != index && total != state && index != state).then_some(())?;
    (i[1].b == i[0].a && i[3].b == i[2].a && i[5].b == index).then_some(())?;
    (i[6].b == i[11].b && i[7].b == i[6].a && i[12].b == i[11].a).then_some(())?;
    named_property(code, 7, "n")?;
    named_property(code, 12, "a")?;
    while_control_bindings(i, total, index)?;
    while_body_bindings(code, i, total, index)?;
    Some((state, total, index))
}

fn while_control_bindings(
    i: &[Instruction; STATE_REGION_END],
    total: u16,
    index: u16,
) -> Option<()> {
    (i[8].b == i[5].a && i[8].c == i[7].a && i[9].a == i[8].a).then_some(())?;
    (usize::from(i[9].b) == 26 && i[10].b == total && i[14].b == index).then_some(())?;
    (i[19].b == index && i[22].a == index && i[23].b == i[19].a).then_some(())?;
    (i[24].b == i[23].a && usize::from(i[25].a) == WHILE_LOOP_HEADER && i[26].b == total)
        .then_some(())?;
    (i[27].a == i[26].a && i[29].a == i[28].a).then_some(())
}

fn while_body_bindings(
    code: CodeView<'_>,
    i: &[Instruction; STATE_REGION_END],
    total: u16,
    _index: u16,
) -> Option<()> {
    require_object(code, 13, i[12].a)?;
    (i[15].b == i[12].a && i[15].c == i[14].a).then_some(())?;
    (i[16].b == i[10].a && i[16].c == i[15].a).then_some(())?;
    (i[17].a == total && i[17].b == i[16].a && i[18].b == i[16].a).then_some(())?;
    (i[21].b == i[19].a && i[21].c == i[20].a && i[22].b == i[21].a).then_some(())
}

fn named_property(code: CodeView<'_>, pc: usize, expected: &str) -> Option<()> {
    (code.metadata_at(pc)?.name.as_deref()? == expected).then_some(())
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source)
        .then_some(())
}
