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
    let seconds = epoch.div_euclid(1_000_000_000);
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
            "epochMilliseconds".into(),
            crate::value::Value::BigInt((epoch / 1_000_000).to_string()),
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
            crate::value::Value::String("+00:00".into()),
        ),
        ("offsetNanoseconds".into(), crate::value::Value::Number(0.0)),
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
        ("hoursInDay".into(), crate::value::Value::Number(24.0)),
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
            let date_parts = date
                .split('-')
                .map(|part| part.parse::<i32>().unwrap_or(0))
                .collect::<Vec<_>>();
            let time_parts = time
                .split(':')
                .map(|part| part.parse::<i64>().unwrap_or(0))
                .collect::<Vec<_>>();
            if date_parts.len() < 3 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            let date = chrono::NaiveDate::from_ymd_opt(
                date_parts[0],
                date_parts[1] as u32,
                date_parts[2] as u32,
            )
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            let days = date
                .signed_duration_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch"))
                .num_days() as i128;
            let epoch = days * 86_400_000_000_000
                + time_parts.get(0).copied().unwrap_or(0) as i128 * 3_600_000_000_000
                + time_parts.get(1).copied().unwrap_or(0) as i128 * 60_000_000_000
                + time_parts.get(2).copied().unwrap_or(0) as i128 * 1_000_000_000;
            let timezone = text
                .split('[')
                .nth(1)
                .and_then(|part| part.split(']').next())
                .unwrap_or("UTC")
                .to_string();
            return Ok(super::zoned_record(
                epoch,
                timezone,
                crate::ops::Builtin::TemporalZonedDateTimePrototype,
            ));
        }
        let epoch = crate::execute::get_property_result(value, "epochNanoseconds")?;
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
