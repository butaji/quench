use crate::intl::tolocale::value::{
    is_finite, loose_equal, parse_float, parse_int, strict_equal, to_int32, to_number, to_string,
    type_of,
};
use crate::{ops::Op, value::Value};
use std::rc::Rc;

pub(crate) use crate::intl::tolocale::value::is_truthy;

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
    use Op::*;
    for op in ops {
        match op {
            Const { dst, value } => write_value(registers, *dst, value.into()),
            StoreLocal { slot, src } => crate::locals::store(registers, *slot, *src)?,
            LoadLocal { dst, slot } => copy_register(registers, *dst, *slot)?,
            MakeArray { dst, elements } => execute_array(registers, *dst, elements)?,
            MakeObject { dst, properties } => execute_object(registers, *dst, properties)?,
            MakeBuiltin { dst, builtin } => write_value(registers, *dst, Value::Builtin(*builtin)),
            GetProperty { .. } => crate::properties::execute_get(registers, op)?,
            GetPropertyDynamic { .. } => crate::properties::execute_get_dynamic(registers, op)?,
            SetProperty { .. } | SetPropertyDynamic { .. } => {
                crate::properties::execute_set_property(registers, op)?
            }
            DeleteProperty { .. } => crate::properties::execute_delete_property(registers, op)?,
            ForIn { .. } => crate::loops::execute_for_in(registers, op)?,
            MakeFunction { .. } => crate::functions::write_op(registers, op),
            Call { dst, callee, args } => execute_call(registers, *dst, *callee, args)?,
            Branch { .. } => crate::branch::execute_branch!(registers, op),
            Try { .. } => crate::branch::execute_try!(registers, op),
            CallMethod { .. }
            | Construct { .. }
            | Loop { .. }
            | Switch { .. }
            | Conditional { .. } => crate::branch::execute_special(registers, op)?,
            Unary { dst, operator, src } => execute_unary(registers, *dst, *operator, *src)?,
            Binary {
                dst,
                operator,
                lhs,
                rhs,
            } => execute_binary(registers, *dst, *operator, *lhs, *rhs)?,
            Return { .. } | Throw { .. } => return execute_terminal(op, registers),
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
    use Value::*;
    match value {
        Builtin(builtin) => builtin_property(*builtin, key),
        Array(values) => crate::arrays::property(values, key),
        Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or_else(
                || crate::builtins::object_method(value, key),
                |(_, value)| value.clone(),
            ),
        String(value) => string_property(value, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        Function(function) if key == "length" => Value::Number(f64::from(function.params)),
        Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        _ => Value::Undefined,
    }
}
fn builtin_property(builtin: crate::ops::Builtin, key: &str) -> Value {
    let value = crate::builtins::property(builtin, key);
    if let Value::Builtin(symbol) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(symbol) {
            return Value::String(name.to_string());
        }
    }
    value
}
fn string_property(value: &str, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "length" => return Value::Number(value.chars().count() as f64),
        "toLocaleLowerCase" => return Value::Builtin(StringToLocaleLowerCase),
        "toLocaleUpperCase" => return Value::Builtin(StringToLocaleUpperCase),
        _ => {}
    }
    if let Some(method) = crate::strings::property_method(key) {
        return Value::Builtin(method);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| value.chars().nth(index))
        .map(|character| Value::String(character.to_string()))
        .unwrap_or(Value::Undefined)
}
fn number_property(_value: f64, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "toLocaleString" => Value::Builtin(NumberToLocaleString),
        "toString" => Value::Builtin(NumberToString),
        "valueOf" => Value::Builtin(NumberValueOf),
        "toFixed" => Value::Builtin(NumberToFixed),
        "toPrecision" => Value::Builtin(NumberToPrecision),
        "toExponential" => Value::Builtin(NumberToExponential),
        _ => Value::Undefined,
    }
}
fn boolean_property(value: bool, key: &str) -> Value {
    match key {
        "toString" => Value::String(value.to_string()),
        "valueOf" => Value::Boolean(value),
        _ => Value::Undefined,
    }
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
        Value::Function(body) => crate::functions::execute(&body, &arguments)?,
        Value::BoundFunction(bound) => crate::functions::execute_bound(&bound, &arguments)?,
        Value::Builtin(builtin) => execute_builtin_with_receiver(builtin, &arguments, None)?,
        _ => return Err(VmError::NotCallable),
    };
    write_value(registers, dst, value);
    Ok(())
}
pub(crate) fn execute_builtin_with_receiver(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    use crate::ops::Builtin;
    if let Some(result) = crate::intl::tolocale::symbol::dispatch(builtin, arguments, receiver)
        .or_else(|| crate::arrays::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::intl::tolocale::dispatch(builtin, receiver, arguments))
    {
        return result;
    }
    match builtin {
        Builtin::FunctionCall | Builtin::FunctionBind | Builtin::ArrayJoin => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        Builtin::Boolean => Ok(Value::Boolean(arguments.first().is_some_and(is_truthy))),
        Builtin::Eval | Builtin::ReflectConstruct => crate::reflect::builtin(builtin, arguments),
        Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        Builtin::IsFinite => Ok(Value::Boolean(is_finite(arguments.first()))),
        Builtin::IsNaN => Ok(Value::Boolean(to_number(arguments.first()).is_nan())),
        Builtin::Number => Ok(Value::Number(to_number(arguments.first()))),
        Builtin::NumberToString => Ok(Value::String(to_string(arguments.first()))),
        Builtin::NumberValueOf => Ok(Value::Number(to_number(arguments.first()))),
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            crate::number_fmt::number_format(arguments.first(), arguments.get(1), builtin)
        }
        Builtin::Object => Ok(crate::builtins::object(arguments)),
        _ => execute_builtin_tail(builtin, arguments, receiver),
    }
}
fn execute_builtin_tail(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    use crate::ops::Builtin;
    if let Some(result) = crate::strings::execute_builtin(builtin, receiver, arguments)
        .or_else(|| crate::intl::execute(builtin, arguments, receiver))
    {
        return result;
    }
    if crate::math::is_builtin(builtin) {
        return crate::math::execute(builtin, arguments);
    }
    Ok(match builtin {
        Builtin::ObjectIs => Value::Boolean(crate::builtins::same_value(
            arguments.first(),
            arguments.get(1),
        )),
        Builtin::ObjectKeys => crate::builtins::keys(arguments.first()),
        Builtin::ObjectHasOwnProperty | Builtin::ObjectGetOwnPropertyDescriptor => {
            crate::builtins::object_special(builtin, receiver, arguments)
        }
        Builtin::ParseFloat => Value::Number(parse_float(arguments.first())),
        Builtin::ParseInt => Value::Number(parse_int(arguments)),
        Builtin::String => Value::String(to_string(arguments.first())),
        Builtin::Unescape => crate::builtins::unescape(arguments.first()),
        Builtin::MathPow => crate::builtins::math_pow(arguments),
        _ => Value::Undefined,
    })
}
pub(crate) fn copy_register(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
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
    use crate::ops::UnaryOp;
    let value = read_register(registers, src)?;
    let result = match operator {
        UnaryOp::Plus => numeric_unary(value, |number| number)?,
        UnaryOp::Minus => numeric_unary(value, |number| -number)?,
        UnaryOp::Not => Value::Boolean(!is_truthy(&value)),
        UnaryOp::Void => Value::Undefined,
        UnaryOp::Typeof => Value::String(type_of(&value).to_string()),
        UnaryOp::ToString => Value::String(to_string(Some(&value))),
        UnaryOp::Delete => Value::Boolean(true),
    };
    write_value(registers, dst, result);
    Ok(())
}
fn numeric_unary(value: Value, transform: fn(f64) -> f64) -> Result<Value, VmError> {
    Ok(Value::Number(transform(to_number(Some(&value)))))
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
    write_value(registers, dst, evaluate_binary(&left, &right, operator)?);
    Ok(())
}
fn evaluate_binary(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    use crate::ops::BinaryOp;
    Ok(match operator {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Remainder
        | BinaryOp::Exponentiate => arithmetic_value(left, right, operator),
        BinaryOp::Equal => Value::Boolean(loose_equal(left, right)),
        BinaryOp::NotEqual => Value::Boolean(!loose_equal(left, right)),
        BinaryOp::StrictEqual => Value::Boolean(strict_equal(left, right)),
        BinaryOp::StrictNotEqual => Value::Boolean(!strict_equal(left, right)),
        BinaryOp::LessThan => compare_values(left, right, |a, b| a < b)?,
        BinaryOp::LessEqual => compare_values(left, right, |a, b| a <= b)?,
        BinaryOp::GreaterThan => compare_values(left, right, |a, b| a > b)?,
        BinaryOp::GreaterEqual => compare_values(left, right, |a, b| a >= b)?,
        BinaryOp::BitwiseOr => bitwise_numbers(left, right, |a, b| a | b)?,
        BinaryOp::BitwiseXor => bitwise_numbers(left, right, |a, b| a ^ b)?,
        BinaryOp::BitwiseAnd => bitwise_numbers(left, right, |a, b| a & b)?,
        BinaryOp::Instanceof => Value::Boolean(matches!(
            (left, right),
            (Value::Object(_), Value::Function(_))
        )),
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
fn bitwise_numbers(
    left: &Value,
    right: &Value,
    operation: fn(i32, i32) -> i32,
) -> Result<Value, VmError> {
    let left = to_int32(to_number(Some(left)));
    let right = to_int32(to_number(Some(right)));
    Ok(Value::Number(f64::from(operation(left, right))))
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
