use chrono::{Offset, TimeZone};

pub(crate) fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if value.is_some_and(crate::conversion::is_symbol) {
        return Err(crate::value::error::throw_type_error(
            "Duration string must not be a Symbol",
        ));
    }
    if let Some(Value::String(text)) = value {
        return from_string(text);
    }
    let Some(value) = value.filter(|value| crate::value::is_object(value)) else {
        return Err(crate::value::error::throw_type_error(
            "Duration.from requires a duration-like object",
        ));
    };
    let names = [
        "days",
        "hours",
        "microseconds",
        "milliseconds",
        "minutes",
        "months",
        "nanoseconds",
        "seconds",
        "weeks",
        "years",
    ];
    let fields = names
        .iter()
        .map(|name| {
            crate::execute::get_property_result(value, name).and_then(|field| match field {
                Value::Undefined => Ok(Value::Undefined),
                field => crate::conversion::to_number(&field).map(Value::Number),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if fields.iter().all(|field| matches!(field, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error(
            "Duration requires at least one field",
        ));
    }
    let order = [9, 5, 8, 0, 1, 4, 7, 3, 2, 6];
    construct(&order.map(|index| fields[index].clone()))
}

fn from_string(text: &str) -> Result<Value, VmError> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let body = body
        .strip_prefix('P')
        .or_else(|| body.strip_prefix('p'))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
    let mut values = [0.0; 10];
    let (date, time) = body.split_once(['T', 't']).unwrap_or((body, ""));
    let date_seen = parse_duration_section(date, false, &mut values)?;
    let time_seen = parse_duration_section(time, true, &mut values)?;
    if !date_seen && !time_seen {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    if negative {
        values.iter_mut().for_each(|value| *value = -*value);
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

pub(crate) fn parse_string(text: &str) -> Result<Value, VmError> {
    from_string(text)
}

fn parse_duration_section(
    section: &str,
    time: bool,
    values: &mut [f64; 10],
) -> Result<bool, VmError> {
    let mut rest = section;
    let mut seen = false;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find_map(|(index, character)| character.is_ascii_alphabetic().then_some(index))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
        let (number, tail) = rest.split_at(end);
        let unit = tail
            .chars()
            .next()
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
        validate_duration_number(number, tail, time, unit)?;
        let (whole, fraction) = parse_duration_number(number)?;
        add_duration_component(values, time, unit, whole, fraction)?;
        seen = true;
        rest = &tail[unit.len_utf8()..];
    }
    Ok(seen)
}

fn parse_duration_number(number: &str) -> Result<(f64, f64), VmError> {
    let (whole, fraction) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let whole = whole
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid duration string"))?;
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let scale = 10_f64.powi(fraction.len() as i32);
        fraction
            .parse::<f64>()
            .map(|value| value / scale)
            .map_err(|_| crate::value::error::throw_range_error("Invalid duration string"))?
    };
    Ok((whole, fraction))
}

fn validate_duration_number(
    number: &str,
    tail: &str,
    time: bool,
    unit: char,
) -> Result<(), VmError> {
    let separators = number.matches(['.', ',']).count();
    let fractional = separators > 0;
    let digits = number.split(['.', ',']).collect::<Vec<_>>();
    if number.is_empty()
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ','))
        || separators > 1
        || digits.first().is_some_and(|part| part.is_empty())
        || digits.get(1).is_some_and(|part| part.is_empty())
        || fractional && (!time || !tail[unit.len_utf8()..].is_empty())
        || unit.eq_ignore_ascii_case(&'S') && digits.get(1).is_some_and(|part| part.len() > 9)
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    Ok(())
}

fn add_duration_component(
    values: &mut [f64; 10],
    time: bool,
    unit: char,
    whole: f64,
    fraction: f64,
) -> Result<(), VmError> {
    let index = match (time, unit.to_ascii_uppercase()) {
        (false, 'Y') => 0,
        (false, 'M') => 1,
        (false, 'W') => 2,
        (false, 'D') => 3,
        (true, 'H') => 4,
        (true, 'M') => 5,
        (true, 'S') => 6,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid duration string",
            ))
        }
    };
    values[index] += whole;
    if time && fraction != 0.0 {
        add_fractional_time(values, index, fraction);
    }
    Ok(())
}

fn add_fractional_time(values: &mut [f64; 10], index: usize, fraction: f64) {
    let multiplier = [3_600.0, 60.0, 1.0][index - 4] * 1_000_000_000.0;
    let mut nanos = (fraction * multiplier).round() as i64;
    if index == 4 {
        values[5] += (nanos / 60_000_000_000) as f64;
        nanos %= 60_000_000_000;
    }
    values[6] += (nanos / 1_000_000_000) as f64;
    nanos %= 1_000_000_000;
    values[7] += (nanos / 1_000_000) as f64;
    values[8] += (nanos / 1_000 % 1_000) as f64;
    values[9] += (nanos % 1_000) as f64;
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    if same_fields(arguments.first(), arguments.get(1)) {
        if let Some(options) = arguments.get(2) {
            validate_compare_options(Some(options))?;
            if crate::value::is_object(options) {
                let relative_to = relative_to_option(options)?;
                if !matches!(relative_to, Value::Undefined) {
                    relative_date(&relative_to)?;
                }
            }
        }
        return Ok(Value::Number(0.0));
    }
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    validate_compare_options(arguments.get(2))?;
    for value in [&left, &right] {
        let calendar_days = number_property(value, "weeks") * 7.0 + number_property(value, "days");
        if !calendar_days.is_finite() || calendar_days.abs() > 104_249_991_374.0 {
            return Err(crate::value::error::throw_range_error(
                "Duration exceeds relative date range",
            ));
        }
    }
    let relative_to = match arguments.get(2) {
        Some(value) if !matches!(value, Value::Undefined) => Some(relative_to_option(value)?),
        _ => None,
    };
    if (date_units(&left) || date_units(&right))
        && relative_to
            .as_ref()
            .is_none_or(|value| matches!(value, Value::Undefined))
    {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for date units",
        ));
    }
    let date = relative_to
        .as_ref()
        .filter(|value| !matches!(value, Value::Undefined))
        .map(|value| relative_date(value))
        .transpose()?;
    let zoned_relative = relative_to.as_ref().and_then(zoned_relative_value);
    let day_units = number_property(&left, "days") != 0.0 || number_property(&right, "days") != 0.0;
    let difference =
        if date_units(&left) || date_units(&right) || day_units && zoned_relative.is_some() {
            if let Some(relative) = zoned_relative.as_ref() {
                duration_difference_zoned(&left, &right, relative)?
            } else {
                duration_difference(&left, &right, date)?
            }
        } else {
            exact_time_difference(&left, &right)
        };
    if difference == 0 {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(if difference < 0 { -1.0 } else { 1.0 }))
}

fn zoned_relative_value(value: &Value) -> Option<Value> {
    if is_zoned(value) {
        return Some(value.clone());
    }
    let candidate = match value {
        Value::String(text) if text.contains('[') => true,
        Value::Object(_) => crate::execute::get_property_result(value, "timeZone")
            .ok()
            .is_some_and(|value| !matches!(value, Value::Undefined)),
        _ => false,
    };
    if !candidate {
        return None;
    }
    crate::temporal::execute(
        crate::ops::Builtin::TemporalZonedDateTimeFrom,
        None,
        std::slice::from_ref(value),
    )?
    .ok()
}

fn is_zoned(value: &Value) -> bool {
    let resolved = crate::locals::resolved_replacement(value.clone());
    matches!(
        resolved,
        Value::Object(ref object)
            if object.iter().any(|(key, value)| {
                key == "\0prototype"
                    && matches!(
                        value,
                        Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
                    )
            })
    )
}

fn epoch_of(value: &Value) -> Result<i128, VmError> {
    match crate::execute::get_property_result(value, "epochNanoseconds")? {
        Value::BigInt(value) => value
            .parse::<i128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid epochNanoseconds")),
        _ => Err(crate::value::error::throw_type_error(
            "Invalid ZonedDateTime",
        )),
    }
}

fn zoned_target(duration: &Value, relative: &Value) -> Result<Value, VmError> {
    let result = crate::temporal::execute(
        crate::ops::Builtin::TemporalZonedDateTimeAdd,
        Some(relative),
        std::slice::from_ref(duration),
    )
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))??;
    Ok(result)
}

fn duration_epoch_delta(duration: &Value, relative: &Value) -> Result<i128, VmError> {
    let result = zoned_target(duration, relative)?;
    Ok(epoch_of(&result)? - epoch_of(relative)?)
}

fn duration_difference_zoned(
    left: &Value,
    right: &Value,
    relative: &Value,
) -> Result<i128, VmError> {
    let left_delta = duration_epoch_delta(left, relative)?;
    let right_delta = duration_epoch_delta(right, relative)?;
    Ok(left_delta - right_delta)
}

fn zoned_day_length_hours(relative: &Value) -> Option<f64> {
    let timezone = crate::conversion::to_string(
        &crate::execute::get_property_result(relative, "timeZoneId").ok()?,
    )
    .ok()?;
    let epoch = match crate::execute::get_property_result(relative, "epochNanoseconds").ok()? {
        Value::BigInt(value) => value.parse::<i128>().ok()?,
        _ => return None,
    };
    let start = crate::temporal::timezone_start_of_day_epoch(&timezone, epoch)?;
    let mut probe = start + 86_400_000_000_000;
    for _ in 0..4 {
        if let Some(next) = crate::temporal::timezone_start_of_day_epoch(&timezone, probe) {
            if next > start {
                return Some((next - start) as f64 / 3_600_000_000_000.0);
            }
        }
        probe += 86_400_000_000_000;
    }
    None
}

fn zoned_total_days(duration: &Value, relative: &Value) -> Result<f64, VmError> {
    let actual = duration_epoch_delta(duration, relative)?;
    if actual == 0 {
        return Ok(0.0);
    }
    let target = zoned_target(duration, relative)?;
    let start_date = (
        crate::conversion::to_number(&crate::execute::get_property_result(relative, "year")?)?
            as i32,
        crate::conversion::to_number(&crate::execute::get_property_result(relative, "month")?)?
            as u32,
        crate::conversion::to_number(&crate::execute::get_property_result(relative, "day")?)?
            as u32,
    );
    let end_date = (
        crate::conversion::to_number(&crate::execute::get_property_result(&target, "year")?)?
            as i32,
        crate::conversion::to_number(&crate::execute::get_property_result(&target, "month")?)?
            as u32,
        crate::conversion::to_number(&crate::execute::get_property_result(&target, "day")?)? as u32,
    );
    let mut day_delta = crate::temporal::plain_date::date_serial(
        end_date.0 as f64,
        end_date.1 as f64,
        end_date.2 as f64,
    ) - crate::temporal::plain_date::date_serial(
        start_date.0 as f64,
        start_date.1 as f64,
        start_date.2 as f64,
    );
    let day_duration = |days: f64| {
        construct(&[
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(days),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
        ])
    };
    // A skipped or repeated local midnight means the ISO date difference can
    // be off by one from the number of whole Temporal days. Adjust against
    // the actual epoch delta before computing the fractional day.
    for _ in 0..4 {
        let candidate = day_duration(day_delta as f64)?;
        let candidate_delta = duration_epoch_delta(&candidate, relative)?;
        if (actual >= 0 && candidate_delta > actual) || (actual < 0 && candidate_delta < actual) {
            day_delta += if actual < 0 { 1 } else { -1 };
        } else {
            break;
        }
    }
    let anchor = day_duration(day_delta as f64)?;
    let anchor_delta = duration_epoch_delta(&anchor, relative)?;
    let direction = if actual < 0 { -1.0 } else { 1.0 };
    let next = day_duration(day_delta as f64 + direction)?;
    let day_length = (duration_epoch_delta(&next, relative)? - anchor_delta) as f64;
    Ok(day_delta as f64 + (actual - anchor_delta) as f64 / day_length.abs())
}

fn zoned_total_calendar(duration: &Value, relative: &Value, unit: usize) -> Result<f64, VmError> {
    // Calendar totals use the explicit calendar portion as the whole-unit
    // anchor. The residual is measured in local calendar dates, so a DST
    // disambiguation in the time portion cannot perturb an exact half-month.
    let target = zoned_target(duration, relative)?;
    let field = |value: &Value, name: &str| {
        crate::conversion::to_number(&crate::execute::get_property_result(value, name)?)
    };
    let start = (
        field(relative, "year")? as f64,
        field(relative, "month")? as f64,
        field(relative, "day")? as f64,
    );
    let end = (
        field(&target, "year")? as f64,
        field(&target, "month")? as f64,
        field(&target, "day")? as f64,
    );
    let mut whole = if unit == 0 {
        field(duration, "years")?
    } else {
        field(duration, "years")? * 12.0 + field(duration, "months")?
    };
    if whole == 0.0 && (unit == 0 || unit == 1) {
        let mut date_units = if unit == 0 {
            end.0 - start.0
        } else {
            (end.0 - start.0) * 12.0 + end.1 - start.1
        };
        if (end.2, end.1) < (start.2, start.1) {
            date_units -= 1.0;
        }
        whole = date_units;
    }
    let anchor_duration = construct(&[
        Value::Number(if unit == 0 {
            whole
        } else if field(duration, "years")? == 0.0 && field(duration, "months")? == 0.0 {
            (whole / 12.0).trunc()
        } else {
            field(duration, "years")?
        }),
        Value::Number(if unit == 0 {
            0.0
        } else if field(duration, "years")? == 0.0 && field(duration, "months")? == 0.0 {
            whole % 12.0
        } else {
            field(duration, "months")?
        }),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
    ])?;
    let anchor = zoned_target(&anchor_duration, relative)?;
    let anchor_date = (
        field(&anchor, "year")? as f64,
        field(&anchor, "month")? as f64,
        field(&anchor, "day")? as f64,
    );
    let date_residual_days = (crate::temporal::plain_date::date_serial(end.0, end.1, end.2)
        - crate::temporal::plain_date::date_serial(anchor_date.0, anchor_date.1, anchor_date.2))
        as f64;
    let epoch_residual_days = (duration_epoch_delta(duration, relative)?
        - duration_epoch_delta(&anchor_duration, relative)?) as f64
        / 86_400_000_000_000.0;
    let iana_timezone = crate::execute::get_property_result(relative, "timeZoneId")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value.contains('/')),
            _ => None,
        })
        .unwrap_or(false);
    let mut residual_days = if iana_timezone
        || zoned_day_length_hours(relative)
            .is_some_and(|length| (length - 24.0).abs() > 1e-9)
    {
        date_residual_days
    } else {
        epoch_residual_days
    };
    if unit == 1 {
        let month_delta = (end.0 - anchor_date.0) * 12.0 + end.1 - anchor_date.1;
        let extra_months = if date_residual_days >= 0.0 {
            if end.2 >= anchor_date.2 { month_delta } else { month_delta - 1.0 }
        } else if end.2 <= anchor_date.2 {
            month_delta
        } else {
            month_delta + 1.0
        };
        if extra_months != 0.0 {
            let month = (anchor_date.1 as i32 - 1 + extra_months as i32).rem_euclid(12) + 1;
            let year = anchor_date.0 + (anchor_date.1 as i32 - 1 + extra_months as i32).div_euclid(12) as f64;
            let day = anchor_date.2.min(
                crate::temporal::plain_date::days_in_month_for_record(year as i32, month as u32)
                    as f64,
            );
            let shifted = crate::temporal::plain_date::date_serial(year, month as f64, day as f64);
            let extra_days = (shifted
                - crate::temporal::plain_date::date_serial(
                    anchor_date.0,
                    anchor_date.1,
                    anchor_date.2,
                )) as f64;
            whole += extra_months;
            residual_days -= date_residual_days - (date_residual_days - extra_days);
        }
    }
    let span =
        crate::temporal::plain_date::days_in_month_for_record(end.0 as i32, end.1 as u32) as f64;
    if unit == 0 {
        let year = end.0 as i32;
        let year_span = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366.0
        } else {
            365.0
        };
        Ok(whole + residual_days / year_span)
    } else {
        Ok(whole + residual_days / span)
    }
}

fn relative_to_option(value: &Value) -> Result<Value, VmError> {
    let result = crate::execute::get_property_result(value, "relativeTo")?;
    if !matches!(result, Value::Undefined) {
        return Ok(result);
    }
    let object = match value {
        Value::Object(object) => Some(object.clone()),
        Value::ObjectAlias(alias) => alias.target(),
        _ => None,
    };
    if let Some(object) = object {
        if let Some((_, value)) = object.iter().find(|(key, _)| key == "relativeTo") {
            return Ok(value.clone());
        }
    }
    Ok(result)
}

fn duration_difference(
    left: &Value,
    right: &Value,
    relative_to: Option<(i32, u32, u32)>,
) -> Result<i128, VmError> {
    let value = |duration: &Value| {
        let (year, month, _) = relative_to.unwrap_or((1970, 1, 1));
        let day = 86_400_000_000_000_i128;
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        i128::from(number_property(duration, "years") as i64) * i128::from(year_days) * day
            + i128::from(number_property(duration, "months") as i64)
                * i128::from(calendar_days_in_month(year, month))
                * day
            + duration_value_without_calendar(duration)
    };
    let left = value(left);
    let right = value(right);
    let limit = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
    if left.abs() > limit || right.abs() > limit {
        return Err(crate::value::error::throw_range_error(
            "Duration exceeds relative date range",
        ));
    }
    Ok(left - right)
}

fn duration_value_without_calendar(value: &Value) -> i128 {
    [
        ("weeks", 604_800_000_000_000_i128),
        ("days", 86_400_000_000_000),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| number_property(value, name) as i128 * scale)
    .sum()
}

fn relative_date(value: &Value) -> Result<(i32, u32, u32), VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid relativeTo"));
    }
    if matches!(value, Value::Undefined) {
        return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    if matches!(
        value,
        Value::Null
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::BigInt(_)
            | Value::Builtin(crate::ops::Builtin::Symbol)
    ) {
        return Err(crate::value::error::throw_type_error("Invalid relativeTo"));
    }
    if zoned_date_out_of_range(value) {
        return Err(crate::value::error::throw_range_error(
            "Invalid relativeTo range",
        ));
    }
    if matches!(value, Value::Proxy(_)) {
        return proxy_relative_date(value);
    }
    let date = match value {
        Value::String(text) => {
            validate_relative_string(text)?;
            validate_offset_match(text)?;
            if has_z_without_annotation(text) {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
            let date = text.split_once('T').map_or(text.as_str(), |(date, _)| date);
            let date = date.split_once('[').map_or(date, |(date, _)| date);
            let date_value =
                crate::temporal::plain_date::from(Some(&Value::String(date.into())), None)?;
            validate_date_limits(text, &date_value)?;
            date_value
        }
        _ => {
            let resolved = crate::locals::resolved_replacement(value.clone());
            if let Value::Object(object) = &resolved {
                let has_prototype = object.iter().any(|(key, _)| key == "\0prototype");
                let direct_temporal = object.iter().any(|(key, value)| {
                    key == "\0prototype"
                        && matches!(
                            value,
                            Value::Builtin(
                                crate::ops::Builtin::TemporalPlainDatePrototype
                                    | crate::ops::Builtin::TemporalZonedDateTimePrototype
                            )
                        )
                });
                if direct_temporal {
                    let field = |name: &str| {
                        object
                            .iter()
                            .find(|(key, _)| key == name)
                            .and_then(|(_, value)| match value {
                                Value::Number(value) => Some(value),
                                _ => None,
                            })
                            .unwrap_or(0.0)
                    };
                    return Ok((field("year") as i32, field("month") as u32, field("day") as u32));
                }
                if !has_prototype {
                    validate_property_bag_fields(value)?;
                }
            }
            let date = crate::temporal::plain_date::from(Some(value), None)?;
            let timezone = crate::execute::get_property_result(value, "timeZone")?;
            if !matches!(timezone, Value::Undefined) {
                let Value::String(timezone) = timezone else {
                    return Err(crate::value::error::throw_type_error("Invalid time zone"));
                };
                if crate::conversion::is_symbol_string(&timezone) {
                    return Err(crate::value::error::throw_type_error("Invalid time zone"));
                }
                validate_relative_string(&timezone)?;
                validate_timezone_string(&timezone)?;
            }
            let offset = crate::execute::get_property_result(value, "offset")?;
            if !matches!(offset, Value::Undefined) {
                let Value::String(offset) = offset else {
                    return Err(crate::value::error::throw_type_error("Invalid offset"));
                };
                if !valid_offset(&offset) {
                    return Err(crate::value::error::throw_range_error("Invalid offset"));
                }
                let timezone = crate::execute::get_property_result(value, "timeZone")?;
                if let Value::String(timezone) = timezone {
                    if timezone.contains('/') {
                        validate_property_offset_match(value, &offset, &timezone)?;
                    }
                }
            }
            date
        }
    };
    Ok((
        number_property(&date, "year") as i32,
        number_property(&date, "month") as u32,
        number_property(&date, "day") as u32,
    ))
}

fn proxy_relative_date(value: &Value) -> Result<(i32, u32, u32), VmError> {
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    let day = integer_property(value, "day")?;
    let _ = integer_property(value, "hour")?;
    let _ = integer_property(value, "microsecond")?;
    let _ = integer_property(value, "millisecond")?;
    let _ = integer_property(value, "minute")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month = if matches!(month, Value::Undefined) {
        let code = crate::execute::get_property_result(value, "monthCode")?;
        month_code_number(code)?
    } else {
        let month = crate::conversion::to_number(&month)?;
        let code = crate::execute::get_property_result(value, "monthCode")?;
        if !matches!(code, Value::Undefined) {
            crate::conversion::to_string(&code)?;
        }
        month
    };
    let _ = integer_property(value, "nanosecond")?;
    let offset = crate::execute::get_property_result(value, "offset")?;
    if !matches!(offset, Value::Undefined) {
        crate::conversion::to_string(&offset)?;
    }
    let _ = integer_property(value, "second")?;
    let _ = crate::execute::get_property_result(value, "timeZone")?;
    let year = integer_property(value, "year")?;
    let calendar = match calendar {
        Value::Undefined => Value::Undefined,
        Value::String(_) | Value::StringUnits(_) => {
            Value::String(crate::conversion::to_string(&calendar)?)
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid calendar")),
    };
    crate::temporal::plain_date::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
        calendar,
    ])?;
    Ok((year as i32, month as u32, day as u32))
}

fn integer_property(value: &Value, name: &str) -> Result<f64, VmError> {
    let value = crate::execute::get_property_result(value, name)?;
    if matches!(value, Value::Undefined) {
        return Ok(0.0);
    }
    let value = crate::conversion::to_number(&value)?;
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid relativeTo time",
        ));
    }
    Ok(value)
}

fn month_code_number(value: Value) -> Result<f64, VmError> {
    let Value::String(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    };
    let month = value
        .strip_prefix('M')
        .filter(|value| value.len() == 2)
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|month| (1..=12).contains(month))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
    Ok(f64::from(month))
}

fn zoned_date_out_of_range(value: &Value) -> bool {
    let value = crate::locals::resolved_replacement(value.clone());
    let Value::Object(object) = value else {
        return false;
    };
    let zoned = object.iter().any(|(key, value)| {
        key == "\0prototype"
            && matches!(
                value,
                Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
            )
    });
    if !zoned {
        return false;
    }
    let Some((_, Value::BigInt(epoch))) = object.iter().find(|(key, _)| key == "epochNanoseconds")
    else {
        return false;
    };
    epoch
        .parse::<i128>()
        .map(|epoch| epoch.abs() >= 8_640_000_000_000_000_000_000)
        .unwrap_or(true)
}

fn validate_date_limits(text: &str, date: &Value) -> Result<(), VmError> {
    let year = number_property(date, "year") as i32;
    let month = number_property(date, "month") as u32;
    let day = number_property(date, "day") as u32;
    let Some((_, time)) = text.split_once(['T', 't']) else {
        return Ok(());
    };
    let base = time.split_once('[').map_or(time, |(base, _)| base);
    if (year, month, day) == (-271_821, 4, 19)
        && (base.starts_with("23") || base.contains(['+', '-']))
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid relativeTo range",
        ));
    }
    if (year, month, day) == (275_760, 9, 13) {
        let clock = base.split_once(['+', '-']).map_or(base, |(clock, _)| clock);
        if clock.matches(':').count() > 1 {
            return Err(crate::value::error::throw_range_error(
                "Invalid relativeTo range",
            ));
        }
        if let Some(sign) = base[1..].find(['+', '-']).map(|index| index + 1) {
            let offset = &base[sign..];
            if offset != "+01:00" && offset != "+23:59" {
                return Err(crate::value::error::throw_range_error(
                    "Invalid relativeTo range",
                ));
            }
        }
    }
    Ok(())
}

fn validate_property_bag_fields(value: &Value) -> Result<(), VmError> {
    let year = crate::execute::get_property_result(value, "year")?;
    let era = crate::execute::get_property_result(value, "era")?;
    let era_year = crate::execute::get_property_result(value, "eraYear")?;
    if matches!(year, Value::Undefined)
        && (matches!(era, Value::Undefined) || matches!(era_year, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error("Invalid relativeTo"));
    }
    for field in [year, era_year] {
        if !matches!(field, Value::Undefined) {
            let number = crate::conversion::to_number(&field)?;
            if !number.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
            }
        }
    }
    let day = crate::execute::get_property_result(value, "day")?;
    if matches!(day, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid relativeTo"));
    }
    let number = crate::conversion::to_number(&day)?;
    if !number.is_finite() {
        return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    if matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid relativeTo"));
    }
    if !matches!(month, Value::Undefined) {
        let number = crate::conversion::to_number(&month)?;
        if !number.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
        }
    }
    for name in [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ] {
        let field = crate::execute::get_property_result(value, name)?;
        if !matches!(field, Value::Undefined) && !crate::conversion::to_number(&field)?.is_finite()
        {
            return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
        }
    }
    Ok(())
}

fn validate_relative_string(text: &str) -> Result<(), VmError> {
    if text.is_empty() {
        return Err(crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    if let Some(value) = text
        .split_once("[u-ca=")
        .and_then(|(_, rest)| rest.split(']').next())
    {
        if !crate::temporal::plain_date::is_supported_calendar_name(value) {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    if text.starts_with("-000000-") {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if let Some((_, time)) = text.split_once(['T', 't']) {
        let base = time.split_once('[').map_or(time, |(base, _)| base);
        let clock = base
            .trim_end_matches('Z')
            .split_once(['+', '-'])
            .map_or(base.trim_end_matches('Z'), |(clock, _)| clock);
        let clock_parts = clock.split(':').collect::<Vec<_>>();
        if clock_parts.iter().any(|part| part.contains('.')) {
            return Err(crate::value::error::throw_range_error(
                "Fractional relativeTo time",
            ));
        }
        if let Some(sign) = base[1..].find(['+', '-']).map(|index| index + 1) {
            let offset = &base[sign..];
            let valid = if time.contains('[') {
                valid_string_offset(offset)
            } else {
                valid_timezone_offset(offset)
            };
            if !valid {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
        }
        if let Some((_, annotation)) = time.split_once('[') {
            let annotation = annotation.strip_suffix(']').unwrap_or(annotation);
            if annotation.starts_with(['+', '-']) && !valid_timezone_offset(annotation) {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
        }
    }
    Ok(())
}

fn validate_offset_match(text: &str) -> Result<(), VmError> {
    let date = text
        .split_once(['T', 't'])
        .map(|(date, _)| date)
        .unwrap_or("");
    let Some((_, time)) = text.split_once(['T', 't']) else {
        return Ok(());
    };
    let Some((base, annotation)) = time.split_once('[') else {
        return Ok(());
    };
    let annotation = annotation.strip_suffix(']').unwrap_or(annotation);
    if annotation.contains('/') && base.matches(':').count() > 1 {
        if let Some(sign) = base[1..].find(['+', '-']).map(|index| index + 1) {
            let offset = &base[sign..];
            let supplied_seconds = offset_seconds(offset);
            let clock = &base[..sign];
            let clock = if clock.matches(':').count() == 1 {
                format!("{clock}:00")
            } else {
                clock.to_string()
            };
            let local = format!("{date}T{clock}");
            if let Ok(date_time) =
                chrono::NaiveDateTime::parse_from_str(&local, "%Y-%m-%dT%H:%M:%S")
            {
                if let Ok(zone) = annotation.parse::<chrono_tz::Tz>() {
                    let local = zone.from_local_datetime(&date_time);
                    let candidates = [local.earliest(), local.latest()];
                    let matches = candidates.iter().flatten().any(|date| {
                        let actual = date.offset().fix().local_minus_utc();
                        if offset.matches(':').count() > 1 {
                            actual == supplied_seconds
                        } else {
                            (f64::from(actual) / 60.0).round() as i32 == supplied_seconds / 60
                        }
                    });
                    if !matches {
                        return Err(crate::value::error::throw_range_error(
                            "Offset does not match time zone",
                        ));
                    }
                }
            }
        }
    }
    if let Some(base) = offset_minutes(base) {
        if let Some(annotation) = offset_minutes(annotation) {
            if base != annotation {
                return Err(crate::value::error::throw_range_error(
                    "Offset does not match time zone",
                ));
            }
        }
    }
    Ok(())
}

fn validate_property_offset_match(
    value: &Value,
    supplied: &str,
    timezone: &str,
) -> Result<(), VmError> {
    let year = integer_property(value, "year")? as i32;
    let month = integer_property(value, "month")? as u32;
    let day = integer_property(value, "day")? as u32;
    let hour = integer_property(value, "hour")? as u32;
    let minute = integer_property(value, "minute")? as u32;
    let second = integer_property(value, "second")? as u32;
    let Some(local) = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
    else {
        return Ok(());
    };
    let actual = timezone.parse::<chrono_tz::Tz>().ok().and_then(|zone| {
        zone.from_local_datetime(&local)
            .earliest()
            .map(|date| date.offset().fix().local_minus_utc())
    });
    if actual != Some(offset_seconds(supplied)) {
        return Err(crate::value::error::throw_range_error(
            "Offset does not match time zone",
        ));
    }
    Ok(())
}

fn offset_seconds(value: &str) -> i32 {
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let digits = value[1..].replace(':', "");
    let hour = digits
        .get(0..2)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);
    let minute = digits
        .get(2..4)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);
    let second = digits
        .get(4..6)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);
    sign * (hour * 3600 + minute * 60 + second)
}

fn has_z_without_annotation(text: &str) -> bool {
    text.split_once(['T', 't'])
        .is_some_and(|(_, time)| time.ends_with('Z') && !text.contains('['))
}

fn validate_timezone_string(text: &str) -> Result<(), VmError> {
    if let Some(index) = text.find(['T', 't']).filter(|index| *index >= 8) {
        let time = &text[index + 1..];
        let base = time.split_once('[').map_or(time, |(base, _)| base);
        let has_offset = base.ends_with('Z') || base[1..].contains(['+', '-']);
        if !has_offset && !text.contains('[') {
            return Err(crate::value::error::throw_range_error("Invalid time zone"));
        }
        if let Some(sign) = base[1..].find(['+', '-']).map(|index| index + 1) {
            let valid = if text.contains('[') {
                valid_string_offset(&base[sign..])
            } else {
                valid_timezone_offset(&base[sign..])
            };
            if !valid {
                return Err(crate::value::error::throw_range_error("Invalid time zone"));
            }
        }
    }
    Ok(())
}

fn valid_offset(value: &str) -> bool {
    if !value.starts_with(['+', '-']) {
        return false;
    }
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [compact] => compact.len() == 4 && compact.bytes().all(|byte| byte.is_ascii_digit()),
        [hour, minute] => {
            hour.len() == 2
                && minute.len() == 2
                && hour.bytes().all(|byte| byte.is_ascii_digit())
                && minute.bytes().all(|byte| byte.is_ascii_digit())
        }
        [hour, minute, second] => {
            hour.len() == 2
                && minute.len() == 2
                && hour.bytes().all(|byte| byte.is_ascii_digit())
                && minute.bytes().all(|byte| byte.is_ascii_digit())
                && second
                    .split_once('.')
                    .map_or(*second == "00", |(whole, fraction)| {
                        whole == "00" && !fraction.is_empty() && fraction.bytes().all(|b| b == b'0')
                    })
        }
        _ => false,
    }
}

fn valid_timezone_offset(value: &str) -> bool {
    valid_offset(value) && value.matches(':').count() <= 1
}

fn valid_string_offset(value: &str) -> bool {
    if valid_offset(value) {
        return true;
    }
    let Some(value) = value.strip_prefix(['+', '-']) else {
        return false;
    };
    let parts = value.split(':').collect::<Vec<_>>();
    let [hour, minute, second] = parts.as_slice() else {
        return false;
    };
    let (second, fraction) = second
        .split_once(['.', ','])
        .map_or((*second, None), |(s, f)| (s, Some(f)));
    hour.len() == 2
        && minute.len() == 2
        && second.len() == 2
        && hour.bytes().all(|byte| byte.is_ascii_digit())
        && minute.bytes().all(|byte| byte.is_ascii_digit())
        && second.bytes().all(|byte| byte.is_ascii_digit())
        && hour.parse::<u8>().is_ok_and(|value| value <= 23)
        && minute.parse::<u8>().is_ok_and(|value| value <= 59)
        && second.parse::<u8>().is_ok_and(|value| value <= 59)
        && fraction.is_none_or(|value| {
            !value.is_empty() && value.len() <= 9 && value.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn offset_minutes(value: &str) -> Option<i32> {
    if value == "UTC" || value == "Z" {
        return Some(0);
    }
    let start = value[1..].find(['+', '-']).map_or(0, |index| index + 1);
    let value = &value[start..];
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let value = &value[1..];
    let (hour, minute) = value
        .split_once(':')
        .map_or((value.get(..2)?, value.get(2..4)?), |(h, rest)| {
            (h, rest.get(..2).unwrap_or(rest))
        });
    Some(sign * (hour.parse::<i32>().ok()? * 60 + minute.parse::<i32>().ok()?))
}

fn calendar_days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn validate_compare_options(options: Option<&Value>) -> Result<(), VmError> {
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error(
                "Duration.compare options must be an object",
            ));
        }
    }
    Ok(())
}

fn date_units(value: &Value) -> bool {
    ["years", "months", "weeks"]
        .iter()
        .any(|name| number_property(value, name) != 0.0)
}

fn same_fields(left: Option<&Value>, right: Option<&Value>) -> bool {
    let Some((Value::Object(left), Value::Object(right))) = left.zip(right) else {
        return false;
    };
    [
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
    ]
    .iter()
    .all(|name| {
        let left = left
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        let right = right
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        crate::builtins::same_value(left.as_ref(), right.as_ref())
    })
}

fn duration_value(value: &Value) -> i128 {
    [
        ("years", 31_536_000_000_000_000_i128),
        ("months", 2_592_000_000_000_000),
        ("weeks", 604_800_000_000_000),
        ("days", 86_400_000_000_000),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| number_property(value, name) as i128 * scale)
    .sum()
}

fn exact_time_difference(left: &Value, right: &Value) -> i128 {
    let left = time_nanoseconds(left);
    let right = time_nanoseconds(right);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn time_nanoseconds(value: &Value) -> i128 {
    [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| i128::from(number_property(value, name) as i64) * scale)
    .sum()
}

fn validate_range(values: &[f64]) -> Result<(), VmError> {
    if mixed_signs(values)
        || date_fields_out_of_range(&values[..3])
        || time_fields_out_of_range(&values[3..])
    {
        return Err(crate::value::error::throw_range_error(
            "Duration fields are out of range",
        ));
    }
    Ok(())
}

fn mixed_signs(values: &[f64]) -> bool {
    let Some(sign) = values
        .iter()
        .find(|value| **value != 0.0)
        .map(|value| value.signum())
    else {
        return false;
    };
    values
        .iter()
        .any(|value| *value != 0.0 && value.signum() != sign)
}

fn date_fields_out_of_range(values: &[f64]) -> bool {
    values.iter().any(|value| value.abs() > 4_294_967_295.0)
}

fn time_fields_out_of_range(values: &[f64]) -> bool {
    let limits = [
        104_249_991_374.0,
        2_501_999_792_983.0,
        150_119_987_579_016.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991_000.0,
        9_007_199_254_740_991_000_000.0,
        9_007_199_254_740_991_000_000_000.0,
    ];
    values
        .iter()
        .zip(limits)
        .any(|(value, limit)| value.abs() > limit)
        || total_time_out_of_range(values)
}
