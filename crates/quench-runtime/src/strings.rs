use crate::value::Value;

pub(crate) fn property_method(key: &str) -> Option<crate::ops::Builtin> {
    match key {
        "includes" => Some(crate::ops::Builtin::StringIncludes),
        "startsWith" => Some(crate::ops::Builtin::StringStartsWith),
        "endsWith" => Some(crate::ops::Builtin::StringEndsWith),
        "repeat" => Some(crate::ops::Builtin::StringRepeat),
        "trim" => Some(crate::ops::Builtin::StringTrim),
        "toLowerCase" => Some(crate::ops::Builtin::StringToLowerCase),
        "toUpperCase" => Some(crate::ops::Builtin::StringToUpperCase),
        "charAt" => Some(crate::ops::Builtin::StringCharAt),
        "charCodeAt" => Some(crate::ops::Builtin::StringCharCodeAt),
        "indexOf" => Some(crate::ops::Builtin::StringIndexOf),
        "lastIndexOf" => Some(crate::ops::Builtin::StringLastIndexOf),
        _ => None,
    }
}

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
        crate::ops::Builtin::StringCharCodeAt => char_code_at(receiver, arguments),
        crate::ops::Builtin::StringIndexOf => index_of(receiver, arguments),
        crate::ops::Builtin::StringLastIndexOf => last_index_of(receiver, arguments),
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

pub(crate) fn char_code_at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let index = arguments.first().and_then(number).unwrap_or(0.0).max(0.0) as usize;
    value
        .chars()
        .nth(index)
        .map_or(Value::Number(f64::NAN), |value| {
            Value::Number(value as u32 as f64)
        })
}

pub(crate) fn index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(-1.0);
    };
    let search = argument(arguments);
    Value::Number(value.find(&search).map_or(-1.0, |index| index as f64))
}

pub(crate) fn last_index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else {
        return Value::Number(-1.0);
    };
    let search = argument(arguments);
    Value::Number(value.rfind(&search).map_or(-1.0, |index| index as f64))
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
