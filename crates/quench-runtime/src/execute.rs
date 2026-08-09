//! Minimal residual-op interpreter.

use crate::{ops::Op, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    NonNumericOperand,
    MissingReturn,
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    let mut registers = Vec::new();
    for op in ops {
        match op {
            Op::Const { dst, value } => write_register(&mut registers, *dst, value),
            Op::StoreLocal { slot, src } => copy_register(&mut registers, *slot, *src)?,
            Op::LoadLocal { dst, slot } => copy_register(&mut registers, *dst, *slot)?,
            Op::Binary {
                dst,
                operator,
                lhs,
                rhs,
            } => execute_binary(&mut registers, *dst, *operator, *lhs, *rhs)?,
            Op::Return { src } => return read_register(&registers, *src),
        }
    }
    Err(VmError::MissingReturn)
}

fn copy_register(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = read_register(registers, src)?;
    write_value(registers, dst, value);
    Ok(())
}

fn execute_binary(
    registers: &mut Vec<Value>,
    dst: u16,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Result<(), VmError> {
    let Value::Number(left) = read_register(registers, lhs)? else {
        return Err(VmError::NonNumericOperand);
    };
    let Value::Number(right) = read_register(registers, rhs)? else {
        return Err(VmError::NonNumericOperand);
    };
    let value = match operator {
        crate::ops::BinaryOp::Add => left + right,
        crate::ops::BinaryOp::Subtract => left - right,
        crate::ops::BinaryOp::Multiply => left * right,
        crate::ops::BinaryOp::Divide => left / right,
        crate::ops::BinaryOp::Remainder => left % right,
        crate::ops::BinaryOp::Exponentiate => left.powf(right),
    };
    write_register(registers, dst, &crate::ops::Constant::Number(value));
    Ok(())
}

fn write_register(registers: &mut Vec<Value>, index: u16, value: &crate::ops::Constant) {
    write_value(registers, index, value.into());
}

fn write_value(registers: &mut Vec<Value>, index: u16, value: Value) {
    let index = usize::from(index);
    if registers.len() <= index {
        registers.resize(index + 1, Value::Undefined);
    }
    registers[index] = value;
}

fn read_register(registers: &[Value], index: u16) -> Result<Value, VmError> {
    registers
        .get(usize::from(index))
        .cloned()
        .ok_or(VmError::RegisterOutOfBounds(index))
}

impl From<&crate::ops::Constant> for Value {
    fn from(value: &crate::ops::Constant) -> Self {
        match value {
            crate::ops::Constant::Number(value) => Self::Number(*value),
            crate::ops::Constant::Boolean(value) => Self::Boolean(*value),
            crate::ops::Constant::String(value) => Self::String(value.clone()),
            crate::ops::Constant::Null => Self::Null,
            crate::ops::Constant::Undefined => Self::Undefined,
        }
    }
}
