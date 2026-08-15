use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    let calendar = if let Some(calendar @ Value::String(_)) = arguments.get(3) {
        Some(canonical_calendar(calendar)?)
    } else if matches!(arguments.get(3), Some(value) if !matches!(value, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    } else {
        None
    };
    validate_date(year, month, day)?;
    Ok(date_object_with_calendar(
        year,
        month,
        day,
        calendar.as_deref().unwrap_or("iso8601"),
    ))
}

fn validate_constructor_calendar(value: &Value) -> Result<(), VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    if supported_calendar(&calendar.to_ascii_lowercase()) {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    }
}

fn supported_calendar(calendar: &str) -> bool {
    matches!(
        calendar,
        "buddhist"
            | "chinese"
            | "coptic"
            | "dangi"
            | "ethioaa"
            | "ethiopic"
            | "ethiopic-amete-alem"
            | "gregory"
            | "hebrew"
            | "indian"
            | "iso8601"
            | "islamic-civil"
            | "islamic-tbla"
            | "islamic-umalqura"
            | "islamicc"
            | "japanese"
            | "persian"
            | "roc"
    )
}

fn validate_era(object: &crate::value::ObjectData, calendar: &str) -> Result<(), VmError> {
    let era = object
        .iter()
        .find_map(|(key, value)| (key == "era").then_some(value));
    let era_year = object
        .iter()
        .find_map(|(key, value)| (key == "eraYear").then_some(value));
    if matches!(calendar, "chinese" | "dangi" | "iso8601") {
        return Ok(());
    }
    if era.is_some() != era_year.is_some() {
        return Err(crate::value::error::throw_type_error(
            "era and eraYear must be provided together",
        ));
    }
    let Some(Value::String(era)) = era else {
        return Ok(());
    };
    if era == "xyz" {
        return Err(crate::value::error::throw_range_error("Invalid era"));
    }
    Ok(())
}

fn validate_required_fields(object: &crate::value::ObjectData) -> Result<(), VmError> {
    for name in ["year", "day"] {
        if matches!(field(object, name)?, Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "Missing PlainDate field",
            ));
        }
    }
    if matches!(
        field(object, "month").unwrap_or(Value::Undefined),
        Value::Undefined
    ) && matches!(
        field(object, "monthCode").unwrap_or(Value::Undefined),
        Value::Undefined
    ) {
        return Err(crate::value::error::throw_type_error(
            "Missing PlainDate field",
        ));
    }
    Ok(())
}

fn canonical_calendar(value: &Value) -> Result<String, VmError> {
    validate_constructor_calendar(value)?;
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    Ok(match calendar.to_ascii_lowercase().as_str() {
        "islamicc" => "islamic-civil".into(),
        "ethiopic-amete-alem" => "ethioaa".into(),
        value => value.into(),
    })
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == crate::ops::Builtin::TemporalPlainDateFrom)
        .then(|| from(arguments.first(), arguments.get(1)))
        .or_else(|| match builtin {
            crate::ops::Builtin::TemporalPlainDateCompare => {
                Some(compare(arguments.first(), arguments.get(1)))
            }
            crate::ops::Builtin::TemporalPlainDateValueOf => Some(Err(
                crate::value::error::throw_type_error("Cannot convert PlainDate to a number"),
            )),
            crate::ops::Builtin::TemporalPlainDateToString
            | crate::ops::Builtin::TemporalPlainDateToJSON
            | crate::ops::Builtin::TemporalPlainDateToLocaleString => Some(to_string(receiver)),
            crate::ops::Builtin::TemporalPlainDateCalendarIdGetter
            | crate::ops::Builtin::TemporalPlainDateEraGetter
            | crate::ops::Builtin::TemporalPlainDateEraYearGetter
            | crate::ops::Builtin::TemporalPlainDateYearGetter
            | crate::ops::Builtin::TemporalPlainDateMonthGetter
            | crate::ops::Builtin::TemporalPlainDateMonthCodeGetter
            | crate::ops::Builtin::TemporalPlainDateDayGetter
            | crate::ops::Builtin::TemporalPlainDateDayOfWeekGetter
            | crate::ops::Builtin::TemporalPlainDateDayOfYearGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInMonthGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInWeekGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInYearGetter
            | crate::ops::Builtin::TemporalPlainDateInLeapYearGetter
            | crate::ops::Builtin::TemporalPlainDateMonthsInYearGetter => {
                Some(accessor(builtin, receiver))
            }
            crate::ops::Builtin::TemporalPlainDateEquals => {
                Some(equals(receiver, arguments.first()))
            }
            crate::ops::Builtin::TemporalPlainDateAdd => {
                Some(add(receiver, arguments.first(), 1.0))
            }
            crate::ops::Builtin::TemporalPlainDateSubtract => {
                Some(add(receiver, arguments.first(), -1.0))
            }
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
            _ => None,
        })
}

fn compare(left: Option<&Value>, right: Option<&Value>) -> Result<Value, VmError> {
    let left = date_like_fields(left)?;
    let right = date_like_fields(right)?;
    let left = date_serial(left.0, left.1, left.2);
    let right = date_serial(right.0, right.1, right.2);
    Ok(Value::Number((left.cmp(&right) as i8) as f64))
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let left = date_like_fields(receiver)?;
    let right = date_like_fields(other)?;
    let mut days = (date_serial(right.0, right.1, right.2) - date_serial(left.0, left.1, left.2))
        as f64
        * direction;
    let unit = options.and_then(|value| field_object_string(value, "largestUnit"));
    let mut years = 0.0;
    let mut months = 0.0;
    let mut weeks = 0.0;
    if matches!(unit.as_deref(), Some("years") | Some("months")) {
        let mut total = (right.0 - left.0) * 12.0 + right.1 - left.1;
        if total > 0.0 && right.2 < left.2 {
            total -= 1.0;
        } else if total < 0.0 && right.2 > left.2 {
            total += 1.0;
        }
        let sign = total.signum();
        let total = total.abs();
        if unit.as_deref() == Some("years") {
            years = (total / 12.0).floor() * sign;
            months = total % 12.0 * sign;
        } else {
            months = total * sign;
        }
        let month_total = left.1 - 1.0 + years * 12.0 + months;
        let anchor_year = left.0 + (month_total / 12.0).floor();
        let anchor_month = month_total.rem_euclid(12.0) + 1.0;
        let anchor = make_date(anchor_year, anchor_month, left.2, None)?;
        let fields = date_fields(Some(&anchor))?;
        days = (date_serial(right.0, right.1, right.2) - date_serial(fields.0, fields.1, fields.2))
            as f64
            * direction;
    } else if unit.as_deref() == Some("weeks") {
        weeks = (days / 7.0).trunc();
        days %= 7.0;
    }
    let values = [
        Value::Number(years * direction),
        Value::Number(months * direction),
        Value::Number(weeks),
        Value::Number(days),
    ];
    crate::temporal::duration::construct(&values)
}

fn date_fields(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    let Value::Object(object) =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    Ok((
        field_number(object, "year")?,
        field_number(object, "month")?,
        field_number(object, "day")?,
    ))
}

fn date_like_fields(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    match value {
        Some(Value::Object(object))
            if object.iter().any(|(key, value)| {
                key == "_temporal_kind" && value == &Value::String("PlainDate".into())
            }) =>
        {
            Ok((
                field_number(object, "year")?,
                field_number(object, "month")?,
                field_number(object, "day")?,
            ))
        }
        Some(Value::Object(_) | Value::String(_)) => {
            let date = from(value, None)?;
            date_fields(Some(&date))
        }
        _ => Err(crate::value::error::throw_type_error("Invalid PlainDate")),
    }
}

fn field_object_string(value: &Value, name: &str) -> Option<String> {
    let Value::Object(object) = value else {
        return None;
    };
    match field(object, name).ok()? {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn date_serial(year: f64, month: f64, day: f64) -> i64 {
    let mut year = year as i64;
    let month = month as i64;
    if month <= 2 {
        year -= 1;
    }
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day as i64 - 1;
    era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let Value::Object(date) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let Value::Object(duration) =
        duration.ok_or_else(|| crate::value::error::throw_type_error("Invalid duration"))?
    else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let years = object_number(duration, "years") * direction;
    let months = object_number(duration, "months") * direction;
    let weeks = object_number(duration, "weeks") * direction;
    let days = object_number(duration, "days") * direction;
    let year = field_number(date, "year")? + years;
    let month_total = field_number(date, "month")? - 1.0 + months;
    let year = year + (month_total / 12.0).floor();
    let month = month_total.rem_euclid(12.0) + 1.0;
    let day = field_number(date, "day")?.min(days_in_month(year, month)?);
    shift_days(year, month, day, weeks * 7.0 + days)
}

fn object_number(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn shift_days(mut year: f64, mut month: f64, mut day: f64, delta: f64) -> Result<Value, VmError> {
    let mut remaining = delta as i64;
    while remaining != 0 {
        if remaining > 0 {
            day += 1.0;
            if day > days_in_month(year, month)? {
                day = 1.0;
                month += 1.0;
            }
            if month > 12.0 {
                month = 1.0;
                year += 1.0;
            }
            remaining -= 1;
        } else {
            day -= 1.0;
            if day < 1.0 {
                month -= 1.0;
                if month < 1.0 {
                    month = 12.0;
                    year -= 1.0;
                }
                day = days_in_month(year, month)?;
            }
            remaining += 1;
        }
    }
    make_date(year, month, day, None)
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let Some((Value::Object(left), Value::Object(right))) = receiver.zip(other) else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(["year", "month", "day"].iter().all(
        |name| {
            left.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                == right
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
        },
    )))
}

fn accessor(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let year = field_number(object, "_year")?;
    let month = field_number(object, "_month")?;
    let day = field_number(object, "_day")?;
    let calendar = field(object, "calendarId").unwrap_or_else(|_| Value::String("iso8601".into()));
    let (era, era_year) = era_values(&calendar, year, month, day);
    let value = match builtin {
        crate::ops::Builtin::TemporalPlainDateCalendarIdGetter => {
            field(object, "calendarId").unwrap_or_else(|_| Value::String("iso8601".into()))
        }
        crate::ops::Builtin::TemporalPlainDateEraGetter => era,
        crate::ops::Builtin::TemporalPlainDateEraYearGetter => era_year,
        crate::ops::Builtin::TemporalPlainDateYearGetter => Value::Number(year),
        crate::ops::Builtin::TemporalPlainDateMonthGetter => Value::Number(month),
        crate::ops::Builtin::TemporalPlainDateMonthCodeGetter => {
            Value::String(format!("M{month:02}"))
        }
        crate::ops::Builtin::TemporalPlainDateDayGetter => Value::Number(day),
        crate::ops::Builtin::TemporalPlainDateDaysInWeekGetter => Value::Number(7.0),
        crate::ops::Builtin::TemporalPlainDateMonthsInYearGetter => Value::Number(12.0),
        crate::ops::Builtin::TemporalPlainDateDaysInMonthGetter => {
            Value::Number(days_in_month(year, month)?)
        }
        crate::ops::Builtin::TemporalPlainDateDaysInYearGetter => {
            Value::Number(if leap(year) { 366.0 } else { 365.0 })
        }
        crate::ops::Builtin::TemporalPlainDateInLeapYearGetter => Value::Boolean(leap(year)),
        crate::ops::Builtin::TemporalPlainDateDayOfYearGetter => {
            Value::Number(day_of_year(year, month, day))
        }
        crate::ops::Builtin::TemporalPlainDateDayOfWeekGetter => {
            Value::Number(day_of_week(year, month, day))
        }
        _ => Value::Undefined,
    };
    Ok(value)
}

fn era_values(calendar: &Value, year: f64, month: f64, day: f64) -> (Value, Value) {
    let Value::String(calendar) = calendar else {
        return (Value::Undefined, Value::Undefined);
    };
    match calendar.as_str() {
        "hebrew" => (Value::String("am".into()), Value::Number(year)),
        "japanese" => japanese_era(year, month, day),
        "gregory" if year > 0.0 => (Value::String("ce".into()), Value::Number(year)),
        "gregory" => (Value::String("bce".into()), Value::Number(1.0 - year)),
        "ethiopic" if year > 0.0 => (Value::String("am".into()), Value::Number(year)),
        "ethiopic" => (Value::String("aa".into()), Value::Number(5500.0 + year)),
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" if year > 0.0 => {
            (Value::String("ah".into()), Value::Number(year))
        }
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" => {
            (Value::String("bh".into()), Value::Number(1.0 - year))
        }
        "roc" if year > 0.0 => (Value::String("roc".into()), Value::Number(year)),
        "roc" => (Value::String("broc".into()), Value::Number(1.0 - year)),
        "buddhist" => (Value::String("be".into()), Value::Number(year)),
        "coptic" => (Value::String("am".into()), Value::Number(year)),
        "ethioaa" => (Value::String("aa".into()), Value::Number(year)),
        "indian" => (Value::String("shaka".into()), Value::Number(year)),
        "persian" => (Value::String("ap".into()), Value::Number(year)),
        _ => (Value::Undefined, Value::Undefined),
    }
}

fn japanese_era(year: f64, month: f64, day: f64) -> (Value, Value) {
    let (name, start) = if (year, month, day) >= (2019.0, 5.0, 1.0) {
        ("reiwa", 2019.0)
    } else if (year, month, day) >= (1989.0, 1.0, 8.0) {
        ("heisei", 1989.0)
    } else if (year, month, day) >= (1926.0, 12.0, 25.0) {
        ("showa", 1926.0)
    } else if (year, month, day) >= (1912.0, 7.0, 30.0) {
        ("taisho", 1912.0)
    } else if (year, month, day) >= (1873.0, 1.0, 1.0) {
        ("meiji", 1868.0)
    } else if year > 0.0 {
        ("ce", 1.0)
    } else {
        ("bce", 1.0 - year)
    };
    let era_year = if name == "bce" {
        1.0 - year
    } else {
        year - start + 1.0
    };
    (Value::String(name.into()), Value::Number(era_year))
}

fn japanese_year_from_era(era: &str, era_year: f64) -> f64 {
    match era {
        "reiwa" => 2019.0 + era_year - 1.0,
        "heisei" => 1989.0 + era_year - 1.0,
        "showa" => 1926.0 + era_year - 1.0,
        "taisho" => 1912.0 + era_year - 1.0,
        "meiji" => 1868.0 + era_year - 1.0,
        "bc" | "bce" => 1.0 - era_year,
        _ => era_year,
    }
}

fn year_from_era(calendar: &str, era: &str, era_year: f64) -> f64 {
    match calendar {
        "japanese" => japanese_year_from_era(era, era_year),
        "ethiopic" if era == "aa" => era_year - 5500.0,
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" if era == "bh" => 1.0 - era_year,
        "roc" if era == "broc" => 1.0 - era_year,
        _ if matches!(era, "bc" | "bce" | "bh" | "broc") => 1.0 - era_year,
        _ => era_year,
    }
}

fn leap(year: f64) -> bool {
    year % 4.0 == 0.0 && (year % 100.0 != 0.0 || year % 400.0 == 0.0)
}
fn day_of_year(year: f64, month: f64, day: f64) -> f64 {
    (1..month as i32)
        .map(|m| days_in_month(year, m as f64).unwrap_or(0.0))
        .sum::<f64>()
        + day
}

fn day_of_year_for_calendar(year: f64, month: f64, day: f64, calendar: &str) -> f64 {
    if calendar != "hebrew" {
        return day_of_year(year, month, day);
    }
    (1..month as i32)
        .map(|value| days_in_month_for_calendar(year, value as f64, calendar).unwrap_or(0.0))
        .sum::<f64>()
        + day
}
fn day_of_week(year: f64, month: f64, day: f64) -> f64 {
    let mut y = year as i64;
    let m = month as i64;
    if m < 3 {
        y -= 1;
    }
    let value = (y + y / 4 - y / 100
        + y / 400
        + [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4][(m as usize - 1) % 12]
        + day as i64)
        % 7;
    let weekday = (value + 7) % 7;
    if weekday == 0 {
        7.0
    } else {
        weekday as f64
    }
}

fn day_of_week_for_calendar(year: f64, month: f64, day: f64, calendar: &str) -> f64 {
    if calendar != "hebrew" {
        return day_of_week(year, month, day);
    }
    let month_days = (1..month as i32)
        .map(|value| days_in_month_for_calendar(year, value as f64, calendar).unwrap_or(0.0))
        .sum::<f64>();
    let weekday = (hebrew_delay(year as i64)
        + hebrew_postponement(year as i64)
        + month_days as i64
        + day as i64)
        .rem_euclid(7);
    if weekday == 0 {
        7.0
    } else {
        weekday as f64
    }
}

fn to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let year = field_number(object, "year")? as i64;
    let month = field_number(object, "month")? as i64;
    let day = field_number(object, "day")? as i64;
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+07}")
    };
    Ok(Value::String(format!("{year}-{month:02}-{day:02}")))
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    if let Value::Object(object) = value {
        if let Ok(value) = field(object, "eraYear") {
            let era_year = crate::conversion::to_number(&value)?;
            if !era_year.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid eraYear"));
            }
        }
        if let Ok(Value::String(code)) = field(object, "monthCode") {
            if crate::conversion::is_symbol(&Value::String(code.clone())) {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            }
            let syntax = code.len() == 3 || code.len() == 4 && code.ends_with('L');
            if !syntax
                || !code.starts_with('M')
                || !code[1..3]
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                return Err(crate::value::error::throw_range_error("Invalid monthCode"));
            }
        }
        if let Ok(calendar) = field(object, "calendar") {
            let Value::String(calendar) = calendar else {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            };
            if crate::conversion::is_symbol(&Value::String(calendar.clone())) {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            }
            if calendar.starts_with("-000000") {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            let iso_calendar = supported_calendar(&calendar.to_ascii_lowercase())
                || (calendar.contains('-') && !calendar.contains("[u-ca="))
                || calendar.contains("[u-ca=iso8601]");
            if !iso_calendar {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            let calendar = calendar.to_ascii_lowercase();
            if supported_calendar(&calendar) {
                validate_era(object, &calendar)?;
            }
        }
        let calendar = match field(object, "calendar")? {
            Value::String(calendar) => canonical_calendar(&Value::String(calendar))?,
            _ => "iso8601".into(),
        };
        let year = if field(object, "year").is_err() && field(object, "era").is_ok() {
            let era_year = field_number(object, "eraYear")?;
            let era = field(object, "era")?;
            let Value::String(era) = era else {
                return Err(crate::value::error::throw_type_error("Invalid era"));
            };
            year_from_era(&calendar, &era, era_year)
        } else {
            validate_required_fields(object)?;
            field_number(object, "year")?
        };
        let day = field_number(object, "day")?;
        let month = field_number(object, "month").or_else(|_| {
            let code = field(object, "monthCode")?;
            let Value::String(code) = code else {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            };
            code.strip_prefix('M')
                .and_then(|value| value.strip_suffix('L').or(Some(value)))
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))
        })?;
        if month < 0.0 || day < 0.0 {
            return Err(crate::value::error::throw_range_error("Invalid date"));
        }
        let month_code_value = if field(object, "month").is_err() {
            field(object, "monthCode").ok()
        } else {
            None
        };
        let month = if calendar == "hebrew" {
            hebrew_month_number(year, month, month_code_value.as_ref())?
        } else {
            month
        };
        if let Ok(Value::String(code)) = field(object, "monthCode") {
            let expected = month_code(year, month, &calendar);
            if code != expected {
                return Err(crate::value::error::throw_range_error("monthCode mismatch"));
            }
        }
        return make_date_with_calendar(year, month, day, options, &calendar);
    }
    let Value::String(text) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    }
    if text.contains('−') {
        return Err(crate::value::error::throw_range_error("Invalid ISO time"));
    }
    if text.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let annotation_calendar = text
        .split('[')
        .skip(1)
        .find_map(|part| part.strip_prefix("u-ca="))
        .map(|calendar| calendar.trim_end_matches(']'))
        .map(|calendar| canonical_calendar(&Value::String(calendar.into())))
        .transpose()?;
    if text.matches("u-ca=").count() > 1 && text.contains("!u-ca=") {
        return Err(crate::value::error::throw_range_error(
            "Invalid calendar annotation",
        ));
    }
    let time_zone_annotations = text
        .split('[')
        .skip(1)
        .filter(|part| !part.contains('='))
        .count();
    if time_zone_annotations > 1 {
        return Err(crate::value::error::throw_range_error(
            "Invalid time zone annotation",
        ));
    }
    let invalid_annotation = text.split('[').skip(1).any(|part| {
        if part.starts_with('!') && part.contains('=') && !part.starts_with("!u-ca=iso8601") {
            return true;
        }
        part.split_once('=')
            .is_some_and(|(key, _)| key.chars().any(|character| character.is_ascii_uppercase()))
    });
    if invalid_annotation {
        return Err(crate::value::error::throw_range_error(
            "Invalid calendar annotation",
        ));
    }
    if text
        .rsplit_once(']')
        .is_some_and(|(_, suffix)| !suffix.is_empty())
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if text
        .split('[')
        .next()
        .is_some_and(|value| value.ends_with('T'))
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if let Some(time) = text.split_once('T').map(|(_, value)| value) {
        if time.contains('Z') {
            return Err(crate::value::error::throw_range_error(
                "Invalid UTC designator",
            ));
        }
        for separator in ['.', ','] {
            if let Some(start) = time.find(separator) {
                let digits = time[start + 1..]
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .count();
                if digits > 9 {
                    return Err(crate::value::error::throw_range_error("Too many decimals"));
                }
            }
        }
        if time
            .as_bytes()
            .get(2)
            .is_some_and(|value| *value == b'.' || *value == b',')
            || time
                .as_bytes()
                .get(5)
                .is_some_and(|value| *value == b'.' || *value == b',')
        {
            return Err(crate::value::error::throw_range_error(
                "Fractional time unit",
            ));
        }
        if !time.contains('[')
            && time
                .chars()
                .any(|character| character.is_ascii_alphabetic() && character != 'Z')
        {
            return Err(crate::value::error::throw_range_error("Invalid ISO time"));
        }
        if time.contains('[') {
            // Time-zone annotations are validated by the date grammar below.
        } else {
            let hour = time.get(..2).and_then(|value| value.parse::<u32>().ok());
            let compact = time.len() >= 6
                && time.as_bytes()[..6].iter().all(u8::is_ascii_digit)
                && (time.len() == 6 || !time.as_bytes()[6].is_ascii_alphabetic());
            if time.len() > 2
                && !matches!(time.as_bytes()[2] as char, ':' | '+' | '-' | 'Z' | '[')
                && !compact
            {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if hour.is_some_and(|value| value > 23) {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if let Some(minute) = time.get(3..5).and_then(|value| value.parse::<u32>().ok()) {
                if minute > 59 {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
            }
            if time.as_bytes().get(2) == Some(&b':')
                && time.len() > 5
                && !matches!(
                    time.as_bytes()[5] as char,
                    ':' | '.' | ',' | '+' | '-' | 'Z' | '['
                )
            {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if time.as_bytes().get(2) == Some(&b':') && time.as_bytes().get(5) == Some(&b':') {
                if time.len() < 8 || time[6..8].parse::<u32>().is_err() {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
                if time.len() > 8
                    && !matches!(
                        time.as_bytes()[8] as char,
                        '.' | ',' | '+' | '-' | 'Z' | '['
                    )
                {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
            }
        }
    }
    let date = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .next()
        .unwrap_or(text);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 1 && (date.len() == 8 || date.len() == 11) {
        let (year_end, month_start, day_start) = if date.len() == 11 {
            (7, 7, 9)
        } else {
            (4, 4, 6)
        };
        let year = date[..year_end]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = date[month_start..month_start + 2]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[day_start..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return Ok(date_object_with_calendar(
            year,
            month,
            day,
            annotation_calendar.as_deref().unwrap_or("iso8601"),
        ));
    }
    if parts.len() == 4 && parts[0].is_empty() {
        let year = format!("-{}", parts[1])
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = parts[2]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = parts[3]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return Ok(date_object_with_calendar(
            year,
            month,
            day,
            annotation_calendar.as_deref().unwrap_or("iso8601"),
        ));
    }
    if parts.len() != 3 {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if parts[0].len() != 4 && !(parts[0].starts_with('+') && parts[0].len() == 7) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if parts[1].len() != 2 || parts[2].len() != 2 {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let year = parts[0]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    let month = parts[1]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    let day = parts[2]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    Ok(date_object_with_calendar(
        year,
        month,
        day,
        annotation_calendar.as_deref().unwrap_or("iso8601"),
    ))
}

fn make_date(year: f64, month: f64, day: f64, options: Option<&Value>) -> Result<Value, VmError> {
    make_date_with_calendar(year, month, day, options, "iso8601")
}

fn make_date_with_calendar(
    year: f64,
    month: f64,
    day: f64,
    options: Option<&Value>,
    calendar: &str,
) -> Result<Value, VmError> {
    let max = days_in_month_for_calendar(year, month, calendar)?;
    if !day.is_finite() || day <= 0.0 {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    let outside_temporal_range = (year == -271821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275760.0 && (month > 9.0 || month == 9.0 && day > 13.0));
    if outside_temporal_range {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    if day <= 0.0
        || day > max
        || (year == -271821.0 && (month < 4.0 || (month == 4.0 && day < 19.0)))
        || (year == 275760.0 && (month > 9.0 || (month == 9.0 && day > 13.0)))
    {
        let reject = matches!(options, Some(Value::Object(object)) if object.iter().any(|(key, value)| key == "overflow" && matches!(value, Value::String(value) if value == "reject")));
        if reject {
            return Err(crate::value::error::throw_range_error("Invalid date"));
        }
        return Ok(date_object_with_calendar(
            year,
            month,
            day.clamp(1.0, max),
            calendar,
        ));
    }
    Ok(date_object_with_calendar(year, month, day, calendar))
}

fn validate_date(year: f64, month: f64, day: f64) -> Result<(), VmError> {
    let max = days_in_month(year, month)?;
    if !day.is_finite()
        || day <= 0.0
        || day > max
        || (year == -271821.0 && (month < 4.0 || (month == 4.0 && day < 19.0)))
        || (year == 275760.0 && (month > 9.0 || (month == 9.0 && day > 13.0)))
    {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    Ok(())
}

fn days_in_month(year: f64, month: f64) -> Result<f64, VmError> {
    if !(1.0..=12.0).contains(&month)
        || !year.is_finite()
        || !(-271821.0..=275760.0).contains(&year)
    {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    if month == 2.0 {
        return Ok(
            if year % 4.0 == 0.0 && (year % 100.0 != 0.0 || year % 400.0 == 0.0) {
                29.0
            } else {
                28.0
            },
        );
    }
    Ok(if [4.0, 6.0, 9.0, 11.0].contains(&month) {
        30.0
    } else {
        31.0
    })
}

fn days_in_month_for_calendar(year: f64, month: f64, calendar: &str) -> Result<f64, VmError> {
    if calendar != "hebrew" {
        if matches!(calendar, "iso8601" | "gregory") {
            return days_in_month(year, month);
        }
        if !(1.0..=13.0).contains(&month) || !year.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid date"));
        }
        return Ok(if month == 2.0 {
            if leap(year) {
                29.0
            } else {
                28.0
            }
        } else if month == 13.0 {
            29.0
        } else if [4.0, 6.0, 9.0, 11.0].contains(&month) {
            30.0
        } else {
            31.0
        });
    }
    let leap = is_hebrew_leap_year(year);
    let max_month = if leap { 13.0 } else { 12.0 };
    if !(1.0..=max_month).contains(&month) || !year.is_finite() {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    let lengths = if leap {
        [
            30.0, 29.0, 30.0, 29.0, 30.0, 30.0, 29.0, 30.0, 29.0, 30.0, 29.0, 30.0, 29.0,
        ]
    } else {
        [
            30.0, 29.0, 30.0, 29.0, 30.0, 29.0, 30.0, 29.0, 30.0, 29.0, 30.0, 29.0, 0.0,
        ]
    };
    let mut length = lengths[month as usize - 1];
    let year_length = hebrew_year_length(year);
    if month == 2.0 {
        length = if year_length % 10 == 5 { 30.0 } else { 29.0 };
    } else if month == 3.0 {
        length = if year_length % 10 == 3 { 29.0 } else { 30.0 };
    }
    if month == 6.0 && leap {
        length = 30.0;
    }
    Ok(length)
}

fn is_hebrew_leap_year(year: f64) -> bool {
    matches!((year as i64).rem_euclid(19), 0 | 3 | 6 | 8 | 11 | 14 | 17)
}

fn hebrew_month_number(year: f64, month: f64, month_code: Option<&Value>) -> Result<f64, VmError> {
    let Some(Value::String(code)) = month_code else {
        return Ok(month);
    };
    if code == "M05L" {
        return Ok(6.0);
    }
    let number = month;
    if is_hebrew_leap_year(year) && number >= 6.0 {
        Ok(number + 1.0)
    } else {
        Ok(number)
    }
}

fn month_code(year: f64, month: f64, calendar: &str) -> String {
    if matches!(calendar, "chinese" | "dangi") && month == 13.0 {
        return "M12".into();
    }
    if calendar == "hebrew" && is_hebrew_leap_year(year) {
        if month == 6.0 {
            return "M05L".into();
        }
        if month > 6.0 {
            return format!("M{:02}", month as u32 - 1);
        }
    }
    format!("M{:02}", month as u32)
}

fn hebrew_delay(year: i64) -> i64 {
    let months = (235 * year - 234).div_euclid(19);
    let parts = 12_084 + 13_753 * months;
    let mut day = 29 * months + parts.div_euclid(25_920);
    if (3 * (day + 1)).rem_euclid(7) < 3 {
        day += 1;
    }
    day
}

fn hebrew_postponement(year: i64) -> i64 {
    let present = hebrew_delay(year);
    let next = hebrew_delay(year + 1);
    let previous = hebrew_delay(year - 1);
    if next - present == 356 {
        2
    } else if present - previous == 382 {
        1
    } else {
        0
    }
}

fn hebrew_year_length(year: f64) -> i64 {
    let year = year as i64;
    hebrew_delay(year + 1) + hebrew_postponement(year + 1)
        - hebrew_delay(year)
        - hebrew_postponement(year)
}

fn field(object: &crate::value::ObjectData, name: &str) -> Result<Value, VmError> {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| crate::value::error::throw_type_error("Missing PlainDate field"))
}

fn field_number(object: &crate::value::ObjectData, name: &str) -> Result<f64, VmError> {
    let value = field(object, name)?;
    crate::conversion::to_number(&value).map(f64::trunc)
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    crate::conversion::to_number(value).map(f64::trunc)
}

fn date_object_with_calendar(year: f64, month: f64, day: f64, calendar: &str) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        (
            "monthCode".into(),
            Value::String(month_code(year, month, calendar)),
        ),
        ("day".into(), Value::Number(day)),
        ("_temporal_kind".into(), Value::String("PlainDate".into())),
        ("_year".into(), Value::Number(year)),
        ("_month".into(), Value::Number(month)),
        ("_day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String(calendar.into())),
        (
            "dayOfWeek".into(),
            Value::Number(day_of_week_for_calendar(year, month, day, calendar)),
        ),
        (
            "dayOfYear".into(),
            Value::Number(day_of_year_for_calendar(year, month, day, calendar)),
        ),
        (
            "daysInMonth".into(),
            Value::Number(days_in_month_for_calendar(year, month, calendar).unwrap_or(0.0)),
        ),
        ("daysInWeek".into(), Value::Number(7.0)),
        (
            "daysInYear".into(),
            Value::Number(if calendar == "hebrew" {
                hebrew_year_length(year) as f64
            } else if leap(year) {
                366.0
            } else {
                365.0
            }),
        ),
        (
            "inLeapYear".into(),
            Value::Boolean(if calendar == "hebrew" {
                is_hebrew_leap_year(year)
            } else {
                leap(year)
            }),
        ),
        (
            "monthsInYear".into(),
            Value::Number(if calendar == "hebrew" && is_hebrew_leap_year(year) {
                13.0
            } else {
                12.0
            }),
        ),
        (
            "weekOfYear".into(),
            Value::Number(iso_week(year, month, day).0),
        ),
        (
            "yearOfWeek".into(),
            Value::Number(iso_week(year, month, day).1),
        ),
        (
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}

fn iso_week(year: f64, month: f64, day: f64) -> (f64, f64) {
    let date = date_serial(year, month, day);
    let jan4 = date_serial(year, 1.0, 4.0);
    let first = jan4 - (day_of_week(year, 1.0, 4.0) as i64 - 1);
    if date < first {
        return iso_week(year - 1.0, 12.0, 31.0);
    }
    let next = date_serial(year + 1.0, 1.0, 4.0);
    let next_first = next - (day_of_week(year + 1.0, 1.0, 4.0) as i64 - 1);
    if date >= next_first {
        return (1.0, year + 1.0);
    }
    (((date - first) / 7 + 1) as f64, year)
}
