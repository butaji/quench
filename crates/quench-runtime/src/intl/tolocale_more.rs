pub(crate) fn date_to_locale_string(
    kind: DateLocaleKind,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let _ = arguments;
    Ok(Value::String(kind.default().to_string()))
}
