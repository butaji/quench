//! Minimal residual-op interpreter.

use std::rc::Rc;

use crate::{ops::Op, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    NonNumericOperand,
    MissingReturn,
    NotCallable,
    EvalError(String),
    Thrown,
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
            Op::MakeArray { dst, elements } => execute_array(&mut registers, *dst, elements)?,
            Op::MakeObject { dst, properties } => execute_object(&mut registers, *dst, properties)?,
            Op::MakeBuiltin { dst, builtin } => write_builtin(&mut registers, *dst, *builtin),
            Op::GetProperty { dst, object, key } => {
                let value = get_property(&read_register(&registers, *object)?, key);
                write_value(&mut registers, *dst, value);
            }
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
            Op::Return { .. } | Op::Throw { .. } => return execute_terminal(op, &registers),
        }
    }
    Err(VmError::MissingReturn)
}

fn execute_terminal(op: &Op, registers: &[Value]) -> Result<Value, VmError> {
    match op {
        Op::Return { src } => read_register(registers, *src),
        Op::Throw { .. } => Err(VmError::Thrown),
        _ => Err(VmError::MissingReturn),
    }
}

fn execute_array(registers: &mut Vec<Value>, dst: u16, elements: &[u16]) -> Result<(), VmError> {
    let values = elements
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    write_value(registers, dst, Value::Array(Rc::new(values)));
    Ok(())
}

fn write_builtin(registers: &mut Vec<Value>, dst: u16, builtin: crate::ops::Builtin) {
    write_value(registers, dst, Value::Builtin(builtin));
}

fn execute_object(
    registers: &mut Vec<Value>,
    dst: u16,
    properties: &[(String, u16)],
) -> Result<(), VmError> {
    let values = properties
        .iter()
        .map(|(key, index)| Ok((key.clone(), read_register(registers, *index)?)))
        .collect::<Result<Vec<_>, VmError>>()?;
    write_value(registers, dst, Value::Object(Rc::new(values)));
    Ok(())
}

fn get_property(value: &Value, key: &str) -> Value {
    match value {
        Value::Array(values) => array_property(values, key),
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        Value::String(value) => string_property(value, key),
        Value::Function(function) if key == "length" => Value::Number(f64::from(function.params)),
        _ => Value::Undefined,
    }
}

fn array_property(values: &[Value], key: &str) -> Value {
    if key == "length" {
        return Value::Number(values.len() as f64);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get(index).cloned())
        .unwrap_or(Value::Undefined)
}

fn string_property(value: &str, key: &str) -> Value {
    if key == "length" {
        return Value::Number(value.chars().count() as f64);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| value.chars().nth(index))
        .map(|character| Value::String(character.to_string()))
        .unwrap_or(Value::Undefined)
}

fn execute_call(
    registers: &mut Vec<Value>,
    dst: u16,
    callee: u16,
    args: &[u16],
) -> Result<(), VmError> {
    let arguments = args
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let value = match read_register(registers, callee)? {
        Value::Function(body) => {
            let mut arguments = arguments;
            arguments.resize(usize::from(body.params), Value::Undefined);
            arguments.truncate(usize::from(body.params));
            execute_with_registers(&body.body, arguments)?
        }
        Value::Builtin(builtin) => execute_builtin(builtin, &arguments)?,
        _ => return Err(VmError::NotCallable),
    };
    write_value(registers, dst, value);
    Ok(())
}

fn execute_eval(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let program = crate::reduce::reduce_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    execute(&program.ops).map_err(|error| VmError::EvalError(format!("{error:?}")))
}

fn execute_builtin(builtin: crate::ops::Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::Boolean => {
            Ok(Value::Boolean(arguments.first().is_some_and(is_truthy)))
        }
        crate::ops::Builtin::Eval => execute_eval(arguments),
        crate::ops::Builtin::Number => Ok(Value::Number(to_number(arguments.first()))),
        crate::ops::Builtin::String => Ok(Value::String(to_string(arguments.first()))),
    }
}

fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Undefined) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Boolean(value)) => f64::from(*value),
        Some(Value::Number(value)) => *value,
        Some(Value::String(value)) => parse_number(value),
        Some(Value::Array(_)) | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Function(_)) | Some(Value::Builtin(_)) => f64::NAN,
    }
}

fn parse_number(value: &str) -> f64 {
    let value = value.trim();
    if value.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0b", 2), ("0o", 8), ("0x", 16)] {
        if let Some(digits) = value.strip_prefix(prefix) {
            return i64::from_str_radix(digits, radix)
                .map(|number| number as f64)
                .unwrap_or(f64::NAN);
        }
    }
    value.parse().unwrap_or(f64::NAN)
}

fn to_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Undefined) => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Boolean(value)) => value.to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::Null | Value::Undefined => String::new(),
                _ => to_string(Some(value)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
        Some(Value::Function(_)) | Some(Value::Builtin(_)) => "function".to_string(),
    }
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
        crate::ops::UnaryOp::Typeof => Value::String(type_of(&value).to_string()),
        crate::ops::UnaryOp::ToString => Value::String(to_string(Some(&value))),
    };
    write_value(registers, dst, result);
    Ok(())
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "object",
        Value::Object(_) => "object",
        Value::Builtin(_) => "function",
        Value::Function(_) => "function",
    }
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
        Value::Array(_) => true,
        Value::Object(_) => true,
        Value::Builtin(_) => true,
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
        crate::ops::BinaryOp::Equal => Value::Boolean(loose_equal(left, right)),
        crate::ops::BinaryOp::NotEqual => Value::Boolean(!loose_equal(left, right)),
        crate::ops::BinaryOp::StrictEqual => Value::Boolean(left == right),
        crate::ops::BinaryOp::StrictNotEqual => Value::Boolean(left != right),
        crate::ops::BinaryOp::LessThan => compare_values(left, right, |a, b| a < b)?,
        crate::ops::BinaryOp::LessEqual => compare_values(left, right, |a, b| a <= b)?,
        crate::ops::BinaryOp::GreaterThan => compare_values(left, right, |a, b| a > b)?,
        crate::ops::BinaryOp::GreaterEqual => compare_values(left, right, |a, b| a >= b)?,
        crate::ops::BinaryOp::BitwiseOr => bitwise_numbers(left, right, |a, b| a | b)?,
        crate::ops::BinaryOp::BitwiseXor => bitwise_numbers(left, right, |a, b| a ^ b)?,
        crate::ops::BinaryOp::BitwiseAnd => bitwise_numbers(left, right, |a, b| a & b)?,
    })
}

fn loose_equal(left: &Value, right: &Value) -> bool {
    if std::mem::discriminant(left) == std::mem::discriminant(right) {
        return left == right;
    }
    if matches!(
        (left, right),
        (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
    ) {
        return true;
    }
    if matches!(left, Value::Boolean(_)) || matches!(right, Value::Boolean(_)) {
        return to_number(Some(left)) == to_number(Some(right));
    }
    if matches!(
        (left, right),
        (Value::Number(_), Value::String(_)) | (Value::String(_), Value::Number(_))
    ) {
        return to_number(Some(left)) == to_number(Some(right));
    }
    false
}

fn bitwise_numbers(
    left: &Value,
    right: &Value,
    operation: fn(i32, i32) -> i32,
) -> Result<Value, VmError> {
    let (Value::Number(left), Value::Number(right)) = (left, right) else {
        return Err(VmError::NonNumericOperand);
    };
    Ok(Value::Number(f64::from(operation(
        to_int32(*left),
        to_int32(*right),
    ))))
}

fn to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let truncated = value.trunc() % 4_294_967_296.0;
    let wrapped = if truncated < 0.0 {
        truncated + 4_294_967_296.0
    } else {
        truncated
    };
    if wrapped >= 2_147_483_648.0 {
        (wrapped - 4_294_967_296.0) as i32
    } else {
        wrapped as i32
    }
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

fn compare_values(
    left: &Value,
    right: &Value,
    compare: fn(f64, f64) -> bool,
) -> Result<Value, VmError> {
    if let (Value::String(left), Value::String(right)) = (left, right) {
        return Ok(Value::Boolean(compare_strings(left, right, compare)));
    }
    let left = to_number(Some(left));
    let right = to_number(Some(right));
    Ok(Value::Boolean(
        !left.is_nan() && !right.is_nan() && compare(left, right),
    ))
}

fn compare_strings(left: &str, right: &str, compare: fn(f64, f64) -> bool) -> bool {
    let ordering = left.cmp(right);
    match ordering {
        std::cmp::Ordering::Less => compare(0.0, 1.0),
        std::cmp::Ordering::Equal => compare(0.0, 0.0),
        std::cmp::Ordering::Greater => compare(1.0, 0.0),
    }
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
