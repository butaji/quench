use chrono::Datelike;

use crate::{execute::VmError, value::Value};

#[path = "plain_date_tail.rs"]
mod plain_date_tail;
use plain_date_tail::{date_object, number};

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
        crate::ops::Builtin::TemporalPlainDateAdd => Some(add(receiver, arguments.first(), 1.0)),
        crate::ops::Builtin::TemporalPlainDateSubtract => {
            Some(add(receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateEquals => Some(equals(receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainDateUntil => {
            Some(difference(receiver, arguments.first(), 1.0))
        }
        crate::ops::Builtin::TemporalPlainDateSince => {
            Some(difference(receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateToLocaleString => {
            Some(to_string(receiver, None))
        }
        crate::ops::Builtin::TemporalPlainDateToPlainDateTime => {
            Some(to_plain_date_time(receiver))
        }
        crate::ops::Builtin::TemporalPlainDateToPlainMonthDay => {
            Some(to_stub(receiver, crate::ops::Builtin::TemporalPlainMonthDayPrototype))
        }
        crate::ops::Builtin::TemporalPlainDateToPlainYearMonth => {
            Some(to_stub(receiver, crate::ops::Builtin::TemporalPlainYearMonthPrototype))
        }
        crate::ops::Builtin::TemporalPlainDateToZonedDateTime => {
            Some(to_stub(receiver, crate::ops::Builtin::TemporalZonedDateTimePrototype))
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
        crate::ops::Builtin::TemporalPlainDateToString => {
            Some(to_string(receiver, arguments.first()))
        }
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

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let (Some(Value::Object(left)), Some(Value::Object(right))) = (receiver, other) else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(["year", "month", "day"]
        .iter()
        .all(|name| field(left, name) == field(right, name))))
}

fn add(receiver: Option<&Value>, duration: Option<&Value>, direction: f64) -> Result<Value, VmError> {
    let Value::Object(date) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    let Value::Object(duration) = crate::temporal::duration::from(duration)? else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let years = number_field(field(&date, "year")) + number_property(&duration, "years") * direction;
    let months = number_field(field(&date, "month")) - 1.0
        + number_property(&duration, "months") * direction;
    let year = years + (months / 12.0).floor();
    let month = months.rem_euclid(12.0) + 1.0;
    let day = number_field(field(&date, "day")).min(days_in_month(year, month));
    let days = (number_property(&duration, "weeks") * 7.0 + number_property(&duration, "days"))
        * direction;
    shift_date(year, month, day, days)
}

fn difference(receiver: Option<&Value>, other: Option<&Value>, direction: f64) -> Result<Value, VmError> {
    let left = date_parts(receiver)?;
    let right = date_parts(other)?;
    let days = (date_serial(right.0, right.1, right.2) - date_serial(left.0, left.1, left.2))
        as f64 * direction;
    crate::temporal::duration::construct(&[
        Value::Number(0.0), Value::Number(0.0), Value::Number(0.0), Value::Number(days),
    ])
}

fn date_parts(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    let Value::Object(object) = value.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    Ok((
        number_field(field(object, "year")),
        number_field(field(object, "month")),
        number_field(field(object, "day")),
    ))
}

fn date_serial(year: f64, month: f64, day: f64) -> i64 {
    let year = year as i64 - i64::from(month <= 2.0);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = month as i64 + if month > 2.0 { -3 } else { 9 };
    era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + (153 * month_index + 2) / 5 + day as i64 - 1
}

fn shift_date(year: f64, month: f64, day: f64, delta: f64) -> Result<Value, VmError> {
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainDate"))?
        .checked_add_signed(chrono::Duration::days(delta as i64))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainDate"))?;
    construct(&[
        Value::Number(date.year() as f64),
        Value::Number(date.month() as f64),
        Value::Number(date.day() as f64),
    ])
}

fn to_plain_date_time(receiver: Option<&Value>) -> Result<Value, VmError> {
    let (year, month, day) = date_parts(receiver)?;
    crate::temporal::plain_date_time::construct(&[
        Value::Number(year), Value::Number(month), Value::Number(day),
        Value::Number(0.0), Value::Number(0.0), Value::Number(0.0),
        Value::Number(0.0), Value::Number(0.0), Value::Number(0.0),
    ])
}

fn to_stub(receiver: Option<&Value>, prototype: crate::ops::Builtin) -> Result<Value, VmError> {
    let _ = date_parts(receiver)?;
    crate::temporal::construct_stub(prototype)
}

fn number_property(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(0.0, |(_, value)| number_field(value.clone()))
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

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let day = number_field(field(object, "day")) as u32;
    let calendar_name = calendar_name_option(options)?;
    let mut result = format!("{}-{month:02}-{day:02}", format_year(year));
    if calendar_name == "always" {
        result.push_str("[u-ca=iso8601]");
    } else if calendar_name == "critical" {
        result.push_str("[!u-ca=iso8601]");
    }
    Ok(Value::String(result))
}

fn calendar_name_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("auto".into());
    };
    let object = crate::construct::to_object(options)?;
    let value = crate::execute::get_property_result(&object, "calendarName")?;
    if matches!(value, Value::Undefined) {
        return Ok("auto".into());
    }
    let value = crate::conversion::to_string(&value)?;
    if matches!(value.as_str(), "auto" | "always" | "never" | "critical") {
        Ok(value)
    } else {
        Err(crate::value::error::throw_range_error(
            "Invalid calendarName",
        ))
    }
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

include!("plain_date_from.rs");
