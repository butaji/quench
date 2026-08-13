//! `Intl.Segmenter`.

use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    value::{IteratorData, IteratorState, Value},
};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_string, to_string_value, SLOT,
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
    match builtin {
        crate::ops::Builtin::IntlSegmenterSegment => {
            let slots = receiver_slots(receiver)?;
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            let granularity =
                slot_string(&slots, "granularity").unwrap_or_else(|| "grapheme".to_string());
            let text = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            Ok(segment(&text, &granularity, &locale))
        }
        crate::ops::Builtin::IntlSegmenterSegmentsIterator => segments_iterator(receiver),
        crate::ops::Builtin::IntlSegmenterSegmentsContaining => {
            segments_containing(receiver, arguments)
        }
        crate::ops::Builtin::IntlSegmenterResolvedOptions => {
            let slots = receiver_slots(receiver)?;
            let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
            let granularity =
                slot_string(&slots, "granularity").unwrap_or_else(|| "grapheme".to_string());
            Ok(make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("granularity".to_string(), Value::String(granularity)),
            ]))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn segment(text: &str, granularity: &str, locale: &str) -> Value {
    let _ = locale;
    let segments = match granularity {
        "word" | "sentence" => vec![segment_entry(text, 0, text, true)],
        _ => text
            .char_indices()
            .map(|(index, character)| segment_entry(&character.to_string(), index, text, false))
            .collect(),
    };
    make_object(vec![
        ("__segments".to_string(), Value::array(segments)),
        (
            "Symbol.iterator".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlSegmenterSegmentsIterator),
        ),
        (
            "containing".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlSegmenterSegmentsContaining),
        ),
    ])
}

fn segment_entry(segment: &str, index: usize, input: &str, word_like: bool) -> Value {
    let mut properties = vec![
        ("segment".to_string(), Value::String(segment.to_string())),
        ("index".to_string(), Value::Number(index as f64)),
        ("input".to_string(), Value::String(input.to_string())),
    ];
    if word_like {
        properties.push(("isWordLike".to_string(), Value::Boolean(true)));
    }
    make_object(properties)
}

fn segments_iterator(receiver: Option<&Value>) -> Result<Value, VmError> {
    let values = segment_values(receiver)?;
    Ok(Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Native {
            values,
            index: 0,
            done: false,
        }),
    })))
}

fn segments_containing(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let values = segment_values(receiver)?;
    let number = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    let index = if number.is_nan() { 0.0 } else { number.trunc() };
    if !index.is_finite() || index < 0.0 || index as usize >= values.len() {
        return Ok(Value::Undefined);
    }
    Ok(values[index as usize].clone())
}

fn segment_values(receiver: Option<&Value>) -> Result<Vec<Value>, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Invalid Segments receiver",
        ));
    };
    let stored = crate::execute::get_property_result(&Value::Object(object.clone()), "__segments")?;
    let Value::Array(values) = stored else {
        return Err(crate::value::error::throw_type_error(
            "Invalid Segments receiver",
        ));
    };
    Ok(values.to_vec())
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlSegmenter => Some(construct(arguments)),
        crate::ops::Builtin::IntlSegmenterSegment
        | crate::ops::Builtin::IntlSegmenterSegmentsIterator
        | crate::ops::Builtin::IntlSegmenterSegmentsContaining
        | crate::ops::Builtin::IntlSegmenterResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
