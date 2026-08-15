use crate::{execute::VmError, value::Value};
use std::cell::Cell;

thread_local! {
    static PROPERTY_KEY_COERCION: Cell<bool> = const { Cell::new(false) };
}
pub(crate) fn to_property_key(value: &Value) -> Result<String, VmError> {
    if let Value::Builtin(builtin) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(*builtin) {
            if *builtin == crate::ops::Builtin::SymbolUnscopables {
                return Ok(format!("{name}\0"));
            }
            return Ok(name.to_string());
        }
    }
    let previous = PROPERTY_KEY_COERCION.with(|flag| flag.replace(true));
    let primitive = to_primitive(value, "string");
    PROPERTY_KEY_COERCION.with(|flag| flag.set(previous));
    let primitive = primitive?;
    match primitive {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(number_to_string(value)),
        Value::BigInt(value) => Ok(value),
        value => Ok(crate::intl::tolocale::value::to_string(Some(&value))),
    }
}

pub(crate) fn property_key_coercion() -> bool {
    PROPERTY_KEY_COERCION.with(Cell::get)
}

pub(crate) fn number_to_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    let magnitude = value.abs();
    if !(1.0e-6..1.0e21).contains(&magnitude) {
        return normalize_exponent(format!("{value:e}"));
    }
    value.to_string()
}

fn normalize_exponent(value: String) -> String {
    let Some((coefficient, exponent)) = value.split_once('e') else {
        return value;
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let sign = if exponent < 0 { "" } else { "+" };
    format!("{coefficient}e{sign}{exponent}")
}

pub(crate) fn to_primitive(value: &Value, hint: &str) -> Result<Value, VmError> {
    if !crate::value::is_object(value) || is_symbol(value) {
        return Ok(value.clone());
    }
    let exotic = crate::execute::get_property_result(value, "Symbol.toPrimitive")?;
    if !matches!(exotic, Value::Undefined | Value::Null) {
        return call_primitive(&exotic, value, &[Value::String(hint.to_string())]);
    }
    ordinary_to_primitive(value, hint)
}

pub(crate) fn to_number(value: &Value) -> Result<f64, VmError> {
    let primitive = to_primitive(value, "number")?;
    primitive_to_number(&primitive)
}

pub(crate) fn to_string(value: &Value) -> Result<String, VmError> {
    let primitive = to_primitive(value, "string")?;
    if is_symbol(&primitive) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert a Symbol value to a string",
        ));
    }
    if let Value::BigInt(value) = primitive {
        return Ok(value);
    }
    Ok(crate::intl::tolocale::value::to_string(Some(&primitive)))
}

pub(crate) fn to_string_explicit(value: &Value) -> Result<String, VmError> {
    let primitive = to_primitive(value, "string")?;
    if let Value::BigInt(value) = primitive {
        return Ok(value);
    }
    Ok(crate::intl::tolocale::value::to_string(Some(&primitive)))
}

pub(crate) fn primitive_to_number(value: &Value) -> Result<f64, VmError> {
    if is_symbol(value) || matches!(value, Value::BigInt(_)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert value to number",
        ));
    }
    Ok(crate::intl::tolocale::value::to_number(Some(value)))
}

pub(crate) fn is_symbol(value: &Value) -> bool {
    match value {
        Value::String(value) => is_symbol_string(value),
        Value::Builtin(builtin) => crate::intl::tolocale::symbol::name(*builtin).is_some(),
        _ => false,
    }
}

pub(crate) fn is_symbol_string(value: &str) -> bool {
    value.starts_with("Symbol.") && value.contains('\0')
}

pub(crate) fn ordinary_to_primitive(value: &Value, hint: &str) -> Result<Value, VmError> {
    let string_hint = hint == "string" || hint == "default" && is_date_object(value);
    let methods = if string_hint {
        ["toString", "valueOf"]
    } else {
        ["valueOf", "toString"]
    };
    for name in methods {
        let method = crate::execute::get_property_result(value, name)?;
        let owns_method = crate::builtins::object::has_own_property(
            Some(value),
            Some(&Value::String(name.to_string())),
        ) == Value::Boolean(true);
        if matches!(method, Value::Undefined) && !owns_method {
            let present = crate::with_scope::has_property(value, name)?;
            if name == "valueOf" && !present {
                if let Some(boxed) = boxed_primitive(value) {
                    return Ok(boxed);
                }
            }
            if name == "toString"
                && !present
                && !matches!(
                    crate::builtins::object::get_prototype_of(Some(value)),
                    Ok(Value::Null)
                )
            {
                return Ok(crate::builtins::prototype_to_string(Some(value)));
            }
            continue;
        }
        if !is_callable(&method) {
            continue;
        }
        let result = crate::functions::execute_target(&method, value, &[])?;
        if !crate::value::is_object(&result) {
            return Ok(result);
        }
    }
    Err(crate::value::error::throw_type_error(
        "Cannot convert object to primitive value",
    ))
}

fn is_date_object(value: &Value) -> bool {
    let Value::Object(properties) = value else {
        return false;
    };
    properties.iter().any(|(name, _)| name == "timeValue")
}

fn boxed_primitive(value: &Value) -> Option<Value> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "_value").then(|| value.clone()))
}

fn call_primitive(method: &Value, receiver: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    if !is_callable(method) {
        return Err(crate::value::error::throw_type_error(
            "Symbol.toPrimitive is not callable",
        ));
    }
    let result = crate::functions::execute_target(method, receiver, arguments)?;
    if crate::value::is_object(&result) {
        return Err(crate::value::error::throw_type_error(
            "Symbol.toPrimitive returned an object",
        ));
    }
    Ok(result)
}

pub(crate) fn is_callable(value: &Value) -> bool {
    match value {
        Value::Builtin(
            crate::ops::Builtin::Math | crate::ops::Builtin::Reflect | crate::ops::Builtin::Json,
        ) => false,
        Value::Builtin(builtin) if crate::intl::tolocale::symbol::name(*builtin).is_some() => false,
        Value::Builtin(builtin) if crate::builtins::object::is_intrinsic_prototype(*builtin) => {
            false
        }
        Value::Builtin(_) | Value::Function(_) => true,
        Value::BoundFunction(bound) => {
            !matches!(&bound.target, Value::Builtin(builtin) if crate::builtins::object::is_intrinsic_prototype(*builtin))
        }
        Value::Proxy(proxy) => is_callable(&proxy.target),
        _ => false,
    }
}
