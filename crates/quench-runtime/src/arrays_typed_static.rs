fn typed_array_static(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    match builtin {
        crate::ops::Builtin::TypedArrayFrom => Some(typed_array_from(receiver, arguments)),
        crate::ops::Builtin::TypedArrayOf => Some(typed_array_of(receiver, arguments)),
        _ => None,
    }
}

fn typed_array_from(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| is_constructor(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.from called on a non-constructor",
        ));
    };
    from(Some(receiver), arguments)
}

fn typed_array_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| is_constructor(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.of called on a non-constructor",
        ));
    };
    create_result(Some(receiver), arguments.to_vec(), false)
}
