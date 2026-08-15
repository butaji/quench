fn unit_word(unit: &str, style: &str, plural: bool) -> String {
    if style == "short" || style == "narrow" {
        short_word(unit, plural)
    } else {
        long_word(unit, plural)
    }
}

fn short_word(unit: &str, plural: bool) -> String {
    const WORDS: &[(&str, bool, &str)] = &[
        ("second", false, "sec."),
        ("second", true, "sec."),
        ("minute", false, "min."),
        ("minute", true, "min."),
        ("hour", false, "hr."),
        ("hour", true, "hr."),
        ("week", false, "wk."),
        ("week", true, "wk."),
        ("month", false, "mo."),
        ("month", true, "mo."),
        ("year", false, "yr."),
        ("year", true, "yr."),
        ("day", false, "day"),
        ("day", true, "days"),
        ("quarter", false, "qtr."),
        ("quarter", true, "qtrs."),
    ];
    WORDS
        .iter()
        .find(|(name, is_plural, _)| *name == unit && *is_plural == plural)
        .map_or_else(|| long_word(unit, plural), |(_, _, word)| word.to_string())
}

fn long_word(unit: &str, plural: bool) -> String {
    let (single, multi) = match unit {
        "second" => ("second", "seconds"),
        "minute" => ("minute", "minutes"),
        "hour" => ("hour", "hours"),
        "day" => ("day", "days"),
        "week" => ("week", "weeks"),
        "month" => ("month", "months"),
        "quarter" => ("quarter", "quarters"),
        "year" => ("year", "years"),
        _ => (unit, unit),
    };
    if plural {
        multi.to_string()
    } else {
        single.to_string()
    }
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let style = slot_string(&slots, "style").unwrap_or_else(|| "long".to_string());
    let numeric = slot_string(&slots, "numeric").unwrap_or_else(|| "always".to_string());
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormatFormat => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            Ok(Value::String(format_relative(
                value, &unit, &style, &numeric, &locale,
            )?))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            parts_value(value, &unit, &style, &numeric, &locale)
        }
        crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String(style)),
            ("numeric".to_string(), Value::String(numeric)),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlRelativeTimeFormatFormat
        | crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts
        | crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
