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

fn symbol_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::String(value)) if crate::conversion::is_symbol_string(value) => {
            Ok(Value::String(value.clone()))
        }
        Some(Value::Builtin(builtin)) if crate::intl::tolocale::symbol::name(*builtin).is_some() => {
            Ok(Value::Builtin(*builtin))
        }
        Some(value @ Value::Object(_)) => wrapped_symbol(value),
        _ => Err(crate::value::error::throw_type_error(
            "Symbol.prototype.valueOf called on incompatible receiver",
        )),
    }
}

fn symbol_to_string(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = symbol_value_of(receiver)?;
    Ok(Value::String(crate::intl::tolocale::value::to_string(Some(&value))))
}

fn symbol_description(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Value::String(symbol) = symbol_value_of(receiver)? else {
        return Err(crate::value::error::throw_type_error(
            "Symbol description requires a symbol",
        ));
    };
    let description = symbol
        .strip_prefix("Symbol.for.")
        .or_else(|| symbol.strip_prefix("Symbol."))
        .and_then(|value| value.rsplit_once('\0').map(|(value, _)| value))
        .ok_or_else(|| crate::value::error::throw_type_error("Symbol description requires a symbol"))?;
    if description == "\u{1}" {
        return Ok(Value::Undefined);
    }
    Ok(Value::String(description.to_string()))
}

fn wrapped_symbol(value: &Value) -> Result<Value, crate::execute::VmError> {
    let constructor = crate::execute::get_property_result(value, "constructor")?;
    let wrapped = crate::execute::get_property_result(value, "_value")?;
    if constructor == Value::Builtin(Builtin::Symbol) && crate::conversion::is_symbol(&wrapped) {
        return Ok(wrapped);
    }
    Err(crate::value::error::throw_type_error(
        "Symbol.prototype.valueOf called on incompatible receiver",
    ))
}

fn string_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::String(value)) if !crate::conversion::is_symbol_string(value) => {
            Ok(Value::String(value.clone()))
        }
        Some(value @ Value::Object(_)) => wrapped_string(value),
        _ => Err(crate::value::error::throw_type_error(
            "String.prototype.valueOf called on incompatible receiver",
        )),
    }
}

fn number_is_integer(value: Option<&Value>) -> bool {
    let Some(Value::Number(value)) = value else {
        return false;
    };
    value.is_finite() && value.fract() == 0.0
}

fn number_is_safe_integer(value: Option<&Value>) -> bool {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    number_is_integer(value)
        && value.is_some_and(|value| {
            matches!(value, Value::Number(value) if value.abs() <= MAX_SAFE_INTEGER)
        })
}

fn number_predicate(builtin: Builtin, value: Option<&Value>) -> bool {
    if builtin == Builtin::NumberIsSafeInteger {
        number_is_safe_integer(value)
    } else {
        number_is_integer(value)
    }
}

fn simple_prelude(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, crate::execute::VmError>> {
    if builtin == Builtin::WeakRefDeref {
        return Some(weak_ref_deref(receiver));
    }
    if matches!(builtin, Builtin::NumberIsInteger | Builtin::NumberIsSafeInteger) {
        return Some(Ok(Value::Boolean(number_predicate(builtin, arguments.first()))));
    }
    if is_error_constructor(builtin) {
        return Some(Ok(crate::builtins::error(builtin, arguments)));
    }
    crate::functions_dynamic::construct_builtin(builtin, arguments)
}

fn wrapped_string(value: &Value) -> Result<Value, crate::execute::VmError> {
    let constructor = crate::execute::get_property_result(value, "constructor")?;
    let wrapped = crate::execute::get_property_result(value, "_value")?;
    if constructor == Value::Builtin(Builtin::String)
        && matches!(&wrapped, Value::String(value) if !crate::conversion::is_symbol_string(value))
    {
        return Ok(wrapped);
    }
    Err(crate::value::error::throw_type_error(
        "String.prototype.valueOf called on incompatible receiver",
    ))
}
