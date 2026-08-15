fn prototype_descriptor(value: crate::value::Value) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(false)),
    ])))
}

pub(crate) fn write(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &crate::machine::FunctionCode,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
) {
    let value = make(
        body.clone(),
        params,
        metadata.length,
        crate::locals::capture(captures),
        metadata,
    );
    if matches!(metadata.kind, FunctionKind::Ordinary) {
        if let crate::value::Value::Function(function) = &value {
            function.properties.borrow_mut().push((
                "\0ordinary_function".to_string(),
                crate::value::Value::Boolean(true),
            ));
        }
    }
    crate::execute::write_value(registers, dst, value);
}

pub(crate) fn write_op(registers: &mut Vec<crate::value::Value>, op: &Op) {
    crate::functions_write::write_op(registers, op);
}
