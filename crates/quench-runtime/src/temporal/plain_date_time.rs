use crate::{execute::VmError, value::Value};
use chrono::{Datelike, NaiveDate};

const NAMES: [&str; 9] = [
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let fields = (0..9)
        .map(|index| {
            let value = arguments.get(index).unwrap_or(&Value::Undefined);
            if index >= 3 && matches!(value, Value::Undefined) {
                Ok(0.0)
            } else {
                Ok(crate::conversion::to_number(value)?.trunc())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::temporal::plain_date::construct(
        &fields[..3]
            .iter()
            .copied()
            .map(Value::Number)
            .collect::<Vec<_>>(),
    )?;
    validate(&fields)?;
    let month_code = format!("M{:02}", fields[1] as u32);
    let properties = NAMES
        .into_iter()
        .zip(fields)
        .flat_map(|(name, value)| {
            let number = Value::Number(value);
            [
                (name.into(), number.clone()),
                (format!("\0temporal-slot:\0{name}"), number),
            ]
        })
        .chain([
            ("monthCode".into(), Value::String(month_code.clone())),
            (
                "\0temporal-slot:\0monthCode".into(),
                Value::String(month_code),
            ),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
            ),
            ("\0temporal-plain-date-time".into(), Value::Boolean(true)),
        ])
        .collect();
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

/// The constructor's calendar argument is a calendar identifier, not a value
/// that may be coerced with the general string conversion rules.  Keep this
/// check at the constructor boundary so operations such as `withCalendar`
/// can continue to accept ISO date/time strings as specified.
pub(crate) fn construct_from_constructor(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(calendar) = arguments
        .get(9)
        .filter(|value| !matches!(value, Value::Undefined))
    {
        if !matches!(calendar, Value::String(_) | Value::StringUnits(_))
            || crate::conversion::is_symbol(calendar)
        {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
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
        if date_like || !crate::temporal::plain_date::is_iso_calendar_value(calendar)? {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    construct(arguments)
}

fn validate(fields: &[f64]) -> Result<(), VmError> {
    if !(1.0..=12.0).contains(&fields[1])
        || !(1.0..=31.0).contains(&fields[2])
        || !(0.0..=23.0).contains(&fields[3])
        || !(0.0..=59.0).contains(&fields[4])
        || !(0.0..=59.0).contains(&fields[5])
        || fields[6..]
            .iter()
            .any(|value| !(0.0..=999.0).contains(value))
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if fields[0] == -271_821.0
        && fields[1] == 4.0
        && fields[2] == 19.0
        && fields[3..].iter().all(|value| *value == 0.0)
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    Ok(())
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainDateTime => Some(Err(
            crate::value::error::throw_type_error("Temporal.PlainDateTime requires 'new'"),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeFrom => {
            Some(from(arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter
        | crate::ops::Builtin::TemporalPlainDateTimeYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDayGetter
        | crate::ops::Builtin::TemporalPlainDateTimeHourGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter
        | crate::ops::Builtin::TemporalPlainDateTimeSecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeNanosecondGetter => {
            Some(getter(builtin, _receiver))
        }
        crate::ops::Builtin::TemporalPlainDateTimeToString => {
            Some(to_string(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeToJSON
        | crate::ops::Builtin::TemporalPlainDateTimeToLocaleString => Some(to_string(_receiver, None)),
        crate::ops::Builtin::TemporalPlainDateTimeCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainDateTimeEquals => {
            Some(equals(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert PlainDateTime to a number"),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeAdd => {
            Some(add(_receiver, arguments.first(), arguments.get(1), 1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeSubtract => {
            Some(add(_receiver, arguments.first(), arguments.get(1), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeWith => {
            Some(with(_receiver, arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateTimeRound => {
            Some(round(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeUntil => Some(difference(
            _receiver,
            arguments.first(),
            1.0,
            arguments.get(1),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeSince => Some(difference(
            _receiver,
            arguments.first(),
            -1.0,
            arguments.get(1),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeToPlainDate => Some(to_plain_date(_receiver)),
        crate::ops::Builtin::TemporalPlainDateTimeToPlainTime => Some(to_plain_time(_receiver)),
        crate::ops::Builtin::TemporalPlainDateTimeToZonedDateTime => {
            Some(to_zoned_date_time(_receiver, arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateTimeWithCalendar => {
            Some(with_calendar(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeWithPlainTime => {
            Some(with_plain_time(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeDayOfWeekGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDayOfYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDaysInMonthGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDaysInWeekGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDaysInYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthsInYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeInLeapYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeEraGetter
        | crate::ops::Builtin::TemporalPlainDateTimeEraYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeWeekOfYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeYearOfWeekGetter => {
            Some(calendar_getter(builtin, _receiver))
        }
        _ => None,
    }
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: f64,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let left = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    let right = fields(&from(other, None)?)?;
    let mut smallest_unit = "nanosecond".to_string();
    let mut rounding_increment = 1.0;
    let mut rounding_mode = "trunc".to_string();
    let largest = if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined))
    {
        if !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
        let largest_value = crate::execute::get_property_result(options, "largestUnit")?;
        let largest_was_default = matches!(largest_value, Value::Undefined);
        let mut largest = if largest_was_default {
            "day".into()
        } else {
            let text = crate::conversion::to_string(&largest_value)?;
            let text = text.strip_suffix('s').unwrap_or(&text);
            if text == "auto" {
                "day".into()
            } else {
                text.to_string()
            }
        };
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
            return Err(crate::value::error::throw_range_error("Invalid largestUnit"));
        }
        let increment = crate::execute::get_property_result(options, "roundingIncrement")?;
        if !matches!(increment, Value::Undefined) {
            let increment = crate::conversion::to_number(&increment)?;
            if !increment.is_finite() || increment.trunc() < 1.0 || increment.trunc() > 1e9 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingIncrement",
                ));
            }
            rounding_increment = increment.trunc();
        }
        let mode = crate::execute::get_property_result(options, "roundingMode")?;
        if !matches!(mode, Value::Undefined) {
            let mode = crate::conversion::to_string(&mode)?;
            if !matches!(
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
            ) {
                return Err(crate::value::error::throw_range_error("Invalid roundingMode"));
            }
            rounding_mode = mode;
        }
        let smallest = crate::execute::get_property_result(options, "smallestUnit")?;
        if !matches!(smallest, Value::Undefined) {
            let smallest = crate::conversion::to_string(&smallest)?;
            let smallest = smallest.strip_suffix('s').unwrap_or(&smallest);
            if !matches!(
                smallest,
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
                return Err(crate::value::error::throw_range_error("Invalid smallestUnit"));
            }
            smallest_unit = smallest.to_string();
            if largest_was_default && unit_rank(smallest) < unit_rank(&largest) {
                largest = smallest.to_string();
            } else if unit_rank(smallest) < unit_rank(&largest) {
                return Err(crate::value::error::throw_range_error(
                    "smallestUnit larger than largestUnit",
                ));
            }
        }
        largest
    } else {
        "day".into()
    };
    let increment_max = match smallest_unit.as_str() {
        "day" => 0,
        "hour" => 24,
        "minute" | "second" => 60,
        "millisecond" | "microsecond" | "nanosecond" => 1_000,
        _ => 0,
    };
    if increment_max > 1
            && (rounding_increment as u64) >= increment_max
        || increment_max > 1
            && increment_max % (rounding_increment as u64) != 0
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    if matches!(largest.as_str(), "year" | "month" | "week") {
        return calendar_difference(
            &left,
            &right,
            direction,
            &largest,
            &smallest_unit,
            rounding_increment,
            &rounding_mode,
        );
    }
    let left_total = date_time_total_nanos(&left);
    let right_total = date_time_total_nanos(&right);
    let mut delta = (right_total - left_total) * direction as i128;
    let quantum = match smallest_unit.as_str() {
        "day" => 86_400_000_000_000_i128,
        "hour" => 3_600_000_000_000,
        "minute" => 60_000_000_000,
        "second" => 1_000_000_000,
        "millisecond" => 1_000_000,
        "microsecond" => 1_000,
        _ => 1,
    } * rounding_increment as i128;
    if quantum > 1 {
        delta = round_integer(delta, quantum, &rounding_mode);
    }
    let sign = delta.signum();
    let mut remainder = delta.unsigned_abs();
    let days = remainder / 86_400_000_000_000;
    remainder %= 86_400_000_000_000;
    let hours = remainder / 3_600_000_000_000;
    remainder %= 3_600_000_000_000;
    let minutes = remainder / 60_000_000_000;
    remainder %= 60_000_000_000;
    let seconds = remainder / 1_000_000_000;
    remainder %= 1_000_000_000;
    let milliseconds = remainder / 1_000_000;
    remainder %= 1_000_000;
    let microseconds = remainder / 1_000;
    let nanoseconds = remainder % 1_000;
    let mut days = days as i128;
    let mut hours = hours as i128;
    let mut minutes = minutes as i128;
    let mut seconds = seconds as i128;
    let mut milliseconds = milliseconds as i128;
    let mut microseconds = microseconds as i128;
    let mut nanoseconds = nanoseconds as i128;
    match largest.as_str() {
        "hour" => {
            hours += days * 24;
            days = 0;
        }
        "minute" => {
            minutes += days * 1_440 + hours * 60;
            days = 0;
            hours = 0;
        }
        "second" => {
            seconds += days * 86_400 + hours * 3_600 + minutes * 60;
            days = 0;
            hours = 0;
            minutes = 0;
        }
        "millisecond" => {
            milliseconds += days * 86_400_000 + hours * 3_600_000 + minutes * 60_000 + seconds * 1_000;
            days = 0;
            hours = 0;
            minutes = 0;
            seconds = 0;
        }
        "microsecond" => {
            microseconds += days * 86_400_000_000 + hours * 3_600_000_000 + minutes * 60_000_000 + seconds * 1_000_000 + milliseconds * 1_000;
            days = 0;
            hours = 0;
            minutes = 0;
            seconds = 0;
            milliseconds = 0;
        }
        "nanosecond" => {
            nanoseconds += days * 86_400_000_000_000 + hours * 3_600_000_000_000 + minutes * 60_000_000_000 + seconds * 1_000_000_000 + milliseconds * 1_000_000 + microseconds * 1_000;
            days = 0;
            hours = 0;
            minutes = 0;
            seconds = 0;
            milliseconds = 0;
            microseconds = 0;
        }
        _ => {}
    }
    crate::temporal::duration::construct(&[
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(days as f64 * sign as f64),
        Value::Number(hours as f64 * sign as f64),
        Value::Number(minutes as f64 * sign as f64),
        Value::Number(seconds as f64 * sign as f64),
        Value::Number(milliseconds as f64 * sign as f64),
        Value::Number(microseconds as f64 * sign as f64),
        Value::Number(nanoseconds as f64 * sign as f64),
    ])
}

fn calendar_difference(
    left: &[f64],
    right: &[f64],
    direction: f64,
    largest: &str,
    smallest: &str,
    increment: f64,
    mode: &str,
) -> Result<Value, VmError> {
    let left_total = date_time_total_nanos(left);
    let right_total = date_time_total_nanos(right);
    let (start, end, sign) = if left_total <= right_total {
        (left, right, if direction > 0.0 { 1_i64 } else { -1_i64 })
    } else {
        (right, left, if direction > 0.0 { -1_i64 } else { 1_i64 })
    };
    let start_serial = crate::temporal::plain_date::date_serial(start[0], start[1], start[2]);
    let end_serial = crate::temporal::plain_date::date_serial(end[0], end[1], end[2]);
    let mut years = 0i64;
    let mut months = 0i64;
    if largest == "year" {
        years = end[0] as i64 - start[0] as i64;
        let candidate = add_months_serial(start, years * 12);
        if candidate > end_serial {
            years -= 1;
        }
        months = (end[0] as i64 - (start[0] as i64 + years)) * 12 + end[1] as i64 - start[1] as i64;
    } else if largest == "month" {
        months = (end[0] as i64 - start[0] as i64) * 12 + end[1] as i64 - start[1] as i64;
    }
    let anchor = add_months_serial(
        start,
        years * 12 + months * if largest == "week" { 0 } else { 1 },
    );
    let mut days = end_serial - anchor;
    if largest == "week" {
        months = 0;
        days = end_serial - start_serial;
    }
    let mut weeks = if largest == "week" { days / 7 } else { 0 };
    if largest == "week" {
        days %= 7;
    }
    let time_fraction_days = (time_of_day_nanos(end) - time_of_day_nanos(start)) as f64
        / 86_400_000_000_000.0;
    if smallest == "day" && matches!(largest, "year" | "month" | "week") {
        let rounded = round_quotient((days as f64 + time_fraction_days) / increment, mode)
            * increment;
        years = 0;
        months = 0;
        weeks = 0;
        days = rounded as i64;
    }
    if matches!(smallest, "year" | "month" | "week") {
        let unit_value = match smallest {
            "year" => years as f64 + months as f64 / 12.0 + (days as f64 + time_fraction_days) / 365.0,
            "month" => years as f64 * 12.0 + months as f64 + (days as f64 + time_fraction_days) / 30.0,
            _ => (weeks as f64) + (days as f64 + time_fraction_days) / 7.0,
        };
        let rounded = (round_quotient(unit_value * sign as f64 / increment, mode) * increment).abs();
        match smallest {
            "year" => {
                years = rounded as i64;
                months = 0;
                days = 0;
                weeks = 0;
            }
            "month" => {
                if largest == "month" {
                    years = 0;
                    months = rounded as i64;
                } else {
                    years = (rounded / 12.0).trunc() as i64;
                    months = (rounded - years as f64 * 12.0) as i64;
                }
                days = 0;
                weeks = 0;
            }
            _ => {
                years = 0;
                months = 0;
                weeks = if largest == "week" {
                    rounded as i64
                } else {
                    0
                };
                days = if largest == "week" {
                    0
                } else {
                    (rounded * 7.0) as i64
                };
            }
        }
    }
    crate::temporal::duration::construct(&[
        Value::Number((years * sign) as f64),
        Value::Number((months * sign) as f64),
        Value::Number((weeks * sign) as f64),
        Value::Number((days * sign) as f64),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
    ])
}

fn add_months_serial(values: &[f64], months: i64) -> i64 {
    let total = values[0] as i64 * 12 + values[1] as i64 - 1 + months;
    let year = (total.div_euclid(12)) as i32;
    let month = total.rem_euclid(12) as u32 + 1;
    let day = (values[2] as u32).min(days_in_month(year, month));
    crate::temporal::plain_date::date_serial(year as f64, month as f64, day as f64)
}

fn date_time_total_nanos(values: &[f64]) -> i128 {
    let date_days = (crate::temporal::plain_date::date_serial(values[0], values[1], values[2])
        - crate::temporal::plain_date::date_serial(1970.0, 1.0, 1.0)) as i128;
    date_days * 86_400_000_000_000
        + values[3] as i128 * 3_600_000_000_000
        + values[4] as i128 * 60_000_000_000
        + values[5] as i128 * 1_000_000_000
        + values[6] as i128 * 1_000_000
        + values[7] as i128 * 1_000
        + values[8] as i128
}

fn time_of_day_nanos(values: &[f64]) -> i128 {
    values[3] as i128 * 3_600_000_000_000
        + values[4] as i128 * 60_000_000_000
        + values[5] as i128 * 1_000_000_000
        + values[6] as i128 * 1_000_000
        + values[7] as i128 * 1_000
        + values[8] as i128
}

fn to_plain_date(receiver: Option<&Value>) -> Result<Value, VmError> {
    let values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    crate::temporal::plain_date::construct(
        &values[..3]
            .iter()
            .copied()
            .map(Value::Number)
            .collect::<Vec<_>>(),
    )
}

fn to_plain_time(receiver: Option<&Value>) -> Result<Value, VmError> {
    let values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    crate::temporal::plain_time::construct(
        &values[3..]
            .iter()
            .copied()
            .map(Value::Number)
            .collect::<Vec<_>>(),
    )
}

fn to_zoned_date_time(
    receiver: Option<&Value>,
    time_zone: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let disambiguation = if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
        if !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
        let value = crate::execute::get_property_result(options, "disambiguation")?;
        if matches!(value, Value::Undefined) {
            "compatible".to_string()
        } else {
            if crate::conversion::is_symbol(&value) {
                return Err(crate::value::error::throw_type_error("Invalid disambiguation"));
            }
            let value = crate::conversion::to_string(&value)?;
            if !matches!(value.as_str(), "compatible" | "earlier" | "later" | "reject") {
                return Err(crate::value::error::throw_range_error("Invalid disambiguation"));
            }
            value
        }
    } else {
        "compatible".to_string()
    };
    let values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    let time_zone = time_zone
        .ok_or_else(|| crate::value::error::throw_type_error("Missing time zone"))?;
    if crate::conversion::is_symbol(time_zone) {
        return Err(crate::value::error::throw_type_error("Invalid time zone"));
    }
    let time_zone = match time_zone {
        Value::String(value) => value.clone(),
        Value::StringUnits(_) => crate::conversion::to_string(time_zone)?,
        _ => return Err(crate::value::error::throw_type_error("Invalid time zone")),
    };
    let time_zone = normalize_time_zone_identifier(&time_zone)?;
    let _ = disambiguation;
    let mut epoch = epoch_nanos(&values);
    if let Some(offset) = fixed_offset_nanos(&time_zone) {
        epoch -= offset;
    }
    const INSTANT_LIMIT: i128 = 8_640_000_000_000_000_000_000;
    if !(-INSTANT_LIMIT..=INSTANT_LIMIT).contains(&epoch) {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            (
                "epochNanoseconds".into(),
                Value::BigInt(epoch.to_string()),
            ),
            ("calendarId".into(), Value::String("iso8601".into())),
            ("timeZoneId".into(), Value::String(time_zone)),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
            ),
        ]),
    )))
}

fn with_calendar(receiver: Option<&Value>, calendar: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let calendar =
        calendar.ok_or_else(|| crate::value::error::throw_type_error("Missing calendar"))?;
    if !crate::temporal::plain_date::is_temporal_date_like(calendar) {
        if !matches!(calendar, Value::String(_) | Value::StringUnits(_))
            || crate::conversion::is_symbol(calendar)
        {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if !crate::temporal::plain_date::is_iso_calendar_value(calendar)? {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    let _calendar = crate::conversion::to_string(calendar)?;
    let values = fields(receiver)?;
    let month_code = format!("M{:02}", values[1] as u32);
    let properties = NAMES
        .iter()
        .copied()
        .zip(values)
        .map(|(name, value)| (name.into(), Value::Number(value)))
        .chain([
            ("monthCode".into(), Value::String(month_code)),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
            ),
        ])
        .collect();
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

fn epoch_nanos(values: &[f64]) -> i128 {
    let days = (crate::temporal::plain_date::date_serial(values[0], values[1], values[2])
        - crate::temporal::plain_date::date_serial(1970.0, 1.0, 1.0)) as i128;
    days * 86_400_000_000_000
        + values[3] as i128 * 3_600_000_000_000
        + values[4] as i128 * 60_000_000_000
        + values[5] as i128 * 1_000_000_000
        + values[6] as i128 * 1_000_000
        + values[7] as i128 * 1_000
        + values[8] as i128
}

fn fixed_offset_nanos(time_zone: &str) -> Option<i128> {
    let sign = match time_zone.as_bytes().first()? {
        b'+' => 1_i128,
        b'-' => -1_i128,
        _ => return None,
    };
    let text = &time_zone[1..];
    let (hours, minutes) = text
        .split_once(':')
        .map_or((text, "0"), |(hours, minutes)| (hours, minutes));
    let hours = hours.parse::<i128>().ok()?;
    let minutes = minutes.parse::<i128>().ok()?;
    Some(sign * (hours * 3_600_000_000_000 + minutes * 60_000_000_000))
}

fn normalize_time_zone_identifier(text: &str) -> Result<String, VmError> {
    if text.is_empty() || text.starts_with("-000000-") || text.contains('\u{2212}') {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    if text.eq_ignore_ascii_case("utc") {
        return Ok("UTC".into());
    }
    if let Some((_, annotation)) = text.rsplit_once('[') {
        let annotation = annotation
            .strip_suffix(']')
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"))?;
        if annotation.is_empty() {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        if annotation.eq_ignore_ascii_case("utc") {
            return Ok("UTC".into());
        }
        if annotation.starts_with(['+', '-']) {
            if fixed_offset_nanos(annotation).is_none() {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
            return Ok(annotation.to_string());
        }
        return Ok(annotation.to_string());
    }
    let time = text.split(['T', 't', ' ']).nth(1);
    if let Some(time) = time {
        if time.ends_with(['Z', 'z']) {
            return Ok("UTC".into());
        }
        let offset = time
            .get(1..)
            .and_then(|value| value.find(['+', '-']).map(|index| &value[index..]));
        if let Some(offset) = offset {
            if matches!(offset.len(), 3 | 5 | 6)
                && (offset.len() == 3
                    && offset[1..].bytes().all(|byte| byte.is_ascii_digit())
                    || offset.len() == 6
                        && offset.as_bytes().get(3) == Some(&b':')
                        && offset[1..3].bytes().all(|byte| byte.is_ascii_digit())
                        && offset[4..].bytes().all(|byte| byte.is_ascii_digit()))
            {
                return Ok(offset.to_string());
            }
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    if fixed_offset_nanos(text).is_some() {
        return Ok(text.to_string());
    }
    Ok(text.to_string())
}

fn with_plain_time(receiver: Option<&Value>, time: Option<&Value>) -> Result<Value, VmError> {
    let mut values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    if let Some(time) = time.filter(|value| !matches!(value, Value::Undefined)) {
        let time = crate::temporal::plain_time::execute(
            crate::ops::Builtin::TemporalPlainTimeFrom,
            None,
            std::slice::from_ref(time),
        )
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainTime"))??;
        let time = [
            "hour",
            "minute",
            "second",
            "millisecond",
            "microsecond",
            "nanosecond",
        ]
        .iter()
        .map(|name| crate::execute::get_property_result(&time, name))
        .map(|value| value.and_then(|value| crate::conversion::to_number(&value)))
        .collect::<Result<Vec<_>, _>>()?;
        values[3..].copy_from_slice(&time);
    } else {
        values[3..].fill(0.0);
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn calendar_getter(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    let year = values[0] as i32;
    let month = values[1] as u32;
    let day = values[2] as u32;
    Ok(match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeDayOfWeekGetter => {
            Value::Number(proleptic_weekday(year, month, day) as f64)
        }
        crate::ops::Builtin::TemporalPlainDateTimeDayOfYearGetter => Value::Number(
            (1..month).map(|m| days_in_month(year, m)).sum::<u32>() as f64 + day as f64,
        ),
        crate::ops::Builtin::TemporalPlainDateTimeDaysInMonthGetter => {
            Value::Number(days_in_month(year, month) as f64)
        }
        crate::ops::Builtin::TemporalPlainDateTimeDaysInWeekGetter => Value::Number(7.0),
        crate::ops::Builtin::TemporalPlainDateTimeDaysInYearGetter => {
            Value::Number(if chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some() {
                366.0
            } else {
                365.0
            })
        }
        crate::ops::Builtin::TemporalPlainDateTimeMonthsInYearGetter => Value::Number(12.0),
        crate::ops::Builtin::TemporalPlainDateTimeInLeapYearGetter => {
            Value::Boolean(chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some())
        }
        crate::ops::Builtin::TemporalPlainDateTimeEraGetter => Value::Undefined,
        crate::ops::Builtin::TemporalPlainDateTimeEraYearGetter => Value::Undefined,
        crate::ops::Builtin::TemporalPlainDateTimeWeekOfYearGetter => Value::Number(
            chrono::NaiveDate::from_ymd_opt(year, month, day)
                .map(|date| date.iso_week().week() as f64)
                .unwrap_or(f64::NAN),
        ),
        crate::ops::Builtin::TemporalPlainDateTimeYearOfWeekGetter => Value::Number(
            chrono::NaiveDate::from_ymd_opt(year, month, day)
                .map(|date| date.iso_week().year() as f64)
                .unwrap_or(f64::NAN),
        ),
        _ => Value::Undefined,
    })
}

fn proleptic_weekday(year: i32, month: u32, day: u32) -> u32 {
    let date = NaiveDate::from_ymd_opt(year, month, day).unwrap_or(NaiveDate::MIN);
    date.weekday().number_from_monday()
}

fn getter(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let Value::Object(object) = receiver else {
        return Err(crate::value::error::throw_type_error("Not a PlainDateTime"));
    };
    if !object.iter().any(|(key, value)| {
        key == "\0prototype"
            && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
    }) && !object.iter().any(|(key, value)| {
        key == "\0temporal-plain-date-time" && value == Value::Boolean(true)
    }) {
        return Err(crate::value::error::throw_type_error("Not a PlainDateTime"));
    }
    let name = match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter => "calendarId",
        crate::ops::Builtin::TemporalPlainDateTimeYearGetter => "year",
        crate::ops::Builtin::TemporalPlainDateTimeMonthGetter => "month",
        crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter => "monthCode",
        crate::ops::Builtin::TemporalPlainDateTimeDayGetter => "day",
        crate::ops::Builtin::TemporalPlainDateTimeHourGetter => "hour",
        crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter => "minute",
        crate::ops::Builtin::TemporalPlainDateTimeSecondGetter => "second",
        crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter => "millisecond",
        crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter => "microsecond",
        _ => "nanosecond",
    };
    if let Value::Object(object) = receiver {
        if let Some((_, value)) = object
            .iter()
            .find(|(key, _)| key == &format!("\0temporal-slot:\0{name}"))
        {
            return Ok(value.clone());
        }
    }
    crate::execute::get_property_result(receiver, name)
}

fn fields(value: &Value) -> Result<Vec<f64>, VmError> {
    let Value::Object(object) = value else {
        return Err(crate::value::error::throw_type_error("Not a PlainDateTime"));
    };
    if !object.iter().any(|(key, value)| {
        key == "\0prototype"
            && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
    }) && !object.iter().any(|(key, value)| {
        key == "\0temporal-plain-date-time" && value == Value::Boolean(true)
    }) {
        return Err(crate::value::error::throw_type_error("Not a PlainDateTime"));
    }
    NAMES
        .iter()
        .map(|name| {
            object
                .iter()
                .find(|(key, value)| {
                    (key == name || key == &format!("\0temporal-slot:\0{name}"))
                        && matches!(value, Value::Number(_))
                })
                .map(|(_, value)| match value {
                    Value::Number(value) => Ok(value),
                    _ => unreachable!(),
                })
                .unwrap_or_else(|| {
                    crate::execute::get_property_result(value, name)
                        .and_then(|value| crate::conversion::to_number(&value))
                })
        })
        .collect()
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = fields(&from(arguments.first(), None)?)?;
    let right = fields(&from(arguments.get(1), None)?)?;
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    Ok(Value::Boolean(fields(receiver)? == fields(&from(other, None)?)?))
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let duration = crate::temporal::duration::from(duration)?;
    let overflow = overflow_option(options)?;
    let mut values = fields(receiver)?;
    let months = (number_property(&duration, "years") * 12.0
        + number_property(&duration, "months"))
        * direction;
    let total = values[0] * 12.0 + values[1] - 1.0 + months;
    values[0] = (total / 12.0).floor();
    values[1] = total.rem_euclid(12.0) + 1.0;
    let original_day = values[2];
    values[2] = values[2].min(days_in_month(values[0] as i32, values[1] as u32) as f64);
    if overflow == "reject" && values[2] != original_day {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let days = (number_property(&duration, "weeks") * 7.0 + number_property(&duration, "days"))
        * direction;
    let current_time = (values[3] as i128) * 3_600_000_000_000
        + (values[4] as i128) * 60_000_000_000
        + (values[5] as i128) * 1_000_000_000
        + (values[6] as i128) * 1_000_000
        + (values[7] as i128) * 1_000
        + values[8] as i128;
    let time_delta = current_time
        + ((number_property(&duration, "hours") as i128) * 3_600_000_000_000
            + (number_property(&duration, "minutes") as i128) * 60_000_000_000
            + (number_property(&duration, "seconds") as i128) * 1_000_000_000
            + (number_property(&duration, "milliseconds") as i128) * 1_000_000
            + (number_property(&duration, "microseconds") as i128) * 1_000
            + number_property(&duration, "nanoseconds") as i128)
            * direction as i128;
    let day_nanos = 86_400_000_000_000i128;
    let carry_days = time_delta.div_euclid(day_nanos);
    let remainder = time_delta.rem_euclid(day_nanos) as f64;
    let total_days = days as i128 + carry_days;
    if total_days != 0 {
        let serial = crate::temporal::plain_date::date_serial(values[0], values[1], values[2])
            .checked_add(total_days as i64)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
        let (year, month, day) = crate::temporal::plain_date::civil_from_serial(serial);
        values[0] = year as f64;
        values[1] = month as f64;
        values[2] = day as f64;
    }
    values[3] = (remainder / 3_600_000_000_000.0).floor();
    let mut remainder = remainder - values[3] * 3_600_000_000_000.0;
    values[4] = (remainder / 60_000_000_000.0).floor();
    remainder -= values[4] * 60_000_000_000.0;
    values[5] = (remainder / 1_000_000_000.0).floor();
    remainder -= values[5] * 1_000_000_000.0;
    values[6] = (remainder / 1_000_000.0).floor();
    remainder -= values[6] * 1_000_000.0;
    values[7] = (remainder / 1_000.0).floor();
    values[8] = remainder - values[7] * 1_000.0;
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(0.0)
}

fn overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("constrain".into());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok("constrain".into());
    }
    let value = crate::conversion::to_string(&value)?;
    if matches!(value.as_str(), "constrain" | "reject") {
        Ok(value)
    } else {
        Err(crate::value::error::throw_range_error("Invalid overflow"))
    }
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let owned_options;
    let options = match options {
        Some(value) if crate::value::is_object(value) => value,
        Some(value @ (Value::String(_) | Value::StringUnits(_))) => {
            owned_options = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                (
                    "smallestUnit".into(),
                    value.clone(),
                ),
            ])));
            &owned_options
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid rounding options")),
    };
    let unit = crate::execute::get_property_result(options, "smallestUnit")?;
    if crate::conversion::is_symbol(&unit) {
        return Err(crate::value::error::throw_type_error(
            "Invalid smallestUnit",
        ));
    }
    let unit = crate::conversion::to_string(&unit)?;
    let unit = unit.strip_suffix('s').unwrap_or(&unit).to_string();
    let quantum = match unit.as_str() {
        "day" => 86_400_000_000_000.0,
        "hour" => 3_600_000_000_000.0,
        "minute" => 60_000_000_000.0,
        "second" => 1_000_000_000.0,
        "millisecond" => 1_000_000.0,
        "microsecond" => 1_000.0,
        "nanosecond" => 1.0,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ))
        }
    };
    let increment_value = crate::execute::get_property_result(options, "roundingIncrement")?;
    let increment = if matches!(increment_value, Value::Undefined) {
        1.0
    } else {
        crate::conversion::to_number(&increment_value)?
    }
    .trunc();
    let maximum: f64 = match unit.as_str() {
        "day" => 1.0,
        "hour" => 24.0,
        "minute" | "second" => 60.0,
        _ => 1_000.0,
    };
    if !increment.is_finite()
        || increment < 1.0
        || if unit == "day" {
            increment > maximum
        } else {
            increment >= maximum
        }
        || (maximum as u64) % (increment as u64) != 0
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let mode_value = crate::execute::get_property_result(options, "roundingMode")?;
    let mut mode = "halfExpand".to_string();
    if !matches!(mode_value, Value::Undefined) {
        if crate::conversion::is_symbol(&mode_value) {
            return Err(crate::value::error::throw_type_error(
                "Invalid roundingMode",
            ));
        }
        mode = crate::conversion::to_string(&mode_value)?;
        if !matches!(
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
        ) {
            return Err(crate::value::error::throw_range_error(
                "Invalid roundingMode",
            ));
        }
    }
    round_values(fields(receiver)?, quantum, increment, &mode)
}

fn round_values(
    mut values: Vec<f64>,
    quantum: f64,
    increment: f64,
    mode: &str,
) -> Result<Value, VmError> {
    let total = values[3] * 3_600_000_000_000.0
        + values[4] * 60_000_000_000.0
        + values[5] * 1_000_000_000.0
        + values[6] * 1_000_000.0
        + values[7] * 1_000.0
        + values[8];
    let quotient = total / (quantum * increment);
    let rounded = round_quotient(quotient, mode) * quantum * increment;
    let day_nanos = 86_400_000_000_000.0;
    let carry_days = (rounded / day_nanos).floor();
    if carry_days != 0.0 {
        let serial = crate::temporal::plain_date::date_serial(values[0], values[1], values[2])
            .checked_add(carry_days as i64)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
        let (year, month, day) = crate::temporal::plain_date::civil_from_serial(serial);
        values[0] = year as f64;
        values[1] = month as f64;
        values[2] = day as f64;
    }
    let rounded = rounded.rem_euclid(day_nanos);
    values[3] = (rounded / 3_600_000_000_000.0).floor();
    let mut remainder = rounded - values[3] * 3_600_000_000_000.0;
    values[4] = (remainder / 60_000_000_000.0).floor();
    remainder -= values[4] * 60_000_000_000.0;
    values[5] = (remainder / 1_000_000_000.0).floor();
    remainder -= values[5] * 1_000_000_000.0;
    values[6] = (remainder / 1_000_000.0).floor();
    remainder -= values[6] * 1_000_000.0;
    values[7] = (remainder / 1_000.0).floor();
    values[8] = remainder - values[7] * 1_000.0;
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn round_quotient(value: f64, mode: &str) -> f64 {
    let floor = value.floor();
    let ceil = value.ceil();
    let fraction = value - floor;
    match mode {
        "ceil" => ceil,
        "floor" => floor,
        "trunc" => value.trunc(),
        "expand" => {
            if value >= 0.0 {
                ceil
            } else {
                floor
            }
        }
        "halfTrunc" => {
            if fraction < 0.5 {
                floor
            } else if fraction > 0.5 {
                ceil
            } else {
                value.trunc()
            }
        }
        "halfCeil" => {
            if fraction >= 0.5 {
                ceil
            } else {
                floor
            }
        }
        "halfFloor" => {
            if fraction > 0.5 {
                ceil
            } else {
                floor
            }
        }
        "halfEven" => {
            if fraction < 0.5 {
                floor
            } else if fraction > 0.5 {
                ceil
            } else if (floor as i64) % 2 == 0 {
                floor
            } else {
                ceil
            }
        }
        _ => {
            if fraction >= 0.5 {
                ceil
            } else {
                floor
            }
        }
    }
}

fn round_integer(value: i128, quantum: i128, mode: &str) -> i128 {
    let sign = value.signum();
    let magnitude = value.unsigned_abs();
    let quotient = magnitude / quantum as u128;
    let remainder = magnitude % quantum as u128;
    if remainder == 0 {
        return value;
    }
    let twice = remainder.saturating_mul(2);
    let increment = match mode {
        "ceil" => sign > 0,
        "floor" => sign < 0,
        "expand" => true,
        "trunc" => false,
        "halfExpand" => twice >= quantum as u128,
        "halfTrunc" => twice > quantum as u128,
        "halfCeil" => twice > quantum as u128 || (twice == quantum as u128 && sign > 0),
        "halfFloor" => twice > quantum as u128 || (twice == quantum as u128 && sign < 0),
        "halfEven" => twice > quantum as u128 || (twice == quantum as u128 && quotient % 2 == 1),
        _ => twice >= quantum as u128,
    };
    let rounded = quotient + u128::from(increment);
    (rounded as i128) * sign * quantum
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid date-time"))?;
    let mut values = fields(receiver)?;
    if crate::temporal::plain_date::is_temporal_date_like(changes) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let time_zone = crate::execute::get_property_result(changes, "timeZone")?;
    if !matches!(time_zone, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid time zone"));
    }
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month = crate::execute::get_property_result(changes, "month")?;
    if !matches!(month_code, Value::Undefined) {
        values[1] = crate::conversion::to_number(&month_code_number(&month_code)?)?;
    }
    for (index, name) in NAMES.iter().enumerate() {
        let value = crate::execute::get_property_result(changes, name)?;
        if !matches!(value, Value::Undefined) {
            values[index] = if *name == "monthCode" {
                crate::conversion::to_number(&month_code_number(&value)?)?
            } else {
                crate::conversion::to_number(&value)?.trunc()
            };
        }
    }
    values[1] = values[1].trunc();
    let recognized = NAMES.iter().any(|name| {
        crate::execute::get_property_result(changes, name)
            .is_ok_and(|value| !matches!(value, Value::Undefined))
    }) || !matches!(month_code, Value::Undefined);
    if !recognized {
        return Err(crate::value::error::throw_type_error(
            "Insufficient date-time data",
        ));
    }
    if !values[0].is_finite()
        || !values[1].is_finite()
        || !values[2].is_finite()
        || values[1] < 1.0
        || values[2] < 1.0
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if let Some(value) = options {
        if !matches!(
            value,
            Value::Undefined | Value::Object(_) | Value::Function(_) | Value::BoundFunction(_)
        ) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
    }
    let overflow = options
        .and_then(|value| crate::execute::get_property_result(value, "overflow").ok())
        .unwrap_or(Value::String("constrain".into()));
    let overflow = match overflow {
        Value::Undefined => "constrain".to_string(),
        value => crate::conversion::to_string(&value)?,
    };
    if overflow != "constrain" && overflow != "reject" {
        return Err(crate::value::error::throw_range_error("Invalid overflow"));
    }
    values[1] = values[1].min(12.0);
    if values[2] > days_in_month(values[0] as i32, values[1] as u32) as f64 {
        if overflow == "constrain" {
            values[2] = days_in_month(values[0] as i32, values[1] as u32) as f64;
        } else {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
    }
    for (index, limit) in [23.0, 59.0, 59.0, 999.0, 999.0, 999.0]
        .into_iter()
        .enumerate()
    {
        let index = index + 3;
        if !values[index].is_finite() || values[index] < 0.0 {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
        if values[index] > limit {
            if overflow == "constrain" {
                values[index] = limit;
            } else {
                return Err(crate::value::error::throw_range_error("Invalid date-time"));
            }
        }
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let _ = fields(receiver)?;
    let mut calendar_name = "auto".to_string();
    let mut normalized_smallest_unit = None;
    let mut rounding_mode = "trunc".to_string();
    let mut digits = usize::MAX;
    if let Some(options) = options {
        if !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error(
                "Invalid string options",
            ));
        }
        let calendar_name_value = crate::execute::get_property_result(options, "calendarName")?;
        if !matches!(calendar_name_value, Value::Undefined) {
            if crate::conversion::is_symbol(&calendar_name_value) {
                return Err(crate::value::error::throw_type_error("Invalid calendarName"));
            }
            let calendar_name_text = crate::conversion::to_string(&calendar_name_value)?;
            if !matches!(calendar_name_text.as_str(), "auto" | "always" | "never" | "critical") {
                return Err(crate::value::error::throw_range_error(
                    "Invalid calendarName",
                ));
            }
            calendar_name = calendar_name_text;
        };
        let fractional = crate::execute::get_property_result(options, "fractionalSecondDigits")?;
        digits = match fractional {
            Value::Number(value) => {
                let value = value.floor();
                if !(0.0..=9.0).contains(&value) {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid fractionalSecondDigits",
                    ));
                }
                value as usize
            }
            Value::String(value) if value == "auto" => usize::MAX,
            Value::Undefined => usize::MAX,
            value if crate::conversion::is_symbol(&value) => {
                return Err(crate::value::error::throw_type_error(
                    "Invalid fractionalSecondDigits",
                ))
            }
            Value::String(_) => {
                return Err(crate::value::error::throw_range_error(
                    "Invalid fractionalSecondDigits",
                ))
            }
            value => {
                let value = crate::conversion::to_string(&value)?;
                if value == "auto" {
                    usize::MAX
                } else {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid fractionalSecondDigits",
                    ));
                }
            }
        };
        let rounding_mode_value = crate::execute::get_property_result(options, "roundingMode")?;
        if !matches!(rounding_mode_value, Value::Undefined) {
            if crate::conversion::is_symbol(&rounding_mode_value) {
                return Err(crate::value::error::throw_type_error("Invalid roundingMode"));
            }
            let rounding_mode_text = crate::conversion::to_string(&rounding_mode_value)?;
            if !matches!(
                rounding_mode_text.as_str(),
                "ceil"
                    | "floor"
                    | "expand"
                    | "halfCeil"
                    | "halfFloor"
                    | "halfEven"
                    | "halfExpand"
                    | "halfTrunc"
                    | "trunc"
            ) {
                return Err(crate::value::error::throw_range_error("Invalid roundingMode"));
            }
            rounding_mode = rounding_mode_text;
        }
        let smallest_unit = crate::execute::get_property_result(options, "smallestUnit")?;
        if !matches!(smallest_unit, Value::Undefined) {
            if crate::conversion::is_symbol(&smallest_unit) {
                return Err(crate::value::error::throw_type_error("Invalid smallestUnit"));
            }
            let smallest_unit = crate::conversion::to_string(&smallest_unit)?;
            let unit = smallest_unit.strip_suffix('s').unwrap_or(&smallest_unit);
            if !matches!(
                unit,
                "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
            ) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid smallestUnit",
                ));
            }
            normalized_smallest_unit = Some(unit.to_string());
        }
    }
    let mut values = NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(receiver, name))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| crate::conversion::to_number(&value))
        .collect::<Result<Vec<_>, _>>()?;
    let quantum = normalized_smallest_unit.as_deref().map(|unit| match unit {
        "minute" => 60_000_000_000.0,
        "second" => 1_000_000_000.0,
        "millisecond" => 1_000_000.0,
        "microsecond" => 1_000.0,
        _ => 1.0,
    }).or_else(|| (digits != usize::MAX).then(|| 10_f64.powi((9 - digits) as i32)));
    if let Some(quantum) = quantum.filter(|quantum| *quantum > 1.0) {
        let rounded = round_values(values, quantum, 1.0, &rounding_mode)?;
        values = fields(&rounded)?;
    }
    let output_digits = match normalized_smallest_unit.as_deref() {
        Some("minute" | "second") => 0,
        Some("millisecond") => 3,
        Some("microsecond") => 6,
        Some("nanosecond") => 9,
        None => digits,
        _ => digits,
    };
    let omit_seconds = matches!(normalized_smallest_unit.as_deref(), Some("minute"));
    let fraction = values[6] as u32 * 1_000_000 + values[7] as u32 * 1_000 + values[8] as u32;
    let suffix = if output_digits == 0 || (fraction == 0 && output_digits == usize::MAX) {
        String::new()
    } else {
        let text = format!("{fraction:09}");
        let text = if output_digits == usize::MAX {
            text.trim_end_matches('0')
        } else {
            &text[..output_digits]
        };
        format!(".{text}")
    };
    let calendar_suffix = if calendar_name == "always" {
        "[u-ca=iso8601]".into()
    } else if calendar_name == "critical" {
        "[!u-ca=iso8601]".into()
    } else {
        String::new()
    };
    let year = year_text(values[0] as i32);
    let text = if omit_seconds {
        format!(
            "{year}-{:02}-{:02}T{:02}:{:02}{calendar_suffix}",
            values[1], values[2], values[3], values[4]
        )
    } else {
        format!(
            "{year}-{:02}-{:02}T{:02}:{:02}:{:02}{suffix}{calendar_suffix}",
            values[1], values[2], values[3], values[4], values[5]
        )
    };
    Ok(Value::String(text))
}

fn year_text(year: i32) -> String {
    if year < 0 {
        format!("-{year_abs:06}", year_abs = year.unsigned_abs())
    } else if year > 9999 {
        format!("+{year:06}")
    } else {
        format!("{year:04}")
    }
}

fn unit_rank(unit: &str) -> usize {
    match unit {
        "year" => 0,
        "month" => 1,
        "week" => 2,
        "day" => 3,
        "hour" => 4,
        "minute" => 5,
        "second" => 6,
        "millisecond" => 7,
        "microsecond" => 8,
        "nanosecond" => 9,
        _ => usize::MAX,
    }
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    };
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    if let Value::String(text) = value {
        let result = parse_string(text)?;
        from_overflow_option(options)?;
        return Ok(result);
    }
    if matches!(value, Value::StringUnits(_)) {
        let text = crate::conversion::to_string(value)?;
        let result = parse_string(&text)?;
        from_overflow_option(options)?;
        return Ok(result);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    let overflow = from_overflow_option(options)?;
    if let Value::Object(object) = value {
        let is_plain_date = object.iter().any(|(key, value)| {
            key == "\0temporal-plain-date" && value == Value::Boolean(true)
                || key == "\0prototype"
                    && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype)
        });
        if is_plain_date {
            let hidden = ["year", "month", "day"].map(|name| {
                object
                    .iter()
                    .find(|(key, value)| {
                        key == &format!("\0temporal-slot:\0{name}")
                            && matches!(value, Value::Number(_))
                    })
                    .map(|(_, value)| value.clone())
            });
            if let [Some(year), Some(month), Some(day)] = hidden {
                return construct(&[
                    year,
                    month,
                    day,
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                ]);
            }
        }
    }
    let year = crate::execute::get_property_result(value, "year")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    if matches!(year, Value::Undefined)
        || matches!(day, Value::Undefined)
        || (matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "Missing date-time field",
        ));
    }
    let month_code_value = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(month_code_number(&month_code)?)
    };
    let month = if matches!(month, Value::Undefined) {
        month_code_value
            .clone()
            .ok_or_else(|| crate::value::error::throw_type_error("Missing month"))?
    } else {
        month
    };
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        validate_calendar(&calendar)?;
    }
    let mut fields = vec![year, month, day];
    for name in &NAMES[3..] {
        let field = crate::execute::get_property_result(value, name)?;
        fields.push(if matches!(field, Value::Undefined) {
            Value::Number(0.0)
        } else {
            field
        });
    }
    let mut numeric = fields
        .iter()
        .map(crate::conversion::to_number)
        .map(|value| value.map(f64::trunc))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(month_code) = month_code_value {
        let month_code = crate::conversion::to_number(&month_code)?;
        if !(1.0..=12.0).contains(&month_code) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if numeric[1] != month_code {
            return Err(crate::value::error::throw_range_error("Month mismatch"));
        }
    }
    if numeric.iter().any(|value| !value.is_finite()) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if overflow == "constrain" {
        if numeric[1] > 12.0 {
            numeric[1] = 12.0;
        }
        if numeric[2] >= 1.0 && numeric[2].is_finite() {
            numeric[2] = numeric[2].min(days_in_month(numeric[0] as i32, numeric[1] as u32) as f64);
        }
        for (index, limit) in [23.0, 59.0, 59.0, 999.0, 999.0, 999.0]
            .into_iter()
            .enumerate()
        {
            let index = index + 3;
            if numeric[index] >= 0.0 && numeric[index].is_finite() {
                numeric[index] = numeric[index].min(limit);
            }
        }
    }
    construct(&numeric.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn from_overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("constrain".into());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let overflow = crate::execute::get_property_result(options, "overflow")?;
    if matches!(overflow, Value::Undefined) {
        return Ok("constrain".into());
    }
    let overflow = crate::conversion::to_string(&overflow)?;
    if !matches!(overflow.as_str(), "constrain" | "reject") {
        return Err(crate::value::error::throw_range_error("Invalid overflow"));
    }
    Ok(overflow)
}

fn month_code_number(value: &Value) -> Result<Value, VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    let code = match value {
        Value::String(code) => code.clone(),
        Value::StringUnits(_) => crate::conversion::to_string(value)?,
        Value::Object(_) => {
            let method = crate::execute::get_property_result(value, "toString")?;
            if !crate::conversion::is_callable(&method) {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            }
            let primitive = crate::functions::execute_target(&method, value, &[])?;
            if !matches!(primitive, Value::String(_) | Value::StringUnits(_)) {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            }
            crate::conversion::to_string(&primitive)?
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid monthCode")),
    };
    let core = code.strip_suffix('L').unwrap_or(&code);
    if core.len() != 3
        || !core.starts_with('M')
        || !core[1..].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    if code.ends_with('L') {
        return Ok(Value::Number(f64::NAN));
    }
    Ok(Value::Number(core[1..].parse::<f64>().unwrap_or(f64::NAN)))
}

fn validate_calendar(value: &Value) -> Result<(), VmError> {
    if let Value::Object(object) = value {
        if object.iter().any(|(key, value)| {
            key == "\0prototype"
                && matches!(
                    value,
                    Value::Builtin(
                        crate::ops::Builtin::TemporalPlainDatePrototype
                            | crate::ops::Builtin::TemporalPlainDateTimePrototype
                            | crate::ops::Builtin::TemporalPlainMonthDayPrototype
                            | crate::ops::Builtin::TemporalPlainYearMonthPrototype
                            | crate::ops::Builtin::TemporalZonedDateTimePrototype
                    )
                )
        }) {
            return Ok(());
        }
    }
    if !matches!(value, Value::String(_) | Value::StringUnits(_))
        || crate::conversion::is_symbol(value)
    {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let text = crate::conversion::to_string(value)?;
    if text.starts_with("-000000-") {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if !crate::temporal::plain_date::is_iso_calendar_value(value)? {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    } else {
        Ok(())
    }
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    let mut calendar_annotation = false;
    let mut calendar_critical = false;
    let mut time_zone_annotation = false;
    for part in text.split('[').skip(1) {
        let annotation = part
            .strip_suffix(']')
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid annotation"))?;
        let critical = annotation.starts_with('!');
        let annotation = annotation.strip_prefix('!').unwrap_or(annotation);
        if let Some((key, value)) = annotation.split_once('=') {
            if key.chars().any(|character| character.is_ascii_uppercase()) {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
            if key == "u-ca" && calendar_annotation {
                if critical || calendar_critical {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
                continue;
            }
            if key == "u-ca" {
                if value.is_empty() || !value.eq_ignore_ascii_case("iso8601")
                {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
                calendar_annotation = true;
                calendar_critical = critical;
            } else if critical {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
        } else if time_zone_annotation {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        } else if annotation.is_empty() {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        } else {
            time_zone_annotation = true;
        }
    }
    let main = text.split('[').next().unwrap_or(text);
    let (date, time) = main
        .split_once('T')
        .or_else(|| main.split_once('t'))
        .or_else(|| main.split_once(' '))
        .unwrap_or((main, "00:00"));
    let date_fields =
        if date.starts_with(['+', '-']) && date.len() >= 8 && date.as_bytes().get(7) == Some(&b'-')
        {
            let mut fields = vec![&date[..7]];
            fields.extend(date[8..].split('-'));
            fields
        } else if date.starts_with(['+', '-'])
            && date.len() == 11
            && date[1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            vec![&date[..7], &date[7..9], &date[9..]]
        } else if date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()) {
            vec![&date[..4], &date[4..6], &date[6..]]
        } else {
            date.split('-').collect::<Vec<_>>()
        };
    if date_fields.len() != 3 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if (!date_fields[0].starts_with(['+', '-']) && date_fields[0].len() != 4)
        || (date_fields[0].starts_with(['+', '-']) && date_fields[0].len() != 7)
        || date_fields[1].len() != 2
        || date_fields[2].len() != 2
        || !date_fields[1].bytes().all(|byte| byte.is_ascii_digit())
        || !date_fields[2].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if date_fields[0] == "-000000" {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if time.ends_with(['Z', 'z']) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if main.contains('\u{2212}') {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let time_offset = time
        .get(1..)
        .and_then(|value| value.find(['+', '-']).map(|index| &value[index + 1..]));
    if let Some(offset) = time_offset {
        let (core, fraction) = offset
            .split_once(['.', ','])
            .map_or((offset, None), |(core, fraction)| (core, Some(fraction)));
        let compact = core.replace(':', "");
        let valid_shape = matches!(compact.len(), 2 | 4 | 6)
            && compact.bytes().all(|byte| byte.is_ascii_digit())
            && (core.matches(':').count() == 0
                || (core.matches(':').count() == 1 && core.len() == 5)
                || (core.matches(':').count() == 2 && core.len() == 8));
        let valid_fraction = fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        });
        let clock_has_minutes = time
            .split(['+', '-'])
            .next()
            .unwrap_or(time)
            .split(['.', ','])
            .next()
            .is_some_and(|clock| clock.matches(':').count() >= 1 || clock.len() >= 4);
        let valid = valid_shape && valid_fraction && clock_has_minutes;
        if !valid {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
    }
    let time = time.split(['+', '-']).next().unwrap_or(time);
    let (clock, fraction) = time
        .split_once('.')
        .or_else(|| time.split_once(','))
        .map_or((time, ""), |parts| parts);
    let colon_clock = clock.contains(':');
    let clock = if colon_clock {
        clock.split(':').collect::<Vec<_>>()
    } else if matches!(clock.len(), 2 | 4 | 6) && clock.bytes().all(|byte| byte.is_ascii_digit()) {
        match clock.len() {
            2 => vec![&clock[..2]],
            4 => vec![&clock[..2], &clock[2..]],
            _ => vec![&clock[..2], &clock[2..4], &clock[4..]],
        }
    } else {
        Vec::new()
    };
    if colon_clock
        && clock
            .iter()
            .any(|part| part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if clock.is_empty() || clock.len() > 3 || fraction.len() > 9 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if (clock.len() < 3 && !fraction.is_empty()) || (!colon_clock && clock.len() == 1 && !fraction.is_empty()) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let mut fields = date_fields
        .into_iter()
        .chain(clock)
        .map(|part| part.parse::<f64>().unwrap_or(f64::NAN))
        .collect::<Vec<_>>();
    if fields.iter().any(|value| !value.is_finite() || value.fract() != 0.0) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    while fields.len() < 6 {
        fields.push(0.0);
    }
    if fields.get(5) == Some(&60.0) {
        fields[5] = 59.0;
    }
    if fields[3] > 23.0 || fields[4] > 59.0 || fields[5] > 59.0 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let nanos = format!("{fraction:0<9}")
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid date-time"))?;
    fields.extend([
        (nanos / 1_000_000.0).trunc(),
        (nanos / 1_000.0).trunc() % 1_000.0,
        nanos % 1_000.0,
    ]);
    if fields[6..].iter().any(|value| *value > 999.0) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    construct(&fields.into_iter().map(Value::Number).collect::<Vec<_>>())
}
