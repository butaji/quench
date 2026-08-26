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
        if !matches!(
            calendar,
            Value::Undefined | Value::String(_) | Value::StringUnits(_)
        ) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(value) if crate::conversion::is_symbol_string(value)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(_) | Value::StringUnits(_))
            && !is_iso_calendar_value(calendar)?
        {
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
        crate::ops::Builtin::TemporalPlainDateFrom => {
            Some(from(arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainDateWithCalendar => {
            Some(with_calendar(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateWith => {
            Some(with(receiver, arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateAdd => {
            Some(add(receiver, arguments.first(), arguments.get(1), 1.0))
        }
        crate::ops::Builtin::TemporalPlainDateSubtract => {
            Some(add(receiver, arguments.first(), arguments.get(1), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateEquals => Some(equals(receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainDateUntil => Some(difference(
            receiver,
            arguments.first(),
            arguments.get(1),
            1.0,
        )),
        crate::ops::Builtin::TemporalPlainDateSince => Some(difference(
            receiver,
            arguments.first(),
            arguments.get(1),
            -1.0,
        )),
        crate::ops::Builtin::TemporalPlainDateToLocaleString => Some(to_string(receiver, None)),
        crate::ops::Builtin::TemporalPlainDateToPlainDateTime => {
            Some(to_plain_date_time(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateToPlainMonthDay => Some(to_stub(
            receiver,
            crate::ops::Builtin::TemporalPlainMonthDayPrototype,
        )),
        crate::ops::Builtin::TemporalPlainDateToPlainYearMonth => Some(to_stub(
            receiver,
            crate::ops::Builtin::TemporalPlainYearMonthPrototype,
        )),
        crate::ops::Builtin::TemporalPlainDateToZonedDateTime => {
            Some(to_zoned_date_time(receiver, arguments.first()))
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
    let left = date_parts(receiver)?;
    let right_value = from(other, None)?;
    let right = date_parts(Some(&right_value))?;
    Ok(Value::Boolean(left == right))
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let Value::Object(date) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    let Value::Object(duration) = crate::temporal::duration::from(duration)? else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    validate_date_options(options, false)?;
    let overflow = overflow_option(options)?;
    let years =
        number_field(field(&date, "year")) + number_property(&duration, "years") * direction;
    let months = number_field(field(&date, "month")) - 1.0
        + number_property(&duration, "months") * direction;
    let year = years + (months / 12.0).floor();
    let month = months.rem_euclid(12.0) + 1.0;
    let original_day = number_field(field(&date, "day"));
    let max_day = days_in_month(year, month);
    if overflow == "reject" && original_day > max_day {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    let day = original_day.min(max_day);
    let days = (number_property(&duration, "weeks") * 7.0 + number_property(&duration, "days"))
        * direction;
    shift_date(year, month, day, days)
}

fn overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("constrain".into());
    };
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok("constrain".into());
    }
    option_string(&value)
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let left = date_parts(receiver)?;
    let right_value = from(other, None)?;
    let right = date_parts(Some(&right_value))?;
    let settings = difference_settings(options)?;
    let raw_days =
        (date_serial(right.0, right.1, right.2) - date_serial(left.0, left.1, left.2)) as f64;
    let signed_days = raw_days * direction;
    let sign = if signed_days == 0.0 {
        1.0
    } else {
        signed_days.signum()
    };
    let mut smallest = settings.smallest.clone();
    if smallest == "auto" && (settings.increment != 1.0 || settings.mode != "trunc") {
        smallest = "days".into();
    }
    let largest = settings.largest.clone();
    let largest = if largest == "days" && smallest != "auto" {
        smallest.clone()
    } else {
        largest
    };
    let (mut years, mut months, mut weeks, mut days) = match largest.as_str() {
        "years" => {
            let step = if raw_days < 0.0 { -1_i64 } else { 1 };
            let step_f = step as f64;
            let (base, limit) = (left, right);
            let mut years = (limit.0 - base.0) * step_f;
            let mut cursor = add_calendar_months(base, (years as i64) * 12 * step);
            let passed = if step < 0 {
                date_serial(cursor.0, cursor.1, cursor.2) < date_serial(limit.0, limit.1, limit.2)
                    || (cursor.0 == limit.0 && cursor.1 == limit.1 && cursor.2 < limit.2)
            } else {
                date_serial(cursor.0, cursor.1, cursor.2) > date_serial(limit.0, limit.1, limit.2)
                    || (cursor.0 == limit.0 && cursor.1 == limit.1 && cursor.2 > limit.2)
            };
            if passed {
                years -= 1.0;
                cursor = add_calendar_months(base, (years as i64) * 12 * step);
            }
            let mut months = (limit.0 * 12.0 + limit.1 - (cursor.0 * 12.0 + cursor.1)) * step_f;
            cursor = add_calendar_months(base, (years as i64 * 12 + months as i64) * step);
            let passed = if step < 0 {
                date_serial(cursor.0, cursor.1, cursor.2) < date_serial(limit.0, limit.1, limit.2)
                    || (cursor.0 == limit.0 && cursor.1 == limit.1 && cursor.2 < limit.2)
            } else {
                date_serial(cursor.0, cursor.1, cursor.2) > date_serial(limit.0, limit.1, limit.2)
                    || (cursor.0 == limit.0 && cursor.1 == limit.1 && cursor.2 > limit.2)
            };
            if passed {
                months -= 1.0;
                cursor = add_calendar_months(base, (years as i64 * 12 + months as i64) * step);
            }
            let days = (date_serial(limit.0, limit.1, limit.2)
                - date_serial(cursor.0, cursor.1, cursor.2)) as f64;
            (years * sign, months * sign, 0.0, days.abs() * sign)
        }
        "months" => {
            let step = if raw_days < 0.0 { -1_i64 } else { 1 };
            let step_f = step as f64;
            let (base, limit) = (left, right);
            let mut months = (limit.0 * 12.0 + limit.1 - (base.0 * 12.0 + base.1)) * step_f;
            let mut cursor = add_calendar_months(base, months as i64 * step);
            let passed = if step < 0 {
                date_serial(cursor.0, cursor.1, cursor.2) < date_serial(limit.0, limit.1, limit.2)
            } else {
                date_serial(cursor.0, cursor.1, cursor.2) > date_serial(limit.0, limit.1, limit.2)
            };
            if passed {
                months -= 1.0;
                cursor = add_calendar_months(base, months as i64 * step);
            }
            let days = (date_serial(limit.0, limit.1, limit.2)
                - date_serial(cursor.0, cursor.1, cursor.2)) as f64;
            (0.0, months * sign, 0.0, days.abs() * sign)
        }
        "weeks" => {
            let weeks = (signed_days.abs() / 7.0).floor();
            let days = signed_days.abs() - weeks * 7.0;
            (0.0, 0.0, weeks * sign, days * sign)
        }
        _ => (0.0, 0.0, 0.0, signed_days),
    };
    if smallest != "auto" {
        let increment = settings.increment;
        let mode = settings.mode.as_str();
        let scalar = match smallest.as_str() {
            "years" => years + months / 12.0 + days / 365.0,
            "months" => months + days / 31.0,
            "weeks" => weeks + days / 7.0,
            _ => days,
        };
        let rounded = round_difference(scalar, increment, mode);
        years = 0.0;
        months = 0.0;
        weeks = 0.0;
        days = 0.0;
        match smallest.as_str() {
            "years" => years = rounded,
            "months" => months = rounded,
            "weeks" => weeks = rounded,
            _ => days = rounded,
        }
    }
    crate::temporal::duration::construct(&[
        Value::Number(years),
        Value::Number(months),
        Value::Number(weeks),
        Value::Number(days),
    ])
}

fn largest_unit_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("days".into());
    };
    let value = crate::execute::get_property_result(options, "largestUnit")?;
    if matches!(value, Value::Undefined) {
        return Ok("days".into());
    }
    let value = crate::conversion::to_string(&value)?;
    Ok(match value.as_str() {
        "year" | "years" => "years",
        "month" | "months" => "months",
        "week" | "weeks" => "weeks",
        _ => "days",
    }
    .into())
}

struct DifferenceSettings {
    largest: String,
    smallest: String,
    increment: f64,
    mode: String,
}

fn difference_settings(options: Option<&Value>) -> Result<DifferenceSettings, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(DifferenceSettings {
            largest: "days".into(),
            smallest: "auto".into(),
            increment: 1.0,
            mode: "trunc".into(),
        });
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let largest_value = crate::execute::get_property_result(options, "largestUnit")?;
    let largest_text = if matches!(largest_value, Value::Undefined) {
        "days".into()
    } else {
        option_string(&largest_value)?
    };
    let largest = match largest_text.as_str() {
        "year" | "years" => "years",
        "month" | "months" => "months",
        "week" | "weeks" => "weeks",
        "auto" | "day" | "days" => "days",
        _ => return Err(crate::value::error::throw_range_error("Invalid unit")),
    };
    let increment_value = crate::execute::get_property_result(options, "roundingIncrement")?;
    let increment = if matches!(increment_value, Value::Undefined) {
        1.0
    } else {
        crate::conversion::to_number(&increment_value)?.trunc()
    };
    if !increment.is_finite() || !(1.0..=1_000_000_000.0).contains(&increment) {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let mode_value = crate::execute::get_property_result(options, "roundingMode")?;
    let mode = if matches!(mode_value, Value::Undefined) {
        "trunc".into()
    } else {
        option_string(&mode_value)?
    };
    let smallest_value = crate::execute::get_property_result(options, "smallestUnit")?;
    let smallest = if matches!(smallest_value, Value::Undefined) {
        "auto".into()
    } else {
        match option_string(&smallest_value)?.as_str() {
            "year" | "years" => "years",
            "month" | "months" => "months",
            "week" | "weeks" => "weeks",
            "day" | "days" => "days",
            _ => return Err(crate::value::error::throw_range_error("Invalid unit")),
        }
        .into()
    };
    Ok(DifferenceSettings {
        largest: largest.into(),
        smallest,
        increment,
        mode,
    })
}

fn smallest_unit_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("auto".into());
    };
    let value = crate::execute::get_property_result(options, "smallestUnit")?;
    if matches!(value, Value::Undefined) {
        return Ok("auto".into());
    }
    let value = option_string(&value)?;
    Ok(match value.as_str() {
        "year" | "years" => "years",
        "month" | "months" => "months",
        "week" | "weeks" => "weeks",
        "day" | "days" => "days",
        _ => "auto",
    }
    .into())
}

fn rounding_increment_option(options: Option<&Value>) -> Result<f64, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(1.0);
    };
    let value = crate::execute::get_property_result(options, "roundingIncrement")?;
    if matches!(value, Value::Undefined) {
        return Ok(1.0);
    }
    Ok(crate::conversion::to_number(&value)?.trunc().max(1.0))
}

fn rounding_mode_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("trunc".into());
    };
    let value = crate::execute::get_property_result(options, "roundingMode")?;
    if matches!(value, Value::Undefined) {
        return Ok("trunc".into());
    }
    option_string(&value)
}

fn has_rounding_option(options: Option<&Value>) -> Result<bool, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(false);
    };
    Ok(!matches!(
        crate::execute::get_property_result(options, "roundingIncrement")?,
        Value::Undefined
    ) || !matches!(
        crate::execute::get_property_result(options, "roundingMode")?,
        Value::Undefined
    ))
}

fn round_difference(value: f64, increment: f64, mode: &str) -> f64 {
    let scaled = value / increment;
    let rounded = match mode {
        "ceil" => scaled.ceil(),
        "floor" => scaled.floor(),
        "expand" => {
            if scaled.is_sign_negative() {
                scaled.floor()
            } else {
                scaled.ceil()
            }
        }
        "halfExpand" => {
            if scaled.is_sign_negative() {
                (scaled - 0.5).ceil()
            } else {
                (scaled + 0.5).floor()
            }
        }
        "halfCeil" => (scaled + 0.5).floor(),
        "halfFloor" => (scaled - 0.5).ceil(),
        "halfEven" => {
            let floor = scaled.floor();
            let fraction = scaled - floor;
            if (fraction - 0.5).abs() < f64::EPSILON {
                if (floor as i64) % 2 == 0 {
                    floor
                } else {
                    floor + 1.0
                }
            } else if fraction < 0.5 {
                floor
            } else {
                floor + 1.0
            }
        }
        "halfTrunc" => {
            let trunc = scaled.trunc();
            if (scaled.abs() - trunc.abs()) > 0.5 {
                trunc + scaled.signum()
            } else {
                trunc
            }
        }
        _ => scaled.trunc(),
    };
    rounded * increment
}

fn add_calendar_months(date: (f64, f64, f64), months: i64) -> (f64, f64, f64) {
    let index = date.0 as i64 * 12 + date.1 as i64 - 1 + months;
    let year = index.div_euclid(12) as f64;
    let month = (index.rem_euclid(12) + 1) as f64;
    (year, month, date.2.min(days_in_month(year, month)))
}

fn date_parts(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    let Value::Object(object) = value.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year"));
    let month = number_field(field(object, "month"));
    let day = number_field(field(object, "day"));
    if !year.is_finite()
        || !month.is_finite()
        || !day.is_finite()
        || !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok((year, month, day))
}

fn date_serial(year: f64, month: f64, day: f64) -> i64 {
    let year = year as i64 - i64::from(month <= 2.0);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = month as i64 + if month > 2.0 { -3 } else { 9 };
    era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + (153 * month_index + 2) / 5
        + day as i64
        - 1
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

fn to_plain_date_time(receiver: Option<&Value>, time: Option<&Value>) -> Result<Value, VmError> {
    let (year, month, day) = date_parts(receiver)?;
    let time = match time.filter(|value| !matches!(value, Value::Undefined)) {
        None => vec![Value::Number(0.0); 6],
        Some(value) => {
            let time = crate::temporal::plain_time::from(Some(value), None)?;
            [
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ]
            .iter()
            .map(|name| crate::execute::get_property_result(&time, name))
            .collect::<Result<Vec<_>, _>>()?
        }
    };
    crate::temporal::plain_date_time::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
        time[0].clone(),
        time[1].clone(),
        time[2].clone(),
        time[3].clone(),
        time[4].clone(),
        time[5].clone(),
    ])
}

fn to_zoned_date_time(
    receiver: Option<&Value>,
    argument: Option<&Value>,
) -> Result<Value, VmError> {
    let (year, month, day) = date_parts(receiver)?;
    let argument = argument
        .filter(|value| !matches!(value, Value::Undefined))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid time zone"))?;
    let (timezone, time_value) = if crate::value::is_object(argument) {
        let timezone = crate::execute::get_property_result(argument, "timeZone")?;
        if matches!(timezone, Value::Undefined) {
            return Err(crate::value::error::throw_type_error("Invalid time zone"));
        }
        let time = crate::execute::get_property_result(argument, "plainTime")?;
        (timezone, time)
    } else {
        (argument.clone(), Value::Undefined)
    };
    let timezone = timezone_identifier(&timezone)?;
    let time = if matches!(time_value, Value::Undefined) {
        crate::temporal::plain_time::construct(&[])?
    } else {
        crate::temporal::plain_time::from(Some(&time_value), None)?
    };
    let values = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ]
    .iter()
    .map(|name| crate::execute::get_property_result(&time, name))
    .collect::<Result<Vec<_>, _>>()?;
    let hour = crate::conversion::to_number(&values[0])? as u32;
    let minute = crate::conversion::to_number(&values[1])? as u32;
    let second = crate::conversion::to_number(&values[2])? as u32;
    let nanos = crate::conversion::to_number(&values[3])? as u32 * 1_000_000
        + crate::conversion::to_number(&values[4])? as u32 * 1_000
        + crate::conversion::to_number(&values[5])? as u32;
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainDate"))?;
    let local = date
        .and_hms_nano_opt(hour, minute, second, nanos)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid time"))?;
    let epoch = local.and_utc().timestamp_nanos_opt().unwrap_or(0) as i128
        - fixed_timezone_offset(&timezone);
    crate::temporal::zoned_construct(&[Value::BigInt(epoch.to_string()), Value::String(timezone)])
}

fn timezone_identifier(value: &Value) -> Result<String, VmError> {
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let text = crate::conversion::to_string(value)?;
        if text.contains("-000000-") || text.contains('−') {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        if text.eq_ignore_ascii_case("utc") {
            return Ok("UTC".into());
        }
        if text.starts_with(['+', '-']) && is_fixed_timezone(&text) {
            return Ok(text);
        }
        if text.contains('T') {
            let base = text.split('[').next().unwrap_or(&text);
            if let Some(annotation) = text
                .split('[')
                .nth(1)
                .and_then(|part| part.strip_suffix(']'))
            {
                if annotation.eq_ignore_ascii_case("utc") {
                    return Ok("UTC".into());
                }
                if is_fixed_timezone(annotation) {
                    return Ok(annotation.to_string());
                }
                if !annotation.is_empty()
                    && !annotation.contains(':')
                    && annotation
                        .chars()
                        .any(|character| character.is_ascii_alphabetic())
                {
                    return Ok(annotation.to_string());
                }
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
            if base.ends_with('Z') || base.ends_with('z') {
                return Ok("UTC".into());
            }
            if let Some(index) = base.rfind(['+', '-']) {
                let offset = &base[index..];
                if is_fixed_timezone(offset) {
                    return Ok(offset.to_string());
                }
            }
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        if !text.is_empty()
            && !text.contains('T')
            && !text
                .chars()
                .all(|character| character.is_ascii_digit() || ".,:+-".contains(character))
        {
            return Ok(text);
        }
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    Err(crate::value::error::throw_type_error("Invalid time zone"))
}

fn is_fixed_timezone(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 6
        && matches!(bytes[0], b'+' | b'-')
        && bytes[3] == b':'
        && value[1..3].parse::<u8>().is_ok()
        && value[4..6].parse::<u8>().is_ok()
}

fn fixed_timezone_offset(value: &str) -> i128 {
    let bytes = value.as_bytes();
    if bytes.len() != 6 || !matches!(bytes[0], b'+' | b'-') || bytes[3] != b':' {
        return 0;
    }
    let Ok(hour) = value[1..3].parse::<i128>() else {
        return 0;
    };
    let Ok(minute) = value[4..6].parse::<i128>() else {
        return 0;
    };
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    sign * (hour * 3_600 + minute * 60) * 1_000_000_000
}

fn to_stub(receiver: Option<&Value>, prototype: crate::ops::Builtin) -> Result<Value, VmError> {
    let (year, month, day) = date_parts(receiver)?;
    match prototype {
        crate::ops::Builtin::TemporalPlainMonthDayPrototype => {
            crate::temporal::plain_month_day::construct(month, day)
        }
        crate::ops::Builtin::TemporalPlainYearMonthPrototype => {
            crate::temporal::plain_year_month::construct(year, month)
        }
        crate::ops::Builtin::TemporalZonedDateTimePrototype => {
            let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
            let epoch = date
                .signed_duration_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch"))
                .num_days() as i128
                * 86_400_000_000_000;
            crate::temporal::zoned_construct(&[
                Value::BigInt(epoch.to_string()),
                Value::String("UTC".into()),
            ])
        }
        _ => crate::temporal::construct_stub(prototype),
    }
}

fn number_property(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(0.0, |(_, value)| number_field(value.clone()))
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first(), None)?;
    let right = from(arguments.get(1), None)?;
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
    let calendar = calendar
        .filter(|value| !matches!(value, Value::Undefined))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid calendar"))?;
    if matches!(calendar, Value::Object(object) if object.iter().any(|(key, value)| key == "calendarId" && value == Value::String("iso8601".into())))
    {
        return construct(&[
            field(object, "year"),
            field(object, "month"),
            field(object, "day"),
        ]);
    }
    if matches!(calendar, Value::String(_) | Value::StringUnits(_)) {
        if !is_iso_calendar_value(calendar)? {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    } else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    construct(&[
        field(object, "year"),
        field(object, "month"),
        field(object, "day"),
        calendar.clone(),
    ])
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let branded = matches!(receiver, Some(Value::Object(object)) if has_date_fields(object));
    if !branded {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainDate receiver",
        ));
    }
    let (year, month, day) = date_parts(receiver)?;
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let _ = crate::execute::get_property_result(changes, "calendar")?;
    let _ = crate::execute::get_property_result(changes, "timeZone")?;
    let mut day = number_or_field(changes, "day", day)?;
    let explicit_month = number_or_field(changes, "month", month)?;
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month_code_text = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_string(&month_code)?)
    };
    let year = number_or_field(changes, "year", year)?;
    let overflow = if let Some(value) = options {
        if !matches!(value, Value::Undefined) && !crate::value::is_object(value) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
        value
    } else {
        &Value::Undefined
    };
    let overflow = if matches!(overflow, Value::Undefined) {
        Value::String("constrain".into())
    } else {
        crate::execute::get_property_result(overflow, "overflow")?
    };
    let overflow = option_string(&overflow)?;
    let month = if month_code_text.is_none() {
        explicit_month
    } else {
        let month = month_code_number_text(month_code_text.as_deref().unwrap_or_default())?;
        if explicit_month != month
            && !matches!(
                crate::execute::get_property_result(changes, "month")?,
                Value::Undefined
            )
        {
            return Err(crate::value::error::throw_range_error(
                "month and monthCode conflict",
            ));
        }
        month
    };
    if overflow != "constrain" && overflow != "reject" {
        return Err(crate::value::error::throw_range_error("Invalid overflow"));
    }
    if !year.is_finite()
        || !month.is_finite()
        || !day.is_finite()
        || month < 1.0
        || (month > 12.0 && overflow == "reject")
        || day < 1.0
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    let month = month.min(12.0);
    let max_day = days_in_month(year, month);
    if day > max_day {
        if overflow == "constrain" {
            day = max_day;
        } else {
            return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
        }
    }
    construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
    ])
}

fn number_or_field(object: &Value, name: &str, default: f64) -> Result<f64, VmError> {
    match crate::execute::get_property_result(object, name)? {
        Value::Undefined => Ok(default),
        value => Ok(crate::conversion::to_number(&value)?.trunc()),
    }
}

fn option_string(value: &Value) -> Result<String, VmError> {
    if crate::value::is_object(value) {
        let method = crate::execute::get_property_result(value, "toString")?;
        if crate::conversion::is_callable(&method) {
            let primitive = crate::functions::execute_target(&method, value, &[])?;
            return crate::conversion::to_string(&primitive);
        }
    }
    crate::conversion::to_string(value)
}

fn month_code_number(value: &Value) -> Result<f64, VmError> {
    let code = crate::conversion::to_string(value)?;
    month_code_number_text(&code)
}

fn month_code_number_text(code: &str) -> Result<f64, VmError> {
    code.strip_prefix('M')
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| (1.0..=12.0).contains(value))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))
}

fn invalid_receiver() -> VmError {
    crate::value::error::throw_type_error(
        "Temporal.PlainDate.prototype.withCalendar called on incompatible receiver",
    )
}

fn validate_date_options(options: Option<&Value>, difference: bool) -> Result<(), VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    if !difference {
        let overflow = crate::execute::get_property_result(options, "overflow")?;
        if !matches!(overflow, Value::Undefined) {
            let overflow = crate::conversion::to_string(&overflow)?;
            if !matches!(overflow.as_str(), "constrain" | "reject") {
                return Err(crate::value::error::throw_range_error("Invalid overflow"));
            }
        }
        return Ok(());
    }
    let largest = crate::execute::get_property_result(options, "largestUnit")?;
    if !matches!(largest, Value::Undefined) {
        let largest = crate::conversion::to_string(&largest)?;
        if !matches!(
            largest.trim_end_matches('s'),
            "auto" | "year" | "month" | "week" | "day"
        ) {
            return Err(crate::value::error::throw_range_error("Invalid unit"));
        }
    }
    let increment = crate::execute::get_property_result(options, "roundingIncrement")?;
    if !matches!(increment, Value::Undefined) {
        let increment = crate::conversion::to_number(&increment)?;
        if !increment.is_finite() || increment.trunc() < 1.0 || increment.trunc() > 1_000_000_000.0
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid roundingIncrement",
            ));
        }
    }
    let mode = crate::execute::get_property_result(options, "roundingMode")?;
    if !matches!(mode, Value::Undefined) {
        let mode = crate::conversion::to_string(&mode)?;
        if !matches!(
            mode.as_str(),
            "ceil"
                | "floor"
                | "expand"
                | "trunc"
                | "halfCeil"
                | "halfFloor"
                | "halfExpand"
                | "halfTrunc"
                | "halfEven"
        ) {
            return Err(crate::value::error::throw_range_error(
                "Invalid roundingMode",
            ));
        }
    }
    let smallest = crate::execute::get_property_result(options, "smallestUnit")?;
    if !matches!(smallest, Value::Undefined) {
        let smallest = crate::conversion::to_string(&smallest)?;
        if !matches!(
            smallest.trim_end_matches('s'),
            "auto" | "year" | "month" | "week" | "day"
        ) {
            return Err(crate::value::error::throw_range_error("Invalid unit"));
        }
    }
    Ok(())
}

fn has_date_fields(object: &crate::value::ObjectData) -> bool {
    object.iter().any(|(key, value)| {
        (key == "\0prototype"
            && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype))
            || (key == "\0temporal-plain-date" && value == Value::Boolean(true))
    }) && ["year", "month", "day"].iter().all(|name| {
        object
            .iter()
            .any(|(key, value)| key == *name && matches!(value, Value::Number(_)))
    })
}

fn is_iso_calendar_value(value: &Value) -> Result<bool, VmError> {
    let text = crate::conversion::to_string(value)?;
    if text.eq_ignore_ascii_case("iso8601") {
        return Ok(true);
    }
    let (base, annotation) = text
        .split_once('[')
        .map_or((text.as_str(), None), |(base, annotation)| {
            (base, Some(annotation))
        });
    if let Some(annotation) = annotation {
        if !annotation
            .strip_suffix(']')
            .is_some_and(|value| value.eq_ignore_ascii_case("u-ca=iso8601"))
        {
            return Ok(false);
        }
    }
    let date = base.split(['T', 't', ' ']).next().unwrap_or(base);
    let fields: Vec<_> = date.split('-').collect();
    let digits = |value: &str, min: usize, max: usize| {
        (min..=max).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
    };
    let structured = match fields.as_slice() {
        [year, month, day] => digits(year, 4, 6) && digits(month, 2, 2) && digits(day, 2, 2),
        [month, day] => digits(month, 2, 2) && digits(day, 2, 2),
        _ => false,
    };
    if structured {
        return Ok(true);
    }
    Ok(!date.is_empty()
        && date.chars().any(|character| character.is_ascii_digit())
        && date
            .chars()
            .all(|character| character.is_ascii_digit() || "-+:.,".contains(character)))
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
