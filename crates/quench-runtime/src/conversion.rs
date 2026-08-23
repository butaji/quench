use crate::{execute::VmError, value::Value};

pub(crate) fn to_property_key(value: &Value) -> Result<String, VmError> {
    if let Value::Builtin(builtin) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(*builtin) {
            return Ok(name.to_string());
        }
    }
    let primitive = to_primitive(value, "string")?;
    match primitive {
        Value::String(value) => {
            let name = value.trim_end_matches('\0');
            if value.ends_with('\0') && well_known_symbol(name).is_some() {
                Ok(name.to_string())
            } else {
                Ok(value)
            }
        }
        Value::Number(value) => Ok(number_to_string(value)),
        Value::BigInt(value) => Ok(value),
        value => Ok(crate::intl::tolocale::value::to_string(Some(&value))),
    }
}

/// Expose an internal property key as the JS value `Reflect.ownKeys` yields.
pub(crate) fn own_key_value(key: &str) -> Value {
    let name = key.trim_end_matches('\0');
    match well_known_symbol(name) {
        Some(builtin) => Value::Builtin(builtin),
        None if is_symbol_string(key) => Value::String(key.to_string()),
        None => Value::String(key.to_string()),
    }
}

fn well_known_symbol(name: &str) -> Option<crate::ops::Builtin> {
    use crate::ops::Builtin::*;
    Some(match name {
        "Symbol.iterator" => SymbolIterator,
        "Symbol.asyncIterator" => SymbolAsyncIterator,
        "Symbol.dispose" => SymbolDispose,
        "Symbol.asyncDispose" => SymbolAsyncDispose,
        "Symbol.unscopables" => SymbolUnscopables,
        "Symbol.toStringTag" => SymbolToStringTag,
        "Symbol.toPrimitive" => SymbolToPrimitive,
        "Symbol.hasInstance" => SymbolHasInstance,
        "Symbol.isConcatSpreadable" => SymbolIsConcatSpreadable,
        "Symbol.species" => SymbolSpecies,
        "Symbol.match" => SymbolMatch,
        "Symbol.replace" => SymbolReplace,
        "Symbol.search" => SymbolSearch,
        "Symbol.split" => SymbolSplit,
        "Symbol.matchAll" => SymbolMatchAll,
        _ => return None,
    })
}

pub(crate) fn property_key_value(key: &str) -> Value {
    match key {
        "Symbol.iterator"
        | "Symbol.asyncIterator"
        | "Symbol.dispose"
        | "Symbol.asyncDispose"
        | "Symbol.unscopables"
        | "Symbol.toStringTag"
        | "Symbol.toPrimitive"
        | "Symbol.hasInstance"
        | "Symbol.isConcatSpreadable"
        | "Symbol.species"
        | "Symbol.match"
        | "Symbol.replace"
        | "Symbol.search"
        | "Symbol.split"
        | "Symbol.matchAll" => Value::String(format!("{key}\0")),
        _ => Value::String(key.to_string()),
    }
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
    if let Some(value) = crate::strings::materialize(&primitive) {
        return Ok(value);
    }
    Ok(crate::intl::tolocale::value::to_string(Some(&primitive)))
}

pub(crate) fn to_string_explicit(value: &Value) -> Result<String, VmError> {
    let primitive = to_primitive(value, "string")?;
    if is_symbol(&primitive) {
        let description = match &primitive {
            Value::Builtin(builtin) => crate::intl::tolocale::symbol::name(*builtin)
                .map(|name| name.strip_prefix("Symbol.").unwrap_or(name).to_string())
                .unwrap_or_default(),
            Value::String(value) => value
                .strip_prefix("Symbol.")
                .and_then(|value| value.split('\0').next())
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        };
        return Ok(format!("Symbol({description})"));
    }
    if let Value::BigInt(value) = primitive {
        return Ok(value);
    }
    if let Some(value) = crate::strings::materialize(&primitive) {
        return Ok(value);
    }
    Ok(crate::intl::tolocale::value::to_string(Some(&primitive)))
}

/// Convert an already-primitive value without routing common immediate values
/// through the generic Intl conversion layer. `to_number` still performs
/// `ToPrimitive` first; this is the authoritative primitive conversion.
#[inline]
pub(crate) fn primitive_to_number(value: &Value) -> Result<f64, VmError> {
    match value {
        Value::Number(number) => Ok(*number),
        Value::Boolean(boolean) => Ok(if *boolean { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        Value::Undefined => Ok(f64::NAN),
        Value::BigInt(_) => Err(crate::value::error::throw_type_error(
            "Cannot convert value to number",
        )),
        _ if is_symbol(value) => Err(crate::value::error::throw_type_error(
            "Cannot convert value to number",
        )),
        _ => Ok(crate::intl::tolocale::value::to_number(Some(value))),
    }
}

pub(crate) fn is_symbol(value: &Value) -> bool {
    match value {
        Value::String(value) => is_symbol_string(value),
        Value::Builtin(builtin) => crate::intl::tolocale::symbol::name(*builtin).is_some(),
        _ => false,
    }
}

pub(crate) fn is_symbol_string(value: &str) -> bool {
    // The runtime stores Symbol values as `Value::String` with the
    // shape `Symbol.<desc>\0<id>`. Plain description strings that
    // happen to start with `Symbol.` (e.g. `Symbol.iterator`) do not
    // have the trailing nul + counter, so we discriminate by that.
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
        if let Some(result) = try_primitive_method(value, name)? {
            return Ok(result);
        }
    }
    Err(crate::value::error::throw_type_error(
        "Cannot convert object to primitive value",
    ))
}

fn try_primitive_method(value: &Value, name: &str) -> Result<Option<Value>, VmError> {
    let method = crate::execute::get_property_result(value, name)?;
    let owns_method = crate::builtins::object::has_own_property(
        Some(value),
        Some(&Value::String(name.to_string())),
    ) == Value::Boolean(true);
    if matches!(method, Value::Undefined) && !owns_method {
        return fallback_primitive(value, name);
    }
    if !is_callable(&method) {
        return Ok(None);
    }
    let result = crate::functions::execute_target(&method, value, &[])?;
    Ok((!crate::value::is_object(&result)).then_some(result))
}

fn fallback_primitive(value: &Value, name: &str) -> Result<Option<Value>, VmError> {
    let present = crate::with_scope::has_property(value, name)?;
    if name == "valueOf" && !present {
        if let Some(boxed) = boxed_primitive(value) {
            return Ok(Some(boxed));
        }
    }
    if name == "toString"
        && !present
        && !matches!(
            crate::builtins::object::get_prototype_of(Some(value)),
            Ok(Value::Null)
        )
    {
        return Ok(Some(crate::builtins::prototype_to_string(Some(value))));
    }
    Ok(None)
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

pub(crate) fn is_html_dda(value: &Value) -> bool {
    match value {
        Value::BindingCell(cell) => is_html_dda(&cell.borrow()),
        Value::Builtin(crate::ops::Builtin::HostCapability(
            crate::ops::HostCapabilityKind::IsHTMLDDA,
        )) => true,
        Value::HostCapability(token) => {
            token.descriptor.kind == crate::ops::HostCapabilityKind::IsHTMLDDA
        }
        Value::BoundFunction(bound) => is_html_dda(&bound.target),
        _ => false,
    }
}

/// `IsCallable` — hosts query this to validate callback arguments.
pub fn is_callable(value: &Value) -> bool {
    if let Value::BindingCell(cell) = value {
        return is_callable(&cell.borrow());
    }
    match value {
        Value::Builtin(
            crate::ops::Builtin::Math
            | crate::ops::Builtin::Reflect
            | crate::ops::Builtin::Json
            | crate::ops::Builtin::Temporal,
        ) => false,
        Value::Builtin(builtin) if crate::intl::tolocale::symbol::name(*builtin).is_some() => false,
        Value::Builtin(builtin) if crate::builtins::object::is_intrinsic_prototype(*builtin) => {
            false
        }
        Value::Builtin(_) | Value::Function(_) => true,
        Value::BoundFunction(bound)
            if crate::vm::is_intrinsic_bound(bound)
                && matches!(
                    bound.target,
                    Value::Builtin(
                        crate::ops::Builtin::AsyncFunctionPrototype
                            | crate::ops::Builtin::GeneratorFunctionPrototype
                            | crate::ops::Builtin::AsyncGeneratorFunctionPrototype,
                    )
                ) =>
        {
            false
        }
        Value::BoundFunction(_) => true,
        Value::Proxy(proxy) => is_callable(&proxy.target),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{primitive_to_number, to_number};
    use crate::value::Value;

    fn generic(value: &Value) -> Result<f64, crate::execute::VmError> {
        if super::is_symbol(value) || matches!(value, Value::BigInt(_)) {
            return Err(crate::value::error::throw_type_error(
                "Cannot convert value to number",
            ));
        }
        Ok(crate::intl::tolocale::value::to_number(Some(value)))
    }

    #[test]
    fn immediate_decoding_matches_generic_conversion() {
        let values = [
            Value::Number(-0.0),
            Value::Number(f64::INFINITY),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
            Value::String("42.5".into()),
        ];
        for value in &values {
            let fast = primitive_to_number(value);
            let slow = generic(value);
            match (fast, slow) {
                (Ok(left), Ok(right)) => assert_eq!(left.to_bits(), right.to_bits()),
                (Err(_left), Err(_right)) => {}
                (left, right) => panic!("conversion mismatch: {left:?} vs {right:?}"),
            }
        }
    }

    #[test]
    fn direct_number_conversion_preserves_negative_zero_and_nan_payload() {
        let negative_zero = primitive_to_number(&Value::Number(-0.0)).unwrap();
        assert_eq!(negative_zero.to_bits(), (-0.0_f64).to_bits());

        let nan = f64::from_bits(0x7ff8_0000_0000_0042);
        let converted = primitive_to_number(&Value::Number(nan)).unwrap();
        assert_eq!(converted.to_bits(), nan.to_bits());
    }

    #[test]
    fn number_coercion_preserves_primitive_semantics() {
        assert_eq!(to_number(&Value::Null).unwrap(), 0.0);
        assert_eq!(to_number(&Value::Boolean(true)).unwrap(), 1.0);
        assert_eq!(to_number(&Value::String(" 42.5 ".into())).unwrap(), 42.5);
        assert!(to_number(&Value::Undefined).unwrap().is_nan());
    }

    #[test]
    fn primitive_decoder_rejects_bigint_like_generic_path() {
        let value = Value::BigInt("7".into());
        assert!(primitive_to_number(&value).is_err());
        assert!(generic(&value).is_err());
    }
}
