//! Pure use-def selection for the ordered dense-F64 recurrence family.

use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct IndexedLoad {
    pub(super) array_slot: u16,
    pub(super) offset: i32,
}

#[derive(Clone)]
pub(super) enum NumericExpr {
    Local(u16),
    Constant(f64),
    Index { slot: u16, offset: i32 },
    Load(IndexedLoad),
    Add(Box<Self>, Box<Self>),
    Multiply(Box<Self>, f64),
    Divide(Box<Self>, f64),
}

pub(super) struct OrderedRecurrence {
    pub(super) index_slot: u16,
    pub(super) start: i32,
    pub(super) bound: Bound,
    pub(super) inclusive: bool,
    pub(super) destination: IndexedLoad,
    pub(super) value: NumericExpr,
}

#[derive(Clone, Copy)]
pub(super) enum Bound {
    Constant(i32),
    Local { slot: u16, offset: i32 },
}

pub(super) fn select(
    init: CodeView<'_>,
    test: CodeView<'_>,
    body: CodeView<'_>,
    update: CodeView<'_>,
) -> Option<OrderedRecurrence> {
    let (index_slot, start) = select_init(init)?;
    let (bound, inclusive) = select_test(test, index_slot)?;
    select_update(update, index_slot)?;
    let (destination, value) = select_body(body, index_slot)?;
    Some(OrderedRecurrence {
        index_slot,
        start,
        bound,
        inclusive,
        destination,
        value,
    })
}

pub(super) fn select_init(code: CodeView<'_>) -> Option<(u16, i32)> {
    let mut constant = None;
    let mut initialized = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadConst => select_init_constant(code, instruction, &mut constant)?,
            Opcode::InitLocal => initialized = Some((instruction.a, instruction.b)),
            Opcode::Return => {}
            opcode if opcode.is_cold_marker() && is_uninitialized_marker(code, pc) => {}
            _ => return None,
        }
    }
    let (register, value) = constant?;
    let (slot, source) = initialized?;
    (source == register).then_some((slot, value))
}

fn select_init_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    selected: &mut Option<(u16, i32)>,
) -> Option<()> {
    match code.constant(instruction.b)? {
        crate::ops::Constant::Number(value) => {
            *selected = Some((instruction.a, exact_i32_value(*value)?));
            Some(())
        }
        crate::ops::Constant::Undefined => Some(()),
        _ => None,
    }
}

fn is_uninitialized_marker(code: CodeView<'_>, pc: usize) -> bool {
    matches!(
        code.cold_at(pc),
        Some(crate::ops::Op::MarkUninitialized { .. })
    )
}

pub(super) fn select_test(code: CodeView<'_>, index_slot: u16) -> Option<(Bound, bool)> {
    let mut values = BTreeMap::new();
    let mut comparison = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked => {
                values.insert(instruction.a, NumericExpr::Local(instruction.b));
            }
            Opcode::LoadConst => {
                values.insert(
                    instruction.a,
                    NumericExpr::Constant(number(code, instruction.b)?),
                );
            }
            Opcode::Sub | Opcode::Add => apply_binary(&mut values, instruction)?,
            opcode if opcode.is_binary_family() => {
                comparison = select_comparison(&values, instruction, index_slot)
            }
            Opcode::Return => {}
            _ => return None,
        }
    }
    comparison
}

fn select_comparison(
    values: &BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
    index_slot: u16,
) -> Option<(Bound, bool)> {
    let operator = instruction.opcode.binary_operator(instruction.flags)?;
    let inclusive = match operator {
        crate::ops::BinaryOp::LessThan => false,
        crate::ops::BinaryOp::LessEqual => true,
        _ => return None,
    };
    matches!(values.get(&instruction.b)?, NumericExpr::Local(slot) if *slot == index_slot)
        .then_some(())?;
    Some((bound(values.get(&instruction.c)?)?, inclusive))
}

fn bound(value: &NumericExpr) -> Option<Bound> {
    match value {
        NumericExpr::Constant(value) => Some(Bound::Constant(exact_i32_value(*value)?)),
        NumericExpr::Local(slot) => Some(Bound::Local {
            slot: *slot,
            offset: 0,
        }),
        NumericExpr::Index { slot, offset } => Some(Bound::Local {
            slot: *slot,
            offset: *offset,
        }),
        _ => None,
    }
}

pub(super) fn select_update(code: CodeView<'_>, index_slot: u16) -> Option<()> {
    let mut found = false;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        match instruction.opcode {
            Opcode::UpdateLocal if instruction.c == index_slot && instruction.flags == 0 => {
                found = true
            }
            Opcode::Return => {}
            _ => return None,
        }
    }
    found.then_some(())
}

fn select_body(code: CodeView<'_>, index_slot: u16) -> Option<(IndexedLoad, NumericExpr)> {
    let mut values = BTreeMap::new();
    let mut store = None;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        select_body_instruction(code, instruction, pc, index_slot, &mut values, &mut store)?;
    }
    store
}

fn select_body_instruction(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    pc: usize,
    index_slot: u16,
    values: &mut BTreeMap<u16, NumericExpr>,
    store: &mut Option<(IndexedLoad, NumericExpr)>,
) -> Option<()> {
    match instruction.opcode {
        Opcode::LoadLocal | Opcode::LoadLocalChecked => {
            values.insert(instruction.a, NumericExpr::Local(instruction.b));
        }
        Opcode::LoadConst => {
            values.insert(
                instruction.a,
                NumericExpr::Constant(number(code, instruction.b)?),
            );
        }
        Opcode::Move => clone_value(values, instruction)?,
        Opcode::AddConst => apply_add_const(code, values, instruction)?,
        Opcode::Sub | Opcode::Add | Opcode::Mul | Opcode::Div => apply_binary(values, instruction)?,
        Opcode::AGetI => apply_indexed_load(values, instruction, index_slot)?,
        Opcode::ASetI => *store = Some(select_store(values, instruction, index_slot)?),
        opcode if opcode.is_cold_marker() && require_object(code, pc, values) => {}
        _ => return None,
    }
    Some(())
}

fn clone_value(
    values: &mut BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
) -> Option<()> {
    let value = values.get(&instruction.b)?.clone();
    values.insert(instruction.a, value);
    Some(())
}

fn apply_add_const(
    code: CodeView<'_>,
    values: &mut BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
) -> Option<()> {
    (!instruction.add_const_is_left()).then_some(())?;
    let source = values.get(&instruction.b)?.clone();
    let constant = number(code, instruction.c)?;
    values.insert(
        instruction.a,
        add_values(source, NumericExpr::Constant(constant))?,
    );
    Some(())
}

fn apply_binary(
    values: &mut BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
) -> Option<()> {
    let left = values.get(&instruction.b)?.clone();
    let right = values.get(&instruction.c)?.clone();
    let value = match instruction.opcode {
        Opcode::Add => NumericExpr::Add(Box::new(left), Box::new(right)),
        Opcode::Sub => add_values(left, negate_constant(right)?)?,
        Opcode::Mul => scale_value(left, right, false)?,
        Opcode::Div => scale_value(left, right, true)?,
        _ => return None,
    };
    values.insert(instruction.a, value);
    Some(())
}

fn add_values(left: NumericExpr, right: NumericExpr) -> Option<NumericExpr> {
    match (left, right) {
        (NumericExpr::Local(slot), NumericExpr::Constant(value)) => Some(NumericExpr::Index {
            slot,
            offset: exact_i32_value(value)?,
        }),
        (NumericExpr::Index { slot, offset }, NumericExpr::Constant(value)) => {
            Some(NumericExpr::Index {
                slot,
                offset: offset.checked_add(exact_i32_value(value)?)?,
            })
        }
        (left, right) => Some(NumericExpr::Add(Box::new(left), Box::new(right))),
    }
}

fn negate_constant(value: NumericExpr) -> Option<NumericExpr> {
    let NumericExpr::Constant(value) = value else {
        return None;
    };
    Some(NumericExpr::Constant(-value))
}

fn scale_value(left: NumericExpr, right: NumericExpr, divide: bool) -> Option<NumericExpr> {
    let NumericExpr::Constant(factor) = right else {
        return None;
    };
    if divide {
        (factor != 0.0).then(|| NumericExpr::Divide(Box::new(left), factor))
    } else {
        Some(NumericExpr::Multiply(Box::new(left), factor))
    }
}

fn apply_indexed_load(
    values: &mut BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
    index_slot: u16,
) -> Option<()> {
    let NumericExpr::Local(array_slot) = values.get(&instruction.b)? else {
        return None;
    };
    let offset = index_offset(values.get(&instruction.c)?, index_slot)?;
    values.insert(
        instruction.a,
        NumericExpr::Load(IndexedLoad {
            array_slot: *array_slot,
            offset,
        }),
    );
    Some(())
}

fn select_store(
    values: &BTreeMap<u16, NumericExpr>,
    instruction: crate::ir::Instruction,
    index_slot: u16,
) -> Option<(IndexedLoad, NumericExpr)> {
    let NumericExpr::Local(array_slot) = values.get(&instruction.a)? else {
        return None;
    };
    let offset = index_offset(values.get(&instruction.b)?, index_slot)?;
    let value = values.get(&instruction.c)?.clone();
    Some((
        IndexedLoad {
            array_slot: *array_slot,
            offset,
        },
        value,
    ))
}

fn index_offset(value: &NumericExpr, index_slot: u16) -> Option<i32> {
    match value {
        NumericExpr::Local(slot) if *slot == index_slot => Some(0),
        NumericExpr::Index { slot, offset } if *slot == index_slot => Some(*offset),
        _ => None,
    }
}

fn require_object(code: CodeView<'_>, pc: usize, values: &BTreeMap<u16, NumericExpr>) -> bool {
    matches!(code.cold_at(pc), Some(crate::ops::Op::RequireObjectCoercible { src })
        if matches!(values.get(src), Some(NumericExpr::Local(_))))
}

fn number(code: CodeView<'_>, id: u16) -> Option<f64> {
    let crate::ops::Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    Some(*value)
}

pub(super) fn exact_i32_value(value: f64) -> Option<i32> {
    crate::stencil_numeric_integer_selection::exact_i32(value)
}
