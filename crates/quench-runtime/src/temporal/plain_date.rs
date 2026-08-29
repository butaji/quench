use chrono::Datelike;
use icu_calendar::{types::Month, AnyCalendar, AnyCalendarKind, Date};

use crate::{execute::VmError, value::Value};

#[path = "plain_date_tail.rs"]
mod plain_date_tail;
use plain_date_tail::{date_object_with_calendar, number};

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
        if matches!(calendar, Value::String(_) | Value::StringUnits(_)) {
            if !is_iso_calendar_value(calendar)? {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
        }
    }
    let calendar_hint = arguments.get(3).and_then(|value| match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    });
    let max_day = if calendar_hint
        .is_some_and(|value| {
            !value.eq_ignore_ascii_case("iso8601") && !value.eq_ignore_ascii_case("gregory")
        })
    {
        31.0
    } else {
        days_in_month(year, month)
    };
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
            && !(month == 13.0
                && calendar_hint.is_some_and(calendar_supports_month13))
        || !(1.0..=max_day).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    let calendar = arguments
        .get(3)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("iso8601");
    let calendar = canonical_calendar_id(calendar).unwrap_or_else(|| calendar.to_string());
    Ok(date_object_with_calendar(year, month, day, &calendar))
}

pub(crate) fn construct_from_constructor(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(calendar) = arguments
        .get(3)
        .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
    {
        let text = crate::conversion::to_string(calendar)?;
        let date = text.split(['T', 't', ' ', '[']).next().unwrap_or(&text);
        let fields: Vec<_> = date.split('-').collect();
        let date_like = fields.len() == 3
            && fields[0].len() >= 4
            && fields[1].len() == 2
            && fields[2].len() == 2
            || fields.len() == 1
                && date.len() == 8
                && date.bytes().all(|byte| byte.is_ascii_digit());
        if date_like {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    construct(arguments)
}

fn days_in_month(year: f64, month: f64) -> f64 {
    match month as u32 {
        2 if is_leap_year(year as i32) => 29.0,
        2 => 28.0,
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

/// Derive calendar facts from one ICU4X date value shared by all calendars.
fn calendar_date(year: i32, month: u32, day: u32, calendar: &str) -> Option<Date<AnyCalendar>> {
    let kind = match calendar {
        "buddhist" => AnyCalendarKind::Buddhist,
        "chinese" => AnyCalendarKind::Chinese,
        "coptic" => AnyCalendarKind::Coptic,
        "dangi" => AnyCalendarKind::Dangi,
        "ethiopic" => AnyCalendarKind::Ethiopian,
        "ethioaa" => AnyCalendarKind::EthiopianAmeteAlem,
        "gregory" => AnyCalendarKind::Gregorian,
        "hebrew" => AnyCalendarKind::Hebrew,
        "indian" => AnyCalendarKind::Indian,
        "islamic-civil" => AnyCalendarKind::HijriTabularTypeIIFriday,
        "islamic-tbla" => AnyCalendarKind::HijriTabularTypeIIThursday,
        "islamic-umalqura" => AnyCalendarKind::HijriUmmAlQura,
        "japanese" => AnyCalendarKind::Japanese,
        "persian" => AnyCalendarKind::Persian,
        "roc" => AnyCalendarKind::Roc,
        _ => return None,
    };
    let calendar = AnyCalendar::new(kind);
    let input_month = if matches!(
        calendar,
        AnyCalendar::Chinese(_) | AnyCalendar::Dangi(_) | AnyCalendar::Hebrew(_)
    ) {
        let leap_ordinal = (1..=12).find_map(|base| {
            Date::try_new(year.into(), Month::leap(base), 1, AnyCalendar::new(kind))
                .ok()
                .filter(|date| date.month().leap_status() != icu_calendar::types::LeapStatus::Normal)
                .map(|date| u32::from(date.month().ordinal))
        });
        match leap_ordinal {
            Some(ordinal) if month == ordinal => Month::leap(month.saturating_sub(1) as u8),
            Some(ordinal) if month > ordinal => Month::new(month.saturating_sub(1) as u8),
            _ => Month::new(month as u8),
        }
    } else {
        Month::new(month as u8)
    };
    Date::try_new(year.into(), input_month, day as u8, calendar).ok()
}

fn calendar_days_in_month(year: i32, month: u32, calendar: &str) -> Option<u32> {
    calendar_date(year, month, 1, calendar).map(|date| u32::from(date.days_in_month()))
}

fn calendar_days_in_year(year: i32, month: u32, calendar: &str) -> Option<u32> {
    calendar_date(year, month, 1, calendar).map(|date| u32::from(date.days_in_year()))
}

fn calendar_months_in_year(year: i32, month: u32, calendar: &str) -> Option<u32> {
    calendar_date(year, month, 1, calendar).map(|date| u32::from(date.months_in_year()))
}

fn calendar_is_leap_year(year: i32, month: u32, calendar: &str) -> Option<bool> {
    calendar_date(year, month, 1, calendar).map(|date| date.is_in_leap_year())
}

pub(crate) fn calendar_supports_month13(calendar: &str) -> bool {
    matches!(calendar, "coptic" | "ethiopic" | "ethioaa")
}

pub(crate) fn days_in_month_for_record(year: i32, month: u32) -> u32 {
    days_in_month(f64::from(year), f64::from(month)) as u32
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
        crate::ops::Builtin::TemporalPlainDateEraYearGetter => Some(era_year(receiver)),
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
    if !has_date_fields(&date) {
        return Err(invalid_receiver());
    }
    let calendar = calendar_name(&date);
    let Value::Object(duration) = crate::temporal::duration::from(duration)? else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
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
    let time_nanos = i128::from(number_property(&duration, "hours") as i64) * 3_600_000_000_000
        + i128::from(number_property(&duration, "minutes") as i64) * 60_000_000_000
        + i128::from(number_property(&duration, "seconds") as i64) * 1_000_000_000
        + i128::from(number_property(&duration, "milliseconds") as i64) * 1_000_000
        + i128::from(number_property(&duration, "microseconds") as i64) * 1_000
        + i128::from(number_property(&duration, "nanoseconds") as i64);
    let time_days = (time_nanos / 86_400_000_000_000) as f64;
    let days = (number_property(&duration, "weeks") * 7.0
        + number_property(&duration, "days")
        + time_days.trunc())
        * direction;
    shift_date(year, month, day, days, &calendar)
}

fn overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    crate::temporal::options::overflow(options)
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
    let raw_sign = if raw_days == 0.0 {
        1.0
    } else {
        raw_days.signum()
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
        if smallest == "months" && increment >= 100_000_000.0 {
            return Err(crate::value::error::throw_range_error(
                "Rounded PlainDate is out of range",
            ));
        }
        let mode = settings.mode.as_str();
        let scalar = match smallest.as_str() {
            "years" => {
                let magnitude = years * sign;
                let anchor = add_calendar_months(left, (magnitude * raw_sign) as i64 * 12);
                let remainder = (date_serial(right.0, right.1, right.2)
                    - date_serial(anchor.0, anchor.1, anchor.2))
                .unsigned_abs() as f64;
                (magnitude
                    + remainder
                        / if is_leap_year(anchor.0 as i32) {
                            366.0
                        } else {
                            365.0
                        })
                    * sign
            }
            "months" => {
                let magnitude = months * sign;
                let anchor = add_calendar_months(left, (magnitude * raw_sign) as i64);
                let remainder = (date_serial(right.0, right.1, right.2)
                    - date_serial(anchor.0, anchor.1, anchor.2))
                .unsigned_abs() as f64;
                let span = if raw_sign > 0.0 {
                    anchor
                } else {
                    add_calendar_months(anchor, -1)
                };
                (magnitude + remainder / days_in_month(span.0, span.1)) * sign
            }
            "weeks" => weeks + days / 7.0,
            _ => days,
        };
        let rounded = round_difference(scalar, increment, mode);
        match smallest.as_str() {
            "years" => {
                years = rounded;
                months = 0.0;
                weeks = 0.0;
                days = 0.0;
            }
            "months" => {
                months = rounded;
                weeks = 0.0;
                days = 0.0;
            }
            "weeks" => {
                weeks = rounded;
                days = 0.0;
            }
            _ => days = rounded,
        }
        if smallest == "months" && largest == "years" {
            years = (months / 12.0).trunc();
            months -= years * 12.0;
            if years == 0.0
                && months.signum() == sign
                && days == 0.0
                && left.2 == right.2
                && left.0 != right.0
            {
                years = sign;
                months = 0.0;
            }
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
    let largest_explicit = !matches!(largest_value, Value::Undefined);
    let largest_text = if matches!(largest_value, Value::Undefined) {
        "days".into()
    } else {
        option_string(&largest_value)?
    };
    let largest_valid = matches!(
        largest_text.as_str(),
        "year" | "years" | "month" | "months" | "week" | "weeks" | "auto" | "day" | "days"
    );
    let largest = match largest_text.as_str() {
        "year" | "years" => "years",
        "month" | "months" => "months",
        "week" | "weeks" => "weeks",
        "auto" | "day" | "days" => "days",
        _ => "days",
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
    let mode_valid = matches!(
        mode.as_str(),
        "ceil"
            | "floor"
            | "expand"
            | "halfCeil"
            | "halfFloor"
            | "halfEven"
            | "halfExpand"
            | "halfTrunc"
            | "trunc"
    );
    let smallest_value = crate::execute::get_property_result(options, "smallestUnit")?;
    let smallest_text = if matches!(smallest_value, Value::Undefined) {
        "auto".into()
    } else {
        option_string(&smallest_value)?
    };
    let smallest_valid = matches!(
        smallest_text.as_str(),
        "year" | "years" | "month" | "months" | "week" | "weeks" | "day" | "days" | "auto"
    );
    let smallest: String = match smallest_text.as_str() {
        "year" | "years" => "years",
        "month" | "months" => "months",
        "week" | "weeks" => "weeks",
        "day" | "days" => "days",
        _ => "auto",
    }
    .into();
    if !largest_valid || !mode_valid || !smallest_valid {
        return Err(crate::value::error::throw_range_error("Invalid options"));
    }
    if smallest != "auto" && largest_explicit {
        let rank = |unit: &str| match unit {
            "years" => 0,
            "months" => 1,
            "weeks" => 2,
            _ => 3,
        };
        if rank(&smallest) < rank(largest) {
            return Err(crate::value::error::throw_range_error(
                "smallestUnit is larger than largestUnit",
            ));
        }
    }
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
    let calendar = calendar_name(object);
    if !year.is_finite()
        || !month.is_finite()
        || !day.is_finite()
        || !(-271_821.0..=275_760.0).contains(&year)
        || (!(1.0..=12.0).contains(&month)
            && !(month == 13.0 && calendar_supports_month13(&calendar)))
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok((year, month, day))
}

pub(crate) fn date_serial(year: f64, month: f64, day: f64) -> i64 {
    let year = year as i64 - i64::from(month <= 2.0);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = month as i64 + if month > 2.0 { -3 } else { 9 };
    era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + (153 * month_index + 2) / 5
        + day as i64
        - 1
}

fn shift_date(
    year: f64,
    month: f64,
    day: f64,
    delta: f64,
    calendar: &str,
) -> Result<Value, VmError> {
    let serial = date_serial(year, month, day) + delta as i64;
    let (year, month, day) = civil_from_serial(serial);
    construct(&[
        Value::Number(year as f64),
        Value::Number(month as f64),
        Value::Number(day as f64),
        Value::String(calendar.to_string()),
    ])
}

pub(crate) fn civil_from_serial(serial: i64) -> (i32, u32, u32) {
    let mut z = serial;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_index = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * month_index + 2).div_euclid(5) + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
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
    if year == -271_821.0
        && month == 4.0
        && day == 19.0
        && time
            .iter()
            .all(|value| matches!(value, Value::Number(number) if *number == 0.0))
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainDateTime",
        ));
    }
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
    let day_delta = i128::from(date_serial(year, month, day) - date_serial(1970.0, 1.0, 1.0));
    let time_nanos = i128::from(hour) * 3_600_000_000_000
        + i128::from(minute) * 60_000_000_000
        + i128::from(second) * 1_000_000_000
        + i128::from(nanos);
    let mut epoch = day_delta * 86_400_000_000_000 + time_nanos - fixed_timezone_offset(&timezone);
    if time_nanos == 0 {
        if let Some(start) = crate::temporal::timezone_start_of_day_epoch(&timezone, epoch) {
            epoch = start;
        }
    }
    if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
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
        if crate::temporal::looks_like_datetime_identifier(&text) {
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
            && !crate::temporal::looks_like_datetime_identifier(&text)
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
            let epoch = i128::from(date_serial(year, month, day) - date_serial(1970.0, 1.0, 1.0))
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
    let calendar = calendar_name(object);
    let value = calendar_days_in_month(year as i32, month as u32, &calendar)
        .unwrap_or_else(|| days_in_month(year, month) as u32);
    Ok(Value::Number(value as f64))
}

fn days_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let calendar = calendar_name(object);
    let value = calendar_days_in_year(year, month, &calendar)
        .unwrap_or_else(|| if is_leap_year(year) { 366 } else { 365 });
    Ok(Value::Number(value as f64))
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
    let year = number_field(field(object, "year")) as i32;
    let month = number_field(field(object, "month")) as u32;
    let calendar = calendar_name(object);
    Ok(Value::Number(
        calendar_months_in_year(year, month, &calendar).unwrap_or(12) as f64,
    ))
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
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
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
    let month = number_field(field(object, "month")) as u32;
    let calendar = calendar_name(object);
    Ok(Value::Boolean(
        calendar_is_leap_year(year, month, &calendar).unwrap_or_else(|| is_leap_year(year)),
    ))
}

fn era(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let calendar = calendar_name(object);
    let era = era_for_calendar(&calendar, number_field(field(object, "year")));
    Ok(era.map_or(Value::Undefined, |value| Value::String(value.into())))
}

fn era_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    let year = number_field(field(object, "year"));
    let calendar = calendar_name(object);
    let era_year = era_year_for_calendar(&calendar, year);
    Ok(era_year.map_or(Value::Undefined, Value::Number))
}

pub(crate) fn era_for_calendar(calendar: &str, year: f64) -> Option<&'static str> {
    match calendar {
        "buddhist" => Some("be"),
        "hebrew" => Some("am"),
        value if value.starts_with("islamic") && year > 0.0 => Some("ah"),
        value if value.starts_with("islamic") => Some("bh"),
        "persian" => Some("ap"),
        "coptic" => Some("am"),
        "ethiopic" if year > 0.0 => Some("am"),
        "ethiopic" => Some("aa"),
        "ethioaa" => Some("aa"),
        "indian" => Some("shaka"),
        "roc" if year >= 1.0 => Some("roc"),
        "roc" => Some("broc"),
        "japanese" => japanese_era(year),
        "gregory" if year >= 1.0 => Some("ce"),
        "gregory" => Some("bce"),
        _ => None,
    }
}

pub(crate) fn era_year_for_calendar(calendar: &str, year: f64) -> Option<f64> {
    match calendar {
        "buddhist" => Some(year),
        "hebrew" | "persian" | "coptic" => Some(year),
        "ethiopic" if year > 0.0 => Some(year),
        "ethiopic" => Some(year + 5500.0),
        "ethioaa" if year == -5500.0 => Some(0.0),
        "ethioaa" => Some(year),
        "gregory" => Some(if year >= 1.0 { year } else { 1.0 - year }),
        value if value.starts_with("islamic") && year > 0.0 => Some(year),
        value if value.starts_with("islamic") => Some(1.0 - year),
        "indian" => Some(year),
        "roc" if year >= 1.0 => Some(year),
        "roc" => Some(1.0 - year),
        "japanese" => Some(japanese_era_year(year)),
        _ => None,
    }
}

fn japanese_era(year: f64) -> Option<&'static str> {
    if year >= 2019.0 {
        Some("reiwa")
    } else if year >= 1989.0 {
        Some("heisei")
    } else if year >= 1926.0 {
        Some("showa")
    } else if year >= 1912.0 {
        Some("taisho")
    } else if year >= 1868.0 {
        Some("meiji")
    } else if year >= 1.0 {
        Some("ce")
    } else {
        Some("bce")
    }
}

fn japanese_era_year(year: f64) -> f64 {
    if year >= 2019.0 {
        year - 2018.0
    } else if year >= 1989.0 {
        year - 1988.0
    } else if year >= 1926.0 {
        year - 1925.0
    } else if year >= 1912.0 {
        year - 1911.0
    } else if year >= 1868.0 {
        year - 1867.0
    } else if year >= 1.0 {
        year
    } else {
        1.0 - year
    }
}

fn calendar_name(object: &crate::value::ObjectData) -> String {
    match object.iter().find_map(|(key, value)| {
        (key == "calendarId").then(|| match value {
            Value::String(value) => value.to_ascii_lowercase(),
            _ => "iso8601".into(),
        })
    }) {
        Some(value) => value,
        None => "iso8601".into(),
    }
}

fn calendar_id(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) = receiver.ok_or_else(invalid_receiver)? else {
        return Err(invalid_receiver());
    };
    if !has_date_fields(object) {
        return Err(invalid_receiver());
    }
    Ok(object
        .iter()
        .find_map(|(key, value)| {
            (key == "calendarId").then(|| value.clone())
        })
        .unwrap_or_else(|| Value::String("iso8601".to_owned())))
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
    let receiver_calendar = match receiver {
        Some(Value::Object(object)) => calendar_name(object),
        _ => "iso8601".to_string(),
    };
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    if is_temporal_date_like(changes) {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    if matches!(changes, Value::Array(_)) {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    if let Value::Object(object) = changes {
        let has_date_field = object
            .iter()
            .any(|(key, _)| matches!(key.as_str(), "year" | "month" | "monthCode" | "day"));
        if !has_date_field {
            return Err(crate::value::error::throw_type_error("Invalid fields"));
        }
    }
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    let time_zone = crate::execute::get_property_result(changes, "timeZone")?;
    if !matches!(calendar, Value::Undefined) || !matches!(time_zone, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    let mut day = number_or_field(changes, "day", day)?;
    let explicit_month = number_or_field(changes, "month", month)?;
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month_code_text = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(month_code_text(&month_code)?)
    };
    let year = number_or_field(changes, "year", year)?;
    let primitive_options = options
        .is_some_and(|value| !matches!(value, Value::Undefined) && !crate::value::is_object(value));
    let overflow = if let Some(value) = options.filter(|value| !primitive_options) {
        value
    } else {
        &Value::Undefined
    };
    let overflow = if matches!(overflow, Value::Undefined) {
        Value::String("constrain".into())
    } else {
        crate::execute::get_property_result(overflow, "overflow")?
    };
    let overflow = if matches!(overflow, Value::Undefined) {
        "constrain".to_owned()
    } else {
        option_string(&overflow)?
    };
    let month = if month_code_text.is_none() {
        explicit_month
    } else {
        let month = month_code_number_text(
            month_code_text.as_deref().unwrap_or_default(),
            calendar_supports_month13(&receiver_calendar),
        )?;
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
        || (month > 12.0
            && !(month == 13.0 && calendar_supports_month13(&receiver_calendar))
            && overflow == "reject")
        || day < 1.0
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if primitive_options {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let month = if calendar_supports_month13(&receiver_calendar) {
        month.min(13.0)
    } else {
        month.min(12.0)
    };
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
        Value::String(receiver_calendar),
    ])
}

pub(crate) fn is_temporal_date_like(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    object.iter().any(|(key, value)| {
        (key == "\0temporal-plain-date" || key == "\0temporal-plain-time" || key == "\0prototype")
            && matches!(
                value,
                Value::Boolean(true)
                    | Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype)
                    | Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
                    | Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype)
                    | Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype)
                    | Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype)
                    | Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
            )
    })
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
    let code = month_code_text(value)?;
    month_code_number_text(&code, false)
}

fn month_code_text(value: &Value) -> Result<String, VmError> {
    let primitive = crate::conversion::to_primitive(value, "string")?;
    if !matches!(primitive, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    crate::conversion::to_string(&primitive)
}

fn month_code_number_text(code: &str, allow_month13: bool) -> Result<f64, VmError> {
    code.strip_prefix('M')
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| (1.0..=12.0).contains(value) || (allow_month13 && *value == 13.0))
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
            && matches!(
                value,
                Value::Builtin(
                    crate::ops::Builtin::TemporalPlainDatePrototype
                        | crate::ops::Builtin::TemporalZonedDateTimePrototype
                )
            ))
            || (key == "\0temporal-plain-date" && value == Value::Boolean(true))
    }) && ["year", "month", "day"].iter().all(|name| {
        object
            .iter()
            .any(|(key, value)| key == *name && matches!(value, Value::Number(_)))
    })
}

pub(crate) fn is_iso_calendar_value(value: &Value) -> Result<bool, VmError> {
    let text = crate::conversion::to_string(value)?;
    if is_supported_calendar_name(&text) {
        return Ok(true);
    }
    if crate::intl::supported_calendars()
        .iter()
        .any(|value| matches!(value, Value::String(calendar) if calendar == &text.to_ascii_lowercase()))
    {
        return Ok(true);
    }
    if text.eq_ignore_ascii_case("gregory") {
        return Ok(true);
    }
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
    let mut date = base.split(['T', 't', ' ']).next().unwrap_or(base);
    if date.is_empty() && base.starts_with(['T', 't']) {
        date = &base[1..];
    }
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

/// Calendar identifiers are semantic data shared by every Temporal record.
/// The arithmetic core currently uses the ISO field projection, but retaining
/// a supported calendar identifier is still observable through `calendarId`
/// and must not be rejected at construction boundaries.
pub(crate) fn is_supported_calendar_name(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "islamicc" | "ethiopic-amete-alem")
        || crate::intl::supported_calendars().iter().any(|calendar| {
        matches!(calendar, Value::String(name) if name.eq_ignore_ascii_case(value))
    }) || value.eq_ignore_ascii_case("gregory")
}

pub(crate) fn canonical_calendar_id(value: &str) -> Option<String> {
    let value = value.to_ascii_lowercase();
    let canonical = match value.as_str() {
        "islamicc" => "islamic-civil",
        "ethiopic-amete-alem" => "ethioaa",
        other if is_supported_calendar_name(other) => other,
        _ => return None,
    };
    Some(canonical.to_string())
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
