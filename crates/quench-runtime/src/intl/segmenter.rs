//! `Intl.Segmenter`.

use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    value::{IteratorData, IteratorState, Value},
};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, select_supported_locale,
    slot_string, supported_segmenter_locale, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = select_supported_locale(&locales, supported_segmenter_locale);
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
            let text =
                crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
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
        "word" => word_segments(text),
        "sentence" => sentence_segments(text),
        _ => grapheme_segments(text),
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

fn grapheme_segments(text: &str) -> Vec<Value> {
    let mut result = Vec::new();
    let mut start = None;
    let mut utf16_index = 0;
    let mut start_utf16 = 0;
    for (byte_index, character) in text.char_indices() {
        if start.is_none() {
            start = Some(byte_index);
            start_utf16 = utf16_index;
        } else if !is_combining_mark(character) {
            if let Some(cluster_start) = start {
                result.push(segment_entry(
                    &text[cluster_start..byte_index],
                    start_utf16,
                    text,
                    false,
                ));
            }
            start = Some(byte_index);
            start_utf16 = utf16_index;
        }
        utf16_index += character.len_utf16();
    }
    if let Some(start) = start {
        result.push(segment_entry(&text[start..], start_utf16, text, false));
    }
    result
}

fn is_combining_mark(character: char) -> bool {
    matches!(character as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff)
}

fn sentence_segments(text: &str) -> Vec<Value> {
    let mut result = Vec::new();
    let mut start_byte = 0;
    let mut start_utf16 = 0;
    for (index, character) in text.char_indices() {
        if !matches!(character, '.' | '!' | '?') {
            continue;
        }
        let end_byte = text[index + character.len_utf8()..]
            .char_indices()
            .find(|(_, next)| !next.is_whitespace())
            .map_or(text.len(), |(offset, _)| {
                index + character.len_utf8() + offset
            });
        result.push(segment_entry(
            &text[start_byte..end_byte],
            start_utf16,
            text,
            false,
        ));
        start_byte = end_byte;
        start_utf16 = text[..start_byte].encode_utf16().count();
    }
    if start_byte < text.len() {
        result.push(segment_entry(&text[start_byte..], start_utf16, text, false));
    }
    result
}

fn word_segments(text: &str) -> Vec<Value> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut start_utf16 = 0;
    let mut utf16_index = 0;
    let mut kind: Option<u8> = None;
    for (index, character) in text.char_indices() {
        let next_kind = if decimal_point(text, index, character) {
            1
        } else {
            character_kind(character)
        };
        if kind.is_some_and(|previous| previous != next_kind || next_kind == 2) {
            push_word_segment(&mut result, &text[start..index], start_utf16, text, kind);
            start = index;
            start_utf16 = utf16_index;
        }
        utf16_index += character.len_utf16();
        kind = Some(next_kind);
    }
    if start < text.len() {
        push_word_segment(&mut result, &text[start..], start_utf16, text, kind);
    }
    result
}

fn decimal_point(text: &str, index: usize, character: char) -> bool {
    character == '.'
        && text[..index]
            .chars()
            .next_back()
            .is_some_and(|value| value.is_ascii_digit())
        && text[index + character.len_utf8()..]
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
}

fn character_kind(character: char) -> u8 {
    if character.is_whitespace() {
        0
    } else if character.is_alphanumeric() {
        1
    } else {
        2
    }
}

fn push_word_segment(
    result: &mut Vec<Value>,
    segment: &str,
    index: usize,
    input: &str,
    kind: Option<u8>,
) {
    result.push(word_entry(segment, index, input, kind == Some(1)));
}

fn word_entry(segment: &str, index: usize, input: &str, word_like: bool) -> Value {
    let value = segment_entry(segment, index, input, false);
    let Value::Object(object) = value else {
        return value;
    };
    let mut properties = object.properties.clone();
    properties.push(("isWordLike".to_string(), Value::Boolean(word_like)));
    make_object(properties)
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
    if !index.is_finite() || index < 0.0 {
        return Ok(Value::Undefined);
    }
    let target = index as usize;
    if target >= segment_input_length(values.first()) {
        return Ok(Value::Undefined);
    }
    values
        .iter()
        .rev()
        .find(|value| segment_index(value) <= target)
        .cloned()
        .map_or(Ok(Value::Undefined), Ok)
}

fn segment_input_length(value: Option<&Value>) -> usize {
    value
        .and_then(|value| crate::execute::get_property_result(value, "input").ok())
        .and_then(|value| match value {
            Value::String(text) => Some(text.encode_utf16().count()),
            _ => None,
        })
        .unwrap_or(0)
}

fn segment_index(value: &Value) -> usize {
    crate::execute::get_property_result(value, "index")
        .ok()
        .and_then(|value| match value {
            Value::Number(number) => Some(number as usize),
            _ => None,
        })
        .unwrap_or(0)
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
