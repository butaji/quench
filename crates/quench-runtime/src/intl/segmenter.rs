//! `Intl.Segmenter`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut granularity = "grapheme".to_string();
    if matches!(arguments.get(1), Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    if let Some(Value::Object(properties)) = arguments.get(1) {
        let options = Value::Object(properties.clone());
        let matcher = crate::execute::get_property_result(&options, "localeMatcher")?;
        if !matches!(matcher, Value::Undefined) {
            let matcher = crate::conversion::to_string(&matcher)?;
            if matcher != "lookup" && matcher != "best fit" {
                return Err(runtime_error("RangeError: invalid localeMatcher"));
            }
        }
        let value = crate::execute::get_property_result(&options, "granularity")?;
        if !matches!(value, Value::Undefined) {
            granularity = crate::conversion::to_string(&value)?;
            if !matches!(granularity.as_str(), "grapheme" | "word" | "sentence") {
                return Err(runtime_error("RangeError: invalid granularity"));
            }
        }
    }
    Ok(make_object(vec![
        (
            "segment".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlSegmenterSegment),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlSegmenterResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("granularity".to_string(), Value::String(granularity)),
            ]),
        ),
    ]))
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let granularity = slot_string(&slots, "granularity").unwrap_or_else(|| "grapheme".to_string());
    match builtin {
        crate::ops::Builtin::IntlSegmenterSegment => {
            let text = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(segment(&text, &granularity, &locale))
        }
        crate::ops::Builtin::IntlSegmenterResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("granularity".to_string(), Value::String(granularity)),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn segment(text: &str, granularity: &str, locale: &str) -> Value {
    let _ = locale;
    let segments: Vec<Value> = match granularity {
        "word" | "sentence" => vec![make_object(vec![
            ("segment".to_string(), Value::String(text.to_string())),
            ("index".to_string(), Value::Number(0.0)),
            ("isWordLike".to_string(), Value::Boolean(true)),
        ])],
        _ => text
            .chars()
            .map(|character| {
                make_object(vec![
                    ("segment".to_string(), Value::String(character.to_string())),
                    ("index".to_string(), Value::Undefined),
                ])
            })
            .collect(),
    };
    make_object(vec![
        ("length".to_string(), Value::Number(segments.len() as f64)),
        (
            "iterator".to_string(),
            make_array(vec![make_object(vec![
                ("segment".to_string(), Value::String(text.to_string())),
                ("index".to_string(), Value::Number(0.0)),
            ])]),
        ),
    ])
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlSegmenter => Some(construct(arguments)),
        crate::ops::Builtin::IntlSegmenterSegment
        | crate::ops::Builtin::IntlSegmenterResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
