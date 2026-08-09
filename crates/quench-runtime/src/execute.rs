//! Minimal residual-op interpreter.

use crate::{ops::Op, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    let mut registers = Vec::new();
    for op in ops {
        match op {
            Op::Const { dst, value } => write_register(&mut registers, *dst, value),
            Op::Return { src } => return read_register(&registers, *src),
        }
    }
    Err(VmError::MissingReturn)
}

fn write_register(registers: &mut Vec<Value>, index: u16, value: &crate::ops::Constant) {
    let index = usize::from(index);
    if registers.len() <= index {
        registers.resize(index + 1, Value::Undefined);
    }
    registers[index] = value.into();
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
            crate::ops::Constant::Null => Self::Null,
            crate::ops::Constant::Undefined => Self::Undefined,
        }
    }
}
