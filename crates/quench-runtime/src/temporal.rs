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
        crate::value::Value::Boolean(value) => i128::from(*value),
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Invalid epochNanoseconds",
            ))
        }
    };
    let timezone_value = arguments.get(1).unwrap_or(&crate::value::Value::Undefined);
    if matches!(timezone_value, crate::value::Value::String(_) | crate::value::Value::StringUnits(_)) {
        let text = crate::conversion::to_string(timezone_value)?;
        if text.contains('T') && text.contains('-') && text.bytes().any(|byte| byte.is_ascii_digit()) {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
    }
    let timezone = parse_timezone_identifier(timezone_value)?;
    let calendar = arguments
        .get(2)
        .filter(|value| !matches!(value, crate::value::Value::Undefined))
        .map(|value| {
            let calendar = parse_calendar_identifier(value)?;
            if let crate::value::Value::String(_) | crate::value::Value::StringUnits(_) = value {
                let text = crate::conversion::to_string(value)?;
                let date_like = text.chars().filter(|ch| *ch == '-').count() >= 2
                    || (text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()));
                if date_like && !text.eq_ignore_ascii_case("iso8601") {
                    return Err(crate::value::error::throw_range_error("Invalid calendar"));
                }
            }
            Ok(calendar)
        })
        .transpose()?
        .unwrap_or_else(|| "iso8601".into());
    Ok(zoned_record_with_calendar(
        epoch,
        timezone,
        calendar,
    ))
}

pub(crate) fn zoned_record(
    epoch: i128,
    timezone: String,
    prototype: crate::ops::Builtin,
) -> crate::value::Value {
    let offset_nanos = timezone_offset_nanos(&timezone, epoch);
    let seconds = (epoch + offset_nanos).div_euclid(1_000_000_000);
    let nanos = epoch.rem_euclid(1_000_000_000) as i64;
    let date = chrono::DateTime::from_timestamp(seconds as i64, nanos as u32)
        .map(|value| value.date_naive());
    let (year, month, day, weekday, ordinal, week, week_year, days_month) = if let Some(date) = date {
        let days_month = crate::temporal::plain_date::days_in_month_for_record(date.year(), date.month());
        (date.year(), date.month(), date.day(), date.weekday().number_from_monday(), date.ordinal(), date.iso_week().week(), date.iso_week().year(), days_month)
    } else {
        let days = seconds.div_euclid(86_400);
        let (year, month, day) = crate::temporal::plain_date::civil_from_serial((days + 719_468) as i64);
        let weekday = ((days + 3).rem_euclid(7) + 1) as u32;
        let ordinal = (1..month).map(|m| crate::temporal::plain_date::days_in_month_for_record(year, m)).sum::<u32>() + day;
        let days_month = crate::temporal::plain_date::days_in_month_for_record(year, month);
        (year, month, day, weekday, ordinal, 1, year, days_month)
    };
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
            crate::value::Value::Number(year as f64),
        ),
        (
            "month".into(),
            crate::value::Value::Number(month as f64),
        ),
        (
            "monthCode".into(),
            crate::value::Value::String(format!("M{:02}", month)),
        ),
        ("day".into(), crate::value::Value::Number(day as f64)),
        (
            "dayOfWeek".into(),
            crate::value::Value::Number(weekday as f64),
        ),
        (
            "dayOfYear".into(),
            crate::value::Value::Number(ordinal as f64),
        ),
        (
            "weekOfYear".into(),
            crate::value::Value::Number(week as f64),
        ),
        (
            "yearOfWeek".into(),
            crate::value::Value::Number(week_year as f64),
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
            crate::value::Value::Number(days_month as f64),
        ),
        ("monthsInYear".into(), crate::value::Value::Number(12.0)),
        (
            "inLeapYear".into(),
            crate::value::Value::Boolean(
                chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some(),
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

fn format_year(year: i32) -> String {
    if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year < 0 {
        format!("-{year_abs:06}", year_abs = year.unsigned_abs())
    } else {
        format!("+{year:06}")
    }
}

pub(crate) fn parse_timezone_identifier(
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
        return normalize_offset_identifier(&text)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"));
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
            } else {
                base.rfind(['+', '-']).and_then(|index| {
                    let suffix = &base[index..];
                    normalize_offset_identifier(suffix)
                        .is_some()
                        .then_some(suffix)
                })
            }
        });
        let Some(identifier) = identifier else {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        };
        if identifier.eq_ignore_ascii_case("utc") {
            return Ok("UTC".into());
        }
        if identifier.starts_with(['+', '-']) {
            return normalize_offset_identifier(identifier)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"));
        }
        return Ok(identifier.to_string());
    }
    if text
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == ':')
    {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    if text.parse::<chrono_tz::Tz>().is_ok() {
        Ok(text)
    } else {
        Err(crate::value::error::throw_range_error("Invalid time zone"))
    }
}

fn normalize_offset_identifier(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return None;
    }
    let (hour, minute) = match bytes.len() {
        3 => (text[1..3].parse::<u8>().ok()?, 0),
        5 => (
            text[1..3].parse::<u8>().ok()?,
            text[3..5].parse::<u8>().ok()?,
        ),
        6 if bytes[3] == b':' => (
            text[1..3].parse::<u8>().ok()?,
            text[4..6].parse::<u8>().ok()?,
        ),
        _ => return None,
    };
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(format!("{}{:02}:{:02}", bytes[0] as char, hour, minute))
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
        if text.contains("-000000-") {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
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

fn parse_iso_annotations(text: &str) -> Result<(Option<String>, Option<String>), crate::execute::VmError> {
    let mut rest = text;
    let mut calendar = None;
    let mut calendar_critical = false;
    let mut timezone = None;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let end = after
            .find(']')
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
        let annotation = &after[..end];
        if annotation.is_empty() {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        let (critical, body) = annotation
            .strip_prefix('!')
            .map_or((false, annotation), |body| (true, body));
        if let Some((key, value)) = body.split_once('=') {
            if key.bytes().any(|byte| byte.is_ascii_uppercase()) {
                return Err(crate::value::error::throw_range_error("Annotation keys must be lowercase"));
            }
            if key != "u-ca" {
                if critical {
                    return Err(crate::value::error::throw_range_error("Unknown critical annotation"));
                }
            } else {
                if !value.eq_ignore_ascii_case("iso8601") {
                    if critical || calendar.is_none() {
                        return Err(crate::value::error::throw_range_error("Invalid calendar"));
                    }
                } else if calendar.is_some() {
                    if critical || calendar_critical {
                        return Err(crate::value::error::throw_range_error("Invalid calendar"));
                    }
                    // A second non-critical calendar annotation is ignored.
                    rest = &after[end + 1..];
                    continue;
                }
                calendar = Some("iso8601".into());
                calendar_critical |= critical;
            }
        } else {
            if timezone.is_some() {
                return Err(crate::value::error::throw_range_error("Multiple time zones"));
            }
            timezone = Some(body.to_string());
        }
        rest = &after[end + 1..];
    }
    Ok((calendar, timezone))
}

fn validate_plain_time_annotations(text: &str) -> Result<(), crate::execute::VmError> {
    let mut calendars = 0;
    let mut time_zones = 0;
    let mut critical_calendar = false;
    for part in text.split('[').skip(1) {
        let annotation = part
            .strip_suffix(']')
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid annotation"))?;
        let (critical, body) = annotation
            .strip_prefix('!')
            .map_or((false, annotation), |body| (true, body));
        if let Some((key, _)) = body.split_once('=') {
            if key.bytes().any(|byte| byte.is_ascii_uppercase()) {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
            if key == "u-ca" {
                calendars += 1;
                critical_calendar |= critical;
            } else if critical {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
        } else {
            time_zones += 1;
        }
    }
    if time_zones > 1 || calendars > 1 && critical_calendar {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    Ok(())
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

    fn validate_now_timezone(arguments: &[Value]) -> Result<(), VmError> {
        if let Some(value) = arguments
            .first()
            .filter(|value| !matches!(value, Value::Undefined))
        {
            super::parse_timezone_identifier(value)?;
        }
        Ok(())
    }

    pub(super) fn execute(
        builtin: crate::ops::Builtin,
        _receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if builtin == crate::ops::Builtin::TemporalZonedDateTime {
            return Some(Err(crate::value::error::throw_type_error(
                "Temporal.ZonedDateTime requires new",
            )));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeFrom {
            return Some(zoned_from(arguments.first(), arguments.get(1)));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeCompare {
            return Some((|| {
                let left = zoned_from(arguments.first(), None)?;
                let right = zoned_from(arguments.get(1), None)?;
                let left = crate::execute::get_property_result(&left, "epochNanoseconds")?;
                let right = crate::execute::get_property_result(&right, "epochNanoseconds")?;
                let (Value::BigInt(left), Value::BigInt(right)) = (left, right) else {
                    return Err(crate::value::error::throw_type_error("Invalid ZonedDateTime"));
                };
                let ordering = left.parse::<i128>().unwrap_or(0).cmp(&right.parse::<i128>().unwrap_or(0));
                Ok(Value::Number(match ordering {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                }))
            })());
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
                | crate::ops::Builtin::TemporalZonedDateTimeWith
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
                let epoch = super::now_epoch_nanoseconds();
                return Some(Ok(Value::Object(std::rc::Rc::new(
                    crate::value::ObjectData::new(vec![
                        (
                            "epochNanoseconds".to_string(),
                            Value::BigInt(epoch.to_string()),
                        ),
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
                if let Err(error) = validate_now_timezone(arguments) {
                    return Some(Err(error));
                }
                return Some(super::plain_date::construct(&[
                    Value::Number(1970.0),
                    Value::Number(1.0),
                    Value::Number(1.0),
                ]));
            }
            crate::ops::Builtin::TemporalNowPlainDateTimeISO => {
                if let Err(error) = validate_now_timezone(arguments) {
                    return Some(Err(error));
                }
                return Some(super::plain_date_time::construct(&[
                    Value::Number(1970.0),
                    Value::Number(1.0),
                    Value::Number(1.0),
                ]));
            }
            crate::ops::Builtin::TemporalNowPlainTimeISO => {
                if let Err(error) = validate_now_timezone(arguments) {
                    return Some(Err(error));
                }
                return Some(super::plain_time::construct(&[]));
            }
            crate::ops::Builtin::TemporalNowZonedDateTimeISO => {
                let timezone = match arguments
                    .first()
                    .filter(|value| !matches!(value, Value::Undefined))
                {
                    Some(value) => match super::parse_timezone_identifier(value) {
                        Ok(timezone) => timezone,
                        Err(error) => return Some(Err(error)),
                    },
                    None => "UTC".into(),
                };
                return Some(Ok(super::zoned_record(
                    0,
                    timezone,
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

    fn zoned_from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
        let value =
            value.ok_or_else(|| crate::value::error::throw_type_error("Invalid ZonedDateTime"))?;
        if matches!(value, Value::StringUnits(_)) {
            let text = crate::conversion::to_string(value)?;
            return zoned_from(Some(&Value::String(text)), options);
        }
        if let Value::String(text) = value {
            if !text.contains('[') {
                return Err(crate::value::error::throw_range_error("Invalid ZonedDateTime"));
            }
            let (_calendar_annotation, timezone_annotation) = super::parse_iso_annotations(text)?;
            let has_z = text.split('[').next().unwrap_or(text).contains('Z');
            let date_time = text
                .split('[')
                .next()
                .unwrap_or(text)
                .split('Z')
                .next()
                .unwrap_or(text);
            let (date, time) = date_time.split_once('T').unwrap_or((date_time, "00:00:00"));
            if date.starts_with("-000000") {
                return Err(crate::value::error::throw_range_error("Invalid year"));
            }
            let (parsed_year, parsed_month, parsed_day) = super::parse_date_parts(date)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            let offset_start = time[1..].find(['+', '-']).map(|index| index + 1);
            let (clock, offset_text) = offset_start
                .map(|index| (&time[..index], &time[index..]))
                .unwrap_or((time, "+00:00"));
            if time.contains('+') || time.contains('-') {
                let suffix = time
                    .rsplit_once(['+', '-'])
                    .map(|(_, suffix)| suffix)
                    .unwrap_or_default();
                if suffix.matches(':').count() > 1 {
                    return Err(crate::value::error::throw_range_error("Invalid time"));
                }
            }
            if clock.contains('.') && clock.split(':').count() < 3 {
                return Err(crate::value::error::throw_range_error("Fractional minutes not allowed"));
            }
            if clock
                .split_once('.')
                .map(|(_, fraction)| fraction.len() > 9)
                .unwrap_or(false)
            {
                return Err(crate::value::error::throw_range_error("Too many fractional digits"));
            }
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
            if time_parts.get(2).is_some_and(|second| *second == 60) {
                time_parts[2] = 59;
            }
            if time_parts.first().is_some_and(|hour| !(*hour >= 0 && *hour <= 23))
                || time_parts.get(1).is_some_and(|minute| !(*minute >= 0 && *minute <= 59))
                || time_parts.get(2).is_some_and(|second| !(*second >= 0 && *second <= 59))
            {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
            let year = parsed_year;
            let month = parsed_month;
            let day = parsed_day;
            if !(1..=12).contains(&month)
                || !(1..=31).contains(&day)
                || chrono::NaiveDate::from_ymd_opt(year, month, day).is_none()
            {
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
            let timezone_text = timezone_annotation
                .as_deref()
                .unwrap_or(if has_z { "UTC" } else { offset_text });
            let timezone =
                super::parse_timezone_identifier(&Value::String(timezone_text.to_string()))?;
            let offset_mode = options
                .filter(|value| !matches!(value, Value::Undefined))
                .and_then(|value| crate::execute::get_property_result(value, "offset").ok())
                .filter(|value| !matches!(value, Value::Undefined))
                .and_then(|value| crate::conversion::to_string(&value).ok())
                .unwrap_or_else(|| "reject".into());
            if offset_start.is_some() && offset_mode != "ignore" {
                let supplied_offset = super::fixed_offset_nanos(offset_text);
                let actual_offset = super::timezone_offset_nanos(&timezone, epoch);
                if supplied_offset != actual_offset {
                    return Err(crate::value::error::throw_range_error(
                        "Offset does not match time zone",
                    ));
                }
            }
            if offset_start.is_some() && offset_mode == "ignore" {
                epoch = local_epoch - super::timezone_offset_nanos(&timezone, local_epoch);
            }
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
            let year_value = crate::execute::get_property_result(value, "year")?;
            let day_value = crate::execute::get_property_result(value, "day")?;
            let month_value = crate::execute::get_property_result(value, "month")?;
            let month_code_value = crate::execute::get_property_result(value, "monthCode")?;
            if matches!(year_value, Value::Undefined)
                || matches!(day_value, Value::Undefined)
                || (matches!(month_value, Value::Undefined)
                    && matches!(month_code_value, Value::Undefined))
            {
                return Err(crate::value::error::throw_type_error("Missing ZonedDateTime field"));
            }
            let year = crate::conversion::to_number(&year_value)? as i32;
            let month = if matches!(month_value, Value::Undefined) {
                match month_code_value {
                    Value::String(code) if code.len() == 3 && code.starts_with('M') => code[1..]
                        .parse::<u32>()
                        .map_err(|_| crate::value::error::throw_range_error("Invalid monthCode"))?,
                    _ => return Err(crate::value::error::throw_range_error("Invalid monthCode")),
                }
            } else {
                crate::conversion::to_number(&month_value)? as u32
            };
            let day = crate::conversion::to_number(&day_value)? as u32;
            let hour = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "hour").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let minute = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "minute").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let second = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "second").unwrap_or(Value::Number(0.0)),
            )? as i128;
            let millisecond = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "millisecond")
                    .unwrap_or(Value::Number(0.0)),
            )? as i128;
            let microsecond = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "microsecond")
                    .unwrap_or(Value::Number(0.0)),
            )? as i128;
            let nanosecond = crate::conversion::to_number(
                &crate::execute::get_property_result(value, "nanosecond")
                    .unwrap_or(Value::Number(0.0)),
            )? as i128;
            let calendar = crate::execute::get_property_result(value, "calendar")?;
            if !matches!(calendar, Value::Undefined) {
                super::parse_calendar_identifier(&calendar)?;
            }
            let timezone = super::parse_timezone_identifier(&crate::execute::get_property_result(
                value, "timeZone",
            )?)?;
            let offset = crate::execute::get_property_result(value, "offset")?;
            let offset = if matches!(offset, Value::Undefined) {
                None
            } else {
                if !matches!(offset, Value::String(_) | Value::StringUnits(_)) {
                    return Err(crate::value::error::throw_type_error("Invalid offset"));
                }
                let offset = crate::conversion::to_string(&offset)?;
                let normalized = if offset.eq_ignore_ascii_case("z") {
                    Some("+00:00".to_string())
                } else {
                    super::normalize_offset_identifier(&offset)
                }
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid offset"))?;
                Some(normalized)
            };
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
            let local_epoch = days * 86_400_000_000_000
                + hour * 3_600_000_000_000
                + minute * 60_000_000_000
                + second * 1_000_000_000;
            let local_epoch =
                local_epoch + millisecond * 1_000_000 + microsecond * 1_000 + nanosecond;
            let epoch = local_epoch - super::timezone_offset_nanos(&timezone, local_epoch);
            if let Some(offset) = offset {
                if super::fixed_offset_nanos(&offset)
                    != super::timezone_offset_nanos(&timezone, local_epoch)
                {
                    return Err(crate::value::error::throw_range_error(
                        "Offset does not match time zone",
                    ));
                }
            }
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
            let other = zoned_from(Some(other), None)?;
            return Ok(Value::Boolean(
                ["epochNanoseconds", "timeZoneId", "calendarId"]
                    .iter()
                    .all(|name| {
                        property(name).ok()
                            == crate::execute::get_property_result(&other, name).ok()
                    }),
            ));
        }
        if matches!(
            builtin,
            crate::ops::Builtin::TemporalZonedDateTimeToJSON
                | crate::ops::Builtin::TemporalZonedDateTimeToLocaleString
        ) {
            return zoned_method(
                crate::ops::Builtin::TemporalZonedDateTimeToString,
                Some(receiver),
                &[],
            );
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeWith {
            let partial = arguments.first().ok_or_else(|| {
                crate::value::error::throw_type_error("Missing date-time-like argument")
            })?;
            if !crate::value::is_object(partial) {
                return Err(crate::value::error::throw_type_error(
                    "Invalid date-time-like",
                ));
            }
            if crate::temporal::plain_date::is_temporal_date_like(partial) {
                return Err(crate::value::error::throw_type_error("Invalid date-time-like"));
            }
            let options = arguments.get(1);
            if options.is_some_and(|value| {
                !matches!(value, Value::Undefined) && !crate::value::is_object(value)
            }) {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
            let calendar = crate::execute::get_property_result(partial, "calendar")?;
            if !matches!(calendar, Value::Undefined) {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            }
            let partial_time_zone = crate::execute::get_property_result(partial, "timeZone")?;
            if !matches!(partial_time_zone, Value::Undefined) {
                return Err(crate::value::error::throw_type_error("Invalid time zone"));
            }
            let has_field = [
                "year", "month", "monthCode", "day", "hour", "minute", "second",
                "millisecond", "microsecond", "nanosecond",
            ]
            .iter()
            .any(|name| {
                crate::execute::get_property_result(partial, name)
                    .is_ok_and(|value| !matches!(value, Value::Undefined))
            });
            if !has_field {
                return Err(crate::value::error::throw_type_error("Insufficient date-time data"));
            }
            let overflow = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "overflow"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?;
            let overflow = overflow.unwrap_or_else(|| "constrain".into());
            if overflow != "constrain" && overflow != "reject" {
                return Err(crate::value::error::throw_range_error("Invalid overflow"));
            }
            let value_or = |name: &str| -> Result<Value, VmError> {
                let value = crate::execute::get_property_result(partial, name)?;
                if matches!(value, Value::Undefined) {
                    property(name)
                } else {
                    Ok(value)
                }
            };
            let month = crate::execute::get_property_result(partial, "month")?;
            let month_code = crate::execute::get_property_result(partial, "monthCode")?;
            let mut fields = vec![
                ("year".to_string(), value_or("year")?),
                ("day".to_string(), value_or("day")?),
                ("hour".to_string(), value_or("hour")?),
                ("minute".to_string(), value_or("minute")?),
                ("second".to_string(), value_or("second")?),
                ("millisecond".to_string(), value_or("millisecond")?),
                ("microsecond".to_string(), value_or("microsecond")?),
                ("nanosecond".to_string(), value_or("nanosecond")?),
                ("timeZone".to_string(), property("timeZoneId")?),
            ];
            if !matches!(month, Value::Undefined) {
                fields.push(("month".to_string(), month));
            } else if !matches!(month_code, Value::Undefined) {
                fields.push(("monthCode".to_string(), month_code));
            } else {
                fields.push(("month".to_string(), property("month")?));
            }
            let year = fields
                .iter()
                .find(|(name, _)| name == "year")
                .map(|(_, value)| crate::conversion::to_number(value))
                .transpose()?
                .unwrap_or(0.0) as i32;
            let month = if let Some((_, value)) = fields.iter().find(|(name, _)| name == "month") {
                crate::conversion::to_number(value)? as u32
            } else if let Some((_, Value::String(code))) =
                fields.iter().find(|(name, _)| name == "monthCode")
            {
                code.get(1..)
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0)
            } else {
                0
            };
            if let Some((_, day)) = fields.iter_mut().find(|(name, _)| name == "day") {
                let day_number = crate::conversion::to_number(day)? as u32;
                if chrono::NaiveDate::from_ymd_opt(year, month, day_number).is_none() {
                    if overflow == "reject" {
                        return Err(crate::value::error::throw_range_error("Invalid date"));
                    }
                    let mut constrained = day_number.min(31);
                    while constrained > 1
                        && chrono::NaiveDate::from_ymd_opt(year, month, constrained).is_none()
                    {
                        constrained -= 1;
                    }
                    *day = Value::Number(constrained as f64);
                }
            }
            let result = zoned_from(Some(&Value::Object(std::rc::Rc::new(
                crate::value::ObjectData::new(fields),
            ))), None)?;
            return Ok(result);
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
            if let Some(value @ (Value::String(_) | Value::StringUnits(_))) = arguments.first() {
                let text = crate::conversion::to_string(value)?;
                super::validate_plain_time_annotations(&text)?;
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
                .map(|value| {
                    if matches!(value, Value::Undefined) {
                        Ok(0.0)
                    } else {
                        crate::conversion::to_number(value)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let overflow = match arguments.get(1) {
                None | Some(Value::Undefined) => "constrain".to_string(),
                Some(options)
                    if crate::value::is_object(options)
                        || matches!(options, Value::Function(_) | Value::BoundFunction(_)) =>
                {
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
            let other = zoned_from(Some(other), None)?;
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
            let scale: i128 = match smallest.as_str() {
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
            let increment = if let Some(value) = options
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
                increment as i128
            } else {
                1
            };
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
            let quantum = scale.checked_mul(increment).ok_or_else(|| {
                crate::value::error::throw_range_error("Invalid roundingIncrement")
            })?;
            let quotient = delta.div_euclid(quantum);
            let remainder = delta.rem_euclid(quantum);
            let round_up = match rounding_mode.as_str() {
                "trunc" => delta < 0 && remainder != 0,
                "floor" => false,
                "ceil" => remainder != 0,
                "expand" => remainder != 0,
                "halfCeil" => remainder * 2 >= quantum,
                "halfFloor" => remainder * 2 > quantum,
                "halfTrunc" => remainder * 2 > quantum || (remainder * 2 == quantum && delta < 0),
                "halfEven" => {
                    remainder * 2 > quantum || (remainder * 2 == quantum && quotient % 2 != 0)
                }
                _ => remainder * 2 >= quantum,
            };
            delta = (quotient + i128::from(round_up)) * quantum;
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
        let mut year = year as i32;
        let mut month = month as u32;
        let mut day = day as u32;
        let mut hour = crate::conversion::to_number(&property("hour")?)? as u32;
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
                Value::Undefined
                    | Value::Object(_)
                    | Value::Function(_)
                    | Value::BoundFunction(_)
                    | Value::Proxy(_)
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
            let value = if allowed.contains(&value.as_str()) {
                value
            } else {
                value.strip_suffix('s').unwrap_or(&value).to_string()
            };
            if !allowed.contains(&value.as_str()) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid string option",
                ));
            }
            Ok(Some(value))
        };
        let calendar_mode = parse_choice("calendarName", &["auto", "always", "never", "critical"])?
            .unwrap_or_else(|| "auto".into());
        let mut precision = match option("fractionalSecondDigits")? {
            None | Some(Value::Undefined) => usize::MAX,
            Some(Value::String(value)) if value == "auto" => usize::MAX,
            Some(Value::Null | Value::Boolean(_) | Value::BigInt(_)) => {
                return Err(crate::value::error::throw_range_error(
                    "Invalid fractionalSecondDigits",
                ))
            }
            Some(value) => {
                let text = match &value {
                    Value::Number(_) => None,
                    _ => Some(crate::conversion::to_string(&value)?),
                };
                if text.as_deref() == Some("auto") {
                    usize::MAX
                } else {
                    let value = text.as_deref().map_or_else(
                        || crate::conversion::to_number(&value),
                        |text| Ok(text.parse::<f64>().unwrap_or(f64::NAN)),
                    )?;
                    if !value.is_finite() || !(0.0..=9.0).contains(&value) {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid fractionalSecondDigits",
                        ));
                    }
                    value.floor() as usize
                }
            }
        };
        let offset_mode =
            parse_choice("offset", &["auto", "never"])?.unwrap_or_else(|| "auto".into());
        let rounding_mode = parse_choice(
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
        let smallest = parse_choice(
            "smallestUnit",
            &[
                "minute",
                "second",
                "millisecond",
                "microsecond",
                "nanosecond",
                "day",
                "week",
                "month",
                "year",
            ],
        )?;
        let zone_mode = parse_choice("timeZoneName", &["auto", "never", "critical"])?
            .unwrap_or_else(|| "auto".into());
        if smallest
            .as_deref()
            .is_some_and(|unit| matches!(unit, "day" | "week" | "month" | "year"))
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ));
        }
        let mut fraction = i128::from(millisecond) * 1_000_000
            + i128::from(microsecond) * 1_000
            + i128::from(nanosecond);
        let original_minute = minute;
        let original_second = second;
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
        if let Some(unit) = smallest.as_deref() {
            let (remainder, quantum, carry) = match unit {
                "minute" => (
                    i128::from(original_second) * 1_000_000_000 + fraction,
                    60_000_000_000i128,
                    60,
                ),
                "hour" => (
                    i128::from(original_minute) * 60_000_000_000
                        + i128::from(original_second) * 1_000_000_000
                        + fraction,
                    3_600_000_000_000i128,
                    60,
                ),
                _ => (0, 1, 0),
            };
            if carry != 0 && remainder != 0 {
                let mode = rounding_mode.as_deref().unwrap_or("trunc");
                let round_up = match mode {
                    "ceil" | "expand" => true,
                    "halfCeil" | "halfFloor" | "halfExpand" | "halfTrunc" | "halfEven" => {
                        remainder * 2 > quantum
                            || (remainder * 2 == quantum
                                && matches!(mode, "halfCeil" | "halfExpand"))
                    }
                    _ => false,
                };
                if round_up {
                    if unit == "minute" {
                        minute += 1;
                    } else {
                        hour += 1;
                    }
                    if unit == "minute" && minute >= 60 {
                        minute = 0;
                        hour += 1;
                    }
                    if hour >= 24 {
                        hour = 0;
                        if let Some(next) = chrono::NaiveDate::from_ymd_opt(year, month, day)
                            .and_then(|date| date.checked_add_days(chrono::Days::new(1)))
                        {
                            year = next.year();
                            month = next.month();
                            day = next.day();
                        }
                    }
                }
            }
            if unit == "minute" || unit == "hour" {
                fraction = 0;
            }
        }
        if precision != usize::MAX {
            let quantum = 10i128.pow((9 - precision) as u32);
            let quotient = fraction / quantum;
            let remainder = fraction % quantum;
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => value.parse::<i128>().unwrap_or(0),
                _ => 0,
            };
            let mode = rounding_mode.as_deref().unwrap_or("trunc");
            let round_up = match mode {
                "trunc" => false,
                "floor" => false,
                "ceil" | "expand" => remainder != 0,
                "halfCeil" | "halfFloor" | "halfExpand" | "halfTrunc" | "halfEven" => {
                    remainder * 2 > quantum
                        || (remainder * 2 == quantum
                            && match mode {
                                "halfCeil" => true,
                                "halfFloor" => false,
                                "halfTrunc" => epoch < 0,
                                "halfEven" => quotient % 2 != 0,
                                _ => true,
                            })
                }
                _ => false,
            };
            fraction = (quotient + i128::from(round_up)) * quantum;
            if fraction >= 1_000_000_000 {
                fraction = 0;
                second += 1;
                if second >= 60 {
                    second = 0;
                    minute += 1;
                    if minute >= 60 {
                        minute = 0;
                        hour += 1;
                        if hour >= 24 {
                            hour = 0;
                            if let Some(next) = chrono::NaiveDate::from_ymd_opt(year, month, day)
                                .and_then(|date| date.checked_add_days(chrono::Days::new(1)))
                            {
                                year = next.year();
                                month = next.month();
                                day = next.day();
                            }
                        }
                    }
                }
            }
            millisecond = (fraction / 1_000_000) as u32;
            microsecond = (fraction / 1_000 % 1_000) as u32;
            nanosecond = (fraction % 1_000) as u32;
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
            "{}-{month:02}-{day:02}T{clock}{offset_suffix}{zone_suffix}{calendar_suffix}",
            super::format_year(year),
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

fn now_epoch_nanoseconds() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as i128)
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
