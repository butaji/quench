pub(crate) fn search(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else { return Value::Number(-1.0) };
    if let Some(Value::Object(pattern)) = arguments.first() {
        let pattern = Value::Object(pattern.clone());
        if let Ok(Value::Array(result)) = crate::regexp::exec(Some(&pattern), &[Value::String(value.clone())]) {
            return crate::execute::get_property_result(&Value::Array(result), "index")
                .unwrap_or_else(|_| Value::Number(-1.0));
        }
        return Value::Number(-1.0);
    }
    let pattern = arguments.first().map_or_else(String::new, to_string);
    Value::Number(value.find(&pattern).map_or(-1.0, |index| index as f64))
}

fn string_match(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(Value::String(value)) = receiver else { return Ok(Value::Null) };
    let Some(pattern) = arguments.first() else { return Ok(Value::array(vec![Value::String(value.clone())])) };
    match pattern { Value::Object(_) => crate::regexp::exec(Some(pattern), &[Value::String(value.clone())]), _ => Ok(Value::Null) }
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
