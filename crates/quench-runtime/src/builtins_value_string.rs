fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}
fn set_promise_property(target: &Value, key: &str, value: Value) -> Option<Value> {
    let Value::Promise(promise) = target else {
        return None;
    };
    promise.set_property(key, value);
    Some(target.clone())
}
