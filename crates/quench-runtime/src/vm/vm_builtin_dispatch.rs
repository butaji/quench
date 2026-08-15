fn early_dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    crate::intl::tolocale::symbol::dispatch(builtin, arguments, receiver)
        .or_else(|| {
            (builtin == Builtin::ShadowRealmEvaluate || builtin == Builtin::ShadowRealmImportValue)
                .then(|| crate::reflect::builtin(builtin, arguments, receiver))
        })
        .or_else(|| crate::json::execute(builtin, arguments))
        .or_else(|| crate::typed_array_ops::execute(builtin, receiver, arguments))
        .or_else(|| crate::arrays::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::intl::tolocale::dispatch(builtin, receiver, arguments))
        .or_else(|| crate::collections::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::promise::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::disposable_stack::execute(builtin, receiver, arguments))
        .or_else(|| crate::finalization_registry::execute(builtin, receiver, arguments))
        .or_else(|| crate::temporal::execute(builtin, receiver, arguments))
        .or_else(|| {
            (builtin != Builtin::Date)
                .then(|| crate::date::execute(builtin, receiver, arguments))?
        })
}
fn is_function_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::FunctionCall
            | Builtin::FunctionApply
            | Builtin::FunctionBind
            | Builtin::ArrayJoin
            | Builtin::ArrayPush
            | Builtin::ArrayShift
            | Builtin::ArrayReverse
            | Builtin::ArrayPop
            | Builtin::ArrayUnshift
            | Builtin::ArrayFill
            | Builtin::ArrayCopyWithin
            | Builtin::ArrayFindLast
            | Builtin::ArrayFindLastIndex
            | Builtin::ArrayToSorted
    )
}
pub(crate) fn execute_function_apply(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let target = receiver.filter(|value| crate::conversion::is_callable(value));
    let target = target.ok_or_else(|| {
        crate::value::error::throw_type_error("Function.prototype.apply called on non-callable")
    })?;
    let receiver = arguments.first().unwrap_or(&Value::Undefined);
    let list = create_list_from_array_like(arguments.get(1))?;
    crate::functions::execute_target(target, receiver, &list)
}
pub(crate) fn create_list_from_array_like(value: Option<&Value>) -> Result<Vec<Value>, VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Ok(Vec::new());
    };
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Function.prototype.apply requires an object argument list",
        ));
    }
    let length = crate::execute::get_property_result(value, "length")?;
    let length = array_like_length(&length)?;
    (0..length)
        .map(|index| crate::execute::get_property_result(value, &index.to_string()))
        .collect()
}
fn array_like_length(value: &Value) -> Result<usize, VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    Ok(number.floor().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize)
}
