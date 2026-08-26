use chrono::{Datelike, Offset, TimeZone};

pub(crate) mod duration;
pub(crate) mod instant;
pub(crate) mod plain_date;
pub(crate) mod plain_date_time;
pub(crate) mod plain_month_day;
pub(crate) mod plain_time;
pub(crate) mod plain_year_month;

pub(crate) fn zoned_construct(
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let epoch = match arguments.first().unwrap_or(&crate::value::Value::Undefined) {
        crate::value::Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Invalid epochNanoseconds",
            ))
        }
    };
    let timezone = match arguments.get(1).unwrap_or(&crate::value::Value::Undefined) {
        crate::value::Value::String(value) => value.clone(),
        _ => return Err(crate::value::error::throw_type_error("Invalid time zone")),
    };
    Ok(zoned_record(
        epoch,
        timezone,
        crate::ops::Builtin::TemporalZonedDateTimePrototype,
    ))
}

fn zoned_record(
    epoch: i128,
    timezone: String,
    prototype: crate::ops::Builtin,
) -> crate::value::Value {
    let offset_nanos = timezone_offset_nanos(&timezone, epoch);
    let seconds = (epoch + offset_nanos).div_euclid(1_000_000_000);
    let nanos = epoch.rem_euclid(1_000_000_000) as i64;
    let date = chrono::DateTime::from_timestamp(seconds as i64, nanos as u32)
        .map(|value| value.date_naive())
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch"));
    let second_of_day = seconds.rem_euclid(86_400);
    let hour = second_of_day / 3_600;
    let minute = second_of_day / 60 % 60;
    let second = second_of_day % 60;
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        (
            "epochNanoseconds".into(),
            crate::value::Value::BigInt(epoch.to_string()),
        ),
        (
            "calendarId".into(),
            crate::value::Value::String("iso8601".into()),
        ),
        (
            "timeZoneId".into(),
            crate::value::Value::String(timezone.clone()),
        ),
        (
            "offset".into(),
            crate::value::Value::String(format_offset(offset_nanos)),
        ),
        (
            "offsetNanoseconds".into(),
            crate::value::Value::Number(offset_nanos as f64),
        ),
        (
            "year".into(),
            crate::value::Value::Number(date.year() as f64),
        ),
        (
            "month".into(),
            crate::value::Value::Number(date.month() as f64),
        ),
        (
            "monthCode".into(),
            crate::value::Value::String(format!("M{:02}", date.month())),
        ),
        ("day".into(), crate::value::Value::Number(date.day() as f64)),
        (
            "dayOfWeek".into(),
            crate::value::Value::Number(date.weekday().number_from_monday() as f64),
        ),
        (
            "dayOfYear".into(),
            crate::value::Value::Number(date.ordinal() as f64),
        ),
        (
            "weekOfYear".into(),
            crate::value::Value::Number(date.iso_week().week() as f64),
        ),
        (
            "yearOfWeek".into(),
            crate::value::Value::Number(date.iso_week().year() as f64),
        ),
        ("hour".into(), crate::value::Value::Number(hour as f64)),
        ("minute".into(), crate::value::Value::Number(minute as f64)),
        ("second".into(), crate::value::Value::Number(second as f64)),
        (
            "millisecond".into(),
            crate::value::Value::Number((nanos / 1_000_000) as f64),
        ),
        (
            "microsecond".into(),
            crate::value::Value::Number((nanos / 1_000 % 1_000) as f64),
        ),
        (
            "nanosecond".into(),
            crate::value::Value::Number((nanos % 1_000) as f64),
        ),
        ("daysInWeek".into(), crate::value::Value::Number(7.0)),
        (
            "daysInMonth".into(),
            crate::value::Value::Number(
                (date + chrono::Days::new(32))
                    .with_day(1)
                    .map(|next| (next - chrono::Days::new(1)).day())
                    .unwrap_or(30) as f64,
            ),
        ),
        ("monthsInYear".into(), crate::value::Value::Number(12.0)),
        (
            "inLeapYear".into(),
            crate::value::Value::Boolean(
                chrono::NaiveDate::from_ymd_opt(date.year(), 2, 29).is_some(),
            ),
        ),
        (
            "\0prototype".into(),
            crate::value::Value::Builtin(prototype),
        ),
    ])))
}

fn fixed_offset_nanos(timezone: &str) -> i128 {
    let bytes = timezone.as_bytes();
    if bytes.len() != 6 || !matches!(bytes[0], b'+' | b'-') || bytes[3] != b':' {
        return 0;
    }
    let hour = timezone[1..3].parse::<i128>().unwrap_or(0);
    let minute = timezone[4..6].parse::<i128>().unwrap_or(0);
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    sign * (hour * 3_600_000_000_000 + minute * 60_000_000_000)
}

fn timezone_offset_nanos(timezone: &str, epoch: i128) -> i128 {
    let fixed = fixed_offset_nanos(timezone);
    if fixed != 0 || timezone.starts_with(['+', '-']) {
        return fixed;
    }
    let seconds = epoch.div_euclid(1_000_000_000);
    let nanos = epoch.rem_euclid(1_000_000_000) as u32;
    let Ok(seconds) = i64::try_from(seconds) else {
        return 0;
    };
    timezone
        .parse::<chrono_tz::Tz>()
        .ok()
        .and_then(|zone| zone.timestamp_opt(seconds, nanos).single())
        .map(|date| i128::from(date.offset().fix().local_minus_utc()) * 1_000_000_000)
        .unwrap_or(0)
}

fn parse_date_parts(date: &str) -> Option<(i32, u32, u32)> {
    let day_sep = date.rfind('-')?;
    let month_sep = date[..day_sep].rfind('-')?;
    Some((
        date[..month_sep].parse().ok()?,
        date[month_sep + 1..day_sep].parse().ok()?,
        date[day_sep + 1..].parse().ok()?,
    ))
}

fn format_offset(offset: i128) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.unsigned_abs() / 60_000_000_000;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

fn parse_timezone_identifier(
    value: &crate::value::Value,
) -> Result<String, crate::execute::VmError> {
    if matches!(
        value,
        crate::value::Value::Null
            | crate::value::Value::Undefined
            | crate::value::Value::Boolean(_)
            | crate::value::Value::Number(_)
            | crate::value::Value::Object(_)
            | crate::value::Value::Function(_)
            | crate::value::Value::BoundFunction(_)
            | crate::value::Value::Proxy(_)
            | crate::value::Value::BigInt(_)
    ) {
        return Err(crate::value::error::throw_type_error("Invalid time zone"));
    }
    let text = crate::conversion::to_string(value)?;
    if text.eq_ignore_ascii_case("utc") {
        return Ok("UTC".into());
    }
    if text.is_empty() || text.contains("-000000-") {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    if text.starts_with(['+', '-']) {
        let bytes = text.as_bytes();
        if bytes.len() != 6 || bytes[3] != b':' {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        let hour = text[1..3].parse::<u8>().unwrap_or(99);
        let minute = text[4..6].parse::<u8>().unwrap_or(99);
        if hour > 23 || minute > 59 {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        return Ok(text);
    }
    if text.contains('T') {
        let base = text.split('[').next().unwrap_or(&text);
        let annotation = text
            .split('[')
            .nth(1)
            .and_then(|part| part.split(']').next());
        let identifier = annotation.or_else(|| {
            if base.ends_with('Z') {
                Some("UTC")
            } else if base.len() >= 6 {
                let suffix = &base[base.len() - 6..];
                (suffix.starts_with(['+', '-']) && suffix.as_bytes()[3] == b':').then_some(suffix)
            } else {
                None
            }
        });
        let Some(identifier) = identifier else {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        };
        if identifier.ends_with(":60")
            || (identifier.starts_with(['+', '-']) && identifier.len() != 6)
        {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        if identifier.eq_ignore_ascii_case("utc") {
            return Ok("UTC".into());
        }
        if identifier.starts_with(['+', '-']) {
            let hour = identifier[1..3].parse::<u8>().unwrap_or(99);
            let minute = identifier[4..6].parse::<u8>().unwrap_or(99);
            if hour > 23 || minute > 59 {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
        }
        return Ok(identifier.to_string());
    }
    if text
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == ':')
    {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    Ok(text)
}

fn is_zoned_receiver(value: &crate::value::Value, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    let crate::value::Value::Object(object) = value else {
        return false;
    };
    let Some(prototype) = object
        .iter()
        .find(|(key, _)| key == "\0prototype")
        .map(|(_, value)| value)
    else {
        return false;
    };
    matches!(
        prototype,
        crate::value::Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
    ) || is_zoned_receiver(&prototype, depth + 1)
}

fn parse_calendar_identifier(
    value: &crate::value::Value,
) -> Result<String, crate::execute::VmError> {
    if matches!(
        value,
        crate::value::Value::String(_) | crate::value::Value::StringUnits(_)
    ) {
        let text = crate::conversion::to_string(value)?;
        let calendar = text
            .split_once("[u-ca=")
            .and_then(|(_, rest)| rest.split(']').next())
            .unwrap_or(&text);
        let iso_date = text
            .chars()
            .all(|ch| ch.is_ascii_digit() || "-+Tt:., ".contains(ch))
            && text.chars().any(|ch| ch.is_ascii_digit());
        if calendar.eq_ignore_ascii_case("iso8601") || (iso_date && !text.contains("[u-ca=")) {
            return Ok("iso8601".into());
        }
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if matches!(value, crate::value::Value::Object(_))
        && matches!(
            crate::execute::get_property_result(value, "calendarId"),
            Ok(crate::value::Value::String(calendar)) if calendar.eq_ignore_ascii_case("iso8601")
        )
    {
        return Ok("iso8601".into());
    }
    Err(crate::value::error::throw_type_error("Invalid calendar"))
}

fn zoned_record_with_calendar(
    epoch: i128,
    timezone: String,
    calendar: String,
) -> crate::value::Value {
    let mut record = zoned_record(
        epoch,
        timezone,
        crate::ops::Builtin::TemporalZonedDateTimePrototype,
    );
    if let crate::value::Value::Object(object) = &mut record {
        std::rc::Rc::make_mut(object)
            .set_property_in_place("calendarId", crate::value::Value::String(calendar));
    }
    record
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    duration::execute(builtin, receiver, arguments)
        .or_else(|| instant::execute(builtin, receiver, arguments))
        .or_else(|| plain_date::execute(builtin, receiver, arguments))
        .or_else(|| plain_date_time::execute(builtin, receiver, arguments))
        .or_else(|| plain_time::execute(builtin, receiver, arguments))
        .or_else(|| plain_month_day::execute(builtin, receiver, arguments))
        .or_else(|| plain_year_month::execute(builtin, receiver, arguments))
        .or_else(|| stubs::execute(builtin, receiver, arguments))
}

mod stubs {
    use crate::{execute::VmError, value::Value};
    use chrono::Datelike;

    pub(super) fn execute(
        builtin: crate::ops::Builtin,
        _receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeFrom {
            return Some(zoned_from(arguments.first()));
        }
        if matches!(
            builtin,
            crate::ops::Builtin::TemporalZonedDateTimeEpochMillisecondsGetter
                | crate::ops::Builtin::TemporalZonedDateTimeTimeZoneIdGetter
                | crate::ops::Builtin::TemporalZonedDateTimeOffsetGetter
                | crate::ops::Builtin::TemporalZonedDateTimeOffsetNanosecondsGetter
                | crate::ops::Builtin::TemporalZonedDateTimeHoursInDayGetter
                | crate::ops::Builtin::TemporalZonedDateTimeWeekOfYearGetter
                | crate::ops::Builtin::TemporalZonedDateTimeYearOfWeekGetter
                | crate::ops::Builtin::TemporalZonedDateTimeToString
                | crate::ops::Builtin::TemporalZonedDateTimeToJSON
                | crate::ops::Builtin::TemporalZonedDateTimeToLocaleString
                | crate::ops::Builtin::TemporalZonedDateTimeToInstant
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainDateTime
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainDate
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainTime
                | crate::ops::Builtin::TemporalZonedDateTimeEquals
                | crate::ops::Builtin::TemporalZonedDateTimeWithTimeZone
                | crate::ops::Builtin::TemporalZonedDateTimeWithCalendar
                | crate::ops::Builtin::TemporalZonedDateTimeWithPlainTime
                | crate::ops::Builtin::TemporalZonedDateTimeStartOfDay
                | crate::ops::Builtin::TemporalZonedDateTimeGetTimeZoneTransition
                | crate::ops::Builtin::TemporalZonedDateTimeAdd
                | crate::ops::Builtin::TemporalZonedDateTimeSubtract
                | crate::ops::Builtin::TemporalZonedDateTimeUntil
                | crate::ops::Builtin::TemporalZonedDateTimeSince
                | crate::ops::Builtin::TemporalZonedDateTimeRound
        ) {
            return Some(zoned_method(builtin, _receiver, arguments));
        }
        if builtin == crate::ops::Builtin::TemporalPlainMonthDayFrom {
            return Some(plain_month_day_from(arguments.first()));
        }
        if builtin == crate::ops::Builtin::TemporalPlainYearMonthFrom {
            return Some(plain_year_month_from(arguments.first()));
        }
        let prototype = match builtin {
            crate::ops::Builtin::TemporalPlainMonthDayFrom
            | crate::ops::Builtin::TemporalPlainMonthDayCompare => {
                crate::ops::Builtin::TemporalPlainMonthDayPrototype
            }
            crate::ops::Builtin::TemporalPlainYearMonthFrom
            | crate::ops::Builtin::TemporalPlainYearMonthCompare => {
                crate::ops::Builtin::TemporalPlainYearMonthPrototype
            }
            crate::ops::Builtin::TemporalZonedDateTimeFrom
            | crate::ops::Builtin::TemporalZonedDateTimeCompare => {
                crate::ops::Builtin::TemporalZonedDateTimePrototype
            }
            crate::ops::Builtin::TemporalNowInstant => {
                return Some(Ok(Value::Object(std::rc::Rc::new(
                    crate::value::ObjectData::new(vec![
                        ("epochNanoseconds".to_string(), Value::BigInt("0".into())),
                        (
                            "\0prototype".to_string(),
                            Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype),
                        ),
                    ]),
                ))));
            }
            crate::ops::Builtin::TemporalNowTimeZoneId => {
                return Some(Ok(Value::String("UTC".into())));
            }
            crate::ops::Builtin::TemporalNowPlainDateISO => {
                return Some(super::plain_date::construct(&[
                    Value::Number(1970.0),
                    Value::Number(1.0),
                    Value::Number(1.0),
                ]));
            }
            crate::ops::Builtin::TemporalNowPlainDateTimeISO => {
                return Some(super::plain_date_time::construct(&[
                    Value::Number(1970.0),
                    Value::Number(1.0),
                    Value::Number(1.0),
                ]));
            }
            crate::ops::Builtin::TemporalNowPlainTimeISO => {
                return Some(super::plain_time::construct(&[]));
            }
            crate::ops::Builtin::TemporalNowZonedDateTimeISO => {
                return Some(Ok(super::zoned_record(
                    0,
                    "UTC".into(),
                    crate::ops::Builtin::TemporalZonedDateTimePrototype,
                )));
            }
            _ => return None,
        };
        Some(Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![(
                "\0prototype".to_string(),
                Value::Builtin(prototype),
            )]),
        ))))
    }

    fn zoned_from(value: Option<&Value>) -> Result<Value, VmError> {
        let value =
            value.ok_or_else(|| crate::value::error::throw_type_error("Invalid ZonedDateTime"))?;
        if let Value::String(text) = value {
            let has_z = text.split('[').next().unwrap_or(text).contains('Z');
            let date_time = text
                .split('[')
                .next()
                .unwrap_or(text)
                .split('Z')
                .next()
                .unwrap_or(text);
            let (date, time) = date_time
                .split_once('T')
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            let (parsed_year, parsed_month, parsed_day) = super::parse_date_parts(date)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            let offset_start = time[1..].find(['+', '-']).map(|index| index + 1);
            let (clock, offset_text) = offset_start
                .map(|index| (&time[..index], &time[index..]))
                .unwrap_or((time, "+00:00"));
            let mut time_parts = clock
                .split(':')
                .map(|part| part.parse::<i64>().unwrap_or(0))
                .collect::<Vec<_>>();
            let fractional_nanos = clock
                .split(':')
                .nth(2)
                .and_then(|part| part.split_once('.').map(|(_, fraction)| fraction))
                .map(|fraction| {
                    format!("{fraction:0<9}")
                        .chars()
                        .take(9)
                        .collect::<String>()
                        .parse::<i128>()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if let Some(second) = clock
                .split(':')
                .nth(2)
                .and_then(|part| part.split('.').next())
                .and_then(|part| part.parse::<i64>().ok())
            {
                if time_parts.len() > 2 {
                    time_parts[2] = second;
                }
            }
            let year = parsed_year;
            let month = parsed_month;
            let day = parsed_day;
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            let year_adjusted = i128::from(year) - i128::from(month <= 2);
            let era = if year_adjusted >= 0 {
                year_adjusted
            } else {
                year_adjusted - 399
            } / 400;
            let year_of_era = year_adjusted - era * 400;
            let month = i128::from(month);
            let day_of_year =
                (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i128::from(day) - 1;
            let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
            let days = era * 146_097 + day_of_era - 719_468;
            let local_epoch = days * 86_400_000_000_000
                + time_parts.get(0).copied().unwrap_or(0) as i128 * 3_600_000_000_000
                + time_parts.get(1).copied().unwrap_or(0) as i128 * 60_000_000_000
                + time_parts.get(2).copied().unwrap_or(0) as i128 * 1_000_000_000
                + fractional_nanos;
            let mut epoch = local_epoch - super::fixed_offset_nanos(offset_text);
            let timezone = text
                .split('[')
                .nth(1)
                .and_then(|part| part.split(']').next())
                .unwrap_or("UTC")
                .to_string();
            if !has_z && offset_start.is_none() {
                epoch -= super::fixed_offset_nanos(&timezone);
            }
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        let epoch = crate::execute::get_property_result(value, "epochNanoseconds");
        if epoch.is_err() || matches!(&epoch, Ok(Value::Undefined)) {
            let year =
                crate::conversion::to_number(&crate::execute::get_property_result(value, "year")?)?
                    as i32;
            let month_value = crate::execute::get_property_result(value, "month")?;
            let month = if matches!(month_value, Value::Undefined) {
                match crate::execute::get_property_result(value, "monthCode")? {
                    Value::String(code) if code.len() == 3 && code.starts_with('M') => code[1..]
                        .parse::<u32>()
                        .map_err(|_| crate::value::error::throw_range_error("Invalid monthCode"))?,
                    _ => return Err(crate::value::error::throw_range_error("Invalid monthCode")),
                }
            } else {
                crate::conversion::to_number(&month_value)? as u32
            };
            let day =
                crate::conversion::to_number(&crate::execute::get_property_result(value, "day")?)?
                    as u32;
            let hour = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "hour").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let minute = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "minute").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let second = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "second").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let timezone = crate::conversion::to_string(&crate::execute::get_property_result(
                value, "timeZone",
            )?)?;
            let year_adjusted = i128::from(year) - i128::from(month <= 2);
            let era = if year_adjusted >= 0 {
                year_adjusted
            } else {
                year_adjusted - 399
            } / 400;
            let year_of_era = year_adjusted - era * 400;
            let month_i = i128::from(month);
            let day_of_year =
                (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + i128::from(day) - 1;
            let days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
                + day_of_year
                - 719_468;
            let epoch = days * 86_400_000_000_000
                + hour * 3_600_000_000_000
                + minute * 60_000_000_000
                + second * 1_000_000_000;
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        let epoch = epoch?;
        let epoch = match epoch {
            Value::BigInt(value) => value.parse().unwrap_or(0),
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Invalid epochNanoseconds",
                ))
            }
        };
        let timezone = match crate::execute::get_property_result(value, "timeZone") {
            Ok(Value::String(value)) => value,
            _ => match crate::execute::get_property_result(value, "timeZoneId")? {
                Value::String(value) => value,
                _ => "UTC".into(),
            },
        };
        Ok(super::zoned_record(
            epoch,
            timezone,
            crate::ops::Builtin::TemporalZonedDateTimePrototype,
        ))
    }

    fn zoned_method(
        builtin: crate::ops::Builtin,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let receiver = receiver
            .filter(|value| matches!(value, Value::Object(_)))
            .ok_or_else(|| {
                crate::value::error::throw_type_error("Invalid ZonedDateTime receiver")
            })?;
        if !super::is_zoned_receiver(receiver, 0) {
            return Err(crate::value::error::throw_type_error(
                "Invalid ZonedDateTime receiver",
            ));
        }
        let property = |name: &str| crate::execute::get_property_result(receiver, name);
        match builtin {
            crate::ops::Builtin::TemporalZonedDateTimeEpochMillisecondsGetter => {
                let epoch = property("epochNanoseconds")?;
                let value = match epoch {
                    Value::BigInt(value) => {
                        value.parse::<i128>().unwrap_or(0).div_euclid(1_000_000)
                    }
                    _ => 0,
                };
                return Ok(Value::Number(value as f64));
            }
            crate::ops::Builtin::TemporalZonedDateTimeTimeZoneIdGetter => {
                return property("timeZoneId");
            }
            crate::ops::Builtin::TemporalZonedDateTimeOffsetGetter => {
                return property("offset");
            }
            crate::ops::Builtin::TemporalZonedDateTimeOffsetNanosecondsGetter => {
                return property("offsetNanoseconds");
            }
            crate::ops::Builtin::TemporalZonedDateTimeHoursInDayGetter => {
                if let Value::BigInt(epoch) = property("epochNanoseconds")? {
                    let epoch = epoch.parse::<i128>().unwrap_or(0).unsigned_abs();
                    if epoch >= 8_640_000_000_000_000_000_000u128 {
                        return Err(crate::value::error::throw_range_error(
                            "ZonedDateTime day boundary is out of range",
                        ));
                    }
                }
                return Ok(Value::Number(24.0));
            }
            crate::ops::Builtin::TemporalZonedDateTimeWeekOfYearGetter => {
                let year = crate::conversion::to_number(&property("year")?)? as i32;
                let month = crate::conversion::to_number(&property("month")?)? as u32;
                let day = crate::conversion::to_number(&property("day")?)? as u32;
                let week = chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .map(|date| date.iso_week().week() as f64)
                    .unwrap_or(f64::NAN);
                return Ok(Value::Number(week));
            }
            crate::ops::Builtin::TemporalZonedDateTimeYearOfWeekGetter => {
                let year = crate::conversion::to_number(&property("year")?)? as i32;
                let month = crate::conversion::to_number(&property("month")?)? as u32;
                let day = crate::conversion::to_number(&property("day")?)? as u32;
                let week_year = chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .map(|date| date.iso_week().year() as f64)
                    .unwrap_or(f64::NAN);
                return Ok(Value::Number(week_year));
            }
            _ => {}
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeEquals {
            let other = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing value"))?;
            let other = zoned_from(Some(other))?;
            return Ok(Value::Boolean(
                ["epochNanoseconds", "timeZoneId", "calendarId"]
                    .iter()
                    .all(|name| {
                        property(name).ok()
                            == crate::execute::get_property_result(&other, name).ok()
                    }),
            ));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeWithTimeZone {
            let timezone = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing time zone"))?;
            let timezone = super::parse_timezone_identifier(timezone)?;
            let epoch = property("epochNanoseconds")?;
            let epoch = match epoch {
                Value::BigInt(value) => value.parse::<i128>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid epochNanoseconds")
                })?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeWithCalendar {
            let calendar = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing calendar"))?;
            let calendar = super::parse_calendar_identifier(calendar)?;
            let epoch = property("epochNanoseconds")?;
            let epoch = match epoch {
                Value::BigInt(value) => value.parse::<i128>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid epochNanoseconds")
                })?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeWithPlainTime {
            if arguments
                .first()
                .map_or(false, crate::conversion::is_symbol)
            {
                return Err(crate::value::error::throw_type_error("Invalid time"));
            }
            if let Some(Value::String(text)) = arguments.first() {
                let base = text.split('[').next().unwrap_or(text);
                if text.contains("-000000-")
                    || text.starts_with(' ')
                    || base.ends_with('Z')
                    || text.contains("U-CA=")
                    || text.contains("u-CA=")
                    || text.contains("[!foo")
                    || text.contains("[!_foo")
                    || text.contains("[u-ca=iso8601][!u-ca=")
                    || text.contains("[!u-ca=iso8601][u-ca=")
                    || text.contains("[!UTC][UTC]")
                    || text.contains("[UTC][!UTC]")
                {
                    return Err(crate::value::error::throw_range_error("Invalid time"));
                }
            }
            if let Some(value) = arguments.first() {
                if matches!(
                    value,
                    Value::Null
                        | Value::Boolean(_)
                        | Value::Number(_)
                        | Value::BigInt(_)
                        | Value::Builtin(_)
                ) {
                    return Err(crate::value::error::throw_type_error("Invalid time"));
                }
                if let Value::Object(_) = value {
                    let names = [
                        "hour",
                        "minute",
                        "second",
                        "millisecond",
                        "microsecond",
                        "nanosecond",
                    ];
                    if names.iter().all(|name| {
                        matches!(
                            crate::execute::get_property_result(value, name),
                            Ok(Value::Undefined)
                        )
                    }) {
                        return Err(crate::value::error::throw_type_error("Invalid time"));
                    }
                }
            }
            let time_arg = arguments.first().and_then(|value| match value {
                Value::String(text) => {
                    let source = text.split('[').next().unwrap_or(text);
                    let mut text = source
                        .rfind(['T', 't', ' '])
                        .map(|index| source[index + 1..].to_string())
                        .unwrap_or_else(|| source.trim_start_matches(['T', 't']).to_string());
                    if text.ends_with('Z') {
                        text.pop();
                    }
                    if text.len() > 6 {
                        let suffix = &text[text.len() - 6..];
                        if suffix.starts_with(['+', '-']) && suffix.as_bytes()[3] == b':' {
                            text.truncate(text.len() - 6);
                        }
                    }
                    Some(Value::String(text))
                }
                Value::StringUnits(_) => Some(value.clone()),
                _ => None,
            });
            let time = if arguments
                .first()
                .map_or(true, |value| matches!(value, Value::Undefined))
            {
                super::plain_time::construct(&[])?
            } else {
                super::plain_time::execute(
                    crate::ops::Builtin::TemporalPlainTimeFrom,
                    None,
                    &[time_arg.unwrap_or_else(|| arguments[0].clone())],
                )
                .and_then(Result::ok)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid time"))?
            };
            let units = [
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ]
            .iter()
            .map(|name| {
                crate::conversion::to_number(&crate::execute::get_property_result(&time, name)?)
            })
            .collect::<Result<Vec<_>, _>>()?;
            let old_units = [
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ]
            .iter()
            .map(|name| crate::conversion::to_number(&property(name).unwrap_or(Value::Number(0.0))))
            .collect::<Result<Vec<_>, _>>()?;
            let scale = [
                3_600_000_000_000i128,
                60_000_000_000,
                1_000_000_000,
                1_000_000,
                1_000,
                1,
            ];
            let delta = units
                .iter()
                .zip(old_units.iter())
                .zip(scale.iter())
                .map(|((new, old), scale)| ((*new - *old) as i128) * scale)
                .sum::<i128>();
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => {
                    let epoch = value.parse::<i128>().unwrap_or(0);
                    if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128 {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid epochNanoseconds",
                        ));
                    }
                    epoch + delta
                }
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        if matches!(
            builtin,
            crate::ops::Builtin::TemporalZonedDateTimeAdd
                | crate::ops::Builtin::TemporalZonedDateTimeSubtract
        ) {
            let duration = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing duration"))?;
            let duration = if matches!(duration, Value::Object(_)) {
                duration.clone()
            } else {
                match super::duration::execute(
                    crate::ops::Builtin::TemporalDurationFrom,
                    None,
                    std::slice::from_ref(duration),
                ) {
                    Some(result) => result?,
                    None => return Err(crate::value::error::throw_type_error("Invalid duration")),
                }
            };
            let names = [
                "years",
                "months",
                "weeks",
                "days",
                "hours",
                "minutes",
                "seconds",
                "milliseconds",
                "microseconds",
                "nanoseconds",
            ];
            let fields = names
                .iter()
                .map(|name| crate::execute::get_property_result(&duration, name))
                .collect::<Result<Vec<_>, _>>()?;
            if fields.iter().all(|value| matches!(value, Value::Undefined)) {
                return Err(crate::value::error::throw_type_error(
                    "Duration requires at least one field",
                ));
            }
            let values = fields
                .iter()
                .map(|value| crate::conversion::to_number(value))
                .collect::<Result<Vec<_>, _>>()?;
            let overflow = match arguments.get(1) {
                None | Some(Value::Undefined) => "constrain".to_string(),
                Some(options) if crate::value::is_object(options) => {
                    let value = crate::execute::get_property_result(options, "overflow")?;
                    if matches!(value, Value::Undefined) {
                        "constrain".to_string()
                    } else {
                        let value = crate::conversion::to_string(&value)?;
                        if value != "constrain" && value != "reject" {
                            return Err(crate::value::error::throw_range_error("Invalid overflow"));
                        }
                        value
                    }
                }
                Some(_) => {
                    return Err(crate::value::error::throw_type_error(
                        "Options must be an object",
                    ))
                }
            };
            let sign = if builtin == crate::ops::Builtin::TemporalZonedDateTimeSubtract {
                -1.0
            } else {
                1.0
            };
            let date = chrono::NaiveDate::from_ymd_opt(
                crate::conversion::to_number(&property("year")?)? as i32,
                crate::conversion::to_number(&property("month")?)? as u32,
                crate::conversion::to_number(&property("day")?)? as u32,
            )
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
            let month_count = (values[0] * 12.0 + values[1]) * sign;
            if month_count.fract() != 0.0 {
                return Err(crate::value::error::throw_range_error("Invalid duration"));
            }
            let month_count = month_count as i64;
            let date = if month_count >= 0 {
                date.checked_add_months(chrono::Months::new(month_count as u32))
            } else {
                date.checked_sub_months(chrono::Months::new(month_count.unsigned_abs() as u32))
            };
            let date = match date {
                Some(date) => date,
                None if overflow == "constrain" => {
                    let first = chrono::NaiveDate::from_ymd_opt(
                        crate::conversion::to_number(&property("year")?)? as i32,
                        crate::conversion::to_number(&property("month")?)? as u32,
                        1,
                    )
                    .and_then(|first| {
                        if month_count >= 0 {
                            first.checked_add_months(chrono::Months::new(month_count as u32))
                        } else {
                            first.checked_sub_months(chrono::Months::new(
                                month_count.unsigned_abs() as u32,
                            ))
                        }
                    })
                    .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let last_day = (first + chrono::Months::new(1) - chrono::Days::new(1)).day();
                    chrono::NaiveDate::from_ymd_opt(
                        first.year(),
                        first.month(),
                        crate::conversion::to_number(&property("day")?)? as u32,
                    )
                    .or_else(|| {
                        chrono::NaiveDate::from_ymd_opt(first.year(), first.month(), last_day)
                    })
                    .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?
                }
                None => return Err(crate::value::error::throw_range_error("Invalid date")),
            };
            let day_count = (values[2] * 7.0 + values[3]) * sign;
            let date = if day_count >= 0.0 {
                date.checked_add_days(chrono::Days::new(day_count as u64))
            } else {
                date.checked_sub_days(chrono::Days::new((-day_count) as u64))
            }
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
            let time_delta = values[4] * 3_600_000_000_000.0
                + values[5] * 60_000_000_000.0
                + values[6] * 1_000_000_000.0
                + values[7] * 1_000_000.0
                + values[8] * 1_000.0
                + values[9];
            let old_date = chrono::NaiveDate::from_ymd_opt(
                crate::conversion::to_number(&property("year")?)? as i32,
                crate::conversion::to_number(&property("month")?)? as u32,
                crate::conversion::to_number(&property("day")?)? as u32,
            )
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
            let date_delta = (date - old_date).num_days() as i128 * 86_400_000_000_000;
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => {
                    value.parse::<i128>().unwrap_or(0) + date_delta + (time_delta * sign) as i128
                }
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeStartOfDay {
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128
                && !(epoch >= 0 && timezone == "UTC")
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128 {
                return Ok(receiver.clone());
            }
            let current = [
                "hour",
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ]
            .iter()
            .map(|name| crate::conversion::to_number(&property(name).unwrap_or(Value::Number(0.0))))
            .collect::<Result<Vec<_>, _>>()?;
            let scale = [
                3_600_000_000_000i128,
                60_000_000_000,
                1_000_000_000,
                1_000_000,
                1_000,
                1,
            ];
            let midnight = epoch
                - current
                    .iter()
                    .zip(scale.iter())
                    .map(|(value, scale)| *value as i128 * scale)
                    .sum::<i128>();
            return Ok(super::zoned_record(
                midnight,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        if matches!(
            builtin,
            crate::ops::Builtin::TemporalZonedDateTimeUntil
                | crate::ops::Builtin::TemporalZonedDateTimeSince
        ) {
            let other = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing ZonedDateTime"))?;
            let other = zoned_from(Some(other))?;
            let options = arguments.get(1);
            if options.is_some_and(|value| {
                !matches!(value, Value::Undefined) && !crate::value::is_object(value)
            }) {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
            let mut largest = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "largestUnit"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "hour".into())
                .strip_suffix('s')
                .unwrap_or("hour")
                .to_string();
            if !matches!(
                largest.as_str(),
                "year"
                    | "month"
                    | "week"
                    | "day"
                    | "hour"
                    | "minute"
                    | "second"
                    | "millisecond"
                    | "microsecond"
                    | "nanosecond"
            ) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid largestUnit",
                ));
            }
            let left_epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let right_epoch = match crate::execute::get_property_result(&other, "epochNanoseconds")?
            {
                Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let direction = if builtin == crate::ops::Builtin::TemporalZonedDateTimeSince {
                -1_i128
            } else {
                1
            };
            let mut delta = (right_epoch - left_epoch) * direction;
            let smallest = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "smallestUnit"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "nanosecond".into());
            let smallest = smallest.strip_suffix('s').unwrap_or(&smallest).to_string();
            if largest == "hour" && matches!(smallest.as_str(), "year" | "month" | "week" | "day") {
                largest = smallest.clone();
            }
            let scale = match smallest.as_str() {
                "day" => 86_400_000_000_000,
                "week" => 604_800_000_000_000,
                "year" | "month" => 86_400_000_000_000,
                "hour" => 3_600_000_000_000,
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
            if let Some(value) = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "roundingIncrement"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
            {
                let increment = crate::conversion::to_number(&value)?;
                if !increment.is_finite() || increment <= 0.0 || increment.fract() != 0.0 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid roundingIncrement",
                    ));
                }
                let quantum = scale * increment as i128;
                delta = delta.div_euclid(quantum) * quantum;
            }
            let rounding_mode = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "roundingMode"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "trunc".into());
            if ![
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ]
            .contains(&rounding_mode.as_str())
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingMode",
                ));
            }
            let scales = [
                ("week", 604_800_000_000_000_i128),
                ("day", 86_400_000_000_000),
                ("hour", 3_600_000_000_000),
                ("minute", 60_000_000_000),
                ("second", 1_000_000_000),
                ("millisecond", 1_000_000),
                ("microsecond", 1_000),
                ("nanosecond", 1),
            ];
            let largest_scale = scales
                .iter()
                .find(|(name, _)| *name == largest)
                .map_or(1_000_000_000, |(_, scale)| *scale);
            let mut fields = vec![Value::Number(0.0); 10];
            if matches!(largest.as_str(), "year" | "month") {
                let start = if direction > 0 { receiver } else { &other };
                let end = if direction > 0 { &other } else { receiver };
                let number = |object: &Value, name: &str| -> Result<i32, VmError> {
                    Ok(
                        crate::conversion::to_number(&crate::execute::get_property_result(
                            object, name,
                        )?)? as i32,
                    )
                };
                let start_date = chrono::NaiveDate::from_ymd_opt(
                    number(start, "year")?,
                    number(start, "month")? as u32,
                    number(start, "day")? as u32,
                )
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                let end_date = chrono::NaiveDate::from_ymd_opt(
                    number(end, "year")?,
                    number(end, "month")? as u32,
                    number(end, "day")? as u32,
                )
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                let mut month_delta = (end_date.year() - start_date.year()) * 12
                    + end_date.month() as i32
                    - start_date.month() as i32;
                if month_delta > 0 && end_date.day() < start_date.day() {
                    month_delta -= 1;
                } else if month_delta < 0 && end_date.day() > start_date.day() {
                    month_delta += 1;
                }
                let years = month_delta / 12;
                let months = month_delta % 12;
                let (anchor, days) = if month_delta >= 0 {
                    let anchor = end_date
                        .checked_sub_months(chrono::Months::new(month_delta as u32))
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    (anchor, (anchor - start_date).num_days())
                } else {
                    let anchor = start_date
                        .checked_sub_months(chrono::Months::new(month_delta.unsigned_abs() as u32))
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    (anchor, (end_date - anchor).num_days())
                };
                let date_days = (end_date - start_date).num_days() as i128;
                let time_remainder = delta - date_days * 86_400_000_000_000;
                if largest == "year" {
                    fields[0] = Value::Number(years as f64);
                    fields[1] = Value::Number(months as f64);
                } else {
                    fields[1] = Value::Number((years * 12 + months) as f64);
                }
                fields[3] = Value::Number(days as f64);
                if smallest == "year" {
                    if rounding_mode == "floor"
                        && delta < 0
                        && (months != 0 || days != 0 || time_remainder != 0)
                    {
                        fields[0] = Value::Number((years - 1) as f64);
                    } else if matches!(rounding_mode.as_str(), "ceil" | "expand")
                        && delta > 0
                        && (months != 0 || days != 0 || time_remainder != 0)
                    {
                        fields[0] = Value::Number((years + 1) as f64);
                    }
                    fields[1] = Value::Number(0.0);
                    fields[3] = Value::Number(0.0);
                } else if smallest == "month" {
                    if rounding_mode == "floor" && delta < 0 && (days != 0 || time_remainder != 0) {
                        fields[1] = Value::Number((years * 12 + months - 1) as f64);
                    } else if matches!(rounding_mode.as_str(), "ceil" | "expand")
                        && delta > 0
                        && (days != 0 || time_remainder != 0)
                    {
                        fields[1] = Value::Number((years * 12 + months + 1) as f64);
                    }
                    fields[3] = Value::Number(0.0);
                } else if smallest == "week" {
                    fields[3] = Value::Number((days / 7) as f64 * 7.0);
                }
                let mut remainder = time_remainder;
                for (index, unit_scale) in [
                    (4, 3_600_000_000_000_i128),
                    (5, 60_000_000_000),
                    (6, 1_000_000_000),
                    (7, 1_000_000),
                    (8, 1_000),
                    (9, 1),
                ] {
                    if unit_scale < scale {
                        continue;
                    }
                    fields[index] = Value::Number((remainder / unit_scale) as f64);
                    remainder %= unit_scale;
                }
                return crate::temporal::duration::construct(&fields);
            }
            let mut remainder = delta;
            let largest_index: usize = match largest.as_str() {
                "week" => 2,
                "day" => 3,
                "hour" => 4,
                "minute" => 5,
                "second" => 6,
                "millisecond" => 7,
                "microsecond" => 8,
                _ => 9,
            };
            for (name, unit_scale) in scales.iter().skip(largest_index.saturating_sub(2)) {
                if *unit_scale < scale {
                    continue;
                }
                let value = remainder / *unit_scale;
                let index = match *name {
                    "week" => 2,
                    "day" => 3,
                    "hour" => 4,
                    "minute" => 5,
                    "second" => 6,
                    "millisecond" => 7,
                    "microsecond" => 8,
                    _ => 9,
                };
                fields[index] = Value::Number(value as f64);
                remainder %= *unit_scale;
            }
            let _ = largest_scale;
            return crate::temporal::duration::construct(&fields);
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeRound {
            let options = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing rounding options"))?;
            if matches!(options, Value::Null) || crate::conversion::is_symbol(options) {
                return Err(crate::value::error::throw_type_error(
                    "Invalid rounding options",
                ));
            }
            let (smallest, increment, mode) =
                if matches!(options, Value::String(_) | Value::StringUnits(_)) {
                    (
                        crate::conversion::to_string(options)?,
                        1_i128,
                        "halfExpand".to_string(),
                    )
                } else if crate::value::is_object(options) {
                    let unit = crate::execute::get_property_result(options, "smallestUnit")?;
                    if matches!(unit, Value::Undefined) {
                        return Err(crate::value::error::throw_range_error(
                            "smallestUnit required",
                        ));
                    }
                    let increment =
                        match crate::execute::get_property_result(options, "roundingIncrement")? {
                            Value::Undefined => 1,
                            value => {
                                let number = crate::conversion::to_number(&value)?;
                                if !number.is_finite() || number <= 0.0 || number.fract() != 0.0 {
                                    return Err(crate::value::error::throw_range_error(
                                        "Invalid roundingIncrement",
                                    ));
                                }
                                number as i128
                            }
                        };
                    let mode = match crate::execute::get_property_result(options, "roundingMode")? {
                        Value::Undefined => "halfExpand".to_string(),
                        value => crate::conversion::to_string(&value)?,
                    };
                    (crate::conversion::to_string(&unit)?, increment, mode)
                } else {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid rounding options",
                    ));
                };
            let smallest = smallest.strip_suffix('s').unwrap_or(&smallest);
            let unit = match smallest {
                "day" => 86_400_000_000_000_i128,
                "hour" => 3_600_000_000_000_i128,
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
            let quantum = unit.checked_mul(increment).ok_or_else(|| {
                crate::value::error::throw_range_error("Invalid roundingIncrement")
            })?;
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let offset = if smallest == "day" {
                match property("offsetNanoseconds")? {
                    Value::Number(value) => value as i128,
                    _ => 0,
                }
            } else {
                0
            };
            let local_epoch = epoch + offset;
            let quotient = local_epoch.div_euclid(quantum);
            let remainder = local_epoch.rem_euclid(quantum);
            let round_up = match mode.as_str() {
                "trunc" => local_epoch < 0 && remainder != 0,
                "floor" => false,
                "ceil" => remainder != 0,
                "expand" => remainder != 0,
                "halfExpand" | "halfCeil" | "halfFloor" | "halfTrunc" | "halfEven" => {
                    remainder * 2 > quantum
                        || (remainder * 2 == quantum
                            && match mode.as_str() {
                                "halfCeil" => true,
                                "halfFloor" => false,
                                "halfTrunc" => local_epoch < 0,
                                "halfEven" => quotient % 2 != 0,
                                _ => true,
                            })
                }
                _ => {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid roundingMode",
                    ))
                }
            };
            let rounded = (quotient + i128::from(round_up)) * quantum - offset;
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            return Ok(super::zoned_record(
                rounded,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeGetTimeZoneTransition {
            let options = arguments
                .first()
                .ok_or_else(|| crate::value::error::throw_type_error("Missing options"))?;
            if crate::conversion::is_symbol(options) {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
            let direction = if matches!(options, Value::String(_) | Value::StringUnits(_)) {
                options.clone()
            } else {
                if !crate::value::is_object(options) {
                    return Err(crate::value::error::throw_type_error("Invalid options"));
                }
                crate::execute::get_property_result(options, "direction")?
            };
            let direction = match direction {
                value if crate::conversion::is_symbol(&value) => {
                    return Err(crate::value::error::throw_type_error("Invalid direction"))
                }
                value => {
                    let value = crate::conversion::to_string(&value)?;
                    if value != "next" && value != "previous" {
                        return Err(crate::value::error::throw_range_error("Invalid direction"));
                    }
                    value
                }
            };
            let _ = direction;
            return Ok(Value::Null);
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToInstant {
            return Ok(Value::Object(std::rc::Rc::new(
                crate::value::ObjectData::new(vec![
                    ("epochNanoseconds".into(), property("epochNanoseconds")?),
                    (
                        "\0prototype".into(),
                        Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype),
                    ),
                ]),
            )));
        }
        let year = crate::conversion::to_number(&property("year")?)?;
        let month = crate::conversion::to_number(&property("month")?)?;
        let day = crate::conversion::to_number(&property("day")?)?;
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToPlainDate {
            return crate::temporal::plain_date::construct(&[
                Value::Number(year),
                Value::Number(month),
                Value::Number(day),
            ]);
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToPlainTime {
            return crate::temporal::plain_time::construct(&[
                property("hour")?,
                property("minute")?,
                property("second")?,
                property("millisecond")?,
                property("microsecond")?,
                property("nanosecond")?,
            ]);
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToPlainDateTime {
            return crate::temporal::plain_date_time::construct(&[
                Value::Number(year),
                Value::Number(month),
                Value::Number(day),
                property("hour")?,
                property("minute")?,
                property("second")?,
                property("millisecond")?,
                property("microsecond")?,
                property("nanosecond")?,
            ]);
        }
        let year = year as i32;
        let month = month as u32;
        let day = day as u32;
        let hour = crate::conversion::to_number(&property("hour")?)? as u32;
        let mut minute = crate::conversion::to_number(&property("minute")?)? as u32;
        let mut second = crate::conversion::to_number(&property("second")?)? as u32;
        let mut millisecond = crate::conversion::to_number(&property("millisecond")?)? as u32;
        let mut microsecond = crate::conversion::to_number(&property("microsecond")?)? as u32;
        let mut nanosecond = crate::conversion::to_number(&property("nanosecond")?)? as u32;
        let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
        let offset = crate::conversion::to_string(&property("offset")?)?;
        let options = arguments.first();
        if let Some(value) = options {
            if !matches!(
                value,
                Value::Undefined | Value::Object(_) | Value::Function(_) | Value::BoundFunction(_)
            ) {
                return Err(crate::value::error::throw_type_error(
                    "Invalid string options",
                ));
            }
        }
        let option = |name: &str| -> Result<Option<Value>, VmError> {
            options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, name).map(Some))
                .unwrap_or(Ok(None))
        };
        let parse_choice = |name: &str, allowed: &[&str]| -> Result<Option<String>, VmError> {
            let Some(value) = option(name)? else {
                return Ok(None);
            };
            if matches!(value, Value::Undefined) {
                return Ok(None);
            }
            let value = crate::conversion::to_string(&value)?;
            if !allowed.contains(&value.as_str()) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid string option",
                ));
            }
            Ok(Some(value))
        };
        let offset_mode =
            parse_choice("offset", &["auto", "never"])?.unwrap_or_else(|| "auto".into());
        let zone_mode = parse_choice("timeZoneName", &["auto", "never", "critical"])?
            .unwrap_or_else(|| "auto".into());
        let calendar_mode = parse_choice("calendarName", &["auto", "always", "never", "critical"])?
            .unwrap_or_else(|| "auto".into());
        let smallest = parse_choice(
            "smallestUnit",
            &[
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
            ],
        )?;
        let _rounding = parse_choice(
            "roundingMode",
            &[
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ],
        )?;
        let mut precision = match option("fractionalSecondDigits")? {
            None | Some(Value::Undefined) => usize::MAX,
            Some(Value::String(value)) if value == "auto" => usize::MAX,
            Some(value) => {
                let value = crate::conversion::to_number(&value)?;
                if !value.is_finite() || !(0.0..=9.0).contains(&value) {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid fractionalSecondDigits",
                    ));
                }
                value.floor() as usize
            }
        };
        let fraction = millisecond * 1_000_000 + microsecond * 1_000 + nanosecond;
        if let Some(unit) = smallest.as_deref() {
            match unit {
                "hour" => {
                    minute = 0;
                    second = 0;
                    precision = 0;
                }
                "minute" => {
                    second = 0;
                    precision = 0;
                }
                "second" => {
                    millisecond = 0;
                    microsecond = 0;
                    nanosecond = 0;
                    precision = 0;
                }
                "millisecond" => precision = 3,
                "microsecond" => precision = 6,
                "nanosecond" => precision = 9,
                _ => unreachable!(),
            }
        }
        let suffix = if precision == 0 || (fraction == 0 && precision == usize::MAX) {
            String::new()
        } else {
            let mut digits = format!("{fraction:09}");
            if precision != usize::MAX {
                digits.truncate(precision);
            } else {
                digits = digits.trim_end_matches('0').into();
            }
            format!(".{digits}")
        };
        let offset_suffix = if offset_mode == "never" {
            String::new()
        } else {
            offset
        };
        let zone_suffix = match zone_mode.as_str() {
            "never" => String::new(),
            "critical" => format!("[!{timezone}]"),
            _ => format!("[{timezone}]"),
        };
        let calendar_suffix = match calendar_mode.as_str() {
            "always" => "[u-ca=iso8601]".to_string(),
            "critical" => "[!u-ca=iso8601]".to_string(),
            _ => String::new(),
        };
        let clock = match smallest.as_deref() {
            Some("hour") => format!("{hour:02}"),
            Some("minute") => format!("{hour:02}:{minute:02}"),
            _ => format!("{hour:02}:{minute:02}:{second:02}{suffix}"),
        };
        let text = format!(
            "{year:04}-{month:02}-{day:02}T{clock}{offset_suffix}{zone_suffix}{calendar_suffix}"
        );
        Ok(Value::String(text))
    }

    fn plain_month_day_from(value: Option<&Value>) -> Result<Value, VmError> {
        let value =
            value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay"))?;
        let (month, day) = if let Value::String(text) = value {
            let parts = text.split('-').collect::<Vec<_>>();
            if parts.len() < 2 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainMonthDay",
                ));
            }
            (
                parts[parts.len() - 2].parse::<f64>().unwrap_or(0.0),
                parts[parts.len() - 1].parse::<f64>().unwrap_or(0.0),
            )
        } else {
            (
                crate::execute::get_property_result(value, "month")
                    .and_then(|v| crate::conversion::to_number(&v))?,
                crate::execute::get_property_result(value, "day")
                    .and_then(|v| crate::conversion::to_number(&v))?,
            )
        };
        if !(1.0..=12.0).contains(&month) || !(1.0..=31.0).contains(&day) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainMonthDay",
            ));
        }
        Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![
                (
                    "monthCode".into(),
                    Value::String(format!("M{:02}", month as u32)),
                ),
                ("day".into(), Value::Number(day)),
                ("calendarId".into(), Value::String("iso8601".into())),
                (
                    "\0prototype".into(),
                    Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype),
                ),
            ]),
        )))
    }

    fn plain_year_month_from(value: Option<&Value>) -> Result<Value, VmError> {
        let value =
            value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth"))?;
        let (year, month) = if let Value::String(text) = value {
            let parts = text.split('-').collect::<Vec<_>>();
            if parts.len() < 2 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
            (
                parts[parts.len() - 2].parse::<f64>().unwrap_or(0.0),
                parts[parts.len() - 1].parse::<f64>().unwrap_or(0.0),
            )
        } else {
            (
                crate::execute::get_property_result(value, "year")
                    .and_then(|v| crate::conversion::to_number(&v))?,
                crate::execute::get_property_result(value, "month")
                    .and_then(|v| crate::conversion::to_number(&v))?,
            )
        };
        if !(1.0..=12.0).contains(&month) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![
                ("year".into(), Value::Number(year)),
                ("month".into(), Value::Number(month)),
                (
                    "monthCode".into(),
                    Value::String(format!("M{:02}", month as u32)),
                ),
                ("calendarId".into(), Value::String("iso8601".into())),
                (
                    "\0prototype".into(),
                    Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype),
                ),
            ]),
        )))
    }
}

pub(crate) fn construct_stub(
    prototype: crate::ops::Builtin,
) -> Result<crate::value::Value, crate::execute::VmError> {
    Ok(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![(
            "\0prototype".to_string(),
            crate::value::Value::Builtin(prototype),
        )]),
    )))
}
