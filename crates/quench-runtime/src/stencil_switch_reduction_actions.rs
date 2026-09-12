//! Action-expression recognition for integer switch reductions.

use super::*;

pub(super) fn select(code: CodeView<'_>, index_slot: u16) -> Option<(u16, Action)> {
    let mut values = BTreeMap::new();
    let mut stored = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_instruction(code, pc, instruction, index_slot, &mut values, &mut stored)?;
    }
    stored
}

fn select_instruction(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    index_slot: u16,
    values: &mut BTreeMap<u16, Value>,
    stored: &mut Option<(u16, Action)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            values.insert(instruction.a, Value::Local(instruction.b));
        }
        Opcode::LoadConst => load_constant(code, instruction, values, false)?,
        Opcode::AddConst => add_constant(code, instruction, values)?,
        Opcode::Add | Opcode::Sub => add(instruction, values)?,
        opcode if opcode.is_binary_family() => binary(instruction, values, index_slot)?,
        Opcode::StoreLocal => store(instruction, values, stored)?,
        Opcode::Move => copy(instruction, values)?,
        opcode if opcode.is_cold_marker() && is_unlabelled_break(code.cold_at(pc)?) => {}
        _ => return None,
    }
    Some(())
}

fn add_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let Value::Local(slot) = values.get(&instruction.b)? else {
        return None;
    };
    let action = Action {
        kind: ActionKind::AddSigned,
        a: exact_i32_constant(code, instruction.c)?,
        b: 0,
    };
    values.insert(
        instruction.a,
        Value::Action {
            slot: *slot,
            action,
        },
    );
    Some(())
}

fn add(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (slot, mask, sign) = match (instruction.opcode, left, right) {
        (Opcode::Add, Value::Local(slot), Value::Constant(value))
        | (Opcode::Add, Value::Constant(value), Value::Local(slot)) => {
            return store_value(instruction.a, slot, ActionKind::AddSigned, value, 0, values);
        }
        (Opcode::Add, Value::Local(slot), Value::IndexMasked(mask))
        | (Opcode::Add, Value::IndexMasked(mask), Value::Local(slot)) => (slot, mask, 1),
        (Opcode::Sub, Value::Local(slot), Value::IndexMasked(mask)) => (slot, mask, -1),
        (Opcode::Sub, Value::Local(slot), Value::Constant(value)) => {
            return store_value(
                instruction.a,
                slot,
                ActionKind::AddSigned,
                value.checked_neg()?,
                0,
                values,
            );
        }
        _ => return None,
    };
    store_value(
        instruction.a,
        slot,
        ActionKind::AddIndexMasked,
        mask,
        sign,
        values,
    )
}

fn binary(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
    index_slot: u16,
) -> Option<()> {
    let operator = instruction.opcode.binary_operator(instruction.flags)?;
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let value = match (operator, left, right) {
        (crate::ops::BinaryOp::BitwiseAnd, Value::Local(slot), Value::Constant(mask))
        | (crate::ops::BinaryOp::BitwiseAnd, Value::Constant(mask), Value::Local(slot))
            if slot == index_slot =>
        {
            Value::IndexMasked(mask)
        }
        (crate::ops::BinaryOp::BitwiseXor, Value::Local(slot), Value::Constant(value))
        | (crate::ops::BinaryOp::BitwiseXor, Value::Constant(value), Value::Local(slot)) => {
            action(slot, ActionKind::Xor, value, 0)
        }
        (crate::ops::BinaryOp::ShiftLeft, Value::Local(slot), Value::Constant(count)) => {
            Value::ShiftedTotal { slot, count }
        }
        (
            crate::ops::BinaryOp::BitwiseOr,
            Value::ShiftedTotal { slot, count },
            Value::Constant(value),
        ) => action(slot, ActionKind::ShiftLeftOr, count, value),
        (crate::ops::BinaryOp::ShiftRightZeroFill, Value::Local(slot), Value::Constant(count)) => {
            action(slot, ActionKind::ShiftRightUnsigned, count, 0)
        }
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn action(slot: u16, kind: ActionKind, a: i32, b: i32) -> Value {
    Value::Action {
        slot,
        action: Action { kind, a, b },
    }
}

fn store_value(
    destination: u16,
    slot: u16,
    kind: ActionKind,
    a: i32,
    b: i32,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    values.insert(destination, action(slot, kind, a, b));
    Some(())
}

fn store(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    stored: &mut Option<(u16, Action)>,
) -> Option<()> {
    let Value::Action { slot, action } = values.get(&instruction.b)? else {
        return None;
    };
    (*slot == instruction.a && stored.is_none()).then_some(())?;
    *stored = Some((*slot, *action));
    Some(())
}
