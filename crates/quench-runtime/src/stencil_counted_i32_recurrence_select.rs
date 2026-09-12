use super::*;
use crate::ir::{Instruction, Opcode};
use crate::machine::CodeView;
use crate::stencil_plan::NumericSource;
use crate::stencil_value_graph::{ValueDefinition, ValueGraph, ValueId};
use std::collections::BTreeMap;

const MAX_BODY_VALUES: usize = 24;

#[derive(Clone, Copy)]
enum TopValue {
    Number(f64),
    Local(u16),
    UnsignedLocal(u16),
    Other,
}

#[derive(Default)]
struct TopState {
    values: BTreeMap<u16, TopValue>,
    initial: BTreeMap<u16, i32>,
    loop_value: Option<(crate::stencil_counted_loop::CountedLoop, u16, u32, i32)>,
    returned: Option<(u16, ResultRepresentation)>,
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<CountedI32Recurrence> {
    let mut state = TopState::default();
    for pc in 0..code.len() {
        select_top(code, pc, code.instruction(pc)?, &mut state)?;
    }
    let (counted, state_slot, shift, multiplier) = state.loop_value?;
    let (returned, result) = state.returned?;
    (returned == state_slot && state_slot != counted.index_slot).then_some(())?;
    Some(CountedI32Recurrence {
        counted,
        initial: *state.initial.get(&state_slot)?,
        shift,
        multiplier,
        result,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: Instruction,
    state: &mut TopState,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => load_top_constant(code, instruction, state)?,
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            state
                .values
                .insert(instruction.a, TopValue::Local(instruction.b));
        }
        Opcode::InitLocal => initialize_top(instruction, state)?,
        opcode if opcode.is_binary_family() => select_top_binary(instruction, state)?,
        Opcode::Return => select_return(instruction, state),
        opcode if opcode.is_cold_marker() => select_top_slow(code, pc, state)?,
        _ => return None,
    }
    Some(())
}

fn load_top_constant(
    code: CodeView<'_>,
    instruction: Instruction,
    state: &mut TopState,
) -> Option<()> {
    let value = match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => TopValue::Number(*value),
        crate::ops::Constant::Undefined => TopValue::Other,
        _ => return None,
    };
    state.values.insert(instruction.a, value);
    Some(())
}

fn initialize_top(instruction: Instruction, state: &mut TopState) -> Option<()> {
    let TopValue::Number(value) = *state.values.get(&instruction.b)? else {
        return None;
    };
    state.initial.insert(instruction.a, exact_i32(value)?);
    Some(())
}

fn select_top_binary(instruction: Instruction, state: &mut TopState) -> Option<()> {
    let operator = instruction.opcode.binary_operator(instruction.flags)?;
    let left = *state.values.get(&instruction.b)?;
    let right = *state.values.get(&instruction.c)?;
    let result = match (operator, left, right) {
        (
            crate::ops::BinaryOp::ShiftRightZeroFill,
            TopValue::Local(slot),
            TopValue::Number(0.0),
        ) => TopValue::UnsignedLocal(slot),
        _ => return None,
    };
    state.values.insert(instruction.a, result);
    Some(())
}

fn select_return(instruction: Instruction, state: &mut TopState) {
    let returned = match state.values.get(&instruction.a) {
        Some(TopValue::Local(slot)) => Some((*slot, ResultRepresentation::I32)),
        Some(TopValue::UnsignedLocal(slot)) => Some((*slot, ResultRepresentation::U32)),
        _ => None,
    };
    if returned.is_some() {
        state.returned = returned;
    }
}

fn select_top_slow(code: CodeView<'_>, pc: usize, state: &mut TopState) -> Option<()> {
    match code.cold_at(pc)? {
        crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => Some(()),
        crate::ops::Op::Loop {
            label: None,
            init,
            test,
            body,
            update,
            post_test: false,
            per_iteration,
            ..
        } if state.loop_value.is_none() => {
            let counted =
                crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
            (per_iteration.as_slice() == [counted.index_slot]).then_some(())?;
            let (slot, shift, multiplier) = select_body(body.code()?)?;
            state.loop_value = Some((counted, slot, shift, multiplier));
            Some(())
        }
        _ => None,
    }
}

fn select_body(code: CodeView<'_>) -> Option<(u16, u32, i32)> {
    let mut graph = ValueGraph::<MAX_BODY_VALUES>::new();
    let mut global = None;
    let mut callee = None;
    let mut stored = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_body_op(
            code,
            pc,
            instruction,
            &mut graph,
            &mut global,
            &mut callee,
            &mut stored,
        )?;
    }
    select_recurrence(&graph, stored?)
}

fn select_body_op(
    code: CodeView<'_>,
    pc: usize,
    instruction: Instruction,
    graph: &mut ValueGraph<MAX_BODY_VALUES>,
    global: &mut Option<u16>,
    callee: &mut Option<u16>,
    stored: &mut Option<(u16, ValueId)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocalChecked => graph
            .push_guarded_local(instruction.a, instruction.b)
            .then_some(())?,
        Opcode::LoadLocal | Opcode::LoadConst | Opcode::Move => {
            graph
                .push(instruction, |constant| number_bits(code, constant))
                .then_some(())?;
        }
        opcode if opcode.is_binary_family() => graph.push_i32_binary(instruction).then_some(())?,
        Opcode::GetN => select_intrinsic_name(code, pc, instruction, global, callee)?,
        Opcode::CallN => select_intrinsic_call(code, pc, instruction, graph, *global, *callee)?,
        Opcode::StoreLocal => {
            stored.replace((instruction.a, graph.current(instruction.b)?));
        }
        _ => return None,
    }
    Some(())
}

fn select_intrinsic_name(
    code: CodeView<'_>,
    pc: usize,
    instruction: Instruction,
    global: &mut Option<u16>,
    callee: &mut Option<u16>,
) -> Option<()> {
    match code.metadata_at(pc)?.name.as_deref()? {
        "Math" if instruction.flags != 0 => *global = Some(instruction.a),
        "imul" if Some(instruction.b) == *global => *callee = Some(instruction.a),
        _ => return None,
    }
    Some(())
}

fn select_intrinsic_call(
    code: CodeView<'_>,
    pc: usize,
    instruction: Instruction,
    graph: &mut ValueGraph<MAX_BODY_VALUES>,
    global: Option<u16>,
    callee: Option<u16>,
) -> Option<()> {
    (instruction.flags == 2 && Some(instruction.b) == global && Some(instruction.c) == callee)
        .then_some(())?;
    let inputs: [u16; 2] = code.operand_window_at(pc)?.try_into().ok()?;
    graph
        .push_intrinsic_binary(instruction.a, crate::ops::Builtin::MathImul, inputs)
        .then_some(())
}

fn select_recurrence(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    stored: (u16, ValueId),
) -> Option<(u16, u32, i32)> {
    let value = strip_i32_coercion(graph, stored.1)?;
    let ValueDefinition::IntrinsicBinary { builtin, lhs, rhs } = graph.node(value)?.definition
    else {
        return None;
    };
    (builtin == crate::ops::Builtin::MathImul).then_some(())?;
    let (mixed, multiplier) = constant_side(graph, lhs, rhs)?;
    let (state_slot, shift) = select_xor_shift(graph, mixed)?;
    (stored.0 == state_slot).then_some((state_slot, shift, multiplier))
}

fn strip_i32_coercion(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<ValueId> {
    let id = graph.canonical(id)?;
    let ValueDefinition::Binary { operator, lhs, rhs } = graph.node(id)?.definition else {
        return Some(id);
    };
    if operator != crate::ops::BinaryOp::BitwiseOr {
        return Some(id);
    }
    zero_side(graph, lhs, rhs)
}

fn select_xor_shift(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<(u16, u32)> {
    let ValueDefinition::Binary { operator, lhs, rhs } =
        graph.node(graph.canonical(id)?)?.definition
    else {
        return None;
    };
    (operator == crate::ops::BinaryOp::BitwiseXor).then_some(())?;
    select_state_shift(graph, lhs, rhs).or_else(|| select_state_shift(graph, rhs, lhs))
}

fn select_state_shift(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    state: ValueId,
    shifted: ValueId,
) -> Option<(u16, u32)> {
    let state_slot = local_source(graph, state)?;
    let ValueDefinition::Binary { operator, lhs, rhs } =
        graph.node(graph.canonical(shifted)?)?.definition
    else {
        return None;
    };
    (operator == crate::ops::BinaryOp::ShiftRightZeroFill).then_some(())?;
    (local_source(graph, lhs)? == state_slot).then_some(())?;
    Some((state_slot, shift_count(constant(graph, rhs)?)))
}

fn constant_side(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    left: ValueId,
    right: ValueId,
) -> Option<(ValueId, i32)> {
    constant(graph, right)
        .map(crate::vm::numeric_to_int32)
        .map(|value| (left, value))
        .or_else(|| {
            constant(graph, left)
                .map(crate::vm::numeric_to_int32)
                .map(|value| (right, value))
        })
}

fn zero_side(
    graph: &ValueGraph<MAX_BODY_VALUES>,
    left: ValueId,
    right: ValueId,
) -> Option<ValueId> {
    if constant(graph, right).is_some_and(|value| value.to_bits() == 0.0f64.to_bits()) {
        return graph.canonical(left);
    }
    constant(graph, left)
        .is_some_and(|value| value.to_bits() == 0.0f64.to_bits())
        .then(|| graph.canonical(right))
        .flatten()
}

fn local_source(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<u16> {
    match graph.node(graph.canonical(id)?)?.definition {
        ValueDefinition::Source(NumericSource::Local(slot)) => Some(slot),
        _ => None,
    }
}

fn constant(graph: &ValueGraph<MAX_BODY_VALUES>, id: ValueId) -> Option<f64> {
    let NumericSource::Constant(bits) = graph.resolve(id)? else {
        return None;
    };
    Some(f64::from_bits(bits))
}

fn number_bits(code: CodeView<'_>, constant: u16) -> Option<u64> {
    let crate::ops::Constant::Number(value) = code.constant(constant)? else {
        return None;
    };
    Some(value.to_bits())
}

fn exact_i32(value: f64) -> Option<i32> {
    crate::stencil_numeric_integer_selection::exact_i32(value)
}

fn shift_count(value: f64) -> u32 {
    (crate::vm::numeric_to_int32(value) as u32) & 31
}
