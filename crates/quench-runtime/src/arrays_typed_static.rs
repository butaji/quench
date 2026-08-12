fn typed_array_static(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    match builtin {
        crate::ops::Builtin::TypedArrayFrom => Some(from(receiver, arguments)),
        crate::ops::Builtin::TypedArrayOf => Some(typed_array_of(receiver, arguments)),
        _ => None,
    }
}

fn typed_array_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let source = Value::array(arguments.to_vec());
    from(receiver, &[source])
}
