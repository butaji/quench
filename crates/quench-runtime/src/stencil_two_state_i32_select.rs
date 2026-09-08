//! Symbolic use-def admission for the two-state recurrence family.

use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum Value {
    Local(u16),
    Constant(f64),
    Sum { left: u16, right: u16 },
    MaskedSum { left: u16, right: u16, mask: i32 },
    MaskedIndex { index: u16, mask: i32 },
    Result(ResultFact),
}

#[derive(Clone, Copy, Debug)]
struct ResultFact {
    left: u16,
    right: u16,
    index: u16,
    sum_mask: i32,
    index_mask: i32,
}

#[derive(Default)]
struct BodyState {
    values: BTreeMap<u16, Value>,
    next: Option<(u16, Value)>,
    shift: Option<(u16, u16)>,
    result: Option<(u16, ResultFact)>,
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionTwoState> {
    let mut constants = BTreeMap::new();
    let mut initial = BTreeMap::new();
    let mut selected = None;
    let mut returned = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_top(code, pc, instruction, &mut constants, &mut initial, &mut selected, &mut returned)?;
    }
    finish_selection(initial, selected?, returned?)
}

fn finish_selection(
    initial: BTreeMap<u16, f64>,
    selected: TwoState,
    returned: u16,
) -> Option<FunctionTwoState> {
    (returned == selected.second_slot).then_some(())?;
    Some(FunctionTwoState {
        selected,
        first: exact_i32(*initial.get(&selected.first_slot)?)?,
        second: exact_i32(*initial.get(&selected.second_slot)?)?,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
    initial: &mut BTreeMap<u16, f64>,
    selected: &mut Option<TwoState>,
    returned: &mut Option<u16>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => track_constant(code, instruction, constants)?,
        Opcode::InitLocal => {
            initial.insert(instruction.a, *constants.get(&instruction.b)?);
        }
        Opcode::LoadLocal | Opcode::LoadLocalChecked => *returned = Some(instruction.b),
        Opcode::Return => {}
        Opcode::Slow => select_top_slow(code, pc, selected)?,
        _ => return None,
    }
    Some(())
}

fn track_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            constants.insert(instruction.a, *value);
        }
        crate::ops::Constant::Undefined => {
            constants.remove(&instruction.a);
        }
        _ => return None,
    }
    Some(())
}

fn select_top_slow(code: CodeView<'_>, pc: usize, selected: &mut Option<TwoState>) -> Option<()> {
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
        } if selected.is_none() => {
            let counted = crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
            selected.replace(select_body(body.code()?, counted, per_iteration.as_slice())?);
            Some(())
        }
        _ => None,
    }
}

fn select_body(code: CodeView<'_>, counted: crate::stencil_counted_loop::CountedLoop, per_iteration: &[u16]) -> Option<TwoState> {
    let mut state = BodyState::default();
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_body_instruction(code, pc, instruction, &mut state)?;
    }
    finish_body(counted, per_iteration, state)
}

fn finish_body(
    counted: crate::stencil_counted_loop::CountedLoop,
    per_iteration: &[u16],
    state: BodyState,
) -> Option<TwoState> {
    let (next_slot, Value::MaskedSum { left, right, mask }) = state.next? else { return None; };
    let (first_slot, shifted_from) = state.shift?;
    let (second_slot, result) = state.result?;
    (shifted_from == second_slot
        && result.left == left
        && result.right == right
        && result.sum_mask == mask)
    .then_some(())?;
    ([left, right].contains(&first_slot) && [left, right].contains(&second_slot)).then_some(())?;
    (first_slot != second_slot && result.index == counted.index_slot).then_some(())?;
    (per_iteration.len() == 2 && per_iteration.contains(&counted.index_slot) && per_iteration.contains(&next_slot)).then_some(())?;
    Some(TwoState { counted, first_slot, second_slot, sum_mask: mask, index_mask: result.index_mask })
}

fn select_body_instruction(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    state: &mut BodyState,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => load_local(instruction, state),
        Opcode::LoadConst => load_constant(code, instruction, state)?,
        Opcode::Move => copy(instruction, state)?,
        Opcode::Add => add(instruction, state)?,
        Opcode::Binary => binary(instruction, state)?,
        Opcode::InitLocal => init_next(instruction, state)?,
        Opcode::StoreLocal => store(instruction, state)?,
        Opcode::Slow if matches!(code.cold_at(pc)?, crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. }) => {}
        _ => return None,
    }
    Some(())
}

fn set(state: &mut BodyState, register: u16, value: Value) {
    state.values.insert(register, value);
}

fn load_local(instruction: crate::ir::Instruction, state: &mut BodyState) {
    let value = state
        .next
        .filter(|(slot, _)| *slot == instruction.b)
        .map_or(Value::Local(instruction.b), |(_, value)| value);
    set(state, instruction.a, value);
}

fn load_constant(code: CodeView<'_>, instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else { return None; };
    set(state, instruction.a, Value::Constant(*value));
    Some(())
}

fn copy(instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    set(state, instruction.a, *state.values.get(&instruction.b)?);
    Some(())
}

fn add(instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    let left = *state.values.get(&instruction.b)?;
    let right = *state.values.get(&instruction.c)?;
    let (Value::Local(left), Value::Local(right)) = (left, right) else { return None; };
    (left != right).then_some(())?;
    set(state, instruction.a, Value::Sum { left, right });
    Some(())
}

fn binary(instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    let operator = crate::ir::compact_binary_operator(instruction.flags)?;
    let left = *state.values.get(&instruction.b)?;
    let right = *state.values.get(&instruction.c)?;
    let value = match operator {
        crate::ops::BinaryOp::BitwiseAnd => select_and(left, right)?,
        crate::ops::BinaryOp::BitwiseXor => select_xor(left, right)?,
        _ => return None,
    };
    set(state, instruction.a, value);
    Some(())
}

fn select_and(left: Value, right: Value) -> Option<Value> {
    let (value, mask) = constant_side(left, right)?;
    let mask = exact_i32(mask)?;
    match value {
        Value::Sum { left, right } => Some(Value::MaskedSum { left, right, mask }),
        Value::Local(index) => Some(Value::MaskedIndex { index, mask }),
        _ => None,
    }
}

fn select_xor(left: Value, right: Value) -> Option<Value> {
    let (sum, index) = match (left, right) {
        (sum @ Value::MaskedSum { .. }, index @ Value::MaskedIndex { .. })
        | (index @ Value::MaskedIndex { .. }, sum @ Value::MaskedSum { .. }) => (sum, index),
        _ => return None,
    };
    let Value::MaskedSum { left, right, mask: sum_mask } = sum else { return None; };
    let Value::MaskedIndex { index, mask: index_mask } = index else { return None; };
    Some(Value::Result(ResultFact { left, right, index, sum_mask, index_mask }))
}

fn constant_side(left: Value, right: Value) -> Option<(Value, f64)> {
    match (left, right) {
        (value, Value::Constant(constant)) | (Value::Constant(constant), value) => Some((value, constant)),
        _ => None,
    }
}

fn init_next(instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    let value = *state.values.get(&instruction.b)?;
    matches!(value, Value::MaskedSum { .. }).then_some(())?;
    state.next.replace((instruction.a, value)).is_none().then_some(())
}

fn store(instruction: crate::ir::Instruction, state: &mut BodyState) -> Option<()> {
    match *state.values.get(&instruction.b)? {
        Value::Local(source) => state.shift.replace((instruction.a, source)).is_none().then_some(()),
        Value::Result(result) => state.result.replace((instruction.a, result)).is_none().then_some(()),
        _ => None,
    }
}

fn exact_i32(value: f64) -> Option<i32> {
    (value.is_finite() && value.fract() == 0.0 && value >= i32::MIN as f64 && value <= i32::MAX as f64)
        .then_some(value as i32)
}
