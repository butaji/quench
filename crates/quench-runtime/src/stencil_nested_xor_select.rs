//! Pure selection of a bounded nested-loop XOR reduction family.

use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

const REQUIRED_INDEX_SET: u8 = 0b111;

#[derive(Clone, Copy)]
enum Value {
    Local(u16),
    Constant(u32),
    Indices(u8),
    Masked(u8, u32),
    Updated(u16, u8, u32),
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionNestedXor> {
    let mut constants = BTreeMap::new();
    let mut locals = Vec::new();
    let mut selected = None;
    let mut returned_slot = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_top(
            code,
            pc,
            instruction,
            &mut constants,
            &mut locals,
            &mut selected,
            &mut returned_slot,
        )?;
    }
    let reduction = selected?;
    (returned_slot? == reduction.total_slot).then_some(())?;
    let initial_total = initialized_i64(&locals, reduction.total_slot)?;
    Some(FunctionNestedXor {
        reduction,
        initial_total,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
    locals: &mut Vec<(u16, f64)>,
    selected: &mut Option<NestedXor>,
    returned_slot: &mut Option<u16>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => track_constant(code, instruction, constants)?,
        Opcode::InitLocal => locals.push((instruction.a, *constants.get(&instruction.b)?)),
        Opcode::LoadLocal | Opcode::LoadLocalChecked => *returned_slot = Some(instruction.b),
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

fn select_top_slow(code: CodeView<'_>, pc: usize, selected: &mut Option<NestedXor>) -> Option<()> {
    match code.cold_at(pc)? {
        crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => Some(()),
        operation @ crate::ops::Op::Loop {
            label: None,
            post_test: false,
            ..
        } if selected.is_none() => {
            selected.replace(select_nested(operation)?);
            Some(())
        }
        _ => None,
    }
}

fn select_nested(operation: &crate::ops::Op) -> Option<NestedXor> {
    let mut loops = Vec::with_capacity(3);
    let (total_slot, mask) = select_level(operation, 0, &mut loops)?;
    let loops: [_; 3] = loops.try_into().ok()?;
    Some(NestedXor {
        loops,
        total_slot,
        mask,
    })
}

fn select_level(
    operation: &crate::ops::Op,
    depth: usize,
    loops: &mut Vec<crate::stencil_counted_loop::CountedLoop>,
) -> Option<(u16, u32)> {
    let crate::ops::Op::Loop {
        init,
        test,
        body,
        update,
        per_iteration,
        ..
    } = operation
    else {
        return None;
    };
    let counted = crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
    (per_iteration.as_slice() == [counted.index_slot]).then_some(())?;
    loops.push(counted);
    if depth == 2 {
        return select_terminal(body.code()?, loops);
    }
    select_nested_body(body.code()?, depth + 1, loops)
}

fn select_nested_body(
    code: CodeView<'_>,
    depth: usize,
    loops: &mut Vec<crate::stencil_counted_loop::CountedLoop>,
) -> Option<(u16, u32)> {
    let mut selected = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst
                if matches!(
                    code.constant(instruction.b),
                    Some(crate::ops::Constant::Undefined)
                ) => {}
            Opcode::Move => {}
            Opcode::Slow if selected.is_none() => {
                selected = Some(select_level(code.cold_at(pc)?, depth, loops)?);
            }
            _ => return None,
        }
    }
    selected
}

fn select_terminal(
    code: CodeView<'_>,
    loops: &[crate::stencil_counted_loop::CountedLoop],
) -> Option<(u16, u32)> {
    let mut values = BTreeMap::new();
    let mut update = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        terminal_instruction(code, instruction, loops, &mut values, &mut update)?;
    }
    let (slot, indices, mask) = update?;
    (indices == REQUIRED_INDEX_SET).then_some((slot, mask))
}

fn terminal_instruction(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    loops: &[crate::stencil_counted_loop::CountedLoop],
    values: &mut BTreeMap<u16, Value>,
    update: &mut Option<(u16, u8, u32)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => load(instruction, loops, values),
        Opcode::LoadConst => load_constant(code, instruction, values)?,
        Opcode::Binary => binary(instruction, values)?,
        Opcode::Add => add(instruction, values)?,
        Opcode::StoreLocal => store(instruction, values, update)?,
        Opcode::Move => copy(instruction, values)?,
        _ => return None,
    }
    Some(())
}

fn load(
    instruction: crate::ir::Instruction,
    loops: &[crate::stencil_counted_loop::CountedLoop],
    values: &mut BTreeMap<u16, Value>,
) {
    let index = loops
        .iter()
        .position(|counted| counted.index_slot == instruction.b);
    let value = index.map_or(Value::Local(instruction.b), |index| {
        Value::Indices(1 << index)
    });
    values.insert(instruction.a, value);
}

fn load_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    let value = exact_u32(*value)?;
    values.insert(instruction.a, Value::Constant(value));
    Some(())
}

fn binary(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let value = match crate::ir::compact_binary_operator(instruction.flags)? {
        crate::ops::BinaryOp::BitwiseXor => xor(left, right)?,
        crate::ops::BinaryOp::BitwiseAnd => mask(left, right)?,
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn xor(left: Value, right: Value) -> Option<Value> {
    let (Value::Indices(left), Value::Indices(right)) = (left, right) else {
        return None;
    };
    (left & right == 0).then_some(Value::Indices(left | right))
}

fn mask(left: Value, right: Value) -> Option<Value> {
    match (left, right) {
        (Value::Indices(indices), Value::Constant(mask))
        | (Value::Constant(mask), Value::Indices(indices)) => Some(Value::Masked(indices, mask)),
        _ => None,
    }
}

fn add(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (slot, indices, mask) = match (left, right) {
        (Value::Local(slot), Value::Masked(indices, mask))
        | (Value::Masked(indices, mask), Value::Local(slot)) => (slot, indices, mask),
        _ => return None,
    };
    values.insert(instruction.a, Value::Updated(slot, indices, mask));
    Some(())
}

fn store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    update: &mut Option<(u16, u8, u32)>,
) -> Option<()> {
    let Value::Updated(slot, indices, mask) = values.get(&instruction.b)? else {
        return None;
    };
    (*slot == instruction.a && update.is_none()).then_some(())?;
    *update = Some((*slot, *indices, *mask));
    Some(())
}

fn copy(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    values.insert(instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn initialized_i64(locals: &[(u16, f64)], slot: u16) -> Option<i64> {
    let value = locals
        .iter()
        .find_map(|(local, value)| (*local == slot).then_some(*value))?;
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64)
        .then_some(value as i64)
}

fn exact_u32(value: f64) -> Option<u32> {
    (value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value <= u32::MAX as f64)
        .then_some(value as u32)
}
