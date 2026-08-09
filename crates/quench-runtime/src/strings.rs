use crate::value::Value;

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
