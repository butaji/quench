use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

pub(crate) fn property(builtin: Builtin, key: &str) -> Value {
    if let Some(value) = crate::intl::property(builtin, key) {
        return value;
    }
    property_lookup(builtin, key)
}

fn property_lookup(builtin: Builtin, key: &str) -> Value {
    use Builtin::*;
    if let Some(value) = special_property(builtin, key) {
        return value;
    }
    if let Some(value) = callable_property(builtin, key) {
        return value;
    }
    let value = match (builtin, key) {
        (Array, "prototype") => ArrayPrototype,
        (ArrayPrototype, "map") => ArrayMap,
        (ArrayPrototype, "forEach") => ArrayForEach,
        (ArrayMap, "call") => FunctionCall,
        (Array, "isArray") => ArrayIsArray,
        (Object, "is") => ObjectIs,
        (Object, "keys") => ObjectKeys,
        (Object, "getOwnPropertyDescriptor") => ObjectGetOwnPropertyDescriptor,
        _ => return Value::Undefined,
    };
    Value::Builtin(value)
}

fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == Math {
        return crate::math::property(key).map(Value::Builtin);
    }
    if let Some(value) = symbol_property(builtin, key) {
        return Some(Value::Builtin(value));
    }
    let value = match (builtin, key) {
        (Function, "prototype") => FunctionPrototype,
        (FunctionPrototype, "call") => FunctionCall,
        (FunctionPrototype, "bind") => FunctionBind,
        (FunctionCall, "bind") => FunctionBind,
        (ArrayPrototype, "join") => ArrayJoin,
        (ArrayPrototype, "push") => ArrayPush,
        (Object, "prototype") => ObjectPrototype,
        (Date, "prototype") => DatePrototype,
        (DatePrototype, "getYear") => DateGetYear,
        (DatePrototype, "setYear") => DateSetYear,
        (DatePrototype, "toLocaleString") => DateToLocaleString,
        (DatePrototype, "toLocaleDateString") => DateToLocaleDateString,
        (DatePrototype, "toLocaleTimeString") => DateToLocaleTimeString,
        (Reflect, "construct") => ReflectConstruct,
        (RegExp, "prototype") => RegExpPrototype,
        (RegExpPrototype, "test") => RegExpTest,
        (RegExpPrototype, "exec") => RegExpExec,
        (ObjectPrototype, "hasOwnProperty") => ObjectHasOwnProperty,
        (ObjectPrototype, "propertyIsEnumerable") => ObjectPropertyIsEnumerable,
        (Object, "defineProperty") => ObjectDefineProperty,
        (Object, "getOwnPropertyNames") => ObjectGetOwnPropertyNames,
        _ => return None,
    };
    Some(Value::Builtin(value))
}

fn symbol_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Symbol, "iterator") => Some(SymbolIterator),
        (Symbol, "toStringTag") => Some(SymbolToStringTag),
        (Symbol, "toPrimitive") => Some(SymbolToPrimitive),
        (Symbol, "hasInstance") => Some(SymbolHasInstance),
        (Symbol, "isConcatSpreadable") => Some(SymbolIsConcatSpreadable),
        (Symbol, "species") => Some(SymbolSpecies),
        (Symbol, "match") => Some(SymbolMatch),
        (Symbol, "replace") => Some(SymbolReplace),
        (Symbol, "search") => Some(SymbolSearch),
        (Symbol, "split") => Some(SymbolSplit),
        (Symbol, "for") => Some(SymbolFor),
        (Symbol, "keyFor") => Some(SymbolKeyFor),
        _ => None,
    }
}

fn callable_property(builtin: Builtin, key: &str) -> Option<Value> {
    match key {
        "length" => Some(Value::Number(builtin_length(builtin))),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}

fn callable_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    if !matches!(key, "length" | "name") {
        return None;
    }
    Some(callable_property(builtin, key).unwrap())
}

fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Escape => "escape",
        Unescape => "unescape",
        Array => "Array",
        Object => "Object",
        String => "String",
        Symbol => "Symbol",
        Number => "Number",
        Date => "Date",
        DateGetYear => "getYear",
        DateSetYear => "setYear",
        RegExp => "RegExp",
        RegExpTest => "test",
        RegExpExec => "exec",
        _ => "",
    }
}

pub(crate) fn object_method(value: &Value, key: &str) -> Value {
    if matches!(value, Value::Object(_) | Value::Array(_)) && key == "hasOwnProperty" {
        return Value::Builtin(Builtin::ObjectHasOwnProperty);
    }
    Value::Undefined
}

pub(crate) fn has_own_property(receiver: Option<&Value>, key: Option<&Value>) -> Value {
    let Some(key) = key.map(value_to_string) else {
        return Value::Boolean(false);
    };
    let present = match receiver {
        Some(Value::Object(properties)) => properties.iter().any(|(name, _)| name == &key),
        Some(Value::Array(values)) => {
            key == "length" || key.parse::<usize>().is_ok_and(|i| i < values.len())
        }
        Some(Value::String(value)) => {
            key == "length"
                || key
                    .parse::<usize>()
                    .is_ok_and(|i| i < value.chars().count())
        }
        Some(Value::Builtin(builtin)) => {
            callable_property(*builtin, &key).is_some()
                || special_property(*builtin, &key).is_some()
        }
        _ => false,
    };
    Value::Boolean(present)
}

pub(crate) fn object_property_is_enumerable(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    if matches!(receiver, Some(Value::Builtin(_))) {
        return Value::Boolean(false);
    }
    has_own_property(receiver, arguments.first())
}

pub(crate) fn object_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    match builtin {
        Builtin::ObjectHasOwnProperty => has_own_property(receiver, arguments.first()),
        Builtin::ObjectGetOwnPropertyDescriptor => descriptor(arguments.first(), arguments.get(1)),
        _ => Value::Undefined,
    }
}
pub(crate) fn descriptor(value: Option<&Value>, key: Option<&Value>) -> Value {
    let (Some(value), Some(key)) = (value, key.map(value_to_string)) else {
        return Value::Undefined;
    };
    let property = match value {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| value.clone()),
        Value::Array(values) => array_descriptor(values, &key),
        Value::String(value) => string_descriptor(value, &key),
        Value::Builtin(builtin) if matches!(key.as_str(), "length" | "name") => {
            return callable_descriptor(*builtin, &key).unwrap()
        }
        Value::Function(function) if key == "length" => {
            return callable_descriptor_value(Value::Number(f64::from(function.params)))
        }
        _ => None,
    };
    property.map_or(Value::Undefined, |property| descriptor_object(&property))
}

fn callable_descriptor_value(value: Value) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]))
}
fn builtin_length(builtin: Builtin) -> f64 {
    matches!(
        builtin,
        Builtin::Escape | Builtin::Unescape | Builtin::DateSetYear
    ) as i32 as f64
}

fn array_descriptor(values: &[Value], key: &str) -> Option<Value> {
    if key == "length" {
        return None;
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get(index).cloned())
}

fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    value
        .chars()
        .nth(key.parse::<usize>().ok()?)
        .map(|character| Value::String(character.to_string()))
}

fn descriptor_object(value: &Value) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]))
}

fn value_to_string(value: &Value) -> String {
    use Value::*;
    match value {
        String(value) => value.clone(),
        Number(value) => value.to_string(),
        Boolean(value) => value.to_string(),
        Null => "null".to_string(),
        Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}

pub(crate) fn escape(value: Option<&Value>) -> Value {
    let source = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    for character in source.chars() {
        if character.is_ascii_alphanumeric() || "@*_+-./".contains(character) {
            result.push(character);
        } else {
            let code = character as u32;
            let escaped = if code <= 0xFF {
                format!("%{code:02X}")
            } else {
                format!("%u{code:04X}")
            };
            result.push_str(&escaped);
        }
    }
    Value::String(result)
}
pub(crate) fn unescape(value: Option<&Value>) -> Value {
    let text = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        let digits: String = chars.by_ref().take(2).collect();
        if let Some(parsed) = u8::from_str_radix(&digits, 16).ok().map(char::from) {
            result.push(parsed);
        } else {
            result.push('%');
            result.push_str(&digits);
        }
    }
    Value::String(result)
}

pub(crate) fn array(arguments: &[Value]) -> Value {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 {
            return Value::Array(Rc::new(vec![Value::Undefined; *length as usize]));
        }
    }
    Value::Array(Rc::new(arguments.to_vec()))
}

pub(crate) fn array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Array(Rc::new(Vec::new())));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(values.clone()));
    };
    let mut mapped = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let args = [value.clone(), Value::Number(index as f64), Value::Undefined];
        mapped.push(crate::functions::execute_target(
            callback,
            &Value::Undefined,
            &args,
        )?);
    }
    Ok(Value::Array(Rc::new(mapped)))
}

pub(crate) fn array_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    if let Some(callback) = arguments.first() {
        for (index, value) in values.iter().enumerate() {
            crate::functions::execute_target(
                callback,
                &Value::Undefined,
                &[value.clone(), Value::Number(index as f64), Value::Undefined],
            )?;
        }
    }
    Ok(Value::Undefined)
}

pub(crate) fn array_filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Array(Rc::new(Vec::new())));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(values.clone()));
    };
    let mut filtered = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let args = [value.clone(), Value::Number(index as f64), Value::Undefined];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        if crate::execute::is_truthy(&result) {
            filtered.push(value.clone());
        }
    }
    Ok(Value::Array(Rc::new(filtered)))
}

pub(crate) fn array_join(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::String(String::new());
    };
    let separator = arguments
        .first()
        .map_or_else(|| ",".to_string(), value_to_string);
    Value::String(
        values
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(&separator),
    )
}

pub(crate) fn array_push(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let mut result = values.as_ref().clone();
    result.extend_from_slice(arguments);
    Value::Number(result.len() as f64)
}

pub(crate) fn math_pow(arguments: &[Value]) -> Value {
    let base = arguments.first().map_or(f64::NAN, value_to_number);
    let exponent = arguments.get(1).map_or(f64::NAN, value_to_number);
    Value::Number(base.powf(exponent))
}

pub(crate) fn is_array(value: Option<&Value>) -> Value {
    Value::Boolean(matches!(value, Some(Value::Array(_))))
}

pub(crate) fn delete_property(target: Value, key: &str) -> Value {
    match target {
        Value::Object(properties) => Value::Object(Rc::new(
            properties
                .iter()
                .filter(|(name, _)| name != key)
                .cloned()
                .collect(),
        )),
        Value::Array(values) if key != "length" => Value::Array(values),
        value => value,
    }
}

fn value_to_number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub(crate) fn object(arguments: &[Value]) -> Value {
    match arguments.first() {
        Some(Value::Array(_))
        | Some(Value::Object(_))
        | Some(Value::Function(_))
        | Some(Value::Builtin(_)) => arguments[0].clone(),
        _ => Value::Object(Rc::new(Vec::new())),
    }
}

pub(crate) fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return matches!((left, right), (None, None));
    };
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return (left.is_nan() && right.is_nan())
            || (left == right && left.is_sign_negative() == right.is_sign_negative());
    }
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        _ => left == right,
    }
}

pub(crate) fn keys(value: Option<&Value>) -> Value {
    let keys = match value {
        Some(Value::Object(properties)) => properties
            .iter()
            .map(|(key, _)| Value::String(key.clone()))
            .collect(),
        Some(Value::Array(values)) => (0..values.len())
            .map(|index| Value::String(index.to_string()))
            .collect(),
        Some(Value::String(value)) => (0..value.chars().count())
            .map(|index| Value::String(index.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    Value::Array(Rc::new(keys))
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    match target {
        Value::Object(properties) => {
            let mut properties = (*properties).clone();
            if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
                *current = value;
            } else {
                properties.push((key.to_string(), value));
            }
            Value::Object(Rc::new(properties))
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        other => other,
    }
}

fn set_array_property(values: Rc<Vec<Value>>, key: &str, value: Value) -> Value {
    let Ok(index) = key.parse::<usize>() else {
        return Value::Array(values);
    };
    let mut values = (*values).clone();
    values.resize(index.saturating_add(1), Value::Undefined);
    values[index] = value;
    Value::Array(Rc::new(values))
}

fn set_function_property(
    function: Rc<crate::value::FunctionValue>,
    key: &str,
    value: Value,
) -> Value {
    {
        let mut properties = function.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }
    Value::Function(function)
}
