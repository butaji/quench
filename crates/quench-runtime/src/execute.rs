//! Minimal residual-op interpreter.
use crate::{ops::Op, value::Value};
use std::rc::Rc;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
    NotCallable,
    EvalError(String),
    Thrown,
}
pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_registers(ops, Vec::new())
}
pub(crate) fn execute_with_registers(
    ops: &[Op],
    mut registers: Vec<Value>,
) -> Result<Value, VmError> {
    execute_in_place(ops, &mut registers)
}

pub(crate) fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    for op in ops {
        match op {
            Op::Const { dst, value } => write_register(registers, *dst, value),
            Op::StoreLocal { slot, src } => copy_register(registers, *slot, *src)?,
            Op::LoadLocal { dst, slot } => copy_register(registers, *dst, *slot)?,
            Op::MakeArray { dst, elements } => execute_array(registers, *dst, elements)?,
            Op::MakeObject { dst, properties } => execute_object(registers, *dst, properties)?,
            Op::MakeBuiltin { dst, builtin } => write_builtin(registers, *dst, *builtin),
            Op::GetProperty { .. } => crate::properties::execute_get(registers, op)?,
            Op::SetProperty { .. } => execute_set_property_op(registers, op)?,
            Op::MakeFunction { .. } => crate::functions::write_op(registers, op),
            Op::Call { dst, callee, args } => execute_call(registers, *dst, *callee, args)?,
            Op::CallMethod { .. }
            | Op::Construct { .. }
            | Op::Branch { .. }
            | Op::Try { .. }
            | Op::Loop { .. }
            | Op::Switch { .. }
            | Op::Conditional { .. } => crate::branch::execute_special(registers, op)?,
            Op::Unary { dst, operator, src } => execute_unary(registers, *dst, *operator, *src)?,
            Op::Binary {
                dst,
                operator,
                lhs,
                rhs,
            } => execute_binary(registers, *dst, *operator, *lhs, *rhs)?,
            Op::Return { .. } | Op::Throw { .. } => return execute_terminal(op, registers),
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
pub(crate) fn get_property(value: &Value, key: &str) -> Value {
    match value {
        Value::Builtin(builtin) => crate::builtins::property(*builtin, key),
        Value::Array(values) => array_property(values, key),
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or_else(
                || crate::builtins::object_method(value, key),
                |(_, value)| value.clone(),
            ),
        Value::String(value) => string_property(value, key),
        Value::Function(function) if key == "length" => Value::Number(f64::from(function.params)),
        Value::Function(function) => function
            .properties
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        _ => Value::Undefined,
    }
}
fn execute_set_property_op(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::SetProperty { object, key, src } = op else {
        return Err(VmError::MissingReturn);
    };
    let target = read_register(registers, *object)?.clone();
    let value = read_register(registers, *src)?.clone();
    let result = crate::builtins::set_property(target, key, value);
    write_value(registers, *object, result);
    Ok(())
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
            let mut parameters = arguments;
            parameters.resize(usize::from(body.params), Value::Undefined);
            parameters.truncate(usize::from(body.params));
            let mut captured = body.captures.as_ref().clone();
            captured.extend(parameters);
            execute_with_registers(&body.body, captured)?
        }
        Value::Builtin(builtin) => execute_builtin_with_receiver(builtin, &arguments, None)?,
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
pub(crate) fn execute_builtin_with_receiver(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::Array => Ok(crate::builtins::array(arguments)),
        crate::ops::Builtin::ArrayIsArray => Ok(Value::Boolean(matches!(
            arguments.first(),
            Some(Value::Array(_))
        ))),
        crate::ops::Builtin::ArrayMap => Ok(crate::builtins::array_map(arguments)),
        crate::ops::Builtin::FunctionCall => Ok(crate::builtins::function_call(arguments)),
        crate::ops::Builtin::Boolean => {
            Ok(Value::Boolean(arguments.first().is_some_and(is_truthy)))
        }
        crate::ops::Builtin::Eval => execute_eval(arguments),
        crate::ops::Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        crate::ops::Builtin::IsFinite => Ok(Value::Boolean(is_finite(arguments.first()))),
        crate::ops::Builtin::IsNaN => Ok(Value::Boolean(to_number(arguments.first()).is_nan())),
        crate::ops::Builtin::Number => Ok(Value::Number(to_number(arguments.first()))),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::ObjectIs => Ok(Value::Boolean(crate::builtins::same_value(
            arguments.first(),
            arguments.get(1),
        ))),
        crate::ops::Builtin::ObjectKeys => Ok(crate::builtins::keys(arguments.first())),
        crate::ops::Builtin::ObjectHasOwnProperty
        | crate::ops::Builtin::ObjectGetOwnPropertyDescriptor => Ok(
            crate::builtins::object_special(builtin, receiver, arguments),
        ),
        crate::ops::Builtin::ParseFloat => Ok(Value::Number(parse_float(arguments.first()))),
        crate::ops::Builtin::ParseInt => Ok(Value::Number(parse_int(arguments))),
        crate::ops::Builtin::String => Ok(Value::String(to_string(arguments.first()))),
        crate::ops::Builtin::Unescape => Ok(crate::builtins::unescape(arguments.first())),
        _ => Ok(Value::Undefined),
    }
}
fn is_finite(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(number)) if number.is_finite())
}
fn parse_float(value: Option<&Value>) -> f64 {
    to_string(value).trim().parse().unwrap_or(f64::NAN)
}
fn parse_int(arguments: &[Value]) -> f64 {
    let text = to_string(arguments.first()).trim().to_string();
    let radix = arguments
        .get(1)
        .map(|value| to_number(Some(value)) as i32)
        .unwrap_or(0);
    let radix = if radix == 0 { 10 } else { radix };
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let (sign, digits) = match text.strip_prefix('-') {
        Some(value) => (-1.0, value),
        None => (1.0, text.strip_prefix('+').unwrap_or(&text)),
    };
    i64::from_str_radix(digits, radix as u32)
        .map(|value| sign * value as f64)
        .unwrap_or(f64::NAN)
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
    Ok(Value::Number(transform(to_number(Some(&value)))))
}
pub(crate) fn is_truthy(value: &Value) -> bool {
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
        | crate::ops::BinaryOp::Exponentiate => arithmetic_value(left, right, operator),
        crate::ops::BinaryOp::Equal => Value::Boolean(loose_equal(left, right)),
        crate::ops::BinaryOp::NotEqual => Value::Boolean(!loose_equal(left, right)),
        crate::ops::BinaryOp::StrictEqual => Value::Boolean(strict_equal(left, right)),
        crate::ops::BinaryOp::StrictNotEqual => Value::Boolean(!strict_equal(left, right)),
        crate::ops::BinaryOp::LessThan => compare_values(left, right, |a, b| a < b)?,
        crate::ops::BinaryOp::LessEqual => compare_values(left, right, |a, b| a <= b)?,
        crate::ops::BinaryOp::GreaterThan => compare_values(left, right, |a, b| a > b)?,
        crate::ops::BinaryOp::GreaterEqual => compare_values(left, right, |a, b| a >= b)?,
        crate::ops::BinaryOp::BitwiseOr => bitwise_numbers(left, right, |a, b| a | b)?,
        crate::ops::BinaryOp::BitwiseXor => bitwise_numbers(left, right, |a, b| a ^ b)?,
        crate::ops::BinaryOp::BitwiseAnd => bitwise_numbers(left, right, |a, b| a & b)?,
    })
}
fn arithmetic_value(left: &Value, right: &Value, operator: crate::ops::BinaryOp) -> Value {
    if operator == crate::ops::BinaryOp::Add
        && (matches!(left, Value::String(_)) || matches!(right, Value::String(_)))
    {
        return Value::String(format!(
            "{}{}",
            to_string(Some(left)),
            to_string(Some(right))
        ));
    }
    let left = to_number(Some(left));
    let right = to_number(Some(right));
    Value::Number(numeric_binary(left, right, operator))
}
fn loose_equal(left: &Value, right: &Value) -> bool {
    if std::mem::discriminant(left) == std::mem::discriminant(right) {
        return strict_equal(left, right);
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
fn strict_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Function(left), Value::Function(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Number(left), Value::Number(right)) => left == right,
        (Value::Boolean(left), Value::Boolean(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        _ => false,
    }
}
fn bitwise_numbers(
    left: &Value,
    right: &Value,
    operation: fn(i32, i32) -> i32,
) -> Result<Value, VmError> {
    let left = to_int32(to_number(Some(left)));
    let right = to_int32(to_number(Some(right)));
    Ok(Value::Number(f64::from(operation(left, right))))
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
pub(crate) fn write_value(registers: &mut Vec<Value>, index: u16, value: Value) {
    let index = usize::from(index);
    if registers.len() <= index {
        registers.resize(index + 1, Value::Undefined);
    }
    registers[index] = value;
}
pub(crate) fn read_register(registers: &[Value], index: u16) -> Result<Value, VmError> {
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
