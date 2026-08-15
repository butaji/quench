pub(crate) fn date_to_locale_string(
    kind: DateLocaleKind,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let _ = arguments;
    Ok(Value::String(kind.default().to_string()))
}
pub(crate) fn string_to_locale_case(
    receiver: Option<&Value>,
    upper: bool,
) -> Result<Value, VmError> {
    let Some(Value::String(value)) = receiver else {
        return Err(runtime_error("TypeError: String.prototype.toLocale*Case"));
    };
    let result = if upper {
        value.to_uppercase()
    } else {
        value.to_lowercase()
    };
    Ok(Value::String(result))
}
