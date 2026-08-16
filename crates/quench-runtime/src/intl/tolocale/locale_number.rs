use crate::{execute::VmError, value::Value};

use super::{number, runtime_error, to_string_value};

pub(super) fn to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let value = match crate::vm::number_value_of(receiver)? {
        Value::Number(number) => Value::Number(number),
        _ => return Err(runtime_error("TypeError: Number.prototype.toLocaleString")),
    };
    let formatter = crate::intl::number::construct(arguments)?;
    crate::intl::number::prototype_method(
        crate::ops::Builtin::IntlNumberFormatFormat,
        &[value],
        Some(&formatter),
    )
}

pub(super) fn format_number(number: f64, locales: &[String], options: Option<&Value>) -> String {
    let locale = locales.first().cloned().unwrap_or_else(|| "en".to_string());
    let resolved = number_resolved(locale, options);
    number::format_resolved(number, &resolved)
}

fn number_resolved(locale: String, options: Option<&Value>) -> Vec<(String, Value)> {
    let mut properties = vec![
        ("locale".to_string(), Value::String(locale)),
        ("style".to_string(), Value::String("decimal".to_string())),
        ("useGrouping".to_string(), Value::Boolean(true)),
        ("minimumIntegerDigits".to_string(), Value::Number(1.0)),
        ("minimumFractionDigits".to_string(), Value::Number(0.0)),
        ("maximumFractionDigits".to_string(), Value::Number(3.0)),
    ];
    if let Some(Value::Object(option_map)) = options {
        for (key, value) in option_map.iter() {
            let value = to_string_value(value);
            let Some(name) = (key == "minimumFractionDigits")
                .then_some("minimumFractionDigits")
                .or_else(|| (key == "maximumFractionDigits").then_some("maximumFractionDigits"))
            else {
                continue;
            };
            if let Ok(number) = value.parse() {
                properties.push((name.to_string(), Value::Number(number)));
            }
        }
    }
    properties
}
