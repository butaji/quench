fn define_string_length(
    boxed: &Value,
    value: &Value,
    constructor: crate::ops::Builtin,
) -> Result<Value, crate::execute::VmError> {
    if constructor != crate::ops::Builtin::String {
        return Ok(boxed.clone());
    }
    let Value::String(text) = value else {
        return Ok(boxed.clone());
    };
    crate::builtins::define_own_property(
        boxed,
        "length",
        &[
            (
                "value".to_string(),
                Value::Number(crate::strings::utf16_len(text) as f64),
            ),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(false)),
        ],
    )
}
