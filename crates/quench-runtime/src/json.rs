//! JSON builtin: `JSON.parse`, `JSON.stringify`, `JSON.rawJSON`,
//! `JSON.isRawJSON`, plus the serde-backed parser used by JSON modules.

use crate::{execute::VmError, ops::Builtin, value::Value};
use std::rc::Rc;

include!("json/parse_text.rs");
include!("json/reviver.rs");
include!("json/serialize.rs");

const RAW_JSON_KEY: &str = "\0rawjson";

/// Parse JSON into the runtime's canonical JavaScript values.
pub fn parse(source: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(source).map(from_json)
}

fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => {
            Value::array(values.into_iter().map(from_json).collect())
        }
        serde_json::Value::Object(values) => {
            let mut properties = vec![(
                "\0prototype".to_string(),
                Value::Builtin(Builtin::ObjectPrototype),
            )];
            properties.extend(
                values
                    .into_iter()
                    .map(|(key, value)| (key, from_json(value))),
            );
            Value::Object(Rc::new(crate::value::ObjectData::new(properties)))
        }
    }
}

pub(crate) fn execute(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        Builtin::Json => Err(crate::value::error::throw_type_error(
            "JSON is not a function",
        )),
        Builtin::JsonParse => parse_builtin(arguments),
        Builtin::JsonStringify => stringify(arguments),
        Builtin::JsonRawJson => raw_json(arguments),
        Builtin::JsonIsRawJson => Ok(is_raw_json(arguments.first())),
        _ => return None,
    })
}

/// Resolve `JSON` namespace members (methods and `Symbol.toStringTag`).
pub(crate) fn method_property(builtin: Builtin, key: &str) -> Value {
    if builtin != Builtin::Json
        || crate::builtins::builtin_prototype_property_is_removed(builtin, key)
    {
        return Value::Undefined;
    }
    match key {
        "parse" => Value::Builtin(Builtin::JsonParse),
        "stringify" => Value::Builtin(Builtin::JsonStringify),
        "rawJSON" => Value::Builtin(Builtin::JsonRawJson),
        "isRawJSON" => Value::Builtin(Builtin::JsonIsRawJson),
        "Symbol.toStringTag" => Value::String("JSON".to_string()),
        _ => Value::Undefined,
    }
}

fn parse_builtin(arguments: &[Value]) -> Result<Value, VmError> {
    let text = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
    let parsed = parse_text(&text)
        .map_err(|()| crate::value::error::throw_syntax_error("Invalid JSON text"))?;
    let reviver = arguments.get(1).unwrap_or(&Value::Undefined);
    if !crate::conversion::is_callable(reviver) {
        return Ok(parsed.value);
    }
    internalize(parsed, reviver)
}

fn raw_json(arguments: &[Value]) -> Result<Value, VmError> {
    let text = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
    let valid = !text.is_empty()
        && !text.chars().next().is_some_and(is_json_whitespace)
        && !text.chars().last().is_some_and(is_json_whitespace)
        && parse_text(&text).is_ok();
    if !valid {
        return Err(crate::value::error::throw_syntax_error(
            "Invalid raw JSON text",
        ));
    }
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("\0prototype".to_string(), Value::Null),
        ("rawJSON".to_string(), Value::String(text)),
        (RAW_JSON_KEY.to_string(), Value::Boolean(true)),
    ]))))
}

fn is_json_whitespace(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | ' ')
}

fn is_raw_json(value: Option<&Value>) -> Value {
    let is_raw = match value {
        Some(Value::Object(properties)) => properties.iter().any(|(name, _)| name == RAW_JSON_KEY),
        _ => false,
    };
    Value::Boolean(is_raw)
}

#[cfg(test)]
mod regression_tests {
    use super::parse;
    use crate::{ops::Builtin, value::Value};

    #[test]
    fn parsed_objects_have_object_prototype() {
        let Value::Object(object) = parse("{\"x\":1}").expect("JSON parses") else {
            panic!("expected object")
        };
        assert!(matches!(
            object.iter().find(|(key, _)| key == "\0prototype"),
            Some((_, Value::Builtin(Builtin::ObjectPrototype)))
        ));
        assert_eq!(
            crate::builtins::object::get_prototype_of(Some(&Value::Object(object)))
                .expect("prototype lookup"),
            Value::Builtin(Builtin::ObjectPrototype)
        );
    }
}
