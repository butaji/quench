use chrono::Datelike;

use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    if let Some(calendar) = arguments.get(3) {
        if !matches!(calendar, Value::Undefined | Value::String(_)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(value) if crate::conversion::is_symbol_string(value)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(value) if !value.eq_ignore_ascii_case("iso8601")) {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok(date_object(year, month, day))
}

fn days_in_month(year: f64, month: f64) -> f64 {
    match month as u32 {
        2 if is_leap_year(year as i32) => 29.0,
        2 => 28.0,
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainDate => Some(Err(crate::value::error::throw_type_error(
            "Temporal.PlainDate requires new",
        ))),
        crate::ops::Builtin::TemporalPlainDateFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalPlainDateCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainDateWithCalendar => {
            Some(with_calendar(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateValueOf => {
            Some(Err(crate::value::error::throw_type_error(
                "Temporal.PlainDate.prototype.valueOf is not allowed",
            )))
        }
        crate::ops::Builtin::TemporalPlainDateDayOfWeekGetter => Some(day_of_week(receiver)),
        crate::ops::Builtin::TemporalPlainDateDayOfYearGetter => Some(day_of_year(receiver)),
        crate::ops::Builtin::TemporalPlainDateDaysInMonthGetter => {
            Some(days_in_month_getter(receiver))
        }
        crate::ops::Builtin::TemporalPlainDateDaysInYearGetter => Some(days_in_year(receiver)),
        crate::ops::Builtin::TemporalPlainDateDaysInWeekGetter => Some(days_in_week(receiver)),
        crate::ops::Builtin::TemporalPlainDateMonthsInYearGetter => Some(months_in_year(receiver)),
        crate::ops::Builtin::TemporalPlainDateToJSON => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalPlainDateInLeapYearGetter => Some(in_leap_year(receiver)),
        crate::ops::Builtin::TemporalPlainDateEraGetter => Some(era(receiver)),
        crate::ops::Builtin::TemporalPlainDateEraYearGetter => Some(era(receiver)),
        crate::ops::Builtin::TemporalPlainDateCalendarIdGetter => Some(calendar_id(receiver)),
        crate::ops::Builtin::TemporalPlainDateWeekOfYearGetter => Some(week_of_year(receiver)),
        crate::ops::Builtin::TemporalPlainDateYearOfWeekGetter => Some(year_of_week(receiver)),
        crate::ops::Builtin::TemporalPlainDateDayGetter => Some(day(receiver)),
        crate::ops::Builtin::TemporalPlainDateYearGetter => Some(year(receiver)),
        crate::ops::Builtin::TemporalPlainDateMonthCodeGetter => Some(month_code(receiver)),
        crate::ops::Builtin::TemporalPlainDateMonthGetter => Some(month(receiver)),
        _ => None,
    }
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let (Value::Object(left), Value::Object(right)) = (left, right) else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    let left_fields = date_fields(&left);
    let right_fields = date_fields(&right);
    let ordering = left_fields.cmp(&right_fields);
    Ok(Value::Number(match ordering {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }))
}

fn date_fields(object: &crate::value::ObjectData) -> [i64; 3] {
    ["year", "month", "day"].map(|name| number_field(field(object, name)) as i64)
}

fn day_of_week(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    Ok(Value::Number(f64::from(proleptic_weekday(
        year, month, day,
    ))))
}

fn proleptic_weekday(year: i32, month: u32, day: u32) -> u32 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    (days.rem_euclid(7) as u32 + 3) % 7 + 1
}

fn day_of_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    Ok(Value::Number(f64::from(ordinal_day(year, month, day))))
}

fn ordinal_day(year: i32, month: u32, day: u32) -> u32 {
    (1..month)
        .map(|value| days_in_month(f64::from(year), f64::from(value)) as u32)
        .sum::<u32>()
        + day
}

fn days_in_month_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year"));
    let month = number_field(field(object, "month"));
    Ok(Value::Number(days_in_month(year, month)))
}

fn days_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    Ok(Value::Number(if is_leap_year(year) {
        366.0
    } else {
        365.0
    }))
}

fn days_in_week(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(Value::Number(7.0))
}

fn months_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(Value::Number(12.0))
}

fn to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    Ok(Value::String(format!(
        "{}-{month:02}-{day:02}",
        format_year(year)
    )))
}

fn format_year(year: i32) -> String {
    match year {
        year if year < 0 => format!("-{0:06}", year.unsigned_abs()),
        0..=9999 => format!("{year:04}"),
        _ => format!("+{year:06}"),
    }
}

fn in_leap_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    Ok(Value::Boolean(is_leap_year(year)))
}

fn era(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(Value::Undefined)
}

fn calendar_id(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(Value::String("iso8601".to_owned()))
}

fn week_of_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainDate"))?;
    Ok(Value::Number(f64::from(date.iso_week().week())))
}

fn year_of_week(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainDate"))?;
    Ok(Value::Number(f64::from(date.iso_week().year())))
}

fn day(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(field(object, "day"))
}

fn year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(field(object, "year"))
}

fn month_code(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let month = number_field(field(object, "month")) as u32;
    Ok(Value::String(format!("M{month:02}")))
}

fn month(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(field(object, "month"))
}
fn with_calendar(receiver: Option<&Value>, calendar: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let calendar = calendar.unwrap_or(&Value::Undefined);
    if !matches!(calendar, Value::Undefined | Value::String(_)) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    construct(&[
        field(object, "year"),
        field(object, "month"),
        field(object, "day"),
        calendar.clone(),
    ])
}

fn invalid_receiver() -> VmError {
    crate::value::error::throw_type_error(
        "Temporal.PlainDate.prototype.withCalendar called on incompatible receiver",
    )
}

fn has_date_fields(object: &crate::value::ObjectData) -> bool {
    ["year", "month", "day"].iter().all(|name| {
        object
            .iter()
            .any(|(key, value)| key == *name && matches!(value, Value::Number(_)))
    })
}

fn field(object: &crate::value::ObjectData, name: &str) -> Value {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(Value::Undefined, |(_, value)| value.clone())
}

fn number_field(value: Value) -> f64 {
    match value {
        Value::Number(value) => value,
        _ => 0.0,
    }
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if let Some(value) = value.filter(|value| crate::value::is_object(value)) {
        return from_property_bag(value);
    }
    let Some(Value::String(text)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    if has_utc_designator(text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_excess_fraction(text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let calendar_count = text.matches("[u-ca=").count();
    if has_uppercase_annotation_key(text) {
        return Err(crate::value::error::throw_range_error(
            "Invalid annotation key",
        ));
    }
    if has_invalid_calendar_annotation(text) {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if has_multiple_time_zones(text) {
        return Err(crate::value::error::throw_range_error(
            "Multiple time zones",
        ));
    }
    if has_unknown_critical_annotation(text) {
        return Err(crate::value::error::throw_range_error(
            "Unknown critical annotation",
        ));
    }
    if text.contains("[!u-ca=") && calendar_count > 0 {
        return Err(crate::value::error::throw_range_error("Multiple calendars"));
    }
    let date = date_part(text);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 1 && date.len() == 8 {
        let year = date[..4]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = date[4..6]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[6..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return checked_date_object(year, month, day);
    }
    if parts.len() == 1 && date.len() == 11 && matches!(date.as_bytes()[0], b'+' | b'-') {
        let year = date[1..7]
            .parse::<i32>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let year = if date.as_bytes()[0] == b'-' {
            -year
        } else {
            year
        };
        let month = date[7..9]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[9..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return checked_date_object(year, month, day);
    }
    let (year, month, day) = parse_date_parts(&parts)?;
    checked_date_object(year, month, day)
}

fn from_property_bag(value: &Value) -> Result<Value, VmError> {
    let year = crate::execute::get_property_result(value, "year")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    let calendar = match calendar {
        Value::Undefined => calendar,
        Value::String(_) | Value::StringUnits(_) => {
            Value::String(crate::conversion::to_string(&calendar)?)
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid calendar")),
    };
    let month = if matches!(month, Value::Undefined) {
        month_from_code(month_code)?
    } else {
        month
    };
    construct(&[year, month, day, calendar])
}

fn month_from_code(value: Value) -> Result<Value, VmError> {
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    let text = crate::conversion::to_string(&value)?;
    let month = text
        .strip_prefix('M')
        .filter(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|month| (1..=12).contains(month))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
    Ok(Value::Number(f64::from(month)))
}

fn has_uppercase_annotation_key(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| annotation.contains('='))
        .any(|annotation| {
            annotation
                .split('=')
                .next()
                .is_some_and(|key| key.chars().any(|character| character.is_ascii_uppercase()))
        })
}

fn has_unknown_critical_annotation(text: &str) -> bool {
    text.split('[').skip(1).any(|annotation| {
        annotation.starts_with('!') && annotation.contains('=') && !annotation.starts_with("!u-ca=")
    })
}

fn has_invalid_calendar_annotation(text: &str) -> bool {
    text.split('[').skip(1).any(|annotation| {
        ["u-ca=", "!u-ca="]
            .iter()
            .find_map(|prefix| annotation.strip_prefix(prefix))
            .and_then(|value| value.split(']').next())
            .is_some_and(|value| !value.eq_ignore_ascii_case("iso8601"))
    })
}

fn has_multiple_time_zones(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| !annotation.contains('=') && !annotation.is_empty())
        .count()
        > 1
}

fn has_excess_fraction(text: &str) -> bool {
    text.split('[')
        .next()
        .and_then(|value| value.find('.').map(|index| &value[index + 1..]))
        .is_some_and(|fraction| fraction.bytes().take_while(u8::is_ascii_digit).count() > 9)
}

fn has_utc_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|value| value.ends_with('Z'))
}

fn date_part(text: &str) -> &str {
    text.split(['T', 't', ' ', '[']).next().unwrap_or(text)
}

fn parse_date_parts(parts: &[&str]) -> Result<(i32, i32, i32), VmError> {
    let (year, month, day) = match parts {
        [year, month, day] => ((*year).to_owned(), (*month).to_owned(), (*day).to_owned()),
        ["", year, month, day] => (format!("-{year}"), (*month).to_owned(), (*day).to_owned()),
        _ => return Err(crate::value::error::throw_range_error("Invalid ISO date")),
    };
    Ok((
        year.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        month
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        day.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
    ))
}

fn checked_date_object(year: i32, month: i32, day: i32) -> Result<Value, VmError> {
    let year = f64::from(year);
    let month = f64::from(month);
    let day = f64::from(day);
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    Ok(date_object(year, month, day))
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    if !value.is_finite() {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok(value.trunc())
}

fn date_object(year: f64, month: f64, day: f64) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        ("monthCode".into(), Value::String(format!("M{month:02.0}"))),
        ("day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String("iso8601".into())),
        (
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}
