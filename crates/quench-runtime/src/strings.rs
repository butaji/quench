use crate::value::Value;

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::StringIncludes => includes(receiver, arguments),
        crate::ops::Builtin::StringStartsWith => starts_with(receiver, arguments),
        crate::ops::Builtin::StringEndsWith => ends_with(receiver, arguments),
        crate::ops::Builtin::StringRepeat => repeat(receiver, arguments),
        crate::ops::Builtin::StringTrim => trim(receiver),
        crate::ops::Builtin::StringToLowerCase => to_lower_case(receiver),
        crate::ops::Builtin::StringToUpperCase => to_upper_case(receiver),
        crate::ops::Builtin::StringCharAt => char_at(receiver, arguments),
        _ => return None,
    };
    Some(Ok(result))
}

pub(crate) fn includes(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.contains(&argument(arguments)))
}

pub(crate) fn starts_with(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.starts_with(&argument(arguments)))
}

pub(crate) fn ends_with(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Boolean(false);
    };
    Value::Boolean(value.ends_with(&argument(arguments)))
}

pub(crate) fn repeat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let count = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Value::String(value.repeat(count))
}

pub(crate) fn trim(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.trim().to_string())
}

pub(crate) fn to_lower_case(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.to_lowercase())
}

pub(crate) fn to_upper_case(receiver: Option<&Value>) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    Value::String(value.to_uppercase())
}

pub(crate) fn char_at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::String(String::new());
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    Value::String(
        value
            .chars()
            .nth(index)
            .map_or_else(String::new, |c| c.to_string()),
    )
}

fn argument(arguments: &[Value]) -> String {
    arguments.first().map_or_else(String::new, to_string)
}

fn to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}
