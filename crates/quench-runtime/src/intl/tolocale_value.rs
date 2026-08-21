pub(crate) mod value {
    use crate::value::Value;
    pub(crate) fn to_string(value: Option<&Value>) -> String {
        if let Some(Value::BindingCell(value)) = value {
            return to_string(Some(&value.borrow()));
        }
        to_string_value(value)
    }
    #[rustfmt::skip]
    fn to_string_value(value: Option<&Value>) -> String {
        match value {
            None | Some(Value::Undefined) => "undefined".to_string(),
            Some(value @ (Value::Null | Value::Object(_))) => object_string(value),
            Some(Value::Boolean(value)) => value.to_string(),
            Some(Value::Number(value)) => crate::conversion::number_to_string(*value),
            Some(Value::String(value)) => symbol_string(value),
            Some(Value::StringUnits(value)) => String::from_utf16_lossy(value),
            Some(Value::Array(values)) => array_to_string(values),
            Some(Value::ArrayBuffer(_)) => "[object ArrayBuffer]".to_string(), Some(Value::DataView(_)) => "[object DataView]".to_string(),
            Some(Value::Float32Array(_)) => "[object Float32Array]".to_string(),
            Some(Value::Float64Array(_)) => "[object Float64Array]".to_string(),
            Some(Value::Int16Array(_)) => "[object Int16Array]".to_string(),
            Some(Value::Int8Array(_)) => "[object Int8Array]".to_string(),
            Some(Value::Int32Array(_)) => "[object Int32Array]".to_string(),
            Some(Value::Uint16Array(_)) => "[object Uint16Array]".to_string(),
            Some(Value::Uint32Array(_)) => "[object Uint32Array]".to_string(),
            Some(Value::Uint8Array(_)) => "[object Uint8Array]".to_string(),
            Some(Value::Uint8ClampedArray(_)) => "[object Uint8ClampedArray]".to_string(),
            Some(Value::BigInt64Array(_)) => "[object BigInt64Array]".to_string(),
            Some(Value::BigUint64Array(_)) => "[object BigUint64Array]".to_string(),
            Some(Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) | Value::Proxy(_) | Value::Promise(_) | Value::Map(_) | Value::Set(_)) => "function".to_string(),
            Some(Value::BigInt(_)) => "[object BigInt]".to_string(),
            Some(Value::HostCapability(_) | Value::Iterator(_) | Value::Generator(_) | Value::ObjectAlias(_)) => "[object Object]".to_string(),
            Some(Value::BindingCell(value)) => to_string(Some(&value.borrow())),
        }
    }
    fn object_string(value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            _ => "[object Object]".to_string(),
        }
    }
    fn array_to_string(values: &[Value]) -> String {
        values
            .iter()
            .map(|value| match value {
                Value::Null | Value::Undefined => String::new(),
                _ => to_string(Some(value)),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
    fn symbol_string(value: &str) -> String {
        let Some((symbol, _identity)) = value.split_once('\0') else {
            return value.to_string();
        };
        if let Some(description) = symbol.strip_prefix("Symbol.for.") {
            return format!("Symbol({description})");
        }
        if let Some(description) = symbol.strip_prefix("Symbol.") {
            let description = description.strip_prefix('\u{1}').unwrap_or(description);
            return format!("Symbol({description})");
        }
        value.to_string()
    }
    pub(crate) fn to_number(value: Option<&Value>) -> f64 {
        match value {
            Some(Value::BindingCell(value)) => to_number(Some(&value.borrow())),
            None | Some(Value::Undefined) => f64::NAN,
            Some(Value::Null) => 0.0,
            Some(Value::Boolean(value)) => f64::from(*value),
            Some(Value::Number(value)) => *value,
            Some(Value::String(value)) => super::parse_num::parse_number(value),
            Some(Value::StringUnits(value)) => {
                super::parse_num::parse_number(&String::from_utf16_lossy(value))
            }
            Some(Value::Object(properties)) => boxed_number(properties),
            Some(value) if is_non_numeric(value) => f64::NAN,
            Some(_) => f64::NAN,
        }
    }
    fn is_non_numeric(value: &Value) -> bool {
        matches!(
            value,
            Value::Array(_)
                | Value::ArrayBuffer(_)
                | Value::DataView(_)
                | Value::Float32Array(_)
                | Value::Float64Array(_)
                | Value::Int16Array(_)
                | Value::Int8Array(_)
                | Value::Int32Array(_)
                | Value::Uint16Array(_)
                | Value::Uint32Array(_)
                | Value::Uint8Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::BigInt64Array(_)
                | Value::BigUint64Array(_)
                | Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Builtin(_)
                | Value::Proxy(_)
                | Value::Promise(_)
                | Value::Map(_)
                | Value::Set(_)
                | Value::Generator(_)
                | Value::BigInt(_)
                | Value::ObjectAlias(_)
                | Value::HostCapability(_)
                | Value::Iterator(_)
        )
    }
    fn boxed_number(properties: &[(String, Value)]) -> f64 {
        properties
            .iter()
            .find_map(|(key, value)| (key == "_value").then_some(value))
            .map_or(f64::NAN, |value| to_number(Some(value)))
    }
    pub(crate) fn to_number_result(value: Option<&Value>) -> Result<f64, crate::execute::VmError> {
        crate::conversion::to_number(value.unwrap_or(&Value::Undefined))
    }
    pub fn is_truthy(value: &Value) -> bool {
        if crate::conversion::is_html_dda(value) {
            return false;
        }
        match value {
            Value::BindingCell(value) => is_truthy(&value.borrow()),
            Value::Boolean(value) => *value,
            Value::Number(value) => *value != 0.0 && !value.is_nan(),
            Value::String(value) => !value.is_empty(),
            Value::StringUnits(value) => !value.is_empty(),
            Value::BigInt(value) => value != "0",
            Value::Null | Value::Undefined => false,
            Value::Array(_)
            | Value::ArrayBuffer(_)
            | Value::DataView(_)
            | Value::Float32Array(_)
            | Value::Float64Array(_)
            | Value::Int16Array(_)
            | Value::Int8Array(_)
            | Value::Int32Array(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Object(_)
            | Value::Builtin(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
            | Value::Proxy(_)
            | Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Generator(_) => true,
            Value::HostCapability(_) | Value::Iterator(_) | Value::ObjectAlias(_) => true,
        }
    }
    pub(crate) fn type_of(value: &Value) -> &'static str {
        if crate::conversion::is_html_dda(value) {
            return "undefined";
        }
        match value {
            Value::BindingCell(value) => type_of(&value.borrow()),
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Proxy(proxy) => type_of(&proxy.target),
            value if object_value(value) => "object",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(value)
                if value.starts_with("Symbol.") || value.starts_with("Symbol.for.") =>
            {
                "symbol"
            }
            Value::String(_) => "string",
            Value::StringUnits(_) => "string",
            Value::Builtin(builtin) => builtin_type(*builtin),
            Value::Function(_) | Value::BoundFunction(_) => "function",
            Value::BigInt(_) => "bigint",
            Value::Promise(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Iterator(_)
            | Value::Generator(_) => "object",
            Value::ObjectAlias(_) => "object",
            Value::HostCapability(_) => "object",
            _ => "object",
        }
    }

    fn builtin_type(builtin: crate::ops::Builtin) -> &'static str {
        match builtin {
            crate::ops::Builtin::SymbolIterator
            | crate::ops::Builtin::SymbolAsyncIterator
            | crate::ops::Builtin::SymbolDispose
            | crate::ops::Builtin::SymbolAsyncDispose
            | crate::ops::Builtin::SymbolUnscopables
            | crate::ops::Builtin::SymbolToStringTag
            | crate::ops::Builtin::SymbolToPrimitive
            | crate::ops::Builtin::SymbolHasInstance
            | crate::ops::Builtin::SymbolIsConcatSpreadable
            | crate::ops::Builtin::SymbolSpecies
            | crate::ops::Builtin::SymbolMatch
            | crate::ops::Builtin::SymbolReplace
            | crate::ops::Builtin::SymbolSearch
            | crate::ops::Builtin::SymbolSplit
            | crate::ops::Builtin::SymbolMatchAll => "symbol",
            crate::ops::Builtin::Math
            | crate::ops::Builtin::Reflect
            | crate::ops::Builtin::Json
            | crate::ops::Builtin::Temporal => "object",
            builtin if crate::builtin_meta::is_prototype(builtin) => "object",
            _ => "function",
        }
    }

    fn object_value(value: &Value) -> bool {
        matches!(
            value,
            Value::Array(_)
                | Value::ArrayBuffer(_)
                | Value::DataView(_)
                | Value::Float32Array(_)
                | Value::Float64Array(_)
                | Value::Int16Array(_)
                | Value::Int8Array(_)
                | Value::Int32Array(_)
                | Value::Uint16Array(_)
                | Value::Uint32Array(_)
                | Value::Uint8Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::BigInt64Array(_)
                | Value::BigUint64Array(_)
                | Value::Object(_)
                | Value::Proxy(_)
                | Value::Promise(_)
                | Value::Map(_)
                | Value::Set(_)
                | Value::Iterator(_)
                | Value::Generator(_)
                | Value::ObjectAlias(_)
                | Value::HostCapability(_)
        )
    }

    pub(crate) fn is_finite(value: Option<&Value>) -> bool {
        matches!(value, Some(Value::Number(number)) if number.is_finite())
    }
    pub(crate) fn to_int32(value: f64) -> i32 {
        if !value.is_finite() || value == 0.0 {
            return 0;
        }
        let wrapped = value.trunc().rem_euclid(4_294_967_296.0);
        (if wrapped >= 2_147_483_648.0 {
            wrapped - 4_294_967_296.0
        } else {
            wrapped
        }) as i32
    }

    pub(crate) fn strict_equal(left: &Value, right: &Value) -> bool {
        crate::equality::strict_equal(left, right)
    }
}