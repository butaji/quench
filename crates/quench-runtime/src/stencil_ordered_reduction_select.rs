//! Static admission for ordered dense-Number reductions.

use super::{ReductionOperation, ReductionProfile, ReductionSelection, ReductionSource};
use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};

use super::shapes::*;

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
        .or_else(|| select_predictable(code, entries, cfg))
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
        operation: ReductionOperation::Sum,
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
        operation: ReductionOperation::Sum,
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
        operation: ReductionOperation::Sum,
        total_slot: total,
        index_slot: index,
        region_end: STATE_REGION_END,
        loop_header: WHILE_LOOP_HEADER,
        loop_backedge: STATE_LOOP_BACKEDGE,
    })
}

fn select_predictable(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
) -> Option<ReductionSelection> {
    cfg.region_control(0, PREDICTABLE_REGION_END)?;
    let key = crate::stencil_select::conditional_f64_reduction_loop_region_key();
    let operations = crate::stencil_select::select_physical(key)?
        .record
        .operations;
    let i = operation_window::<PREDICTABLE_REGION_END>(entries, operations)?;
    let (threshold, on_true, on_false) = predictable_constants(code, &i)?;
    let (state, total, index) = predictable_bindings(code, &i)?;
    Some(ReductionSelection {
        source: ReductionSource::StateArray {
            slot: state,
            array_pc: PREDICTABLE_ARRAY_PC,
            bound_pc: PREDICTABLE_BOUND_PC,
        },
        profile: ReductionProfile::Conditional,
        operation: ReductionOperation::LessThan {
            threshold,
            on_true,
            on_false,
        },
        total_slot: total,
        index_slot: index,
        region_end: PREDICTABLE_REGION_END,
        loop_header: LOOP_HEADER,
        loop_backedge: PREDICTABLE_LOOP_BACKEDGE,
    })
}

fn operation_window<const N: usize>(
    entries: &[BaselineEntry],
    operations: &[Opcode],
) -> Option<[Instruction; N]> {
    (operations.len() == N).then_some(())?;
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
        .all(|(actual, expected)| expected.matches_physical_contract(actual.opcode))
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

fn predictable_constants(
    code: CodeView<'_>,
    i: &[Instruction; PREDICTABLE_REGION_END],
) -> Option<(f64, f64, f64)> {
    for (pc, value) in [(0, 0.0), (3, 0.0), (30, 1.0)] {
        number_constant(code, i[pc], value)?;
    }
    for pc in [2, 5, 37] {
        undefined_constant(code, i[pc])?
    }
    let threshold = number_value(code, i[17])?;
    let on_true = number_value(code, i[20])?;
    let on_false = -number_value(code, i[23])?;
    Some((threshold, on_true, on_false))
}

fn number_constant(code: CodeView<'_>, op: Instruction, expected: f64) -> Option<()> {
    let value = number_value(code, op)?;
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn number_value(code: CodeView<'_>, op: Instruction) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(op.b)? else {
        return None;
    };
    Some(*value)
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

fn predictable_bindings(
    code: CodeView<'_>,
    i: &[Instruction; PREDICTABLE_REGION_END],
) -> Option<(u16, u16, u16)> {
    let (total, index, state) = (i[1].a, i[4].a, i[7].b);
    (total != index && total != state && index != state).then_some(())?;
    (i[1].b == i[0].a && i[4].b == i[3].a && i[6].b == index).then_some(())?;
    (i[7].b == i[12].b && i[8].b == i[7].a && i[13].b == i[12].a).then_some(())?;
    named_property(code, PREDICTABLE_BOUND_PC, "n")?;
    named_property(code, PREDICTABLE_ARRAY_PC, "a")?;
    predictable_control(i, total, index)?;
    predictable_body(code, i, total)?;
    Some((state, total, index))
}

fn predictable_control(
    i: &[Instruction; PREDICTABLE_REGION_END],
    total: u16,
    index: u16,
) -> Option<()> {
    binary(i[9], crate::ops::BinaryOp::LessThan, i[6].a, i[8].a)?;
    (i[10].a == i[9].a && usize::from(i[10].b) == 35 && i[11].b == total).then_some(())?;
    (i[15].b == index && i[29].b == index && i[32].a == index).then_some(())?;
    binary(i[31], crate::ops::BinaryOp::NumericAdd, i[29].a, i[30].a)?;
    (i[32].b == i[31].a && i[33].b == i[29].a).then_some(())?;
    (usize::from(i[34].a) == LOOP_HEADER && i[35].b == total).then_some(())?;
    (i[36].a == i[35].a && i[38].a == i[37].a).then_some(())
}

fn predictable_body(
    code: CodeView<'_>,
    i: &[Instruction; PREDICTABLE_REGION_END],
    total: u16,
) -> Option<()> {
    require_object(code, 14, i[13].a)?;
    (i[16].b == i[13].a && i[16].c == i[15].a).then_some(())?;
    binary(i[18], crate::ops::BinaryOp::LessThan, i[16].a, i[17].a)?;
    (i[19].a == i[18].a && usize::from(i[19].b) == 23).then_some(())?;
    (i[21].b == i[20].a && usize::from(i[22].a) == 26).then_some(())?;
    (crate::ir::compact_unary_operator(i[24].flags) == Some(crate::ops::UnaryOp::Minus)
        && i[24].b == i[23].a
        && i[25].b == i[24].a)
        .then_some(())?;
    (i[26].b == i[11].a && i[26].c == i[25].a).then_some(())?;
    (i[27].a == total && i[27].b == i[26].a && i[28].b == i[26].a).then_some(())
}

fn binary(op: Instruction, expected: crate::ops::BinaryOp, left: u16, right: u16) -> Option<()> {
    (op.opcode.binary_operator(op.flags) == Some(expected) && op.b == left && op.c == right)
        .then_some(())
}

fn named_property(code: CodeView<'_>, pc: usize, expected: &str) -> Option<()> {
    (code.metadata_at(pc)?.name.as_deref()? == expected).then_some(())
}

fn require_object(code: CodeView<'_>, pc: usize, source: u16) -> Option<()> {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src }) if *src == source)
        .then_some(())
}
