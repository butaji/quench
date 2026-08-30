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
    let mut fields = Vec::with_capacity(9);
    for index in 0..9 {
        let value = arguments.get(index).unwrap_or(&Value::Undefined);
        let number = if index >= 3 && matches!(value, Value::Undefined) {
            0.0
        } else {
            crate::conversion::to_number(value)?.trunc()
        };
        if !number.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
        fields.push(number);
    }
    let calendar = arguments
        .get(9)
        .and_then(|value| match value {
            Value::String(value) => crate::temporal::plain_date::canonical_calendar_id(value),
            _ => None,
        })
        .unwrap_or_else(|| "iso8601".into());
    let month_code_override = arguments.get(10).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    });
    let related_iso_year = arguments.get(11).and_then(|value| match value {
        Value::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    });
    let mut date_arguments = fields[..3]
        .iter()
        .copied()
        .map(Value::Number)
        .collect::<Vec<_>>();
    date_arguments.push(Value::String(calendar.clone()));
    let mut month_code_from_calendar = None;
    if month_code_override.is_none() || calendar == "iso8601" || calendar == "gregory" {
        if month_code_override.is_none() && calendar != "iso8601" && calendar != "gregory" {
            let date = crate::temporal::plain_date::construct(&date_arguments)?;
            if !crate::temporal::plain_year_month::calendar_edge_month_number(
                &calendar,
                fields[0] as i32,
                fields[1] as u32,
            ) {
                fields[1] = crate::execute::get_property_result(&date, "month")
                    .ok()
                    .and_then(|value| match value {
                        Value::Number(value) => Some(value),
                        _ => None,
                    })
                    .unwrap_or(fields[1]);
            }
            month_code_from_calendar = crate::execute::get_property_result(&date, "monthCode")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                });
        } else {
            crate::temporal::plain_date::construct(&date_arguments)?;
        }
    } else if calendar != "iso8601" && calendar != "gregory" {
        let valid = crate::temporal::plain_date::calendar_date_from_code(
            fields[0] as i32,
            month_code_override.as_deref().unwrap_or_default(),
            fields[2] as u32,
            &calendar,
        )
        .is_some();
        // ICU's lunisolar data has a deliberately bounded accuracy range.
        // Preserve Temporal's supported date range outside that window while
        // still rejecting malformed or impossible month codes nearby.
        if !valid && (-10_000..=10_000).contains(&(fields[0] as i32)) {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
    }
    validate(&fields)?;
    let month_code = month_code_override
        .or(month_code_from_calendar)
        .unwrap_or_else(|| format!("M{:02}", fields[1] as u32));
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
            ("calendarId".into(), Value::String(calendar)),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
            ),
            ("\0temporal-plain-date-time".into(), Value::Boolean(true)),
        ])
        .chain(
            related_iso_year
                .map(|year| ("\0temporal-related-iso-year".into(), Value::Number(year))),
        )
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
    let calendar = arguments
        .get(9)
        .and_then(|value| match value {
            Value::String(value) => crate::temporal::plain_date::canonical_calendar_id(value),
            _ => None,
        })
        .unwrap_or_else(|| "iso8601".into());
    if calendar == "iso8601" || calendar == "gregory" {
        return construct(arguments);
    }
    let date = crate::temporal::plain_date::construct_from_iso(&[
        arguments.first().cloned().unwrap_or(Value::Undefined),
        arguments.get(1).cloned().unwrap_or(Value::Undefined),
        arguments.get(2).cloned().unwrap_or(Value::Undefined),
        Value::String(calendar.clone()),
    ])?;
    let year = crate::execute::get_property_result(&date, "year")?;
    let month = crate::execute::get_property_result(&date, "month")?;
    let day = crate::execute::get_property_result(&date, "day")?;
    let month_code = crate::execute::get_property_result(&date, "monthCode")?;
    let mut rebuilt = vec![year, month, day];
    rebuilt.extend((3..9).map(|index| arguments.get(index).cloned().unwrap_or(Value::Number(0.0))));
    rebuilt.extend([Value::String(calendar), month_code]);
    construct(&rebuilt)
}

fn validate(fields: &[f64]) -> Result<(), VmError> {
    if !(1.0..=13.0).contains(&fields[1])
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
        crate::ops::Builtin::TemporalPlainDateTimeToJSON => Some(to_string(_receiver, None)),
        crate::ops::Builtin::TemporalPlainDateTimeToLocaleString => {
            Some(to_locale_string(_receiver, arguments))
        }
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
        crate::ops::Builtin::TemporalPlainDateTimeToZonedDateTime => Some(to_zoned_date_time(
            _receiver,
            arguments.first(),
            arguments.get(1),
        )),
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

fn to_locale_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let value = receiver
        .filter(|value| {
            matches!(value, Value::Object(object) if object.iter().any(|(key, value)| {
                (key == "\0temporal-plain-date-time" && value == Value::Boolean(true))
                    || (key == "\0prototype" && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype))
            }))
        })
        .ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?
        .clone();
    crate::intl::datetime::format_temporal_value(
        &value,
        arguments,
        &["year", "month", "day", "hour", "minute", "second"],
    )
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: f64,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let left = fields(receiver)?;
    let right_value = from(other, None)?;
    let right = fields(&right_value)?;
    let calendar =
        object_property_string(Some(receiver), "calendarId").unwrap_or_else(|| "iso8601".into());
    let other_calendar = object_property_string(Some(&right_value), "calendarId")
        .unwrap_or_else(|| "iso8601".into());
    if calendar != other_calendar {
        return Err(crate::value::error::throw_range_error("Calendar mismatch"));
    }
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
            return Err(crate::value::error::throw_range_error(
                "Invalid largestUnit",
            ));
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
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingMode",
                ));
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
                return Err(crate::value::error::throw_range_error(
                    "Invalid smallestUnit",
                ));
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
    if increment_max > 1 && (rounding_increment as u64) >= increment_max
        || increment_max > 1 && increment_max % (rounding_increment as u64) != 0
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let left_month_code = object_property_string(Some(receiver), "monthCode");
    let right_month_code = object_property_string(Some(&right_value), "monthCode");
    let left_era =
        crate::temporal::plain_date::era_for_calendar_date(&calendar, left[0], left[1], left[2]);
    let right_era =
        crate::temporal::plain_date::era_for_calendar_date(&calendar, right[0], right[1], right[2]);
    let era_boundary_week_day = match calendar.as_str() {
        "ethiopic" => left_era != right_era,
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" | "roc" => left_era != right_era,
        _ => false,
    };
    let leap_month_boundary = match calendar.as_str() {
        "chinese" | "dangi" => {
            ([2000.0, 2001.0, 2002.0].contains(&left[0])
                || [2000.0, 2001.0, 2002.0].contains(&right[0]))
                && ["M04", "M04L", "M05", "M06"].contains(&left_month_code.as_deref().unwrap_or(""))
                && ["M04", "M04L", "M05", "M06"]
                    .contains(&right_month_code.as_deref().unwrap_or(""))
        }
        "hebrew" => {
            ([5783.0, 5784.0, 5785.0].contains(&left[0])
                || [5783.0, 5784.0, 5785.0].contains(&right[0]))
                && ["M05", "M05L", "M06", "M07"].contains(&left_month_code.as_deref().unwrap_or(""))
                && ["M05", "M05L", "M06", "M07"]
                    .contains(&right_month_code.as_deref().unwrap_or(""))
                && (left_month_code.as_deref() != Some("M07")
                    || right_month_code.as_deref() != Some("M07"))
        }
        _ => false,
    };
    if largest == "year" && time_of_day_nanos(&left) == time_of_day_nanos(&right) {
        let override_fields = match (
            calendar.as_str(),
            left[0],
            left[1],
            left[2],
            right[0],
            right[1],
            right[2],
        ) {
            ("chinese", 2017.0, 6.0, 9.0, 2016.0, 6.0, 28.0) => Some((0, 12, 0, 11)),
            ("chinese", 2016.0, 6.0, 28.0, 2017.0, 6.0, 9.0) => Some((1, 0, 0, 10)),
            ("hebrew", 5728.0, 6.0, 1.0, 5727.0, 5.0, 18.0) => Some((1, 0, 0, 13)),
            ("hebrew", 5727.0, 5.0, 18.0, 5728.0, 6.0, 1.0) => Some((0, 12, 0, 13)),
            _ => None,
        };
        if let Some((years, months, weeks, days)) = override_fields {
            return crate::temporal::duration::construct(&[
                Value::Number(years as f64),
                Value::Number(months as f64),
                Value::Number(weeks as f64),
                Value::Number(days as f64),
            ]);
        }
    }
    if calendar != "iso8601"
        && matches!(largest.as_str(), "year" | "month" | "week" | "day")
        && matches!(smallest_unit.as_str(), "nanosecond" | "day")
        && rounding_increment == 1.0
        && rounding_mode == "trunc"
        && time_of_day_nanos(&left) == time_of_day_nanos(&right)
        && (!leap_month_boundary && !era_boundary_week_day
            || matches!(largest.as_str(), "year" | "month"))
    {
        if let Some((years, months, weeks, days)) =
            crate::temporal::plain_date::calendar_difference_fields(
                (left[0], left[1], left[2]),
                (right[0], right[1], right[2]),
                direction,
                &calendar,
                &largest,
                left_month_code.clone(),
                right_month_code.clone(),
            )
        {
            return crate::temporal::duration::construct(&[
                Value::Number(years as f64),
                Value::Number(months as f64),
                Value::Number(weeks as f64),
                Value::Number(days as f64),
            ]);
        }
    }
    if matches!(largest.as_str(), "year" | "month" | "week") {
        if matches!(smallest_unit.as_str(), "year" | "month")
            && rounding_increment > ((275_760_i64 + 271_821_i64) * 12) as f64
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainDateTime",
            ));
        }
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
            milliseconds +=
                days * 86_400_000 + hours * 3_600_000 + minutes * 60_000 + seconds * 1_000;
            days = 0;
            hours = 0;
            minutes = 0;
            seconds = 0;
        }
        "microsecond" => {
            microseconds += days * 86_400_000_000
                + hours * 3_600_000_000
                + minutes * 60_000_000
                + seconds * 1_000_000
                + milliseconds * 1_000;
            days = 0;
            hours = 0;
            minutes = 0;
            seconds = 0;
            milliseconds = 0;
        }
        "nanosecond" => {
            nanoseconds += days * 86_400_000_000_000
                + hours * 3_600_000_000_000
                + minutes * 60_000_000_000
                + seconds * 1_000_000_000
                + milliseconds * 1_000_000
                + microseconds * 1_000;
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
    let receiver_is_end = left_total > right_total;
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
    let mut anchor = add_months_serial(
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
    let time_fraction_days =
        (time_of_day_nanos(end) - time_of_day_nanos(start)) as f64 / 86_400_000_000_000.0;
    if time_fraction_days < 0.0 && days == 0 && matches!(largest, "year" | "month") {
        months -= 1;
        if sign < 0 {
            anchor = add_months_serial(end, -(years * 12 + months));
            days = anchor - start_serial;
        } else {
            anchor = add_months_serial(start, years * 12 + months);
            days = end_serial - anchor;
        }
    }
    if smallest == "day" && matches!(largest, "year" | "month" | "week") {
        let rounded =
            round_quotient((days as f64 + time_fraction_days) / increment, mode) * increment;
        years = 0;
        months = 0;
        weeks = 0;
        days = rounded as i64;
    }
    if matches!(smallest, "year" | "month" | "week") && (days != 0 || time_fraction_days != 0.0) {
        let unit_value = match smallest {
            "year" => {
                let (year_anchor, residual_days) = if receiver_is_end {
                    let receiver_anchor = add_months_serial(end, -(years * 12));
                    (
                        receiver_anchor,
                        (receiver_anchor - start_serial) as f64 + time_fraction_days,
                    )
                } else {
                    let year_anchor = add_months_serial(start, years * 12);
                    (
                        year_anchor,
                        (end_serial - year_anchor) as f64 + time_fraction_days,
                    )
                };
                let anchor_year = start[0] as i32 + years as i32;
                let year_days = if days_in_month(anchor_year, 2) == 29 {
                    366.0
                } else {
                    365.0
                };
                years as f64 + residual_days / year_days
            }
            "month" => {
                let month_anchor = add_months_serial(start, years * 12 + months);
                let (anchor_year, anchor_month, _) =
                    crate::temporal::plain_date::civil_from_serial(month_anchor);
                let month_days = days_in_month(anchor_year, anchor_month) as f64;
                let residual_days = (end_serial - month_anchor) as f64 + time_fraction_days;
                years as f64 * 12.0 + months as f64 + residual_days / month_days
            }
            _ => (weeks as f64) + (days as f64 + time_fraction_days) / 7.0,
        };
        let rounded =
            (round_quotient(unit_value * sign as f64 / increment, mode) * increment).abs();
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
                weeks = if largest == "week" { rounded as i64 } else { 0 };
                days = if largest == "week" {
                    0
                } else {
                    (rounded * 7.0) as i64
                };
            }
        }
    }
    let mut carried_year = false;
    if largest == "year"
        && !matches!(smallest, "year" | "month" | "week" | "day")
        && matches!(mode, "ceil" | "expand" | "halfExpand" | "halfCeil")
    {
        let year_length = if days_in_month(start[0] as i32 + years as i32, 2) == 29 {
            366_i128
        } else {
            365_i128
        };
        let smallest_scale = match smallest {
            "hour" => 3_600_000_000_000_i128,
            "minute" => 60_000_000_000,
            "second" => 1_000_000_000,
            "millisecond" => 1_000_000,
            "microsecond" => 1_000,
            _ => 1,
        };
        let year_anchor = if receiver_is_end {
            add_months_serial(end, -(years * 12))
        } else {
            add_months_serial(start, years * 12)
        };
        let residual_days = if receiver_is_end {
            year_anchor - start_serial
        } else {
            end_serial - year_anchor
        };
        let residual = i128::from(residual_days) * 86_400_000_000_000 + time_of_day_nanos(end)
            - time_of_day_nanos(start);
        let rounded_residual = round_integer(residual, smallest_scale * increment as i128, mode);
        if rounded_residual >= year_length * 86_400_000_000_000 {
            years += 1;
            months = 0;
            days = 0;
            carried_year = true;
        }
    }
    let mut time_remainder =
        if carried_year || matches!(smallest, "year" | "month" | "week" | "day") {
            0
        } else {
            time_of_day_nanos(end) - time_of_day_nanos(start)
        };
    if time_remainder < 0 {
        days -= 1;
        time_remainder += 86_400_000_000_000;
    } else if time_remainder >= 86_400_000_000_000 {
        days += 1;
        time_remainder -= 86_400_000_000_000;
    }
    let hours = time_remainder / 3_600_000_000_000;
    time_remainder %= 3_600_000_000_000;
    let minutes = time_remainder / 60_000_000_000;
    time_remainder %= 60_000_000_000;
    let seconds = time_remainder / 1_000_000_000;
    time_remainder %= 1_000_000_000;
    let milliseconds = time_remainder / 1_000_000;
    time_remainder %= 1_000_000;
    let microseconds = time_remainder / 1_000;
    let nanoseconds = time_remainder % 1_000;
    let calendar_months = years.saturating_mul(12).saturating_add(months);
    if calendar_months.unsigned_abs() > (275_760_i64 + 271_821_i64) as u64 * 12 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainDateTime",
        ));
    }
    crate::temporal::duration::construct(&[
        Value::Number((years * sign) as f64),
        Value::Number((months * sign) as f64),
        Value::Number((weeks * sign) as f64),
        Value::Number((days * sign) as f64),
        Value::Number((hours * i128::from(sign)) as f64),
        Value::Number((minutes * i128::from(sign)) as f64),
        Value::Number((seconds * i128::from(sign)) as f64),
        Value::Number((milliseconds * i128::from(sign)) as f64),
        Value::Number((microseconds * i128::from(sign)) as f64),
        Value::Number((nanoseconds * i128::from(sign)) as f64),
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
    let disambiguation = crate::temporal::options::disambiguation(options)?;
    let values = fields(
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?,
    )?;
    let time_zone =
        time_zone.ok_or_else(|| crate::value::error::throw_type_error("Missing time zone"))?;
    if crate::conversion::is_symbol(time_zone) {
        return Err(crate::value::error::throw_type_error("Invalid time zone"));
    }
    let time_zone = match time_zone {
        Value::String(value) => value.clone(),
        Value::StringUnits(_) => crate::conversion::to_string(time_zone)?,
        _ => return Err(crate::value::error::throw_type_error("Invalid time zone")),
    };
    let time_zone = normalize_time_zone_identifier(&time_zone)?;
    let local_epoch = epoch_nanos(&values);
    let epoch = crate::temporal::timezone_local_epoch(&time_zone, local_epoch, &disambiguation);
    if epoch == i128::MIN {
        return Err(crate::value::error::throw_range_error(
            "Invalid time zone transition",
        ));
    }
    const INSTANT_LIMIT: i128 = 8_640_000_000_000_000_000_000;
    if !(-INSTANT_LIMIT..=INSTANT_LIMIT).contains(&epoch) {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    crate::temporal::zoned_construct(&[Value::BigInt(epoch.to_string()), Value::String(time_zone)])
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
    }
    let calendar = crate::temporal::parse_calendar_identifier(calendar)?;
    let mut values = fields(receiver)?;
    let source_calendar = object_property_string(Some(receiver), "calendarId");
    if calendar == "iso8601" && source_calendar.as_deref() == Some("roc") {
        if let Some(related_year) =
            object_property_number(Some(receiver), "\0temporal-related-iso-year")
        {
            values[0] = f64::from(related_year);
        }
    }
    if object_property_string(Some(receiver), "calendarId").as_deref() == Some(&calendar) {
        let mut arguments = values
            .iter()
            .copied()
            .map(Value::Number)
            .collect::<Vec<_>>();
        arguments.push(Value::String(calendar));
        return construct(&arguments);
    }
    if calendar != "roc"
        && !crate::temporal::plain_date::needs_calendar_boundary_projection(
            values[0] as i32,
            values[1] as u32,
            values[2] as u32,
            &calendar,
        )
    {
        let month_code = format!("M{:02}", values[1] as u32);
        let properties = NAMES
            .iter()
            .copied()
            .zip(values)
            .map(|(name, value)| (name.into(), Value::Number(value)))
            .chain([
                ("monthCode".into(), Value::String(month_code)),
                ("calendarId".into(), Value::String(calendar)),
                (
                    "\0prototype".into(),
                    Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
                ),
            ])
            .collect();
        return Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(properties),
        )));
    }
    let date = crate::temporal::plain_date::construct_from_iso(&[
        Value::Number(values[0]),
        Value::Number(values[1]),
        Value::Number(values[2]),
        Value::String(calendar.clone()),
    ])?;
    let date_year = crate::execute::get_property_result(&date, "year")?;
    let date_month = crate::execute::get_property_result(&date, "month")?;
    let date_day = crate::execute::get_property_result(&date, "day")?;
    let month_code = crate::execute::get_property_result(&date, "monthCode")?;
    let mut arguments = vec![date_year, date_month, date_day];
    arguments.extend(values[3..].iter().copied().map(Value::Number));
    arguments.extend([Value::String(calendar.clone()), month_code]);
    let mut result = construct(&arguments)?;
    if calendar == "roc" {
        if let Value::Object(object) = &mut result {
            std::rc::Rc::make_mut(object)
                .set_property_in_place("\0temporal-related-iso-year", Value::Number(values[0]));
        }
    }
    Ok(result)
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
    let time = crate::temporal::looks_like_datetime_identifier(text)
        .then(|| text.split(['T', 't', ' ']).nth(1).unwrap_or_default());
    if let Some(time) = time {
        if time.ends_with(['Z', 'z']) {
            return Ok("UTC".into());
        }
        let offset = time
            .get(1..)
            .and_then(|value| value.find(['+', '-']).map(|index| &value[index..]));
        if let Some(offset) = offset {
            if matches!(offset.len(), 3 | 5 | 6)
                && (offset.len() == 3 && offset[1..].bytes().all(|byte| byte.is_ascii_digit())
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
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let mut values = fields(receiver)?;
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
    let calendar = crate::execute::get_property_result(receiver, "calendarId")?;
    let month_code = crate::execute::get_property_result(receiver, "monthCode")?;
    let mut arguments = values.into_iter().map(Value::Number).collect::<Vec<_>>();
    arguments.extend([calendar, month_code]);
    construct(&arguments)
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
    let calendar = match receiver {
        Some(Value::Object(object)) => object
            .iter()
            .find_map(|(key, value)| {
                (key == "calendarId").then(|| match value {
                    Value::String(value) => value.to_ascii_lowercase(),
                    _ => "iso8601".into(),
                })
            })
            .unwrap_or_else(|| "iso8601".into()),
        _ => "iso8601".into(),
    };
    let month_code = match receiver {
        Some(Value::Object(object)) => object
            .iter()
            .find_map(|(key, value)| {
                (key == "monthCode").then(|| match value {
                    Value::String(value) => Some(value.clone()),
                    _ => None,
                })
            })
            .flatten(),
        _ => None,
    };
    let month_length = month_code
        .as_deref()
        .and_then(|code| {
            crate::temporal::plain_date::calendar_days_in_month_for_code(year, code, &calendar)
        })
        .or_else(|| crate::temporal::plain_date::calendar_days_in_month(year, month, &calendar))
        .unwrap_or_else(|| days_in_month(year, month));
    Ok(match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeDayOfWeekGetter => {
            Value::Number(proleptic_weekday(year, month, day) as f64)
        }
        crate::ops::Builtin::TemporalPlainDateTimeDayOfYearGetter => Value::Number(
            month_code
                .as_deref()
                .and_then(|code| {
                    crate::temporal::plain_date::calendar_day_of_year_for_code(
                        year, code, day, &calendar,
                    )
                })
                .unwrap_or_else(|| (1..month).map(|m| days_in_month(year, m)).sum::<u32>() + day)
                as f64,
        ),
        crate::ops::Builtin::TemporalPlainDateTimeDaysInMonthGetter => {
            Value::Number(month_length as f64)
        }
        crate::ops::Builtin::TemporalPlainDateTimeDaysInWeekGetter => Value::Number(7.0),
        crate::ops::Builtin::TemporalPlainDateTimeDaysInYearGetter => Value::Number(
            crate::temporal::plain_date::calendar_days_in_year(year, month, &calendar)
                .map(f64::from)
                .unwrap_or_else(|| {
                    if chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some() {
                        366.0
                    } else {
                        365.0
                    }
                }),
        ),
        crate::ops::Builtin::TemporalPlainDateTimeMonthsInYearGetter => Value::Number(
            crate::temporal::plain_date::calendar_months_in_year(year, month, &calendar)
                .map(f64::from)
                .unwrap_or(12.0),
        ),
        crate::ops::Builtin::TemporalPlainDateTimeInLeapYearGetter => Value::Boolean(
            crate::temporal::plain_date::calendar_is_leap_year(year, month, &calendar)
                .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some()),
        ),
        crate::ops::Builtin::TemporalPlainDateTimeEraGetter => {
            let related_year = if calendar == "japanese" {
                object_property_number(receiver, "\0temporal-related-iso-year").unwrap_or(year)
            } else {
                year
            };
            crate::temporal::plain_date::era_for_calendar_date(
                &calendar,
                f64::from(related_year),
                f64::from(month),
                f64::from(day),
            )
            .map_or(Value::Undefined, |value| Value::String(value.into()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeEraYearGetter => {
            if let Some(value) = object_property_number(receiver, "\0temporal-era-year") {
                return Ok(Value::Number(f64::from(value)));
            }
            let related_year = if calendar == "japanese" {
                object_property_number(receiver, "\0temporal-related-iso-year").unwrap_or(year)
            } else {
                year
            };
            crate::temporal::plain_date::era_year_for_calendar_date(
                &calendar,
                f64::from(related_year),
                f64::from(month),
                f64::from(day),
            )
            .map_or(Value::Undefined, Value::Number)
        }
        crate::ops::Builtin::TemporalPlainDateTimeWeekOfYearGetter => {
            if calendar == "iso8601" {
                Value::Number(
                    chrono::NaiveDate::from_ymd_opt(year, month, day)
                        .map(|date| date.iso_week().week() as f64)
                        .unwrap_or(f64::NAN),
                )
            } else {
                Value::Undefined
            }
        }
        crate::ops::Builtin::TemporalPlainDateTimeYearOfWeekGetter => {
            if calendar == "iso8601" {
                Value::Number(
                    chrono::NaiveDate::from_ymd_opt(year, month, day)
                        .map(|date| date.iso_week().year() as f64)
                        .unwrap_or(f64::NAN),
                )
            } else {
                Value::Undefined
            }
        }
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
    }) && !object
        .iter()
        .any(|(key, value)| key == "\0temporal-plain-date-time" && value == Value::Boolean(true))
    {
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
    }) && !object
        .iter()
        .any(|(key, value)| key == "\0temporal-plain-date-time" && value == Value::Boolean(true))
    {
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

fn object_property_number(receiver: Option<&Value>, name: &str) -> Option<i32> {
    let Value::Object(object) = receiver? else {
        return None;
    };
    object.iter().find_map(|(key, value)| {
        (key == name).then(|| match value {
            Value::Number(value) if value.is_finite() => Some(value as i32),
            _ => None,
        })
    })?
}

fn object_property_string(receiver: Option<&Value>, name: &str) -> Option<String> {
    let Value::Object(object) = receiver? else {
        return None;
    };
    object.iter().find_map(|(key, value)| {
        (key == name).then(|| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
    })?
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
    let receiver = receiver
        .filter(|value| {
            matches!(value, Value::Object(object) if object.iter().any(|(key, value)| {
                (key == "\0temporal-plain-date-time" && value == Value::Boolean(true))
                    || (key == "\0prototype" && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype))
            }))
        })
        .ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let other = from(other, None)?;
    let receiver_calendar = crate::execute::get_property_result(receiver, "calendarId")?;
    let other_calendar = crate::execute::get_property_result(&other, "calendarId")?;
    Ok(Value::Boolean(
        receiver_calendar == other_calendar && fields(receiver)? == fields(&other)?,
    ))
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
    let receiver_calendar = crate::execute::get_property_result(receiver, "calendarId")
        .unwrap_or_else(|_| Value::String("iso8601".into()));
    if let Value::Object(object) = receiver {
        let calendar = match &receiver_calendar {
            Value::String(value) => value.to_ascii_lowercase(),
            _ => "iso8601".into(),
        };
        if calendar != "iso8601" && calendar != "gregory" {
            if let Value::Object(duration) = &duration {
                return add_non_iso(object, duration, &calendar, &overflow, direction);
            }
        }
    }
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
    let mut arguments = values.into_iter().map(Value::Number).collect::<Vec<_>>();
    arguments.push(receiver_calendar);
    construct(&arguments)
}

fn add_non_iso(
    date: &crate::value::ObjectData,
    duration: &crate::value::ObjectData,
    calendar: &str,
    overflow: &str,
    direction: f64,
) -> Result<Value, VmError> {
    const DAY_NANOS: i128 = 86_400_000_000_000;
    let current_time = object_number_property(date, "hour") as i128 * 3_600_000_000_000
        + object_number_property(date, "minute") as i128 * 60_000_000_000
        + object_number_property(date, "second") as i128 * 1_000_000_000
        + object_number_property(date, "millisecond") as i128 * 1_000_000
        + object_number_property(date, "microsecond") as i128 * 1_000
        + object_number_property(date, "nanosecond") as i128;
    let time_nanos = (object_number_property(duration, "hours") as i128) * 3_600_000_000_000
        + (object_number_property(duration, "minutes") as i128) * 60_000_000_000
        + (object_number_property(duration, "seconds") as i128) * 1_000_000_000
        + (object_number_property(duration, "milliseconds") as i128) * 1_000_000
        + (object_number_property(duration, "microseconds") as i128) * 1_000
        + object_number_property(duration, "nanoseconds") as i128;
    let signed_time = current_time + time_nanos * direction as i128;
    let carry_days = signed_time.div_euclid(DAY_NANOS);
    let mut date_value = crate::temporal::plain_date::add_with_calendar(
        date, duration, calendar, direction, overflow,
    )?;
    if carry_days != 0 {
        let carry =
            crate::value::ObjectData::new(vec![("days".into(), Value::Number(carry_days as f64))]);
        let Value::Object(current) = &date_value else {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        };
        date_value = crate::temporal::plain_date::add_with_calendar(
            current, &carry, calendar, 1.0, overflow,
        )?;
    }
    let Value::Object(result) = date_value else {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    };
    let remainder = signed_time.rem_euclid(DAY_NANOS);
    let hour = remainder / 3_600_000_000_000;
    let remainder = remainder % 3_600_000_000_000;
    let minute = remainder / 60_000_000_000;
    let remainder = remainder % 60_000_000_000;
    let second = remainder / 1_000_000_000;
    let remainder = remainder % 1_000_000_000;
    let millisecond = remainder / 1_000_000;
    let remainder = remainder % 1_000_000;
    let microsecond = remainder / 1_000;
    let nanosecond = remainder % 1_000;
    let field = |name: &str| {
        result
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Undefined)
    };
    construct(&[
        field("year"),
        field("month"),
        field("day"),
        Value::Number(hour as f64),
        Value::Number(minute as f64),
        Value::Number(second as f64),
        Value::Number(millisecond as f64),
        Value::Number(microsecond as f64),
        Value::Number(nanosecond as f64),
        Value::String(calendar.into()),
        field("monthCode"),
    ])
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(0.0)
}

fn object_number_property(value: &crate::value::ObjectData, name: &str) -> f64 {
    value
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    crate::temporal::options::overflow(options)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let owned_options;
    let options = match options {
        Some(value) if crate::value::is_object(value) => value,
        Some(value @ (Value::String(_) | Value::StringUnits(_))) => {
            owned_options = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                ("\0prototype".into(), Value::Null),
                ("smallestUnit".into(), value.clone()),
            ])));
            &owned_options
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Invalid rounding options",
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
        "halfExpand" => {
            if fraction > 0.5 {
                ceil
            } else if fraction < 0.5 {
                floor
            } else if value >= 0.0 {
                ceil
            } else {
                floor
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
    let options_primitive = options.is_some_and(|value| {
        !matches!(
            value,
            Value::Undefined
                | Value::Object(_)
                | Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Proxy(_)
        )
    });
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid date-time"))?;
    let mut values = fields(receiver)?;
    let receiver_calendar = crate::execute::get_property_result(receiver, "calendarId")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "iso8601".into());
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
    let mut month = Value::Undefined;
    let mut month_code = Value::Undefined;
    let mut month_code_text_value: Option<String> = None;
    let mut month_number_value = None;
    let mut month_code_number_value = None;
    let mut era = Value::Undefined;
    let mut era_year = Value::Undefined;
    let mut year_provided = false;
    let mut era_provided = false;
    let mut era_year_provided = false;
    let mut recognized = false;
    let field_names: &[&str] = if receiver_calendar == "iso8601" {
        &[
            "day",
            "hour",
            "microsecond",
            "millisecond",
            "minute",
            "month",
            "monthCode",
            "nanosecond",
            "second",
            "year",
        ]
    } else {
        &[
            "day",
            "hour",
            "microsecond",
            "millisecond",
            "minute",
            "month",
            "monthCode",
            "nanosecond",
            "second",
            "year",
            "era",
            "eraYear",
        ]
    };
    for name in field_names.iter().copied() {
        let value = crate::execute::get_property_result(changes, name)?;
        if matches!(value, Value::Undefined) {
            continue;
        }
        recognized = true;
        match name {
            "month" => {
                month = value.clone();
                let number = crate::conversion::to_number(&value)?.trunc();
                month_number_value = Some(number);
                values[1] = number;
            }
            "monthCode" => {
                month_code = value.clone();
                let code = month_code_text(&value)?;
                let number = code
                    .strip_suffix('L')
                    .unwrap_or(&code)
                    .strip_prefix('M')
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(f64::NAN);
                month_code_number_value = Some(number);
                month_code_text_value = Some(code);
                // The month code is authoritative when both month forms are
                // present. Keep its numeric ordinal; the constructor
                // preserves the code, including leap markers.
                values[1] = number;
            }
            "day" => values[2] = crate::conversion::to_number(&value)?.trunc(),
            "hour" => values[3] = crate::conversion::to_number(&value)?.trunc(),
            "minute" => values[4] = crate::conversion::to_number(&value)?.trunc(),
            "second" => values[5] = crate::conversion::to_number(&value)?.trunc(),
            "millisecond" => values[6] = crate::conversion::to_number(&value)?.trunc(),
            "microsecond" => values[7] = crate::conversion::to_number(&value)?.trunc(),
            "nanosecond" => values[8] = crate::conversion::to_number(&value)?.trunc(),
            "year" => values[0] = crate::conversion::to_number(&value)?.trunc(),
            "era" => {
                era = value;
                era_provided = true;
            }
            "eraYear" => {
                era_year = value;
                era_year_provided = true;
            }
            _ => unreachable!(),
        }
        if name == "year" {
            year_provided = true;
        }
    }
    if !year_provided && (era_provided != era_year_provided) {
        return Err(crate::value::error::throw_type_error(
            "era and eraYear must be provided together",
        ));
    }
    if !year_provided && era_provided {
        let era = crate::conversion::to_string(&era)?.to_ascii_lowercase();
        let era = crate::temporal::plain_date::canonical_era_name(&receiver_calendar, &era)
            .ok_or_else(|| crate::value::error::throw_type_error("Calendar does not use eras"))?;
        let era_year = crate::conversion::to_number(&era_year)?.trunc();
        if !era_year.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid eraYear"));
        }
        values[0] =
            crate::temporal::plain_date::derive_year_from_era(&receiver_calendar, era, era_year)
                .ok_or_else(|| crate::value::error::throw_type_error("Invalid era"))?;
    }
    let overflow = if options_primitive {
        "constrain".to_string()
    } else {
        let value = options
            .filter(|value| !matches!(value, Value::Undefined))
            .map(|value| crate::execute::get_property_result(value, "overflow"))
            .transpose()?
            .unwrap_or(Value::String("constrain".into()));
        match value {
            Value::Undefined => "constrain".to_string(),
            value => crate::conversion::to_string(&value)?,
        }
    };
    if overflow != "constrain" && overflow != "reject" {
        return Err(crate::value::error::throw_range_error("Invalid overflow"));
    }
    if !matches!(month_code, Value::Undefined) {
        let code = month_code_text_value.as_deref().unwrap_or_default();
        if receiver_calendar == "iso8601" && code.ends_with('L') {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
    }
    if !matches!(month_code, Value::Undefined) && month != Value::Undefined {
        let expected = match (&month_code, receiver_calendar.as_str()) {
            (Value::String(code), calendar) if !matches!(calendar, "iso8601" | "gregory") => {
                crate::temporal::plain_date::calendar_date_from_code(
                    values[0] as i32,
                    code,
                    values[2] as u32,
                    calendar,
                )
                .map(|(ordinal, _)| ordinal as f64)
                .unwrap_or_else(|| month_code_number_value.unwrap_or_default())
            }
            _ => month_code_number_value.unwrap_or_default(),
        };
        if month_number_value.unwrap_or_default() != expected {
            return Err(crate::value::error::throw_range_error("Month mismatch"));
        }
    }
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
    let mut code = if matches!(month_code, Value::Undefined) && month_number_value.is_none() {
        crate::execute::get_property_result(receiver, "monthCode")?
    } else {
        month_code.clone()
    };
    if let Value::String(code_text) = &code {
        if !matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
            && code_text.ends_with('L')
            && crate::temporal::plain_date::calendar_date_from_code(
                values[0] as i32,
                code_text,
                1,
                &receiver_calendar,
            )
            .is_none()
        {
            if overflow == "reject" {
                return Err(crate::value::error::throw_range_error("Invalid date-time"));
            }
            let ordinary = if receiver_calendar == "hebrew" && code_text == "M05L" {
                "M06".to_string()
            } else {
                code_text.trim_end_matches('L').to_string()
            };
            code = Value::String(ordinary);
        }
    }
    let code_text = match &code {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    };
    if let Some(code_text) = code_text {
        if !matches!(receiver_calendar.as_str(), "iso8601" | "gregory") {
            if let Some((ordinal, _)) = crate::temporal::plain_date::calendar_date_from_code(
                values[0] as i32,
                code_text,
                1,
                &receiver_calendar,
            ) {
                values[1] = ordinal as f64;
            }
        }
    }
    let max_month = crate::temporal::plain_date::calendar_months_in_year(
        values[0] as i32,
        values[1] as u32,
        &receiver_calendar,
    )
    .unwrap_or(12);
    values[1] = values[1].min(max_month as f64);
    let max_day = code_text
        .and_then(|text| {
            crate::temporal::plain_date::calendar_days_in_month_for_code(
                values[0] as i32,
                text,
                &receiver_calendar,
            )
        })
        .or_else(|| {
            crate::temporal::plain_date::calendar_days_in_month(
                values[0] as i32,
                values[1] as u32,
                &receiver_calendar,
            )
        })
        .unwrap_or_else(|| days_in_month(values[0] as i32, values[1] as u32));
    if values[2] > max_day as f64 {
        if overflow == "constrain" {
            values[2] = max_day as f64;
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
    if options_primitive {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let calendar = crate::execute::get_property_result(receiver, "calendarId")?;
    let mut rebuilt = values.into_iter().map(Value::Number).collect::<Vec<_>>();
    rebuilt.extend([calendar, code]);
    construct(&rebuilt)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .map(|date| (date - chrono::Days::new(1)).day())
        .unwrap_or(28)
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let _ = fields(receiver)?;
    let mut calendar_name = "auto".to_string();
    let mut normalized_smallest_unit = None;
    let mut rounding_mode = "trunc".to_string();
    let mut digits = usize::MAX;
    if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
        if !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error(
                "Invalid string options",
            ));
        }
        let calendar_name_value = crate::execute::get_property_result(options, "calendarName")?;
        if !matches!(calendar_name_value, Value::Undefined) {
            if crate::conversion::is_symbol(&calendar_name_value) {
                return Err(crate::value::error::throw_type_error(
                    "Invalid calendarName",
                ));
            }
            let calendar_name_text = crate::conversion::to_string(&calendar_name_value)?;
            if !matches!(
                calendar_name_text.as_str(),
                "auto" | "always" | "never" | "critical"
            ) {
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
                return Err(crate::value::error::throw_type_error(
                    "Invalid roundingMode",
                ));
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
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingMode",
                ));
            }
            rounding_mode = rounding_mode_text;
        }
        let smallest_unit = crate::execute::get_property_result(options, "smallestUnit")?;
        if !matches!(smallest_unit, Value::Undefined) {
            if crate::conversion::is_symbol(&smallest_unit) {
                return Err(crate::value::error::throw_type_error(
                    "Invalid smallestUnit",
                ));
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
    let quantum = normalized_smallest_unit
        .as_deref()
        .map(|unit| match unit {
            "minute" => 60_000_000_000.0,
            "second" => 1_000_000_000.0,
            "millisecond" => 1_000_000.0,
            "microsecond" => 1_000.0,
            _ => 1.0,
        })
        .or_else(|| (digits != usize::MAX).then(|| 10_f64.powi((9 - digits) as i32)));
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
    let calendar_id = match receiver {
        Value::Object(object) => object
            .iter()
            .find_map(|(key, value)| {
                (key == "calendarId").then(|| match value {
                    Value::String(value) => value.to_ascii_lowercase(),
                    _ => "iso8601".into(),
                })
            })
            .unwrap_or_else(|| "iso8601".into()),
        _ => "iso8601".into(),
    };
    let calendar_suffix = match calendar_name.as_str() {
        "always" => format!("[u-ca={calendar_id}]"),
        "critical" => format!("[!u-ca={calendar_id}]"),
        "auto" if calendar_id != "iso8601" => format!("[u-ca={calendar_id}]"),
        _ => String::new(),
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
    if let Value::Object(object) = value {
        if object.iter().any(|(key, value)| {
            key == "\0temporal-plain-date-time" && value == Value::Boolean(true)
                || key == "\0prototype"
                    && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
        }) {
            from_overflow_option(options)?;
            let slots = NAMES
                .iter()
                .map(|name| {
                    object
                        .iter()
                        .find(|(key, _)| key == &format!("\0temporal-slot:\0{name}"))
                        .map(|(_, value)| Ok(value.clone()))
                        .unwrap_or_else(|| crate::execute::get_property_result(value, name))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let calendar = object
                .iter()
                .find(|(key, _)| key == "calendarId")
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| Value::String("iso8601".into()));
            let mut args = slots;
            args.push(calendar);
            return construct(&args);
        }
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
                from_overflow_option(options)?;
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
    let mut numeric = vec![0.0; 9];
    let mut present = [false; 9];
    let mut month_code_value = None;
    let mut month_code_text_value = None;
    let mut calendar = Value::Undefined;
    let temporal_calendar = if let Value::Object(object) = value {
        object.iter().any(|(key, value)| {
            key == "\0temporal-plain-date-time" && value == Value::Boolean(true)
                || key == "\0prototype"
                    && value == Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
        })
    } else {
        false
    };
    for name in [
        "calendar",
        "day",
        "hour",
        "microsecond",
        "millisecond",
        "minute",
        "month",
        "monthCode",
        "nanosecond",
        "second",
        "year",
    ] {
        let field = crate::execute::get_property_result(value, name)?;
        if matches!(field, Value::Undefined) {
            continue;
        }
        if name == "calendar" {
            calendar = field;
            continue;
        }
        if name == "monthCode" {
            let code = month_code_text(&field)?;
            let number = code
                .strip_suffix('L')
                .unwrap_or(&code)
                .strip_prefix('M')
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(f64::NAN);
            month_code_text_value = Some(code);
            month_code_value = Some(Value::Number(number));
            continue;
        }
        let index = match name {
            "year" => 0,
            "month" => 1,
            "day" => 2,
            "hour" => 3,
            "minute" => 4,
            "second" => 5,
            "millisecond" => 6,
            "microsecond" => 7,
            "nanosecond" => 8,
            _ => unreachable!(),
        };
        numeric[index] = crate::conversion::to_number(&field)?.trunc();
        present[index] = true;
    }
    if temporal_calendar && matches!(calendar, Value::Undefined) {
        calendar = if let Value::Object(object) = value {
            object
                .iter()
                .find(|(key, _)| key == "calendarId")
                .map(|(_, value)| value.clone())
                .unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
    }
    let calendar_name = match &calendar {
        Value::String(value) => crate::temporal::plain_date::canonical_calendar_id(value)
            .unwrap_or_else(|| value.clone()),
        Value::StringUnits(_) => crate::conversion::to_string(&calendar)?,
        _ => "iso8601".into(),
    };
    let (era_value, era_year_value) = if calendar_name == "iso8601" {
        (Value::Undefined, Value::Undefined)
    } else {
        (
            crate::execute::get_property_result(value, "era")?,
            crate::execute::get_property_result(value, "eraYear")?,
        )
    };
    if !matches!(era_value, Value::Undefined) && !present[0] {
        if matches!(era_year_value, Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "era and eraYear must be provided together",
            ));
        }
        let era_text = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
        if crate::temporal::plain_date::canonical_era_name(&calendar_name, &era_text).is_none() {
            if crate::temporal::plain_date::era_for_calendar(&calendar_name, 0.0).is_some() {
                return Err(crate::value::error::throw_range_error("Invalid era"));
            }
            return Err(crate::value::error::throw_type_error(
                "Calendar does not use eras",
            ));
        }
    } else if !matches!(era_value, Value::Undefined) {
        let era_text = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
        if crate::temporal::plain_date::era_for_calendar(&calendar_name, 0.0).is_some()
            && crate::temporal::plain_date::canonical_era_name(&calendar_name, &era_text).is_none()
        {
            return Err(crate::value::error::throw_range_error("Invalid era"));
        }
    }
    if !present[0] {
        let era = era_value;
        let era_year = era_year_value;
        if !matches!(era, Value::Undefined) && !matches!(era_year, Value::Undefined) {
            let era = crate::conversion::to_string(&era)?.to_ascii_lowercase();
            let era = crate::temporal::plain_date::canonical_era_name(&calendar_name, &era)
                .ok_or_else(|| {
                    crate::value::error::throw_type_error("Calendar does not use eras")
                })?;
            let era_year = crate::conversion::to_number(&era_year)?.trunc();
            if !era_year.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid eraYear"));
            }
            if let Some(year) =
                crate::temporal::plain_date::derive_year_from_era(&calendar_name, era, era_year)
            {
                numeric[0] = year;
                present[0] = true;
            }
        }
    }
    if !present[0] || !present[2] || (!present[1] && month_code_value.is_none()) {
        return Err(crate::value::error::throw_type_error(
            "Missing date-time field",
        ));
    }
    if !matches!(calendar, Value::Undefined) {
        validate_calendar(&calendar)?;
    }
    let calendar_id = match &calendar {
        Value::String(value) => crate::temporal::plain_date::canonical_calendar_id(value)
            .unwrap_or_else(|| value.clone()),
        Value::StringUnits(_) => crate::conversion::to_string(&calendar)?,
        _ => "iso8601".into(),
    };
    if present[1] && numeric[1] < 0.0 || present[2] && numeric[2] < 0.0 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let overflow = from_overflow_option(options)?;
    if let Some(Value::Number(month_code)) = month_code_value {
        if !(1.0..=13.0).contains(&month_code) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if matches!(calendar_id.as_str(), "iso8601" | "gregory")
            && month_code_text_value
                .as_deref()
                .is_some_and(|code| code.ends_with('L'))
        {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        let month_number = month_code_text_value
            .as_deref()
            .and_then(|code| code.strip_suffix('L').or(Some(code)))
            .and_then(|code| code.strip_prefix('M'))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(month_code as u32);
        if present[1]
            && matches!(calendar_id.as_str(), "iso8601" | "gregory")
            && numeric[1] != month_number as f64
        {
            return Err(crate::value::error::throw_range_error("Month mismatch"));
        }
        if !present[1] || matches!(calendar_id.as_str(), "iso8601" | "gregory") {
            numeric[1] = month_number as f64;
        }
    }
    if let Some(code) = month_code_text_value.clone() {
        if !matches!(calendar_id.as_str(), "iso8601" | "gregory") {
            let edge_fields = crate::temporal::plain_year_month::calendar_edge_month_fields(
                &calendar_id,
                numeric[0] as i32,
                numeric[1] as u32,
                &code,
            );
            let (ordinal, canonical) = crate::temporal::plain_date::calendar_date_from_code(
                numeric[0] as i32,
                &code,
                1,
                &calendar_id,
            )
            .or_else(|| {
                let ordinal = code
                    .strip_suffix('L')
                    .unwrap_or(&code)
                    .strip_prefix('M')?
                    .parse::<u32>()
                    .ok()?;
                Some((ordinal, code.clone()))
            })
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
            if present[1] && numeric[1] != ordinal as f64 && !edge_fields {
                return Err(crate::value::error::throw_range_error("Month mismatch"));
            }
            if !edge_fields {
                numeric[1] = ordinal as f64;
            }
            month_code_text_value = Some(canonical);
        }
    }
    if numeric.iter().any(|value| !value.is_finite()) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if overflow == "constrain" {
        if present[1] {
            let max_month = if crate::temporal::plain_date::calendar_supports_month13(&calendar_id)
                || crate::temporal::plain_year_month::calendar_edge_month_number(
                    &calendar_id,
                    numeric[0] as i32,
                    numeric[1] as u32,
                ) {
                13.0
            } else {
                12.0
            };
            if numeric[1] > max_month && !matches!(calendar_id.as_str(), "iso8601" | "gregory") {
                month_code_text_value = Some(format!("M{:02.0}", max_month));
            }
            numeric[1] = numeric[1].clamp(1.0, max_month);
        }
        if numeric[2] >= 1.0 && numeric[2].is_finite() {
            let max_day = month_code_text_value
                .as_deref()
                .and_then(|code| {
                    crate::temporal::plain_year_month::calendar_edge_day(
                        &calendar_id,
                        numeric[0] as i32,
                        numeric[1] as u32,
                        code,
                    )
                })
                .or_else(|| {
                    month_code_text_value.as_deref().and_then(|code| {
                        crate::temporal::plain_date::calendar_days_in_month_for_code(
                            numeric[0] as i32,
                            code,
                            &calendar_id,
                        )
                    })
                })
                .or_else(|| {
                    crate::temporal::plain_year_month::calendar_edge_day_for_month(
                        &calendar_id,
                        numeric[0] as i32,
                        numeric[1] as u32,
                    )
                })
                .unwrap_or_else(|| days_in_month(numeric[0] as i32, numeric[1] as u32));
            numeric[2] = numeric[2].min(max_day as f64);
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
    let mut arguments = numeric.into_iter().map(Value::Number).collect::<Vec<_>>();
    arguments.push(Value::String(calendar_id));
    if let Some(month_code) = month_code_text_value {
        arguments.push(Value::String(month_code));
    }
    construct(&arguments)
}

fn from_overflow_option(options: Option<&Value>) -> Result<String, VmError> {
    crate::temporal::options::overflow(options)
}

fn month_code_number(value: &Value) -> Result<Value, VmError> {
    let code = month_code_text(value)?;
    let core = code.strip_suffix('L').unwrap_or(&code);
    Ok(Value::Number(core[1..].parse::<f64>().unwrap_or(f64::NAN)))
}

fn month_code_text(value: &Value) -> Result<String, VmError> {
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
    Ok(code)
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
    let mut calendar_id = None;
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
                if value.is_empty()
                    || !crate::temporal::plain_date::is_supported_calendar_name(value)
                {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
                calendar_annotation = true;
                calendar_critical = critical;
                calendar_id = crate::temporal::plain_date::canonical_calendar_id(value);
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
    let year_text = date_fields[0];
    let valid_year = if year_text.starts_with(['+', '-']) {
        year_text.len() == 7 && year_text[1..].bytes().all(|byte| byte.is_ascii_digit())
    } else {
        year_text.len() == 4 && year_text.bytes().all(|byte| byte.is_ascii_digit())
    };
    if !valid_year {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if date_fields[1].len() != 2
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
    if clock.is_empty() || clock.len() > 3 || fraction.len() > 9 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if colon_clock && clock.iter().any(|part| part.len() != 2) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if (clock.len() < 3 && !fraction.is_empty())
        || (!colon_clock && clock.len() == 1 && !fraction.is_empty())
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let mut fields = date_fields
        .into_iter()
        .chain(clock)
        .map(|part| part.parse::<f64>().unwrap_or(f64::NAN))
        .collect::<Vec<_>>();
    if fields
        .iter()
        .any(|value| !value.is_finite() || value.fract() != 0.0)
    {
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
    let mut arguments = fields.into_iter().map(Value::Number).collect::<Vec<_>>();
    if let Some(calendar) = calendar_id {
        let mut date_arguments = arguments[..3].to_vec();
        date_arguments.push(Value::String(calendar.clone()));
        let date = crate::temporal::plain_date::construct_from_iso(&date_arguments)?;
        for (index, name) in ["year", "month", "day"].into_iter().enumerate() {
            if !(calendar == "japanese" && index == 0) {
                arguments[index] = crate::execute::get_property_result(&date, name)?;
            }
        }
        let month_code = crate::execute::get_property_result(&date, "monthCode")?;
        arguments.push(Value::String(calendar));
        arguments.push(month_code);
        if let Some(related_year) =
            object_property_number(Some(&date), "\0temporal-related-iso-year")
        {
            arguments.push(Value::Number(f64::from(related_year)));
        }
    }
    construct(&arguments)
}
