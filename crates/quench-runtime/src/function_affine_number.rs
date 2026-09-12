//! Pure affine Number facts derived from canonical residual function bodies.

use crate::{ir::Opcode, machine::CodeView, ops::Constant};
use std::collections::BTreeMap;

const MAX_BODY_OPS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AffineNumberFact {
    pub(crate) multiplier: i32,
    pub(crate) addend: i32,
    pub(crate) discarded_stores: u8,
    pub(crate) requires_nonnegative: bool,
    pub(crate) eliminated_ops: u8,
}

pub(crate) fn affine_number_function(
    function: &crate::machine::FunctionCode,
    parameter_slot: u16,
) -> Option<AffineNumberFact> {
    if let Some(code) = function.code() {
        if let Some(fact) = affine_number_body(code, parameter_slot) {
            return Some(fact);
        }
        if let Some(fact) = affine_nonnegative_body(code, parameter_slot) {
            return Some(fact);
        }
    }
    let source = function.source_ops()?;
    affine_source_body(source, parameter_slot)
}

#[derive(Clone, Copy)]
struct AffineValue {
    multiplier: i32,
    addend: i32,
}

pub(crate) fn affine_number_body(
    code: CodeView<'_>,
    parameter_slot: u16,
) -> Option<AffineNumberFact> {
    (code.len() <= MAX_BODY_OPS).then_some(())?;
    let mut registers: BTreeMap<u16, AffineValue> = BTreeMap::new();
    let mut locals: BTreeMap<u16, AffineValue> = BTreeMap::new();
    let mut stores = 0_u8;
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        if instruction.opcode == Opcode::Return {
            let value = *registers.get(&instruction.a)?;
            validate_tail(code, pc)?;
            return Some(AffineNumberFact {
                multiplier: value.multiplier,
                addend: value.addend,
                discarded_stores: stores.saturating_sub(1),
                requires_nonnegative: false,
                eliminated_ops: 0,
            });
        }
        apply(
            instruction,
            code,
            parameter_slot,
            &mut registers,
            &mut locals,
        )?;
        stores = stores.saturating_add(u8::from(instruction.opcode == Opcode::StoreLocal));
    }
    None
}

fn affine_source_body(body: &[crate::ops::Op], parameter: u16) -> Option<AffineNumberFact> {
    (body.len() <= MAX_BODY_OPS).then_some(())?;
    let mut registers: BTreeMap<u16, AffineValue> = BTreeMap::new();
    let mut locals: BTreeMap<u16, AffineValue> = BTreeMap::new();
    let mut stores = 0_u8;
    for (pc, op) in body.iter().enumerate() {
        if let crate::ops::Op::Return { src } = op {
            validate_source_tail(body, pc)?;
            let value = *registers.get(src)?;
            return Some(AffineNumberFact {
                multiplier: value.multiplier,
                addend: value.addend,
                discarded_stores: stores.saturating_sub(1),
                requires_nonnegative: false,
                eliminated_ops: 0,
            });
        }
        apply_source(op, parameter, &mut registers, &mut locals)?;
        stores = stores.saturating_add(u8::from(matches!(op, crate::ops::Op::StoreLocal { .. })));
    }
    None
}

fn affine_nonnegative_body(code: CodeView<'_>, parameter: u16) -> Option<AffineNumberFact> {
    let start = nonnegative_false_target(code, parameter)?;
    let mut registers: BTreeMap<u16, AffineValue> = BTreeMap::new();
    let mut locals: BTreeMap<u16, AffineValue> = BTreeMap::new();
    for pc in start..code.len() {
        let instruction = code.instruction(pc)?;
        if instruction.opcode == Opcode::Return {
            let value = *registers.get(&instruction.a)?;
            validate_tail(code, pc)?;
            return Some(AffineNumberFact {
                multiplier: value.multiplier,
                addend: value.addend,
                discarded_stores: 0,
                requires_nonnegative: true,
                eliminated_ops: u8::try_from(start.saturating_sub(5)).ok()?,
            });
        }
        apply(instruction, code, parameter, &mut registers, &mut locals)?;
    }
    None
}

fn nonnegative_false_target(code: CodeView<'_>, parameter: u16) -> Option<usize> {
    let load = code.instruction(0)?;
    let zero = code.instruction(1)?;
    let compare = code.instruction(2)?;
    let scratch = code.instruction(3)?;
    let branch = code.instruction(4)?;
    (load.opcode == Opcode::LoadLocal && load.b == parameter).then_some(())?;
    matches!(code.constant(zero.b), Some(Constant::Number(value)) if *value == 0.0).then_some(())?;
    (zero.opcode == Opcode::LoadConst && compare.b == load.a && compare.c == zero.a)
        .then_some(())?;
    (compare.opcode.binary_operator(compare.flags) == Some(crate::ops::BinaryOp::LessThan))
        .then_some(())?;
    matches!(code.constant(scratch.b), Some(Constant::Undefined)).then_some(())?;
    (scratch.opcode == Opcode::LoadConst
        && branch.opcode == Opcode::JumpIfFalse
        && branch.a == compare.a)
        .then_some(())?;
    Some(usize::from(branch.b))
}

fn apply_source(
    op: &crate::ops::Op,
    parameter: u16,
    registers: &mut BTreeMap<u16, AffineValue>,
    locals: &mut BTreeMap<u16, AffineValue>,
) -> Option<()> {
    use crate::ops::Op;
    let (dst, value) = match op {
        Op::LoadLocal { dst, slot } if *slot == parameter => (*dst, AffineValue::input()),
        Op::LoadLocal { dst, slot } => (*dst, *locals.get(slot)?),
        Op::Const {
            dst,
            value: Constant::Number(value),
        } => (*dst, AffineValue::constant(f64_i32(*value)?)),
        Op::Binary {
            dst,
            operator: crate::ops::BinaryOp::Add,
            lhs,
            rhs,
        } => (*dst, registers.get(lhs)?.checked_add(*registers.get(rhs)?)?),
        Op::StoreLocal { slot, src } => {
            locals.insert(*slot, *registers.get(src)?);
            return Some(());
        }
        _ => return None,
    };
    registers.insert(dst, value);
    Some(())
}

fn validate_source_tail(body: &[crate::ops::Op], return_pc: usize) -> Option<()> {
    let crate::ops::Op::Const {
        dst,
        value: Constant::Undefined,
    } = body.get(return_pc + 1)?
    else {
        return None;
    };
    matches!(body.get(return_pc + 2), Some(crate::ops::Op::Return { src }) if src == dst)
        .then_some(())?;
    (return_pc + 3 == body.len()).then_some(())
}

fn apply(
    instruction: crate::ir::Instruction,
    code: CodeView<'_>,
    parameter: u16,
    registers: &mut BTreeMap<u16, AffineValue>,
    locals: &mut BTreeMap<u16, AffineValue>,
) -> Option<()> {
    let value = match instruction.opcode {
        Opcode::LoadLocal if instruction.b == parameter => AffineValue::input(),
        Opcode::LoadLocal => *locals.get(&instruction.b)?,
        Opcode::LoadConst => AffineValue::constant(number_i32(code, instruction.b)?),
        Opcode::Move => *registers.get(&instruction.b)?,
        Opcode::AddConst => add_constant(code, instruction, registers)?,
        Opcode::StoreLocal => {
            locals.insert(instruction.a, *registers.get(&instruction.b)?);
            return Some(());
        }
        _ => return None,
    };
    registers.insert(instruction.a, value);
    Some(())
}

fn add_constant(
    code: CodeView<'_>,
    instruction: crate::ir::Instruction,
    registers: &BTreeMap<u16, AffineValue>,
) -> Option<AffineValue> {
    let source = *registers.get(&instruction.b)?;
    let constant = number_i32(code, instruction.c)?;
    Some(AffineValue {
        multiplier: source.multiplier,
        addend: source.addend.checked_add(constant)?,
    })
}

fn number_i32(code: CodeView<'_>, id: u16) -> Option<i32> {
    let Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    f64_i32(*value)
}

fn f64_i32(value: f64) -> Option<i32> {
    let integer = value as i32;
    (f64::from(integer) == value).then_some(integer)
}

fn validate_tail(code: CodeView<'_>, return_pc: usize) -> Option<()> {
    let load = code.instruction(return_pc.checked_add(1)?)?;
    let ret = code.instruction(return_pc.checked_add(2)?)?;
    (return_pc + 3 == code.len()
        && load.opcode == Opcode::LoadConst
        && matches!(code.constant(load.b), Some(Constant::Undefined))
        && ret == crate::ir::Instruction::ret(load.a))
    .then_some(())
}

impl AffineValue {
    const fn input() -> Self {
        Self {
            multiplier: 1,
            addend: 0,
        }
    }

    const fn constant(addend: i32) -> Self {
        Self {
            multiplier: 0,
            addend,
        }
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            multiplier: self.multiplier.checked_add(other.multiplier)?,
            addend: self.addend.checked_add(other.addend)?,
        })
    }
}
