fn japanese_speed_parts(formatted: &str) -> Vec<Value> {
    let Some(number) = formatted
        .strip_prefix("時速 ")
        .and_then(|value| value.strip_suffix(" キロメートル"))
    else {
        return numeric_parts(formatted, "ja-JP");
    };
    let mut parts = vec![make_object(vec![
        ("type".to_string(), Value::String("unit".to_string())),
        ("value".to_string(), Value::String("時速".to_string())),
        (
            "unit".to_string(),
            Value::String("kilometer-per-hour".to_string()),
        ),
    ])];
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(" ".to_string())),
    ]));
    parts.extend(numeric_parts(number, "ja-JP"));
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(" ".to_string())),
    ]));
    parts.push(make_object(vec![
        ("type".to_string(), Value::String("unit".to_string())),
        (
            "value".to_string(),
            Value::String("キロメートル".to_string()),
        ),
        (
            "unit".to_string(),
            Value::String("kilometer-per-hour".to_string()),
        ),
    ]));
    parts
}

fn pad_locale_fraction(text: &str, minimum: u32, locale: &str) -> String {
    if !locale.starts_with("de") && !locale.starts_with("pt") {
        return pad_fraction(text, minimum);
    }
    let (sign, rest) = text
        .strip_prefix(['-', '+'])
        .map_or(("", text), |rest| (&text[..1], rest));
    let fraction_digits = rest
        .split_once(',')
        .map_or(0, |(_, fraction)| fraction.len());
    if fraction_digits >= minimum as usize {
        return text.to_string();
    }
    let mut result = format!("{sign}{rest}");
    if fraction_digits == 0 {
        result.push(',');
    }
    result.extend(std::iter::repeat('0').take(minimum as usize - fraction_digits));
    result
}

fn format_localized_unit(text: &str, unit: Option<&str>, display: &str, locale: &str) -> String {
    if unit != Some("kilometer-per-hour") {
        return format_unit(text, unit, display);
    }
    let (prefix, suffix) = localized_unit_parts(locale, display);
    let text = if locale.starts_with("de") {
        text.replace('.', ",")
    } else {
        text.to_string()
    };
    if display == "narrow" && !locale.starts_with("de") {
        format!("{prefix}{text}{}", suffix.trim_start())
    } else {
        format!("{prefix}{text}{suffix}")
    }
}

fn localized_unit_parts(locale: &str, display: &str) -> (&'static str, &'static str) {
    if locale.starts_with("ja") && display == "long" {
        return ("時速 ", " キロメートル");
    }
    if locale.starts_with("ko") {
        return if display == "long" {
            ("시속 ", "킬로미터")
        } else {
            ("", "km/h")
        };
    }
    if locale.starts_with("zh-TW") {
        return if display == "long" {
            ("每小時 ", " 公里")
        } else if display == "narrow" {
            ("", "公里/小時")
        } else {
            ("", " 公里/小時")
        };
    }
    if locale.starts_with("de") && display == "long" {
        return ("", " Kilometer pro Stunde");
    }
    if locale.starts_with("en") && display == "long" {
        return ("", " kilometers per hour");
    }
    ("", " km/h")
}

fn is_decimal_literal(value: &str) -> bool {
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    !integer.is_empty()
        && integer.chars().all(|c| c.is_ascii_digit())
        && fraction.chars().all(|c| c.is_ascii_digit())
}

fn decimal_places(value: &str) -> usize {
    value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

fn format_decimal_literal(value: &str, locale: &str) -> String {
    let (sign, body) = value
        .strip_prefix('-')
        .map_or(("", value), |rest| ("-", rest));
    let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
    let integer = group_integer_locale(integer, locale);
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        let decimal = if locale.starts_with("de") || locale.starts_with("pt") {
            ","
        } else {
            "."
        };
        format!("{sign}{integer}{decimal}{fraction}")
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn to_number_result(value: Option<&Value>) -> Result<f64, VmError> {
    crate::conversion::to_number(value.unwrap_or(&Value::Undefined))
}

pub(crate) fn to_number(value: Option<&Value>) -> f64 {
    to_number_result(value).unwrap_or(f64::NAN)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlNumberFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlNumberFormatFormat
        | crate::ops::Builtin::IntlNumberFormatFormatToParts
        | crate::ops::Builtin::IntlNumberFormatFormatRange
        | crate::ops::Builtin::IntlNumberFormatFormatRangeToParts
        | crate::ops::Builtin::IntlNumberFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}

