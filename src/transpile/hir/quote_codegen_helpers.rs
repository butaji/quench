// Helper functions for JS semantics in Rust codegen

fn __js_is_falsy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Undefined => true,
        Value::Number(n) => *n == 0.0 || n.is_nan(),
        Value::String(s) => s.is_empty(),
        Value::Boolean(false) => true,
        _ => false,
    }
}

fn __js_is_nullish(v: &Value) -> bool {
    matches!(v, Value::Null | Value::Undefined)
}
