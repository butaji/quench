//! Minimal residual-op interpreter.

use std::rc::Rc;

use crate::{ops::Op, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    NonNumericOperand,
    MissingReturn,
    NotCallable,
    ArgumentCount,
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_registers(ops, Vec::new())
}

fn execute_with_registers(ops: &[Op], mut registers: Vec<Value>) -> Result<Value, VmError> {
    for op in ops {
        match op {
            Op::Const { dst, value } => write_register(&mut registers, *dst, value),
            Op::StoreLocal { slot, src } => copy_register(&mut registers, *slot, *src)?,
            Op::LoadLocal { dst, slot } => copy_register(&mut registers, *dst, *slot)?,
            Op::MakeFunction { dst, body, params } => {
                write_value(
                    &mut registers,
                    *dst,
                    Value::Function(Rc::new(crate::value::FunctionValue {
                        body: body.clone(),
                        params: *params,
                    })),
                );
            }
            Op::Call { dst, callee, args } => {
                execute_call(&mut registers, *dst, *callee, args)?;
            }
            Op::Unary { dst, operator, src } => {
                execute_unary(&mut registers, *dst, *operator, *src)?
            }
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

fn execute_call(
    registers: &mut Vec<Value>,
    dst: u16,
    callee: u16,
    args: &[u16],
) -> Result<(), VmError> {
    let Value::Function(body) = read_register(registers, callee)? else {
        return Err(VmError::NotCallable);
    };
    let arguments = args
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    if arguments.len() != usize::from(body.params) {
        return Err(VmError::ArgumentCount);
    }
    let value = execute_with_registers(&body.body, arguments)?;
    write_value(registers, dst, value);
    Ok(())
}

fn copy_register(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = read_register(registers, src)?;
    write_value(registers, dst, value);
    Ok(())
}

fn execute_unary(
    registers: &mut Vec<Value>,
    dst: u16,
    operator: crate::ops::UnaryOp,
    src: u16,
) -> Result<(), VmError> {
    let value = read_register(registers, src)?;
    let result = match operator {
        crate::ops::UnaryOp::Plus => numeric_unary(value, |number| number)?,
        crate::ops::UnaryOp::Minus => numeric_unary(value, |number| -number)?,
        crate::ops::UnaryOp::Not => Value::Boolean(!is_truthy(&value)),
        crate::ops::UnaryOp::Void => Value::Undefined,
    };
    write_value(registers, dst, result);
    Ok(())
}

fn numeric_unary(value: Value, transform: fn(f64) -> f64) -> Result<Value, VmError> {
    let Value::Number(number) = value else {
        return Err(VmError::NonNumericOperand);
    };
    Ok(Value::Number(transform(number)))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Boolean(value) => *value,
        Value::Number(value) => *value != 0.0 && !value.is_nan(),
        Value::String(value) => !value.is_empty(),
        Value::Null | Value::Undefined => false,
        Value::Function(_) => true,
    }
}

fn execute_binary(
    registers: &mut Vec<Value>,
    dst: u16,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Result<(), VmError> {
    let left = read_register(registers, lhs)?;
    let right = read_register(registers, rhs)?;
    let value = evaluate_binary(&left, &right, operator)?;
    write_value(registers, dst, value);
    Ok(())
}

fn evaluate_binary(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    Ok(match operator {
        crate::ops::BinaryOp::Add
        | crate::ops::BinaryOp::Subtract
        | crate::ops::BinaryOp::Multiply
        | crate::ops::BinaryOp::Divide
        | crate::ops::BinaryOp::Remainder
        | crate::ops::BinaryOp::Exponentiate => match (&left, &right, operator) {
            (Value::String(left), Value::String(right), crate::ops::BinaryOp::Add) => {
                Value::String(format!("{left}{right}"))
            }
            (Value::Number(left), Value::Number(right), _) => {
                Value::Number(numeric_binary(*left, *right, operator))
            }
            _ => return Err(VmError::NonNumericOperand),
        },
        crate::ops::BinaryOp::Equal => Value::Boolean(left == right),
        crate::ops::BinaryOp::NotEqual => Value::Boolean(left != right),
        crate::ops::BinaryOp::StrictEqual => Value::Boolean(left == right),
        crate::ops::BinaryOp::StrictNotEqual => Value::Boolean(left != right),
        crate::ops::BinaryOp::LessThan => compare_numbers(left, right, |a, b| a < b)?,
        crate::ops::BinaryOp::LessEqual => compare_numbers(left, right, |a, b| a <= b)?,
        crate::ops::BinaryOp::GreaterThan => compare_numbers(left, right, |a, b| a > b)?,
        crate::ops::BinaryOp::GreaterEqual => compare_numbers(left, right, |a, b| a >= b)?,
    })
}

fn numeric_binary(left: f64, right: f64, operator: crate::ops::BinaryOp) -> f64 {
    match operator {
        crate::ops::BinaryOp::Add => left + right,
        crate::ops::BinaryOp::Subtract => left - right,
        crate::ops::BinaryOp::Multiply => left * right,
        crate::ops::BinaryOp::Divide => left / right,
        crate::ops::BinaryOp::Remainder => left % right,
        crate::ops::BinaryOp::Exponentiate => left.powf(right),
        _ => 0.0,
    }
}

fn compare_numbers(
    left: &Value,
    right: &Value,
    compare: fn(f64, f64) -> bool,
) -> Result<Value, VmError> {
    let (Value::Number(left), Value::Number(right)) = (left, right) else {
        return Err(VmError::NonNumericOperand);
    };
    Ok(Value::Boolean(compare(*left, *right)))
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
