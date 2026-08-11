fn boolean_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::BooleanPrototype)) => Ok(Value::Boolean(false)),
        Some(Value::Boolean(value)) => Ok(Value::Boolean(*value)),
        Some(value @ Value::Object(_)) => wrapped_boolean(value),
        _ => incompatible_boolean_receiver(),
    }
}

fn wrapped_boolean(value: &Value) -> Result<Value, crate::execute::VmError> {
    let constructor = crate::execute::get_property_result(value, "constructor")?;
    let wrapped = crate::execute::get_property_result(value, "_value")?;
    if constructor == Value::Builtin(Builtin::Boolean) && matches!(wrapped, Value::Boolean(_)) {
        return Ok(wrapped);
    }
    incompatible_boolean_receiver()
}

fn incompatible_boolean_receiver() -> Result<Value, crate::execute::VmError> {
    Err(crate::value::error::throw_type_error(
        "Boolean.prototype.valueOf called on incompatible receiver",
    ))
}

fn bigint_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::BigInt(value)) => Ok(Value::BigInt(value.clone())),
        Some(value @ Value::Object(_)) => wrapped_bigint(value),
        _ => Err(crate::value::error::throw_type_error(
            "BigInt.prototype.valueOf called on incompatible receiver",
        )),
    }
}

fn wrapped_bigint(value: &Value) -> Result<Value, crate::execute::VmError> {
    let constructor = crate::execute::get_property_result(value, "constructor")?;
    let wrapped = crate::execute::get_property_result(value, "_value")?;
    if constructor == Value::Builtin(Builtin::BigInt) && matches!(wrapped, Value::BigInt(_)) {
        return Ok(wrapped);
    }
    Err(crate::value::error::throw_type_error(
        "BigInt.prototype.valueOf called on incompatible receiver",
    ))
}
