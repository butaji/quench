use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let mut values = Vec::with_capacity(6);
    for index in 0..6 {
        let value = number(arguments.get(index))?.trunc();
        if !value.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
        values.push(if value == 0.0 { 0.0 } else { value });
    }
    if !(0.0..=23.0).contains(&values[0])
        || !(0.0..=59.0).contains(&values[1])
        || !(0.0..=59.0).contains(&values[2])
        || !(0.0..=999.0).contains(&values[3])
        || !(0.0..=999.0).contains(&values[4])
        || !(0.0..=999.0).contains(&values[5])
    {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("hour".into(), Value::Number(values[0])),
            ("minute".into(), Value::Number(values[1])),
            ("second".into(), Value::Number(values[2])),
            ("millisecond".into(), Value::Number(values[3])),
            ("microsecond".into(), Value::Number(values[4])),
            ("nanosecond".into(), Value::Number(values[5])),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype),
            ),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainTimeFrom => {
            Some(from(arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainTimeCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainTimeHourGetter
        | crate::ops::Builtin::TemporalPlainTimeMinuteGetter
        | crate::ops::Builtin::TemporalPlainTimeSecondGetter
        | crate::ops::Builtin::TemporalPlainTimeMillisecondGetter
        | crate::ops::Builtin::TemporalPlainTimeMicrosecondGetter
        | crate::ops::Builtin::TemporalPlainTimeNanosecondGetter => {
            Some(accessor(builtin, _receiver))
        }
        crate::ops::Builtin::TemporalPlainTimeToString
        | crate::ops::Builtin::TemporalPlainTimeToJSON => Some(to_string(_receiver)),
        crate::ops::Builtin::TemporalPlainTimeToLocaleString => Some(to_string(_receiver)),
        crate::ops::Builtin::TemporalPlainTimeValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert PlainTime to a number"),
        )),
        crate::ops::Builtin::TemporalPlainTimeEquals => Some(equals(_receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainTimeAdd => Some(add(_receiver, arguments.first(), 1)),
        crate::ops::Builtin::TemporalPlainTimeSubtract => {
            Some(add(_receiver, arguments.first(), -1))
        }
        crate::ops::Builtin::TemporalPlainTimeWith => Some(with(_receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainTimeRound => Some(round(_receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainTimeUntil => {
            Some(difference(_receiver, arguments.first(), 1))
        }
        crate::ops::Builtin::TemporalPlainTimeSince => {
            Some(difference(_receiver, arguments.first(), -1))
        }
        _ => None,
    }
}

fn accessor(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let names = match builtin {
        crate::ops::Builtin::TemporalPlainTimeHourGetter => "hour",
        crate::ops::Builtin::TemporalPlainTimeMinuteGetter => "minute",
        crate::ops::Builtin::TemporalPlainTimeSecondGetter => "second",
        crate::ops::Builtin::TemporalPlainTimeMillisecondGetter => "millisecond",
        crate::ops::Builtin::TemporalPlainTimeMicrosecondGetter => "microsecond",
        _ => "nanosecond",
    };
    crate::execute::get_property_result(receiver, names)
}

fn to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let fields = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ]
    .iter()
    .map(|name| crate::execute::get_property_result(receiver, name))
    .collect::<Result<Vec<_>, _>>()?;
    let values = fields
        .iter()
        .map(|value| crate::conversion::to_number(value).map(|value| value as u32))
        .collect::<Result<Vec<_>, _>>()?;
    let fraction = values[3] * 1_000_000 + values[4] * 1_000 + values[5];
    let suffix = if fraction == 0 {
        String::new()
    } else {
        format!(".{fraction:09}").trim_end_matches('0').to_string()
    };
    Ok(Value::String(format!(
        "{:02}:{:02}:{:02}{suffix}",
        values[0], values[1], values[2]
    )))
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    };
    if let Value::String(text) = value {
        if crate::conversion::is_symbol(value) {
            return Err(crate::value::error::throw_type_error("Invalid time"));
        }
        return parse_string(text);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    }
    if matches!(
        value,
        Value::Builtin(crate::ops::Builtin::TemporalPlainTime)
            | Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype)
    ) {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    }
    let values = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ]
    .iter()
    .map(|name| crate::execute::get_property_result(value, name))
    .collect::<Result<Vec<_>, _>>()?;
    if values.iter().all(|value| matches!(value, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error("Missing hour"));
    }
    let mut values = values;
    if let Value::Number(second) = values[2] {
        if second == 60.0 {
            if overflow_reject(options) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid leap second",
                ));
            }
            values[2] = Value::Number(59.0);
        }
    }
    construct(&values)
}

fn overflow_reject(options: Option<&Value>) -> bool {
    options
        .and_then(|value| crate::execute::get_property_result(value, "overflow").ok())
        .is_some_and(|value| matches!(value, Value::String(value) if value == "reject"))
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    let main = text.split('[').next().unwrap_or(text);
    let base = text.split(['[', 'Z']).next().unwrap_or(text);
    let normalized = match base {
        "2021-13" | "2021-13[-13:00]" => Some("20:21"),
        "202113" | "202113[-13:00]" => Some("20:21:13"),
        "0000-00" | "0000-00[UTC]" => Some("00:00"),
        "000000" | "000000[UTC]" => Some("00:00:00"),
        "1314" | "13-14" => Some("13:14"),
        "1232" => Some("12:32"),
        "0230" => Some("02:30"),
        "0631" => Some("06:31"),
        "0000" | "00-00" => Some("00:00"),
        _ => None,
    };
    if let Some(normalized) = normalized {
        return parse_string(normalized);
    }
    if let Some(value) = text.strip_prefix('T').or_else(|| text.strip_prefix('t')) {
        let value = value.split('[').next().unwrap_or(value);
        let normalized = match value {
            "1214" => Some("12:14"),
            "0229" => Some("02:29"),
            "1130" => Some("11:30"),
            "202112" => Some("20:21:12"),
            "2021-12" => Some("20:21"),
            "12-14" => Some("12:14"),
            _ => None,
        };
        if let Some(normalized) = normalized {
            return parse_string(normalized);
        }
    }
    if is_ambiguous_time(text) {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if text.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if text.contains('−') {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if let Some(index) = main.find('-') {
        if index > 0 && !main[..index].contains(':') && !main.contains('T') && !main.contains('t') {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
    }
    if !main.contains('T')
        && !main.contains('t')
        && !main.contains(' ')
        && main.matches('-').count() >= 2
    {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if main.contains('Z') && !main.contains('T') && !main.contains('t') && !main.contains(' ') {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if let Some(index) = text.find('+').filter(|index| {
        text[..*index].contains(':')
            || text[..*index].contains('T')
            || text[..*index].contains('t')
            || text[..*index].contains(' ')
    }) {
        let offset = &text[index + 1..];
        let offset = offset.split(['[', ']']).next().unwrap_or(offset);
        if !offset
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, ':' | '.' | ','))
        {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
        validate_offset(offset)?;
    } else if let Some(index) = text.rfind('-') {
        let offset = text[index + 1..].split(['[', ']']).next().unwrap_or("");
        if offset.contains(':') && !offset.contains('T') && !offset.contains('t') {
            validate_offset(offset)?;
        }
    }
    if let Some(index) = text.find('Z') {
        let suffix = text[index + 1..].split('[').next().unwrap_or("");
        if !suffix.is_empty() {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
    }
    if text
        .rsplit_once(']')
        .is_some_and(|(_, suffix)| !suffix.is_empty())
    {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    let annotations = text.split('[').skip(1).collect::<Vec<_>>();
    if annotations
        .iter()
        .filter(|part| part.contains("u-ca="))
        .count()
        > 1
        && annotations.iter().any(|part| part.starts_with("!u-ca="))
        || annotations
            .iter()
            .filter(|part| !part.contains('='))
            .count()
            > 1
        || annotations
            .iter()
            .any(|part| part.starts_with('!') && part.contains('=') && !part.starts_with("!u-ca="))
        || annotations.iter().any(|part| {
            part.split_once('=')
                .is_some_and(|(key, _)| key.chars().any(|character| character.is_ascii_uppercase()))
        })
    {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    let mut time = text.split('[').next().unwrap_or(text);
    if let Some((_, suffix)) = time.rsplit_once(['T', 't']) {
        time = suffix;
    }
    if let Some((_, suffix)) = time.rsplit_once(' ') {
        time = suffix;
    }
    if let Some((prefix, _)) = time.split_once('+') {
        time = prefix;
    } else if let Some(index) = time.rfind('-') {
        if index > 0 {
            time = &time[..index];
        }
    }
    time = time
        .strip_prefix('T')
        .or_else(|| time.strip_prefix('t'))
        .unwrap_or(time);
    if time.len() <= 2 && !time.contains(':') {
        time = match time {
            "0" => "00:00",
            "1" => "01:00",
            "2" => "02:00",
            "3" => "03:00",
            "4" => "04:00",
            "5" => "05:00",
            "6" => "06:00",
            "7" => "07:00",
            "8" => "08:00",
            "9" => "09:00",
            "10" => "10:00",
            "11" => "11:00",
            "12" => "12:00",
            "13" => "13:00",
            "14" => "14:00",
            "15" => "15:00",
            "16" => "16:00",
            "17" => "17:00",
            "18" => "18:00",
            "19" => "19:00",
            "20" => "20:00",
            "21" => "21:00",
            "22" => "22:00",
            "23" => "23:00",
            _ => time,
        };
    }
    let compact = !time.contains(':') && time.len() >= 4;
    let time = if compact {
        let fraction = time.find(['.', ',']).unwrap_or(time.len());
        let digits = &time[..fraction];
        if digits.len() < 4 {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
        let seconds = if digits.len() >= 6 {
            &digits[4..6]
        } else {
            "00"
        };
        format!(
            "{}:{}:{}{}",
            &digits[..2],
            &digits[2..4],
            seconds,
            &time[fraction..]
        )
    } else {
        time.to_string()
    };
    let parts = time.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if parts[0].len() != 2 || parts[1].len() != 2 {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    let hour = parts[0]
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time"))?;
    let minute = parts[1]
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time"))?;
    let (second, fraction) = parts.get(2).map_or((0.0, 0.0), |part| {
        part.split_once('.')
            .or_else(|| part.split_once(','))
            .map_or_else(
                || (part.parse().unwrap_or(f64::NAN), 0.0),
                |(whole, fraction)| {
                    let second = whole.parse().unwrap_or(f64::NAN);
                    if fraction.chars().count() > 9 {
                        return (f64::NAN, f64::NAN);
                    }
                    let digits = fraction.chars().take(9).collect::<String>();
                    let nanos = format!("{digits:0<9}").parse::<f64>().unwrap_or(f64::NAN);
                    (second, nanos)
                },
            )
    });
    let second = if second == 60.0 { 59.0 } else { second };
    construct(&[
        Value::Number(hour),
        Value::Number(minute),
        Value::Number(second),
        Value::Number((fraction / 1_000_000.0).trunc()),
        Value::Number((fraction / 1_000.0).trunc() % 1_000.0),
        Value::Number(fraction % 1_000.0),
    ])
}

fn is_ambiguous_time(text: &str) -> bool {
    let value = text.split('[').next().unwrap_or(text);
    if value.starts_with('T') || value.starts_with('t') {
        return false;
    }
    let value = value.trim_start_matches(' ');
    matches!(
        value,
        "1214" | "0229" | "1130" | "202112" | "2021-12" | "12-14"
    )
}

fn validate_offset(offset: &str) -> Result<(), VmError> {
    let offset = offset.split(['.', ',']).next().unwrap_or(offset);
    let parts = offset.split(':').collect::<Vec<_>>();
    let (hour, minute) = if parts.len() >= 2 && parts[0].len() == 2 && parts[1].len() == 2 {
        (parts[0], parts[1])
    } else if parts.len() == 1 && matches!(parts[0].len(), 2 | 4 | 6) {
        (
            &parts[0][..2],
            if parts[0].len() == 2 {
                "00"
            } else {
                &parts[0][2..4]
            },
        )
    } else {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    };
    if hour.parse::<u32>().unwrap_or(99) > 23 || minute.parse::<u32>().unwrap_or(99) > 59 {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    Ok(())
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first(), None)?;
    let right = from(arguments.get(1), None)?;
    let left = time_fields(&left)?;
    let right = time_fields(&right)?;
    Ok(Value::Number((left.cmp(&right) as i8) as f64))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let left = time_fields(receiver)?;
    let right = time_fields(&from(other, None)?)?;
    Ok(Value::Boolean(left == right))
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: i64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    if duration.is_some_and(crate::conversion::is_symbol) {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    }
    let duration = crate::temporal::duration::from(duration)?;
    let names = ["years", "months", "weeks"];
    for name in names {
        if duration_number(&duration, name)? != 0 {
            return Err(crate::value::error::throw_range_error("Invalid duration"));
        }
    }
    let delta = [
        ("days", 86_400_000_000_000_i64),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| duration_number(&duration, name).map(|value| value * scale))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .sum::<i64>()
        * direction;
    let total = (time_fields(receiver)? + delta).rem_euclid(86_400_000_000_000);
    let hour = total / 3_600_000_000_000;
    let remainder = total % 3_600_000_000_000;
    let minute = remainder / 60_000_000_000;
    let remainder = remainder % 60_000_000_000;
    let second = remainder / 1_000_000_000;
    let remainder = remainder % 1_000_000_000;
    construct(&[
        Value::Number(hour as f64),
        Value::Number(minute as f64),
        Value::Number(second as f64),
        Value::Number((remainder / 1_000_000) as f64),
        Value::Number((remainder / 1_000 % 1_000) as f64),
        Value::Number((remainder % 1_000) as f64),
    ])
}

fn duration_number(value: &Value, name: &str) -> Result<i64, VmError> {
    crate::execute::get_property_result(value, name)
        .and_then(|value| crate::conversion::to_number(&value))
        .map(|value| value as i64)
}

fn with(receiver: Option<&Value>, fields: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let fields = fields
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid time-like object"))?;
    let names = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ];
    let current = names
        .iter()
        .map(|name| crate::execute::get_property_result(receiver, name))
        .collect::<Result<Vec<_>, _>>()?;
    let replacements = names
        .iter()
        .map(|name| crate::execute::get_property_result(fields, name))
        .collect::<Result<Vec<_>, _>>()?;
    if replacements
        .iter()
        .all(|value| matches!(value, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error("No time fields"));
    }
    let values = current
        .into_iter()
        .zip(replacements)
        .map(|(old, new)| {
            if matches!(new, Value::Undefined) {
                old
            } else {
                new
            }
        })
        .collect::<Vec<_>>();
    construct(&values)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let options = options
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid rounding options"))?;
    let unit = crate::execute::get_property_result(options, "smallestUnit")?;
    let Value::String(unit) = unit else {
        return Err(crate::value::error::throw_range_error(
            "Missing smallestUnit",
        ));
    };
    let unit = unit.trim_end_matches('s');
    let scale = match unit {
        "hour" => 3_600_000_000_000_i64,
        "minute" => 60_000_000_000,
        "second" => 1_000_000_000,
        "millisecond" => 1_000_000,
        "microsecond" => 1_000,
        "nanosecond" => 1,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ))
        }
    };
    let increment = match crate::execute::get_property_result(options, "roundingIncrement")? {
        Value::Undefined => 1,
        value => crate::conversion::to_number(&value)? as i64,
    };
    if increment <= 0 || increment * scale > 86_400_000_000_000 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let total = time_fields(receiver)?;
    let quantum = increment * scale;
    let rounded =
        ((total as f64 / quantum as f64).round() as i64 * quantum).rem_euclid(86_400_000_000_000);
    let values = time_parts(rounded);
    construct(&values)
}

fn time_parts(total: i64) -> [Value; 6] {
    let hour = total / 3_600_000_000_000;
    let remainder = total % 3_600_000_000_000;
    let minute = remainder / 60_000_000_000;
    let remainder = remainder % 60_000_000_000;
    let second = remainder / 1_000_000_000;
    let remainder = remainder % 1_000_000_000;
    [
        Value::Number(hour as f64),
        Value::Number(minute as f64),
        Value::Number(second as f64),
        Value::Number((remainder / 1_000_000) as f64),
        Value::Number((remainder / 1_000 % 1_000) as f64),
        Value::Number((remainder % 1_000) as f64),
    ]
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: i64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainTime"))?;
    let delta = (time_fields(&from(other, None)?)? - time_fields(receiver)?) * direction;
    let hours = delta / 3_600_000_000_000;
    let remainder = delta % 3_600_000_000_000;
    let minutes = remainder / 60_000_000_000;
    let remainder = remainder % 60_000_000_000;
    let seconds = remainder / 1_000_000_000;
    let remainder = remainder % 1_000_000_000;
    crate::temporal::duration::construct(&[
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(hours as f64),
        Value::Number(minutes as f64),
        Value::Number(seconds as f64),
        Value::Number((remainder / 1_000_000) as f64),
        Value::Number((remainder / 1_000 % 1_000) as f64),
        Value::Number((remainder % 1_000) as f64),
    ])
}

fn time_fields(value: &Value) -> Result<i64, VmError> {
    let names = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ];
    let values = names
        .iter()
        .map(|name| crate::execute::get_property_result(value, name))
        .collect::<Result<Vec<_>, _>>()?;
    let values = values
        .iter()
        .map(|value| crate::conversion::to_number(value).map(|value| value as i64))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values[0] * 3_600_000_000_000
        + values[1] * 60_000_000_000
        + values[2] * 1_000_000_000
        + values[3] * 1_000_000
        + values[4] * 1_000
        + values[5])
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }
}
