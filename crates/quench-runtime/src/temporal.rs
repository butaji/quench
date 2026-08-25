use chrono::Datelike;

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
    let offset_nanos = fixed_offset_nanos(&timezone);
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
                | crate::ops::Builtin::TemporalZonedDateTimeToString
                | crate::ops::Builtin::TemporalZonedDateTimeToJSON
                | crate::ops::Builtin::TemporalZonedDateTimeToLocaleString
                | crate::ops::Builtin::TemporalZonedDateTimeToInstant
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainDateTime
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainDate
                | crate::ops::Builtin::TemporalZonedDateTimeToPlainTime
                | crate::ops::Builtin::TemporalZonedDateTimeEquals
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
            let month = crate::conversion::to_number(&crate::execute::get_property_result(
                value, "month",
            )?)? as u32;
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
        let timezone = match crate::execute::get_property_result(value, "timeZone")? {
            Value::String(value) => value,
            _ => "UTC".into(),
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
        if !matches!(receiver, Value::Object(object) if object
            .iter()
            .any(|(key, value)| key == "\0prototype"
                && value == Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)))
        {
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
