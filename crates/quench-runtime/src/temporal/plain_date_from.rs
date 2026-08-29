pub(crate) fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    if let Some(value) = value.filter(|value| crate::value::is_object(value)) {
        if let Value::Object(object) = value {
            let hidden = ["year", "month", "day"].map(|name| {
                object
                    .iter()
                    .find(|(key, _)| key == &format!("\0temporal-slot:\0{name}"))
                    .and_then(|(_, value)| matches!(value, Value::Number(_)).then(|| value.clone()))
            });
            if let [Some(year), Some(month), Some(day)] = hidden {
                let _ = overflow_value(options)?;
                let is_plain_date = object.iter().any(|(key, value)| {
                    key == "\0temporal-plain-date"
                        && value == Value::Boolean(true)
                        || key == "\0prototype"
                            && value == Value::Builtin(
                                crate::ops::Builtin::TemporalPlainDatePrototype,
                            )
                });
                if is_plain_date {
                    return Ok(value.clone());
                }
                let calendar = object
                    .iter()
                    .find(|(key, _)| key == "calendarId")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| Value::String("iso8601".into()));
                return construct(&[year, month, day, calendar]);
            }
            let direct = ["year", "month", "day"].map(|name| {
                object
                    .iter()
                    .find(|(key, value)| {
                        (key == name || key == &format!("\0temporal-slot:\0{name}"))
                            && matches!(value, Value::Number(_))
                    })
                    .map(|(_, value)| value.clone())
            });
            let temporal_date = object.iter().any(|(key, value)| {
                key == "\0temporal-plain-date"
                    || key == "\0prototype"
                        && matches!(
                            value,
                            Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
                                | Value::Builtin(
                                    crate::ops::Builtin::TemporalZonedDateTimePrototype
                                )
                        )
            });
            if temporal_date {
                if let [Some(year), Some(month), Some(day)] = direct {
                    let _ = overflow_value(options)?;
                    let calendar = object
                        .iter()
                        .find(|(key, _)| key == "calendarId")
                        .map(|(_, value)| value.clone())
                        .unwrap_or_else(|| Value::String("iso8601".into()));
                    return construct(&[year, month, day, calendar]);
                }
            }
        }
        return from_property_bag(value, options);
    }
    if value.is_some_and(crate::conversion::is_symbol) {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    }
    let text = match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::StringUnits(_)) => crate::conversion::to_string(value.unwrap())?,
        _ => return Err(crate::value::error::throw_type_error("Invalid PlainDate")),
    };
    let calendar_hint = text
        .split_once("[u-ca=")
        .and_then(|(_, rest)| rest.split(']').next())
        .and_then(canonical_calendar_id)
        .unwrap_or_else(|| "iso8601".into());
    if text.starts_with("-000000") || text.contains('−') || has_empty_time_designator(&text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_fractional_minutes(&text)
        || has_invalid_time(&text)
        || has_time_junk(&text)
        || has_annotation_junk(&text)
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_utc_designator(&text) || text.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_excess_fraction(&text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let calendar_count = text.matches("[u-ca=").count();
    if has_uppercase_annotation_key(&text) {
        return Err(crate::value::error::throw_range_error(
            "Invalid annotation key",
        ));
    }
    if has_invalid_calendar_annotation(&text) {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if has_multiple_time_zones(&text) {
        return Err(crate::value::error::throw_range_error(
            "Multiple time zones",
        ));
    }
    if has_unknown_critical_annotation(&text) {
        return Err(crate::value::error::throw_range_error(
            "Unknown critical annotation",
        ));
    }
    if text.contains("[!u-ca=") && calendar_count > 0 {
        return Err(crate::value::error::throw_range_error("Multiple calendars"));
    }
    let date = date_part(&text);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 1 && date.len() == 8 {
        let year = date[..4]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = date[4..6]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[6..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let result = checked_date_object(year, month, day, &calendar_hint)?;
        let _ = overflow_value(options)?;
        return Ok(result);
    }
    if parts.len() == 1 && date.len() == 11 && matches!(date.as_bytes()[0], b'+' | b'-') {
        let year = date[1..7]
            .parse::<i32>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let year = if date.as_bytes()[0] == b'-' {
            -year
        } else {
            year
        };
        let month = date[7..9]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[9..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let result = checked_date_object(year, month, day, &calendar_hint)?;
        let _ = overflow_value(options)?;
        return Ok(result);
    }
    let (year, month, day) = parse_date_parts(&parts)?;
    let result = checked_date_object(year, month, day, &calendar_hint)?;
    let _ = overflow_value(options)?;
    Ok(result)
}

fn preserve_calendar_month_code(
    mut result: Value,
    year: f64,
    month: f64,
    day: f64,
    calendar: &str,
    code: &Value,
) -> Value {
    let Value::String(code) = code else {
        return result;
    };
    if crate::temporal::plain_year_month::calendar_edge_month_fields(
        calendar,
        year as i32,
        month as u32,
        code,
    ) {
        if let Value::Object(object) = &mut result {
            let object = std::rc::Rc::make_mut(object);
            object.set_property_in_place("monthCode", Value::String(code.to_string()));
            object.set_property_in_place(
                "\0temporal-slot:\0monthCode",
                Value::String(code.to_string()),
            );
        }
        return result;
    }
    let Some((ordinal, canonical)) = crate::temporal::plain_date::calendar_date_from_code(
        year as i32,
        code,
        day as u32,
        calendar,
    ) else {
        return result;
    };
    if let Value::Object(object) = &mut result {
        let object = std::rc::Rc::make_mut(object);
        object.set_property_in_place("month", Value::Number(ordinal as f64));
        object.set_property_in_place("\0temporal-slot:\0month", Value::Number(ordinal as f64));
        object.set_property_in_place("monthCode", Value::String(canonical.clone()));
        object.set_property_in_place("\0temporal-slot:\0monthCode", Value::String(canonical));
    }
    result
}

fn from_property_bag(value: &Value, options: Option<&Value>) -> Result<Value, VmError> {
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    let calendar_text = match &calendar {
        Value::String(value) => Some(value.to_ascii_lowercase()),
        Value::StringUnits(_) => {
            Some(crate::conversion::to_string(&calendar)?.to_ascii_lowercase())
        }
        _ => None,
    };
    let day = crate::execute::get_property_result(value, "day")?;
    let day = if matches!(day, Value::Undefined) {
        day
    } else {
        Value::Number(crate::conversion::to_number(&day)?.trunc())
    };
    let month = crate::execute::get_property_result(value, "month")?;
    let month = if matches!(month, Value::Undefined) {
        month
    } else {
        Value::Number(crate::conversion::to_number(&month)?.trunc())
    };
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    let month_code = if matches!(month_code, Value::Undefined) {
        month_code
    } else {
        Value::String(month_code_text(&month_code)?)
    };
    let mut year = crate::execute::get_property_result(value, "year")?;
    let calendar_name = calendar_text.as_deref().unwrap_or("iso8601");
    // ISO calendars have no era fields; do not probe (or reject) optional
    // era/eraYear properties on an ISO property bag.
    let (era, era_year) = if calendar_name == "iso8601" {
        (Value::Undefined, Value::Undefined)
    } else {
        (
            crate::execute::get_property_result(value, "era")?,
            crate::execute::get_property_result(value, "eraYear")?,
        )
    };
    let era_provided = !matches!(era, Value::Undefined);
    let era_year_provided = !matches!(era_year, Value::Undefined);
    if era_provided != era_year_provided {
        return Err(crate::value::error::throw_type_error(
            "era and eraYear must be provided together",
        ));
    }
    if matches!(day, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing day"));
    }
    if matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing month"));
    }
    if let Value::String(code) = &month_code {
        let core = code.strip_suffix('L').unwrap_or(code);
        if core.len() != 3
            || !core.starts_with('M')
            || !core[1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
    }
    let era_name = if matches!(era, Value::Undefined) {
        None
    } else if matches!(calendar_name, "chinese" | "dangi") {
        None
    } else {
        let text = crate::conversion::to_string(&era)?.to_ascii_lowercase();
        match canonical_era_name(calendar_name, &text) {
            Some(canonical) => Some(canonical),
            None if era_for_calendar(calendar_name, 0.0).is_some() => {
                return Err(crate::value::error::throw_range_error("Invalid era"));
            }
            None => None,
        }
    };
    if matches!(year, Value::Undefined) {
        let era_year = if matches!(era_year, Value::Undefined) {
            None
        } else {
            let value = crate::conversion::to_number(&era_year)?.trunc();
            if !value.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid eraYear"));
            }
            Some(value)
        };
        year = match (era_name, era_year) {
            (Some(era), Some(value)) => derive_year_from_era(calendar_name, era, value)
                .map(Value::Number)
                .ok_or_else(|| crate::value::error::throw_type_error("Missing year"))?,
            (None, Some(_)) => {
                return Err(crate::value::error::throw_type_error("Calendar does not use eras"));
            }
            _ => return Err(crate::value::error::throw_type_error("Missing year")),
        };
    }
    let year = Value::Number(crate::conversion::to_number(&year)?.trunc());
    let overflow = overflow_value(options)?;
    let month_code_number = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(month_from_code(month_code.clone())?)
    };
    if let Some(month_number) = &month_code_number {
        let month_number = crate::conversion::to_number(month_number)?;
        let supports_month13 = calendar_text
            .as_deref()
            .is_some_and(crate::temporal::plain_date::calendar_supports_month13);
        if !(1.0..=12.0).contains(&month_number) && !(supports_month13 && month_number == 13.0) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if matches!(&month_code, Value::String(value) if value.ends_with('L'))
            && !calendar_text
                .as_deref()
                .is_some_and(|value| matches!(value, "chinese" | "dangi" | "hebrew"))
        {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if matches!((&month_code, calendar_text.as_deref()),
            (Value::String(value), Some("hebrew")) if value.ends_with('L') && month_number != 5.0)
        {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
    }
    let calendar = match calendar {
        Value::Undefined => calendar,
        Value::String(_) | Value::StringUnits(_) => {
            let text = crate::conversion::to_string(&calendar)?;
            if !is_iso_calendar_string(&text) {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            Value::String(
                crate::temporal::plain_date::canonical_calendar_id(&text)
                    .unwrap_or_else(|| "iso8601".into()),
            )
        }
        value if crate::temporal::plain_date::is_temporal_date_like(&value) => {
            Value::String("iso8601".into())
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid calendar")),
    };
    let month = if matches!(month, Value::Undefined) {
        month_code_number
            .clone()
            .ok_or_else(|| crate::value::error::throw_type_error("Missing month"))?
    } else {
        if let Some(month_code_number) = &month_code_number {
            let edge_fields = if let (Some(calendar), Value::String(code)) =
                (calendar_text.as_deref(), &month_code)
            {
                crate::temporal::plain_year_month::calendar_edge_month_fields(
                    calendar,
                    crate::conversion::to_number(&year)?.trunc() as i32,
                    crate::conversion::to_number(&month)?.trunc() as u32,
                    code,
                )
            } else {
                false
            };
            if crate::conversion::to_number(&month)?
                != crate::conversion::to_number(month_code_number)?
                && !edge_fields
            {
                return Err(crate::value::error::throw_range_error("Month mismatch"));
            }
        }
        month
    };
    let year_number = crate::conversion::to_number(&year)?.trunc();
    let year = Value::Number(year_number);
    let mut day = crate::conversion::to_number(&day)?.trunc();
    let month_number = crate::conversion::to_number(&month)?;
    if !month_number.is_finite() || month_number < 1.0 || !day.is_finite() || day < 1.0 {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if overflow == "constrain" {
        let calendar_name = match &calendar {
            Value::String(name) => crate::temporal::plain_date::canonical_calendar_id(name)
                .unwrap_or_else(|| name.clone()),
            _ => "iso8601".to_string(),
        };
        let max_month = if crate::temporal::plain_date::calendar_has_month13(&calendar_name) {
            13.0
        } else {
            12.0
        };
        let month_number = month_number.clamp(1.0, max_month);
        let max_day = match &month_code {
            Value::String(code) => crate::temporal::plain_year_month::calendar_edge_day(
                &calendar_name,
                year_number as i32,
                month_number as u32,
                code,
            )
            .or_else(|| {
                crate::temporal::plain_date::calendar_days_in_month_for_code(
                    year_number as i32,
                    code,
                    &calendar_name,
                )
            }),
            _ if matches!(calendar_name.as_str(), "chinese" | "dangi" | "hebrew") => {
                let ordinal = crate::temporal::plain_date::calendar_days_in_month(
                    year_number as i32,
                    month_number as u32,
                    &calendar_name,
                );
                let code = format!("M{month_number:02.0}");
                let by_code = crate::temporal::plain_date::calendar_days_in_month_for_code(
                    year_number as i32,
                    &code,
                    &calendar_name,
                );
                match (ordinal, by_code) {
                    (Some(left), Some(right)) => Some(left.min(right)),
                    (Some(value), None) | (None, Some(value)) => Some(value),
                    _ => None,
                }
            }
            _ => crate::temporal::plain_year_month::calendar_edge_day_for_month(
                &calendar_name,
                year_number as i32,
                month_number as u32,
            )
            .or_else(|| {
                crate::temporal::plain_date::calendar_days_in_month(
                    year_number as i32,
                    month_number as u32,
                    &calendar_name,
                )
            }),
        }
        .unwrap_or_else(|| days_in_month(year_number, month_number) as u32)
            as f64;
        day = day.clamp(1.0, max_day);
        let result = construct(&[
            year,
            Value::Number(month_number),
            Value::Number(day),
            calendar.clone(),
        ])?;
        let calendar_name = match &calendar {
            Value::String(name) => name.as_str(),
            _ => "iso8601",
        };
        return Ok(preserve_calendar_month_code(
            result,
            year_number,
            month_number,
            day,
            calendar_name,
            &month_code,
        ));
    }
    if overflow == "reject" {
        let calendar_name = match &calendar {
            Value::String(name) => crate::temporal::plain_date::canonical_calendar_id(name)
                .unwrap_or_else(|| name.clone()),
            _ => "iso8601".to_string(),
        };
        let max_day = match &month_code {
            Value::String(code) => crate::temporal::plain_date::calendar_days_in_month_for_code(
                year_number as i32,
                code,
                &calendar_name,
            ),
            _ => crate::temporal::plain_date::calendar_days_in_month(
                year_number as i32,
                month_number as u32,
                &calendar_name,
            ),
        };
        if max_day.is_some_and(|max_day| day > f64::from(max_day)) {
            return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
        }
    }
    let result = construct(&[year, month, Value::Number(day), calendar.clone()])?;
    let calendar_name = match &calendar {
        Value::String(name) => name.as_str(),
        _ => "iso8601",
    };
    Ok(preserve_calendar_month_code(
        result,
        year_number,
        month_number,
        day,
        calendar_name,
        &month_code,
    ))
}

pub(crate) fn canonical_era_name(calendar: &str, era: &str) -> Option<&'static str> {
    match calendar {
        "gregory" | "japanese" => match era {
            "ad" | "ce" => Some("ce"),
            "bc" | "bce" => Some("bce"),
            "be" => Some("be"),
            "heisei" => Some("heisei"),
            "reiwa" => Some("reiwa"),
            "showa" => Some("showa"),
            "taisho" => Some("taisho"),
            "meiji" => Some("meiji"),
            _ => None,
        },
        "buddhist" if era == "be" => Some("be"),
        "hebrew" if era == "am" => Some("am"),
        value if value.starts_with("islamic") && era == "ah" => Some("ah"),
        value if value.starts_with("islamic") && era == "bh" => Some("bh"),
        "persian" if era == "ap" => Some("ap"),
        "coptic" if era == "am" => Some("am"),
        "ethiopic" if era == "am" => Some("am"),
        "ethiopic" if era == "aa" => Some("aa"),
        "ethioaa" if era == "aa" => Some("aa"),
        "indian" if era == "shaka" => Some("shaka"),
        "roc" if era == "roc" => Some("roc"),
        "roc" if era == "broc" => Some("broc"),
        _ => None,
    }
}

pub(crate) fn derive_year_from_era(calendar: &str, era: &str, era_year: f64) -> Option<f64> {
    if !era_year.is_finite() {
        return None;
    }
    match (calendar, era) {
        ("gregory" | "japanese", "bce") => Some(1.0 - era_year),
        ("gregory" | "japanese", "ce") => Some(era_year),
        ("japanese", "reiwa") => Some(era_year + 2018.0),
        ("japanese", "heisei") => Some(era_year + 1988.0),
        ("japanese", "showa") => Some(era_year + 1925.0),
        ("japanese", "taisho") => Some(era_year + 1911.0),
        ("japanese", "meiji") => Some(era_year + 1867.0),
        ("ethiopic", "aa") => Some(era_year - 5500.0),
        ("roc", "broc") => Some(1.0 - era_year),
        ("islamic-civil" | "islamic-tbla" | "islamic-umalqura", "bh") => Some(1.0 - era_year),
        _ => Some(era_year),
    }
}

fn overflow_value(options: Option<&Value>) -> Result<&'static str, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("constrain");
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok("constrain");
    }
    let value = crate::conversion::to_string(&value)?;
    if matches!(value.as_str(), "constrain" | "reject") {
        Ok(if value == "reject" {
            "reject"
        } else {
            "constrain"
        })
    } else {
        Err(crate::value::error::throw_range_error("Invalid overflow"))
    }
}

fn is_iso_calendar_string(value: &str) -> bool {
    if crate::temporal::plain_date::is_supported_calendar_name(value) {
        return true;
    }
    if crate::intl::supported_calendars()
        .iter()
        .any(|calendar| matches!(calendar, Value::String(name) if name.eq_ignore_ascii_case(value)))
    {
        return true;
    }
    if value.starts_with("-000000") || value.starts_with('\u{2212}') {
        return false;
    }
    if value.eq_ignore_ascii_case("iso8601") {
        return true;
    }
    let (base, annotation) = value
        .split_once('[')
        .map_or((value, None), |(base, annotation)| (base, Some(annotation)));
    if let Some(annotation) = annotation {
        if !annotation
            .strip_suffix(']')
            .is_some_and(|value| value.eq_ignore_ascii_case("u-ca=iso8601"))
        {
            return false;
        }
    }
    let date = base.split(['T', 't', ' ']).next().unwrap_or(base);
    let fields: Vec<_> = date.split('-').collect();
    let valid_year = |year: &str| {
        (year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit()))
            || (year.len() == 7
                && matches!(year.as_bytes().first(), Some(b'+' | b'-'))
                && year[1..].bytes().all(|byte| byte.is_ascii_digit()))
    };
    match fields.as_slice() {
        [year, month, day] => valid_year(year) && month.len() == 2 && day.len() == 2,
        [year, month] if valid_year(year) => month.len() == 2,
        [month, day] => month.len() == 2 && day.len() == 2,
        _ => false,
    }
}

fn month_from_code(value: Value) -> Result<Value, VmError> {
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    let text = month_code_text(&value)?;
    let text = text.strip_suffix('L').unwrap_or(&text);
    let month = text
        .strip_prefix('M')
        .filter(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u8>().ok())
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
    Ok(Value::Number(f64::from(month)))
}

fn has_uppercase_annotation_key(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| annotation.contains('='))
        .any(|annotation| {
            annotation
                .split('=')
                .next()
                .is_some_and(|key| key.chars().any(|character| character.is_ascii_uppercase()))
        })
}

fn has_unknown_critical_annotation(text: &str) -> bool {
    text.split('[').skip(1).any(|annotation| {
        annotation.starts_with('!') && annotation.contains('=') && !annotation.starts_with("!u-ca=")
    })
}

fn has_invalid_calendar_annotation(text: &str) -> bool {
    let mut seen = false;
    for annotation in text.split('[').skip(1) {
        let Some(value) = ["u-ca=", "!u-ca="]
            .iter()
            .find_map(|prefix| annotation.strip_prefix(prefix))
            .and_then(|value| value.split(']').next())
        else {
            continue;
        };
        if seen {
            continue;
        }
        seen = true;
        return !crate::temporal::plain_date::is_supported_calendar_name(value);
    }
    false
}

fn has_time_junk(text: &str) -> bool {
    let Some(base) = text.split('[').next() else {
        return false;
    };
    let Some((_, time)) = base.split_once(['T', 't']) else {
        return false;
    };
    time.chars()
        .any(|character| !character.is_ascii_digit() && !":.,+-Zz".contains(character))
}

fn has_annotation_junk(text: &str) -> bool {
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open + 1..].find(']') else {
            return true;
        };
        let after = &rest[open + 1 + close + 1..];
        if !after.is_empty() && !after.starts_with('[') {
            return true;
        }
        rest = after;
    }
    false
}

fn has_multiple_time_zones(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| !annotation.contains('=') && !annotation.is_empty())
        .count()
        > 1
}

fn has_excess_fraction(text: &str) -> bool {
    text.split('[')
        .next()
        .and_then(|value| value.find('.').map(|index| &value[index + 1..]))
        .is_some_and(|fraction| fraction.bytes().take_while(u8::is_ascii_digit).count() > 9)
}

fn has_utc_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|value| value.ends_with('Z'))
}

fn has_empty_time_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|value| value.ends_with(['T', 't']))
}

fn has_fractional_minutes(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
        .and_then(|value| value.split('[').next())
    else {
        return false;
    };
    let time = time
        .get(1..)
        .and_then(|value| value.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    if !time.contains(':') {
        return false;
    }
    let mut fields = time.split(':');
    let Some(hours) = fields.next() else {
        return false;
    };
    let Some(minutes) = fields.next() else {
        return false;
    };
    hours.contains(['.', ',']) || minutes.contains(['.', ','])
}

fn has_invalid_time(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
        .and_then(|value| value.split('[').next())
    else {
        return false;
    };
    let time = time.trim_end_matches(['Z', 'z']);
    let clock = time
        .get(1..)
        .and_then(|value| value.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    let fields: Vec<_> = clock.split(':').collect();
    let parse = |value: &str| value.parse::<u32>().ok();
    if fields.len() == 1 {
        let has_fraction = fields[0].contains(['.', ',']);
        let compact = fields[0].split(['.', ',']).next().unwrap_or(fields[0]);
        if !matches!(compact.len(), 2 | 4 | 6)
            || !compact.bytes().all(|byte| byte.is_ascii_digit())
            || has_fraction && compact.len() != 6
        {
            return true;
        }
        let hour = compact[0..2].parse::<u32>().unwrap_or(99);
        let minute = if compact.len() >= 4 {
            compact[2..4].parse::<u32>().unwrap_or(99)
        } else {
            0
        };
        let second = if compact.len() == 6 {
            compact[4..6].parse::<u32>().unwrap_or(99)
        } else {
            0
        };
        return hour > 23 || minute > 59 || second > 60;
    }
    if fields.len() > 1 && (fields[0].len() != 2 || fields[1].len() != 2) {
        return true;
    }
    let Some(hour) = parse(fields[0].split(['.', ',']).next().unwrap_or(fields[0])) else {
        return true;
    };
    if hour > 23 {
        return true;
    }
    if fields.len() == 1 {
        return false;
    }
    let Some(minute) = parse(fields[1].split(['.', ',']).next().unwrap_or(fields[1])) else {
        return true;
    };
    if minute > 59 {
        return true;
    }
    fields.get(2).is_some_and(|second| {
        let second = second.split(['.', ',']).next().unwrap_or(second);
        second.len() != 2 || parse(second).is_none_or(|second| second > 60)
    })
}

fn date_part(text: &str) -> &str {
    text.split(['T', 't', ' ', '[']).next().unwrap_or(text)
}

fn parse_date_parts(parts: &[&str]) -> Result<(i32, i32, i32), VmError> {
    let (year, month, day) = match parts {
        [year, month, day]
            if year.len() == 4
                && year.bytes().all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            ((*year).to_owned(), (*month).to_owned(), (*day).to_owned())
        }
        [year, month, day]
            if year.len() == 7
                && year.starts_with('+')
                && year.as_bytes()[1..]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            ((*year).to_owned(), (*month).to_owned(), (*day).to_owned())
        }
        ["", year, month, day]
            if year.len() == 6
                && year.bytes().all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (format!("-{year}"), (*month).to_owned(), (*day).to_owned())
        }
        _ => return Err(crate::value::error::throw_range_error("Invalid ISO date")),
    };
    Ok((
        year.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        month
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        day.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
    ))
}

fn checked_date_object(year: i32, month: i32, day: i32, calendar: &str) -> Result<Value, VmError> {
    let year = f64::from(year);
    let month = f64::from(month);
    let day = f64::from(day);
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    crate::temporal::plain_date::construct_from_iso(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
        Value::String(calendar.to_string()),
    ])
}
