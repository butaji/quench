//! Dataflow admission for typed-lane transform and ordered reduction loops.

use super::*;
use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Value {
    Local(u16),
    Constant(f64),
    Xor { index: u16, mask: i32 },
    Lane(Lane),
    Element { array: u16, index: u16 },
    Square { array: u16, index: u16 },
    Updated { total: u16, array: u16, index: u16 },
}

#[derive(Clone, Copy)]
struct Lane {
    index: u16,
    xor_mask: i32,
    adjustment: i32,
}

#[derive(Clone, Copy)]
struct Store {
    array: u16,
    lane: Lane,
}

pub(crate) fn select_function(code: CodeView<'_>) -> Option<FunctionTypedLane> {
    let mut constants = BTreeMap::new();
    let mut initial = None;
    let mut selected = None;
    let mut returned = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_top(
            code,
            pc,
            instruction,
            &mut constants,
            &mut initial,
            &mut selected,
            &mut returned,
        )?;
    }
    finish_selection(initial?, selected?, returned?)
}

fn finish_selection(
    (initial_slot, initial_total): (u16, f64),
    selected: TypedLane,
    returned: u16,
) -> Option<FunctionTypedLane> {
    (initial_slot == selected.total_slot && returned == selected.total_slot).then_some(())?;
    Some(FunctionTypedLane {
        selected,
        initial_total,
    })
}

fn select_top(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    constants: &mut BTreeMap<u16, f64>,
    initial: &mut Option<(u16, f64)>,
    selected: &mut Option<TypedLane>,
    returned: &mut Option<u16>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadConst => track_constant(code, instruction, constants)?,
        Opcode::InitLocal if initial.is_none() => {
            *initial = Some((instruction.a, *constants.get(&instruction.b)?));
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

fn select_top_slow(code: CodeView<'_>, pc: usize, selected: &mut Option<TypedLane>) -> Option<()> {
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
            let counted =
                crate::stencil_counted_loop::select(init.code()?, test.code()?, update.code()?)?;
            (per_iteration.as_slice() == [counted.index_slot]).then_some(())?;
            selected.replace(select_body(body.code()?, counted)?);
            Some(())
        }
        _ => None,
    }
}

fn select_body(
    code: CodeView<'_>,
    counted: crate::stencil_counted_loop::CountedLoop,
) -> Option<TypedLane> {
    let mut values = BTreeMap::new();
    let mut stored = None;
    let mut update = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_instruction(code, pc, instruction, &mut values, &mut stored, &mut update)?;
    }
    let store = stored?;
    let (total_slot, array, index) = update?;
    (array == store.array && index == store.lane.index && index == counted.index_slot)
        .then_some(())?;
    Some(TypedLane {
        counted,
        total_slot,
        array_slot: array,
        xor_mask: store.lane.xor_mask,
        adjustment: store.lane.adjustment,
    })
}

fn select_instruction(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    stored: &mut Option<Store>,
    update: &mut Option<(u16, u16, u16)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            set(values, instruction.a, Value::Local(instruction.b))
        }
        Opcode::Move => copy(values, instruction)?,
        Opcode::LoadConst => load_constant(code, instruction, values)?,
        Opcode::Binary => select_binary(instruction, values)?,
        Opcode::Sub => select_sub(instruction, values)?,
        Opcode::ASetI => select_store(instruction, values, stored)?,
        Opcode::AGetI => select_load(instruction, values)?,
        Opcode::Mul => select_square(instruction, values)?,
        Opcode::Add => select_add(instruction, values)?,
        Opcode::StoreLocal => select_total_store(instruction, values, update)?,
        Opcode::Slow
            if matches!(
                code.cold_at(pc)?,
                crate::ops::Op::RequireObjectCoercible { .. }
            ) => {}
        _ => return None,
    }
    Some(())
}

fn set(values: &mut BTreeMap<u16, Value>, register: u16, value: Value) {
    values.insert(register, value);
}

fn copy(values: &mut BTreeMap<u16, Value>, instruction: crate::ir::Instruction) -> Option<()> {
    set(values, instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn load_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    set(values, instruction.a, Value::Constant(*value));
    Some(())
}

fn exact_i32(value: Value) -> Option<i32> {
    let Value::Constant(value) = value else {
        return None;
    };
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i32::MIN as f64
        && value <= i32::MAX as f64)
        .then_some(value as i32)
}

fn select_binary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    (crate::ir::compact_binary_operator(instruction.flags)
        == Some(crate::ops::BinaryOp::BitwiseXor))
    .then_some(())?;
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (index, mask) = match (left, right) {
        (Value::Local(index), constant) | (constant, Value::Local(index)) => {
            (index, exact_i32(constant)?)
        }
        _ => return None,
    };
    set(values, instruction.a, Value::Xor { index, mask });
    Some(())
}

fn select_sub(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let Value::Xor { index, mask } = *values.get(&instruction.b)? else {
        return None;
    };
    let adjustment = exact_i32(*values.get(&instruction.c)?)?.wrapping_neg();
    set(
        values,
        instruction.a,
        Value::Lane(Lane {
            index,
            xor_mask: mask,
            adjustment,
        }),
    );
    Some(())
}

fn select_store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    stored: &mut Option<Store>,
) -> Option<()> {
    let Value::Local(array) = *values.get(&instruction.a)? else {
        return None;
    };
    let Value::Local(index) = *values.get(&instruction.b)? else {
        return None;
    };
    let Value::Lane(lane) = *values.get(&instruction.c)? else {
        return None;
    };
    (index == lane.index && stored.is_none()).then_some(())?;
    *stored = Some(Store { array, lane });
    Some(())
}

fn select_load(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let Value::Local(array) = *values.get(&instruction.b)? else {
        return None;
    };
    let Value::Local(index) = *values.get(&instruction.c)? else {
        return None;
    };
    set(values, instruction.a, Value::Element { array, index });
    Some(())
}

fn select_square(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (
        Value::Element { array, index },
        Value::Element {
            array: other,
            index: other_index,
        },
    ) = (left, right)
    else {
        return None;
    };
    (array == other && index == other_index).then_some(())?;
    set(values, instruction.a, Value::Square { array, index });
    Some(())
}

fn select_add(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    if let Some(lane) = adjusted_lane(left, right) {
        set(values, instruction.a, Value::Lane(lane));
        return Some(());
    }
    let (total, array, index) = match (left, right) {
        (Value::Local(total), Value::Square { array, index })
        | (Value::Square { array, index }, Value::Local(total)) => (total, array, index),
        _ => return None,
    };
    set(
        values,
        instruction.a,
        Value::Updated {
            total,
            array,
            index,
        },
    );
    Some(())
}

fn adjusted_lane(left: Value, right: Value) -> Option<Lane> {
    let (xor, constant) = match (left, right) {
        (xor @ Value::Xor { .. }, constant) | (constant, xor @ Value::Xor { .. }) => {
            (xor, constant)
        }
        _ => return None,
    };
    let Value::Xor { index, mask } = xor else {
        return None;
    };
    Some(Lane {
        index,
        xor_mask: mask,
        adjustment: exact_i32(constant)?,
    })
}

fn select_total_store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    update: &mut Option<(u16, u16, u16)>,
) -> Option<()> {
    let Value::Updated {
        total,
        array,
        index,
    } = *values.get(&instruction.b)?
    else {
        return None;
    };
    (instruction.a == total && update.is_none()).then_some(())?;
    *update = Some((total, array, index));
    Some(())
}
