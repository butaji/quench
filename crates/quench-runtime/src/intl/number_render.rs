use super::*;
pub(crate) fn japanese_speed_parts(formatted: &str) -> Vec<Value> {
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

pub(crate) fn pad_locale_fraction(text: &str, minimum: u32, locale: &str) -> String {
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

pub(crate) fn scale_number(options: &NumberOptions, number: f64) -> f64 {
    match options.style.as_str() {
        "percent" => number * 100.0,
        _ => number,
    }
}

pub(crate) fn scientific_notation(options: &NumberOptions, value: f64) -> Option<(f64, i32)> {
    match options.notation.as_str() {
        "scientific" => Some(scientific_parts(value, false)),
        "engineering" => Some(scientific_parts(value, true)),
        _ => None,
    }
}

pub(crate) fn compact_magnitude(options: &NumberOptions, value: f64) -> i32 {
    if options.notation == "compact" {
        compact_scale(value, &options.locale, &options.compact_display)
    } else {
        0
    }
}

pub(crate) fn notation_value(scaled: f64, scientific: Option<(f64, i32)>, magnitude: i32) -> f64 {
    if let Some((coefficient, _)) = scientific {
        coefficient
    } else if magnitude == 0 {
        scaled
    } else {
        scaled / 10f64.powi(magnitude)
    }
}

pub(crate) fn compact_unscaled_german(
    options: &NumberOptions,
    scaled: f64,
    magnitude: i32,
) -> bool {
    options.notation == "compact"
        && options.locale.starts_with("de")
        && magnitude == 0
        && scaled.abs() >= 1_000.0
}

pub(crate) fn output_fraction_digits(
    options: &NumberOptions,
    value: f64,
    compact_unscaled_de: bool,
) -> u32 {
    if options.notation == "compact" && !compact_unscaled_de {
        compact_fraction_digits(value)
    } else {
        options.maximum_fraction_digits
    }
}

pub(crate) fn rounded_text(
    options: &NumberOptions,
    value: f64,
    fraction_digits: u32,
) -> (String, bool) {
    let fraction_text = format_number_rounded(value, fraction_digits, options.rounding_increment);
    let Some(maximum) = options.maximum_significant_digits else {
        return (fraction_text, false);
    };
    let significant_text = format_significant(
        value,
        options.minimum_significant_digits.unwrap_or(1),
        maximum,
        &options.rounding_mode,
    );
    match options.rounding_priority.as_str() {
        "morePrecision" if decimal_places(&fraction_text) > decimal_places(&significant_text) => {
            (fraction_text, false)
        }
        "lessPrecision" if decimal_places(&fraction_text) < decimal_places(&significant_text) => {
            (fraction_text, false)
        }
        _ => (significant_text, true),
    }
}

pub(crate) fn decorate_numeric_text(
    options: &NumberOptions,
    mut text: String,
    scaled: f64,
    scientific: Option<(f64, i32)>,
    compact_unscaled_de: bool,
    significant_selected: bool,
) -> String {
    if scientific.is_none()
        && options.use_grouping
        && (!options.grouping_min2 || scaled.abs() >= 10_000.0)
        && (options.notation != "compact" || (compact_unscaled_de && scaled.abs() >= 10_000.0))
    {
        text = group_integer_locale(&text, &options.locale);
    } else if options.notation == "compact" && options.locale.starts_with("de") {
        text = text.replace('.', ",");
    }
    if let Some((_, exponent)) = scientific.filter(|(value, _)| value.is_finite()) {
        if options.locale.starts_with("de") {
            text = text.replace('.', ",");
        }
        text.push_str(&format!("E{exponent}"));
    }
    text = apply_minimum_integer(&text, options.minimum_integer_digits);
    if options.minimum_fraction_digits > 0 && !significant_selected {
        text = pad_locale_fraction(&text, options.minimum_fraction_digits, &options.locale);
    }
    text
}

pub(crate) fn apply_sign(options: &NumberOptions, mut text: String, number: f64) -> String {
    let negative = text.starts_with('-');
    if number.is_nan() && options.locale.starts_with("zh") {
        text = "非數值".to_string();
    }
    let rounded_zero = text
        .trim_start_matches('-')
        .chars()
        .all(|character| matches!(character, '0' | '.' | ','));
    let hide_negative = options.sign_display == "never"
        || (options.sign_display == "auto"
            && number == 0.0
            && options.style == "currency"
            && options.currency_sign != "accounting")
        || (options.sign_display == "exceptZero" && rounded_zero)
        || (options.sign_display == "negative" && rounded_zero);
    if hide_negative && negative {
        text.remove(0);
    } else if !negative
        && (!number.is_nan() || options.sign_display == "always")
        && (options.sign_display == "always"
            || (options.sign_display == "exceptZero" && !rounded_zero))
    {
        text.insert(0, '+');
    }
    text
}

pub(crate) fn apply_style(options: &NumberOptions, mut text: String) -> String {
    match options.style.as_str() {
        "percent" => text.push('%'),
        "currency" => {
            text = format_currency(
                &text,
                options.currency.as_deref(),
                &options.currency_display,
                &options.locale,
                &options.currency_sign,
            )
        }
        "unit" => {
            text = format_localized_unit(
                &text,
                options.unit.as_deref(),
                &options.unit_display,
                &options.locale,
            )
        }
        _ => {}
    }
    text
}

pub(crate) fn append_compact_suffix(text: &mut String, magnitude: i32, options: &NumberOptions) {
    if magnitude > 0 {
        text.push_str(compact_suffix(
            magnitude,
            &options.locale,
            &options.compact_display,
        ));
    }
}

pub(crate) fn format_localized_unit(
    text: &str,
    unit: Option<&str>,
    display: &str,
    locale: &str,
) -> String {
    if unit != Some("kilometer-per-hour") {
        return format_unit(text, unit, display);
    }
    let (prefix, suffix) = localized_unit_parts(display, locale);
    let text = localized_unit_text(text, locale);
    if display == "narrow" && !locale.starts_with("de") {
        format!("{prefix}{text}{}", suffix.trim_start())
    } else {
        format!("{prefix}{text}{suffix}")
    }
}

pub(crate) fn localized_unit_parts(display: &str, locale: &str) -> (&'static str, &'static str) {
    if display == "long" && locale.starts_with("ja") {
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
        return zh_tw_unit_parts(display);
    }
    if display == "long" && locale.starts_with("de") {
        return ("", " Kilometer pro Stunde");
    }
    if display == "long" && locale.starts_with("en") {
        return ("", " kilometers per hour");
    }
    ("", " km/h")
}

pub(crate) fn zh_tw_unit_parts(display: &str) -> (&'static str, &'static str) {
    match display {
        "long" => ("每小時 ", " 公里"),
        "narrow" => ("", "公里/小時"),
        _ => ("", " 公里/小時"),
    }
}

pub(crate) fn localized_unit_text(text: &str, locale: &str) -> String {
    if locale.starts_with("de") {
        text.replace('.', ",")
    } else {
        text.to_string()
    }
}
use super::*;

pub(crate) fn is_decimal_literal(value: &str) -> bool {
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    !integer.is_empty()
        && integer.chars().all(|c| c.is_ascii_digit())
        && fraction.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn decimal_places(value: &str) -> usize {
    value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

pub(crate) fn format_decimal_literal(value: &str, locale: &str) -> String {
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

pub(crate) fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
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
