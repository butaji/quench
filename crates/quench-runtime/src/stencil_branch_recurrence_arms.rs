//! Effect-free symbolic verification of both recurrence branch arms.

use super::*;

#[derive(Clone, Copy)]
enum Value {
    Condition(bool),
    Local(u16),
    Constant(u32),
    Index,
    MaskedIndex(u32),
    Updated(u16, Delta),
}

pub(super) fn select(
    code: CodeView<'_>,
    branch_pc: usize,
    condition_register: u16,
    index_slot: u16,
    condition: bool,
) -> Option<(u16, Delta)> {
    let mut values = BTreeMap::from([(condition_register, Value::Condition(condition))]);
    let mut pc = branch_pc;
    let mut update = None;
    for _ in 0..code.len().saturating_mul(2) {
        if pc >= code.len() {
            return update;
        }
        let instruction = code.instruction(pc)?;
        pc = instruction_step(code, pc, instruction, index_slot, &mut values, &mut update)?;
    }
    None
}

fn instruction_step(
    code: CodeView<'_>,
    pc: usize,
    instruction: crate::ir::Instruction,
    index_slot: u16,
    values: &mut BTreeMap<u16, Value>,
    update: &mut Option<(u16, Delta)>,
) -> Option<usize> {
    let next = pc + 1;
    match instruction.opcode {
        Opcode::JumpIfFalse => return branch_target(instruction, next, values),
        Opcode::Jump => return Some(usize::from(instruction.a)),
        Opcode::LoadLocal | Opcode::LoadLocalChecked => load(instruction, index_slot, values),
        Opcode::LoadConst => load_constant(code, instruction, values)?,
        opcode if opcode.is_binary_family() => mask_index(instruction, values)?,
        Opcode::Add | Opcode::Sub => score_delta(instruction, values)?,
        Opcode::StoreLocal => store_score(instruction, values, update)?,
        Opcode::Move => copy(instruction, values)?,
        _ => return None,
    }
    Some(next)
}

fn branch_target(
    instruction: crate::ir::Instruction,
    next: usize,
    values: &BTreeMap<u16, Value>,
) -> Option<usize> {
    let Value::Condition(value) = values.get(&instruction.a)? else {
        return None;
    };
    Some(if *value {
        next
    } else {
        usize::from(instruction.b)
    })
}

fn load(instruction: crate::ir::Instruction, index_slot: u16, values: &mut BTreeMap<u16, Value>) {
    let value = if instruction.b == index_slot {
        Value::Index
    } else {
        Value::Local(instruction.b)
    };
    values.insert(instruction.a, value);
}

fn load_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let constant = exact_u32(number(code, instruction.b)?)?;
    values.insert(instruction.a, Value::Constant(constant));
    Some(())
}

fn copy(instruction: crate::ir::Instruction, values: &mut BTreeMap<u16, Value>) -> Option<()> {
    values.insert(instruction.a, *values.get(&instruction.b)?);
    Some(())
}

fn mask_index(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    (instruction.opcode.binary_operator(instruction.flags)? == crate::ops::BinaryOp::BitwiseAnd)
        .then_some(())?;
    let mask = match (values.get(&instruction.b)?, values.get(&instruction.c)?) {
        (Value::Index, Value::Constant(mask)) | (Value::Constant(mask), Value::Index) => *mask,
        _ => return None,
    };
    values.insert(instruction.a, Value::MaskedIndex(mask));
    Some(())
}

fn score_delta(
    instruction: crate::ir::Instruction,
    values: &mut BTreeMap<u16, Value>,
) -> Option<()> {
    let left = *values.get(&instruction.b)?;
    let right = *values.get(&instruction.c)?;
    let (slot, delta) = score_operands(instruction.opcode, left, right)?;
    let mask = match delta {
        Value::Index => u32::MAX,
        Value::MaskedIndex(mask) => mask,
        _ => return None,
    };
    let sign = if instruction.opcode == Opcode::Add {
        1
    } else {
        -1
    };
    values.insert(instruction.a, Value::Updated(slot, Delta { sign, mask }));
    Some(())
}

fn score_operands(opcode: Opcode, left: Value, right: Value) -> Option<(u16, Value)> {
    match (opcode, left, right) {
        (Opcode::Add, Value::Local(slot), delta)
        | (Opcode::Add, delta, Value::Local(slot))
        | (Opcode::Sub, Value::Local(slot), delta) => Some((slot, delta)),
        _ => None,
    }
}

fn store_score(
    instruction: crate::ir::Instruction,
    values: &BTreeMap<u16, Value>,
    update: &mut Option<(u16, Delta)>,
) -> Option<()> {
    let Value::Updated(slot, delta) = values.get(&instruction.b)? else {
        return None;
    };
    (*slot == instruction.a && update.is_none()).then_some(())?;
    *update = Some((*slot, *delta));
    Some(())
}
