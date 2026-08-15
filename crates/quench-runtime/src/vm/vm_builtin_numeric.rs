fn boolean_or_number_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Some(value) = boolean_receiver(receiver) {
        return Ok(Value::String(value.to_string()));
    }
    let Value::Number(value) = number_value_of(receiver)? else {
        return Err(crate::value::error::throw_type_error(
            "Number.prototype.toString called on incompatible receiver",
        ));
    };
    let radix = radix(arguments.first())?;
    Ok(Value::String(number_to_string(value, radix)))
}

fn radix(value: Option<&Value>) -> Result<u32, VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(10);
    };
    let radix = crate::conversion::to_number(value)?.trunc();
    if !(2.0..=36.0).contains(&radix) {
        return Err(crate::value::error::throw_range_error("Invalid radix"));
    }
    Ok(radix as u32)
}

fn number_to_string(value: f64, radix: u32) -> String {
    if radix == 10 || !value.is_finite() || value.fract() != 0.0 {
        return crate::conversion::number_to_string(value);
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    format!("{sign}{}", radix_digits(value.abs() as u64, radix))
}

fn radix_digits(mut value: u64, radix: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value != 0 {
        let digit = (value % u64::from(radix)) as u8;
        digits.push(char::from(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        }));
        value /= u64::from(radix);
    }
    digits.iter().rev().collect()
}
fn boolean_receiver(receiver: Option<&Value>) -> Option<bool> {
    match receiver {
        Some(Value::Boolean(value)) => Some(*value),
        Some(Value::Builtin(Builtin::BooleanPrototype)) => Some(false),
        _ => None,
    }
}

fn realm_from_marker(receiver: Option<&Value>) -> Option<RealmId> {
    let marker = crate::execute::get_property(receiver.unwrap_or(&Value::Undefined), "\0realm");
    let Value::HostCapability(token) = marker else {
        return None;
    };
    realm::id_for_token(&token)
}
fn boolean_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = match receiver {
        Some(Value::Builtin(Builtin::BooleanPrototype)) => false,
        Some(Value::Boolean(value)) => *value,
        Some(Value::Object(object)) => match object
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "_value").then_some(value))
        {
            Some(Value::Boolean(value)) => *value,
            _ => return Err(boolean_receiver_error()),
        },
        _ => return Err(boolean_receiver_error()),
    };
    Ok(Value::String(value.to_string()))
}

fn boolean_receiver_error() -> VmError {
    crate::value::error::throw_type_error(
        "Boolean.prototype.toString called on incompatible receiver",
    )
}
fn weak_ref_deref(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef.prototype.deref called on incompatible receiver",
        ));
    };
    let Some((_, target)) = object.iter().rev().find(|(key, _)| key == "\0weakref") else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef.prototype.deref called on incompatible receiver",
        ));
    };
    Ok(target.clone())
}
pub(crate) fn realm_id_for_intrinsic_receiver(receiver: Option<&Value>) -> Option<RealmId> {
    match receiver {
        Some(Value::HostCapability(token)) => realm::id_for_token(token),
        Some(Value::Object(properties)) => properties.iter().find_map(|(key, value)| {
            (key == "\0realm").then(|| match value {
                Value::HostCapability(token) => realm::id_for_token(token),
                _ => None,
            })?
        }),
        _ => realm_from_marker(receiver),
    }
}
pub(crate) fn explicit_number(value: Option<&Value>) -> Result<f64, VmError> {
    let Some(value) = value else {
        return Ok(0.0);
    };
    if let Value::BigInt(value) = value {
        return Ok(value.parse().unwrap_or(f64::NAN));
    }
    crate::intl::tolocale::value::to_number_result(Some(value))
}
include!("vm_bigint.rs");

fn boxed_value(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, |value| match value {
        Value::Object(_) => crate::execute::get_property(value, "_value"),
        _ => value.clone(),
    })
}
pub(crate) fn number_value_of(receiver: Option<&Value>) -> Result<Value, VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::NumberPrototype)) => Ok(Value::Number(0.0)),
        Some(Value::Number(value)) => Ok(Value::Number(*value)),
        Some(value @ Value::Object(_)) => {
            let constructor = crate::execute::get_property_result(value, "constructor")?;
            if constructor != Value::Builtin(Builtin::Number) {
                return Err(crate::value::error::throw_type_error(
                    "Number.prototype.valueOf called on incompatible receiver",
                ));
            }
            let value = crate::execute::get_property_result(value, "_value")?;
            if let Value::Number(_) = value {
                Ok(value)
            } else {
                Err(crate::value::error::throw_type_error(
                    "Number.prototype.valueOf called on incompatible receiver",
                ))
            }
        }
        _ => Err(crate::value::error::throw_type_error(
            "Number.prototype.valueOf called on incompatible receiver",
        )),
    }
}
