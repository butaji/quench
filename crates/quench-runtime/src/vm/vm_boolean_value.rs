fn boolean_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::BooleanPrototype)) => Ok(Value::Boolean(false)),
        Some(Value::Boolean(value)) => Ok(Value::Boolean(*value)),
        Some(value @ Value::Object(_)) => wrapped_boolean(value),
        _ => incompatible_boolean_receiver(),
    }
}

fn wrapped_boolean(value: &Value) -> Result<Value, crate::execute::VmError> {
    match crate::execute::get_property_result(value, "_value")? {
        wrapped @ Value::Boolean(_) => Ok(wrapped),
        _ => incompatible_boolean_receiver(),
    }
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
    match crate::execute::get_property_result(value, "_value")? {
        wrapped @ Value::BigInt(_) => Ok(wrapped),
        _ => Err(crate::value::error::throw_type_error(
            "BigInt.prototype.valueOf called on incompatible receiver",
        )),
    }
}

fn symbol_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    match receiver {
        Some(Value::String(value)) if crate::conversion::is_symbol_string(value) => {
            Ok(Value::String(value.clone()))
        }
        Some(Value::Builtin(builtin))
            if crate::intl::tolocale::symbol::name(*builtin).is_some() =>
        {
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
    Ok(Value::String(crate::intl::tolocale::value::to_string(
        Some(&value),
    )))
}

fn symbol_description(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let symbol = match symbol_value_of(receiver)? {
        Value::String(symbol) => symbol,
        Value::Builtin(builtin) => {
            let Some(name) = crate::intl::tolocale::symbol::name(builtin) else {
                return Err(crate::value::error::throw_type_error(
                    "Symbol description requires a symbol",
                ));
            };
            return Ok(Value::String(name.to_string()));
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Symbol description requires a symbol",
            ));
        }
    };
    let full = symbol.trim_end_matches('\0');
    if crate::conversion::well_known_symbol(full).is_some() {
        return Ok(Value::String(full.to_string()));
    }
    let description = symbol
        .strip_prefix("Symbol.for.")
        .or_else(|| symbol.strip_prefix("Symbol."))
        .and_then(|value| value.rsplit_once('\0').map(|(value, _)| value))
        .ok_or_else(|| {
            crate::value::error::throw_type_error("Symbol description requires a symbol")
        })?;
    if description == "\u{1}" {
        return Ok(Value::Undefined);
    }
    Ok(Value::String(description.to_string()))
}

fn wrapped_symbol(value: &Value) -> Result<Value, crate::execute::VmError> {
    let wrapped = crate::execute::get_property_result(value, "_value")?;
    if crate::conversion::is_symbol(&wrapped) {
        return Ok(wrapped);
    }
    Err(crate::value::error::throw_type_error(
        "Symbol.prototype.valueOf called on incompatible receiver",
    ))
}

fn string_value_of(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let incompatible = || {
        crate::value::error::throw_type_error(
            "String.prototype.valueOf called on incompatible receiver",
        )
    };
    match receiver {
        Some(Value::Builtin(Builtin::StringPrototype)) => Ok(Value::String(String::new())),
        Some(Value::String(value)) if !crate::conversion::is_symbol_string(value) => {
            Ok(Value::String(value.clone()))
        }
        Some(Value::StringUnits(value)) => Ok(crate::strings::from_units((**value).to_vec())),
        Some(value @ Value::Object(_)) => wrapped_string(value),
        Some(Value::ObjectAlias(alias)) => alias
            .0
            .borrow()
            .upgrade()
            .map(|object| wrapped_string(&Value::Object(object)))
            .unwrap_or_else(|| {
                Err(incompatible())
            }),
        _ => Err(incompatible()),
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
        && value.is_some_and(
            |value| matches!(value, Value::Number(value) if value.abs() <= MAX_SAFE_INTEGER),
        )
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
    if matches!(
        builtin,
        Builtin::NumberIsInteger | Builtin::NumberIsSafeInteger
    ) {
        return Some(Ok(Value::Boolean(number_predicate(
            builtin,
            arguments.first(),
        ))));
    }
    crate::functions_dynamic::construct_builtin(builtin, arguments)
}

fn wrapped_string(value: &Value) -> Result<Value, crate::execute::VmError> {
    match crate::execute::get_property_result(value, "_value")? {
        Value::String(text) if !crate::conversion::is_symbol_string(&text) => {
            Ok(Value::String(text))
        }
        _ => Err(crate::value::error::throw_type_error(
            "String.prototype.valueOf called on incompatible receiver",
        )),
    }
}
