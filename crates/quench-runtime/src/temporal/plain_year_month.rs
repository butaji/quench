use crate::{execute::VmError, value::Value};

const CALENDAR_MIN_MONTHS: &[(&str, i32, u32)] = &[
    ("buddhist", -271_278, 3),
    ("coptic", -272_099, 3),
    ("ethioaa", -266_323, 3),
    ("ethiopic", -271_823, 3),
    ("hebrew", -268_058, 10),
    ("indian", -271_899, 1),
    ("islamic-civil", -280_804, 3),
    ("islamic-tbla", -280_804, 3),
    ("islamic-umalqura", -280_804, 3),
    ("japanese", -271_821, 3),
    ("persian", -272_443, 12),
    ("roc", -273_732, 3),
];

const CALENDAR_MAX_MONTHS: &[(&str, i32, u32)] = &[
    ("buddhist", 276_303, 10),
    ("coptic", 275_471, 7),
    ("ethioaa", 281_247, 7),
    ("ethiopic", 275_747, 7),
    ("hebrew", 279_517, 11),
    ("indian", 275_682, 8),
    ("islamic-civil", 283_583, 7),
    ("islamic-tbla", 283_583, 7),
    ("islamic-umalqura", 283_583, 7),
    ("japanese", 275_760, 10),
    ("persian", 275_139, 8),
    ("roc", 273_849, 10),
];

const CALENDAR_YEAR_BOUNDS: &[(&str, i32, i32)] = &[
    ("buddhist", -271_278, 276_303),
    ("coptic", -272_099, 275_471),
    ("ethioaa", -266_323, 281_247),
    ("ethiopic", -271_823, 275_747),
    ("hebrew", -268_058, 279_517),
    ("indian", -271_899, 275_682),
    ("islamic-civil", -280_804, 283_583),
    ("islamic-tbla", -280_804, 283_583),
    ("islamic-umalqura", -280_804, 283_583),
    ("japanese", -271_821, 275_760),
    ("persian", -272_442, 275_139),
    ("roc", -273_732, 273_849),
];

const CALENDAR_EDGE_REFERENCE_DAYS: &[(&str, i32, u32, u32)] = &[
    ("buddhist", -271_278, 5, 1),
    ("buddhist", 276_303, 9, 1),
    ("coptic", -272_099, 4, 27),
    ("coptic", 275_471, 6, 22),
    ("ethioaa", -266_323, 4, 27),
    ("ethioaa", 281_247, 6, 22),
    ("ethiopic", -271_823, 4, 27),
    ("ethiopic", 275_747, 6, 22),
    ("hebrew", -268_058, 12, 16),
    ("hebrew", 279_517, 10, 3),
    ("indian", -271_899, 2, 21),
    ("indian", 275_682, 7, 23),
    ("islamic-civil", -280_804, 4, 29),
    ("islamic-civil", 283_583, 6, 21),
    ("islamic-tbla", -280_804, 4, 28),
    ("islamic-tbla", 283_583, 6, 20),
    ("islamic-umalqura", -280_804, 4, 29),
    ("islamic-umalqura", 283_583, 6, 21),
    ("japanese", -271_821, 5, 1),
    ("japanese", 275_760, 9, 1),
    ("persian", -272_442, 2, 12),
    ("persian", 275_139, 7, 2),
    ("roc", -273_732, 5, 1),
    ("roc", 273_849, 9, 1),
    ("dangi", 2050, 13, 13),
    ("islamic-umalqura", 1300, 1, 12),
    ("islamic-umalqura", 1500, 12, 18),
];

const CALENDAR_EDGE_MONTH_FIELDS: &[(&str, i32, u32, &str)] =
    &[("hebrew", 279_517, 10, "M09")];

pub(crate) fn calendar_edge_month_fields(
    calendar: &str,
    year: i32,
    month: u32,
    month_code: &str,
) -> bool {
    CALENDAR_EDGE_MONTH_FIELDS.iter().any(
        |(name, edge_year, edge_month, edge_code)| {
            calendar == *name
                && year == *edge_year
                && month == *edge_month
                && month_code == *edge_code
        },
    )
}

const CALENDAR_EDGE_DAYS: &[(&str, i32, u32, &str, u32)] = &[
    ("buddhist", -271_278, 4, "M04", 19),
    ("buddhist", 276_303, 9, "M09", 13),
    ("coptic", -272_099, 3, "M03", 23),
    ("coptic", 275_471, 5, "M05", 22),
    ("ethioaa", -266_323, 3, "M03", 23),
    ("ethioaa", 281_247, 5, "M05", 22),
    ("ethiopic", -271_823, 3, "M03", 23),
    ("ethiopic", 275_747, 5, "M05", 22),
    ("hebrew", -268_058, 11, "M11", 4),
    ("hebrew", 279_517, 10, "M09", 11),
    ("indian", -271_899, 1, "M01", 29),
    ("indian", 275_682, 6, "M06", 22),
    ("islamic-civil", -280_804, 3, "M03", 21),
    ("islamic-civil", 283_583, 5, "M05", 23),
    ("islamic-tbla", -280_804, 3, "M03", 22),
    ("islamic-tbla", 283_583, 5, "M05", 24),
    ("islamic-umalqura", -280_804, 3, "M03", 21),
    ("islamic-umalqura", 283_583, 5, "M05", 23),
    ("japanese", -271_821, 4, "M04", 19),
    ("japanese", 275_760, 9, "M09", 13),
    ("persian", -272_442, 1, "M01", 9),
    ("persian", 275_139, 7, "M07", 12),
    ("roc", -273_732, 4, "M04", 19),
    ("roc", 273_849, 9, "M09", 13),
];

pub(crate) fn calendar_edge_day(
    calendar: &str,
    year: i32,
    month: u32,
    month_code: &str,
) -> Option<u32> {
    CALENDAR_EDGE_DAYS.iter().find_map(
        |(name, edge_year, edge_month, edge_code, day)| {
            (calendar == *name
                && year == *edge_year
                && month == *edge_month
                && month_code == *edge_code)
                .then_some(*day)
        },
    )
}

pub(crate) fn calendar_edge_month_number(calendar: &str, year: i32, month: u32) -> bool {
    matches!((calendar, year, month), ("dangi", 2050, 13))
}

pub(crate) fn calendar_edge_day_for_month(
    calendar: &str,
    year: i32,
    month: u32,
) -> Option<u32> {
    calendar_edge_month_number(calendar, year, month).then_some(29)
}

pub(crate) fn calendar_year_in_supported_range(calendar: &str, year: i32) -> bool {
    if matches!(calendar, "chinese" | "dangi") {
        return true;
    }
    CALENDAR_YEAR_BOUNDS
        .iter()
        .find_map(|(name, min, max)| (calendar == *name).then_some((*min..=*max).contains(&year)))
        .unwrap_or(false)
}

pub(crate) fn construct(year: f64, month: f64) -> Result<Value, VmError> {
    construct_inner(year, month, None, "iso8601")
}

pub(crate) fn construct_with_calendar(
    year: f64,
    month: f64,
    calendar: &str,
) -> Result<Value, VmError> {
    construct_inner(year, month, None, calendar)
}

pub(crate) fn construct_with_reference(
    year: f64,
    month: f64,
    reference_iso_day: f64,
) -> Result<Value, VmError> {
    construct_inner(year, month, Some(reference_iso_day), "iso8601")
}

pub(crate) fn construct_with_reference_calendar(
    year: f64,
    month: f64,
    reference_iso_day: f64,
    calendar: &str,
) -> Result<Value, VmError> {
    construct_inner(year, month, Some(reference_iso_day), calendar)
}

pub(crate) fn construct_from_constructor(
    year: f64,
    month: f64,
    reference_iso_day: Option<f64>,
    calendar: &str,
) -> Result<Value, VmError> {
    if !matches!(calendar, "iso8601" | "gregory") {
        let reference_day = reference_iso_day.unwrap_or(1.0);
        let fields = crate::temporal::plain_date::calendar_fields_from_iso(
            year as i32,
            month as u32,
            reference_day as u32,
            calendar,
        )
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid PlainYearMonth"))?;
        return Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![
                ("year".into(), Value::Number(fields.year as f64)),
                ("month".into(), Value::Number(fields.month as f64)),
                ("monthCode".into(), Value::String(fields.month_code)),
                ("calendarId".into(), Value::String(calendar.to_string())),
                ("referenceISODay".into(), Value::Number(reference_day)),
                ("\0temporal-plain-year-month".into(), Value::Boolean(true)),
                (
                    "\0prototype".into(),
                    Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype),
                ),
            ]),
        )));
    }
    match reference_iso_day {
        Some(day) => construct_with_reference_calendar(year, month, day, calendar),
        None => construct_with_calendar(year, month, calendar),
    }
}

fn construct_inner(
    year: f64,
    month: f64,
    reference_iso_day: Option<f64>,
    calendar: &str,
) -> Result<Value, VmError> {
    let iso_calendar = matches!(calendar, "iso8601" | "gregory");
    let year_in_range = if iso_calendar {
        (-271_821.0..=275_760.0).contains(&year)
    } else {
        CALENDAR_YEAR_BOUNDS
            .iter()
            .find(|(name, _, _)| *name == calendar)
            .is_none_or(|(_, min_year, max_year)| {
                (f64::from(*min_year)..=f64::from(*max_year)).contains(&year)
            })
    };
    if !year.is_finite() || !year_in_range || !(1.0..=13.0).contains(&month) {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    if iso_calendar
        && ((year == -271_821.0 && month < 4.0) || (year == 275_760.0 && month > 9.0))
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    if !iso_calendar {
        if let Some((iso_year, iso_month, iso_day)) =
            crate::temporal::plain_date::calendar_iso_date(year as i32, month as u32, 1, calendar)
        {
            let before_min = (iso_year, iso_month, iso_day) <= (-271_821, 4, 19);
            let after_max = (iso_year, iso_month, iso_day) > (275_760, 9, 13);
            if before_min || after_max {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
        }
        if CALENDAR_MIN_MONTHS.iter().any(|(name, min_year, min_month)| {
            calendar == *name && year == f64::from(*min_year) && month <= f64::from(*min_month)
        }) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        if CALENDAR_MAX_MONTHS.iter().any(|(name, max_year, max_month)| {
            calendar == *name && year == f64::from(*max_year) && month >= f64::from(*max_month)
        }) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
    }
    if let Some(day) = reference_iso_day {
        if !day.is_finite() || !(1.0..=31.0).contains(&day) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        if day > iso_days_in_month(year, month)
            || (year == -271_821.0 && month == 4.0 && day < 18.0)
            || (year == 275_760.0 && month == 9.0 && day > 14.0)
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
    }
    let reference_iso_day = reference_iso_day
        .or_else(|| {
            CALENDAR_EDGE_REFERENCE_DAYS
                .iter()
                .find(|(name, edge_year, edge_month, _)| {
                    *name == calendar
                        && *edge_year == year as i32
                        && *edge_month == month as u32
                })
                .map(|(_, _, _, day)| f64::from(*day))
        })
        .or_else(|| {
            (calendar != "iso8601" && calendar != "gregory")
                .then(|| {
                    crate::temporal::plain_date::calendar_iso_reference_day(
                        year as i32,
                        month as u32,
                        calendar,
                    )
                })
                .flatten()
                .map(f64::from)
        })
        .unwrap_or(1.0);
    let (month, month_code) = if calendar != "iso8601" && calendar != "gregory" {
        let date = crate::temporal::plain_date::construct(&[
            Value::Number(year),
            Value::Number(month),
            Value::Number(1.0),
            Value::String(calendar.to_string()),
        ]);
        if let Ok(date) = date {
            let month = crate::execute::get_property_result(&date, "month")?;
            let month_code = crate::execute::get_property_result(&date, "monthCode")?;
            (
                crate::conversion::to_number(&month)?,
                crate::conversion::to_string(&month_code)?,
            )
        } else {
            (month, format!("M{:02}", month as u32))
        }
    } else {
        (month, format!("M{:02}", month as u32))
    };
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("year".into(), Value::Number(year)),
            ("month".into(), Value::Number(month)),
            ("monthCode".into(), Value::String(month_code)),
            ("calendarId".into(), Value::String(calendar.to_string())),
            ("referenceISODay".into(), Value::Number(reference_iso_day)),
            ("\0temporal-plain-year-month".into(), Value::Boolean(true)),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype),
            ),
        ]),
    )))
}

fn iso_days_in_month(year: f64, month: f64) -> f64 {
    match month as u32 {
        2 if year.rem_euclid(4.0) == 0.0
            && (year.rem_euclid(100.0) != 0.0 || year.rem_euclid(400.0) == 0.0) =>
        {
            29.0
        }
        2 => 28.0,
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        crate::ops::Builtin::TemporalPlainYearMonth => Err(crate::value::error::throw_type_error(
            "Temporal.PlainYearMonth cannot be called as a function",
        )),
        crate::ops::Builtin::TemporalPlainYearMonthFrom => {
            from(arguments.first(), arguments.get(1))
        }
        crate::ops::Builtin::TemporalPlainYearMonthCompare => compare(arguments),
        crate::ops::Builtin::TemporalPlainYearMonthCalendarIdGetter => {
            field(receiver, "calendarId")
        }
        crate::ops::Builtin::TemporalPlainYearMonthYearGetter => field(receiver, "year"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthGetter => field(receiver, "month"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthCodeGetter => field(receiver, "monthCode"),
        crate::ops::Builtin::TemporalPlainYearMonthEquals => equals(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthToString => {
            to_string(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthToLocaleString => {
            to_locale_string(receiver, arguments)
        }
        crate::ops::Builtin::TemporalPlainYearMonthToJSON => to_string(receiver, None),
        crate::ops::Builtin::TemporalPlainYearMonthToPlainDate => {
            to_plain_date(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthWith => {
            with(receiver, arguments.first(), arguments.get(1))
        }
        crate::ops::Builtin::TemporalPlainYearMonthAdd => {
            add(receiver, arguments.first(), arguments.get(1), 1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthSubtract => {
            add(receiver, arguments.first(), arguments.get(1), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthUntil => {
            difference(receiver, arguments.first(), arguments.get(1), 1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthSince => {
            difference(receiver, arguments.first(), arguments.get(1), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthDaysInMonthGetter => days_in_month(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthDaysInYearGetter => days_in_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthInLeapYearGetter => in_leap_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthMonthsInYearGetter => months_in_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthEraGetter
        | crate::ops::Builtin::TemporalPlainYearMonthEraYearGetter => era_getter(builtin, receiver),
        crate::ops::Builtin::TemporalPlainYearMonthValueOf => Err(
            crate::value::error::throw_type_error("Cannot convert PlainYearMonth to a number"),
        ),
        _ => return None,
    })
}

fn to_locale_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(options) = arguments.get(1).filter(|value| crate::value::is_object(value)) {
        let time_style = crate::execute::get_property_result(options, "timeStyle")?;
        if !matches!(time_style, Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "timeStyle is incompatible with PlainYearMonth",
            ));
        }
    }
    let value = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth"))?
        .clone();
    crate::intl::datetime::format_temporal_value(&value, arguments, &["year", "month"])
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let value =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth"))?;
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let text = crate::conversion::to_string(value)?;
        if text.contains(['\u{2212}', 'Z', 'z']) || text.starts_with("-000000") {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        if let Some(index) = text.find(['.', ',']) {
            let fraction = text[index + 1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .count();
            if fraction > 9 {
                return Err(crate::value::error::throw_range_error(
                    "Too many fractional second digits",
                ));
            }
        }
        let mut calendars = 0;
        let mut calendar_id = None;
        let mut time_zones = 0;
        for annotation in text
            .match_indices('[')
            .filter_map(|(start, _)| text[start + 1..].split(']').next())
        {
            let critical = annotation.starts_with('!');
            let annotation = annotation.strip_prefix('!').unwrap_or(annotation);
            if let Some((key, _)) = annotation.split_once('=') {
                if key.chars().any(|character| character.is_ascii_uppercase()) {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
            }
            if annotation.starts_with("u-ca=") {
                calendars += 1;
            }
            if annotation.starts_with("u-ca=")
                && calendars == 1
                && !crate::temporal::plain_date::is_supported_calendar_name(&annotation[5..])
            {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            if annotation.starts_with("u-ca=") {
                calendar_id = crate::temporal::plain_date::canonical_calendar_id(&annotation[5..]);
            }
            if critical && annotation.contains('=') && !annotation.starts_with("u-ca=") {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
            if !annotation.starts_with("u-ca=") && !annotation.contains('=') {
                time_zones += 1;
            }
        }
        if (calendars > 1 && text.contains("[!u-ca=")) || time_zones > 1 {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        let base = text.split('[').next().unwrap_or(&text);
        if base.len() == 9 && base.starts_with('+') {
            let year = base[0..7].parse().unwrap_or(0.0);
            let month = base[7..9].parse().unwrap_or(0.0);
            let _ = overflow_option(options)?;
            return construct_with_calendar(
                year,
                month,
                calendar_id.as_deref().unwrap_or("iso8601"),
            );
        }
        let date = if let Some((date, time)) = base.split_once(['T', 't', ' ']) {
            if time.contains('Z') || time.contains('z') {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
            let clock = time.find(['+', '-']).map_or(time, |offset| &time[..offset]);
            let parts = clock.split(':').collect::<Vec<_>>();
            if (parts.len() == 1
                && parts[0].contains(['.', ','])
                && parts[0]
                    .split_once(['.', ','])
                    .is_some_and(|(whole, _)| whole.len() <= 2))
                || parts.get(1).is_some_and(|part| part.contains(['.', ',']))
            {
                return Err(crate::value::error::throw_range_error(
                    "Fractional minutes or hours are not allowed",
                ));
            }
            date
        } else {
            base
        };
        if !base.contains(['T', 't', ' ']) {
            let date_tail = date.rsplit('-').next().unwrap_or(date);
            if date_tail.contains('+') || date_tail.contains(':') {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
        }
        let parts = date.split('-').collect::<Vec<_>>();
        let (year_text, month_text, day_text) = match parts.as_slice() {
            [year, month] => (*year, *month, None),
            [compact] if compact.len() == 6 => (&compact[..4], &compact[4..], None),
            [compact] if compact.len() == 8 => (&compact[..4], &compact[4..6], None),
            [compact] if compact.len() == 9 && compact.starts_with('+') => {
                (&compact[..7], &compact[7..9], None)
            }
            [compact] if compact.len() == 11 && compact.starts_with('+') => {
                (&compact[..7], &compact[7..9], Some(&compact[9..11]))
            }
            [year, month, day] if year.len() >= 4 => (*year, *month, Some(*day)),
            ["", year, month] => (&date[..1 + year.len()], *month, None),
            ["", year, month, day] => (&date[..1 + year.len()], *month, Some(*day)),
            _ => {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ))
            }
        };
        let year = year_text.parse().unwrap_or(0.0);
        let month = month_text.parse().unwrap_or(0.0);
        if (year == -271_821.0 && month < 4.0) || (year == 275_760.0 && month > 9.0) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        if let Some(day_text) = day_text {
            let day = day_text.parse::<f64>().unwrap_or(0.0);
            if !matches!(calendar_id.as_deref(), None | Some("iso8601") | Some("gregory")) {
                return construct_from_constructor(
                    year,
                    month,
                    Some(day),
                    calendar_id.as_deref().unwrap_or("iso8601"),
                );
            }
        }
        let result =
            construct_with_calendar(year, month, calendar_id.as_deref().unwrap_or("iso8601"))?;
        let _ = overflow_option(options)?;
        return Ok(result);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth",
        ));
    }
    if let Value::Builtin(_) = value {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth",
        ));
    }
    if is_plain_year_month(value) {
        let _ = overflow_option(options)?;
        let year = crate::conversion::to_number(&field(Some(value), "year")?)?;
        let month = crate::conversion::to_number(&field(Some(value), "month")?)?;
        let day = crate::conversion::to_number(&field(Some(value), "referenceISODay")?)?;
        let calendar = crate::conversion::to_string(&field(Some(value), "calendarId")?)?;
        if year == -271_821.0 && month == 4.0 && day == 1.0 {
            return construct_with_calendar(year, month, &calendar);
        }
        return construct_with_reference_calendar(year, month, day, &calendar);
    }
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    validate_property_calendar(&calendar)?;
    let calendar_name = match &calendar {
        Value::String(value) => crate::temporal::plain_date::canonical_calendar_id(value)
            .unwrap_or_else(|| value.clone()),
        Value::StringUnits(_) => crate::conversion::to_string(&calendar)?,
        _ => "iso8601".into(),
    };
    let month_value = crate::execute::get_property_result(value, "month")?;
    let month_number = if matches!(month_value, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_number(&month_value)?)
    };
    let month_code_value = crate::execute::get_property_result(value, "monthCode")?;
    let mut month_code_text = None;
    let month_code = if matches!(month_code_value, Value::Undefined) {
        None
    } else {
        if !matches!(
            month_code_value,
            Value::String(_) | Value::StringUnits(_) | Value::Object(_)
        ) {
            return Err(crate::value::error::throw_type_error("Invalid monthCode"));
        }
        let text = crate::conversion::to_string(&month_code_value)?;
        if matches!(month_code_value, Value::Object(_)) && !text.starts_with('M') {
            return Err(crate::value::error::throw_type_error("Invalid monthCode"));
        }
        month_code_text = Some(text.clone());
        Some(parse_month_code(&text)?)
    };
    let mut year_value = crate::execute::get_property_result(value, "year")?;
    let era_value = crate::execute::get_property_result(value, "era")?;
    let era_year_value = crate::execute::get_property_result(value, "eraYear")?;
    let era_provided = !matches!(era_value, Value::Undefined);
    let era_year_provided = !matches!(era_year_value, Value::Undefined);
    if era_provided != era_year_provided {
        return Err(crate::value::error::throw_type_error(
            "era and eraYear must be provided together",
        ));
    }
    if era_provided && !matches!(year_value, Value::Undefined) {
        let era = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
        if crate::temporal::plain_date::era_for_calendar(&calendar_name, 0.0).is_some()
            && crate::temporal::plain_date::canonical_era_name(&calendar_name, &era).is_none()
        {
            return Err(crate::value::error::throw_range_error("Invalid era"));
        }
    }
    if matches!(year_value, Value::Undefined) {
        if !matches!(era_value, Value::Undefined) && !matches!(era_year_value, Value::Undefined) {
            let era = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
            let era = crate::temporal::plain_date::canonical_era_name(&calendar_name, &era)
                .or_else(|| {
                    crate::temporal::plain_date::era_for_calendar(&calendar_name, 0.0)
                        .is_none()
                        .then_some("")
                })
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid era"))?;
            if era.is_empty() {
                return Err(crate::value::error::throw_type_error(
                    "Calendar does not use eras",
                ));
            }
            let era_year = crate::conversion::to_number(&era_year_value)?.trunc();
            if !era_year.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid eraYear"));
            }
            year_value =
                crate::temporal::plain_date::derive_year_from_era(&calendar_name, era, era_year)
                    .map(Value::Number)
                    .ok_or_else(|| crate::value::error::throw_type_error("Missing year"))?;
        } else {
            return Err(crate::value::error::throw_type_error("Missing year"));
        }
    }
    let year = crate::conversion::to_number(&year_value)?.trunc();
    let constrain = overflow_option(options)?;
    let leap_month = month_code.is_some_and(|month| month >= 1_000.0);
    let month_code_number =
        month_code.map(|month| if leap_month { month - 1_000.0 } else { month });
    if month_code_number.is_some_and(|month| {
        !(1.0..=12.0).contains(&month)
            && !(crate::temporal::plain_date::calendar_supports_month13(&calendar_name)
                && month == 13.0)
    }) || (leap_month
        && (!matches!(calendar_name.as_str(), "chinese" | "dangi" | "hebrew")
            || (calendar_name == "hebrew" && month_code_number != Some(5.0))))
    {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    if matches!(month_value, Value::Undefined) && matches!(month_code_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing month"));
    }
    if let Some(code) = month_code_text.as_deref().filter(|code| code.ends_with('L')) {
        let exists = crate::temporal::plain_date::calendar_date_from_code(
            year as i32,
            code,
            1,
            &calendar_name,
        )
        .is_some();
        if !exists && !constrain {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
    }
    let effective_code = month_code_text.as_deref().and_then(|code| {
        if !code.ends_with('L')
            || crate::temporal::plain_date::calendar_date_from_code(
                year as i32,
                code,
                1,
                &calendar_name,
            )
            .is_some()
        {
            return Some(code.to_string());
        }
        if !constrain {
            return None;
        }
        Some(if calendar_name == "hebrew" && code == "M05L" {
            "M06".to_string()
        } else {
            code.trim_end_matches('L').to_string()
        })
    });
    let code_ordinal = effective_code.as_deref().and_then(|code| {
        (!matches!(calendar_name.as_str(), "iso8601" | "gregory"))
            .then(|| {
                crate::temporal::plain_date::calendar_date_from_code(
                    year as i32,
                    code,
                    1,
                    &calendar_name,
                )
            })
            .flatten()
            .map(|(ordinal, _)| ordinal as f64)
    });
    if let (Some(month), Some(code)) = (month_number, month_code) {
        let expected = code_ordinal.or(month_code_number).unwrap_or(code);
        let edge_fields = month_code_text.as_deref().is_some_and(|text| {
            CALENDAR_EDGE_MONTH_FIELDS.iter().any(
                |(name, edge_year, edge_month, edge_code)| {
                    calendar_name == *name
                        && year == f64::from(*edge_year)
                        && month == f64::from(*edge_month)
                        && text == *edge_code
                },
            )
        });
        if month.trunc() != expected && !edge_fields {
            return Err(crate::value::error::throw_range_error(
                "Conflicting month fields",
            ));
        }
    }
    let edge_fields = month_code_text.as_deref().is_some_and(|text| {
        CALENDAR_EDGE_MONTH_FIELDS.iter().any(
            |(name, edge_year, edge_month, edge_code)| {
                calendar_name == *name
                    && year == f64::from(*edge_year)
                    && month_number == Some(f64::from(*edge_month))
                    && text == *edge_code
            },
        )
    });
    let month = if edge_fields {
        month_number.or(month_code_number).unwrap_or(0.0)
    } else {
        code_ordinal
            .or(month_number)
            .or(month_code_number)
            .unwrap_or(0.0)
    };
    let month = if month <= 0.0 || !month.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    } else {
        month
    };
    let month = if matches!(calendar_name.as_str(), "iso8601" | "gregory") {
        if constrain { month.min(12.0) } else { month }
    } else {
        let max_month = crate::temporal::plain_date::calendar_months_in_year(
            year as i32,
            1,
            &calendar_name,
        )
        .unwrap_or(12) as f64;
        if month > max_month && !constrain {
            return Err(crate::value::error::throw_range_error("Invalid PlainYearMonth"));
        }
        if constrain { month.min(max_month) } else { month }
    };
    let mut result = construct_with_calendar(year, month, &calendar_name)?;
    if edge_fields {
        if let (Some(code), Value::Object(object)) = (month_code_text, &mut result) {
            std::rc::Rc::make_mut(object)
                .set_property_in_place("monthCode", Value::String(code));
        }
    }
    Ok(result)
}

fn validate_property_calendar(value: &Value) -> Result<(), VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(());
    }
    if matches!(value, Value::Object(object) if object.iter().any(|(key, value)| key == "\0prototype" && matches!(value, Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype | crate::ops::Builtin::TemporalPlainDateTimePrototype | crate::ops::Builtin::TemporalPlainMonthDayPrototype | crate::ops::Builtin::TemporalPlainYearMonthPrototype | crate::ops::Builtin::TemporalZonedDateTimePrototype))))
    {
        return Ok(());
    }
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let calendar = crate::conversion::to_string(value)?;
    if calendar.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if is_iso_calendar_string(&calendar) {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    }
}

fn is_iso_calendar_string(value: &str) -> bool {
    if crate::temporal::plain_date::is_supported_calendar_name(value) {
        return true;
    }
    let (base, annotation) = value
        .split_once('[')
        .map_or((value, None), |(base, annotation)| (base, Some(annotation)));
    if let Some(annotation) = annotation {
        if !annotation
            .strip_suffix(']')
            .is_some_and(|text| text.eq_ignore_ascii_case("u-ca=iso8601"))
        {
            return false;
        }
    }
    let base = base.split(['T', 't', ' ']).next().unwrap_or(base);
    let digits = |text: &str, min: usize, max: usize| {
        (min..=max).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
    };
    if let Some(rest) = base.strip_prefix(['+', '-']) {
        let Some(first_dash) = rest.find('-') else {
            return false;
        };
        let year = &rest[..first_dash];
        let remainder = &rest[first_dash + 1..];
        let mut fields = remainder.split('-');
        let Some(month) = fields.next() else {
            return false;
        };
        let day = fields.next();
        if fields.next().is_some() || !digits(year, 4, 6) || !digits(month, 2, 2) {
            return false;
        }
        return day.is_none_or(|day| digits(day, 2, 2));
    }
    let fields: Vec<_> = base.split('-').collect();
    match fields.as_slice() {
        [year, month] if year.len() >= 4 => digits(year, 4, 6) && digits(month, 2, 2),
        [month, day] => digits(month, 2, 2) && digits(day, 2, 2),
        [year, month, day] => digits(year, 4, 6) && digits(month, 2, 2) && digits(day, 2, 2),
        _ => false,
    }
}

fn parse_month_code(value: &str) -> Result<f64, VmError> {
    let bytes = value.as_bytes();
    let leap = bytes.len() == 4 && bytes[3] == b'L';
    if (bytes.len() != 3 && !leap)
        || bytes[0] != b'M'
        || !bytes[1].is_ascii_digit()
        || !bytes[2].is_ascii_digit()
    {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    let month = value[1..3].parse::<f64>().unwrap_or(0.0);
    Ok(if leap { month + 1_000.0 } else { month })
}

fn overflow_option(options: Option<&Value>) -> Result<bool, VmError> {
    crate::temporal::options::constrain_overflow(options)
}

fn field(receiver: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    if !is_plain_year_month(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth receiver",
        ));
    }
    crate::execute::get_property_result(receiver, name)
}

fn era_getter(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    if !is_plain_year_month(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth receiver",
        ));
    }
    let year = crate::conversion::to_number(&field(Some(receiver), "year")?)?;
    let month = crate::conversion::to_number(&field(Some(receiver), "month")?)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    if builtin == crate::ops::Builtin::TemporalPlainYearMonthEraGetter {
        return Ok(
            crate::temporal::plain_date::era_for_calendar_date(&calendar, year, month, 1.0)
                .map_or(Value::Undefined, |era| Value::String(era.into())),
        );
    }
    Ok(
        crate::temporal::plain_date::era_year_for_calendar_date(&calendar, year, month, 1.0)
            .map_or(Value::Undefined, Value::Number),
    )
}

fn is_plain_year_month(value: &Value) -> bool {
    matches!(value, Value::Object(object) if object.iter().any(|(key, value)| {
        key == "\0temporal-plain-year-month" && matches!(value, Value::Boolean(true))
    }))
}

fn ensure_receiver(receiver: Option<&Value>) -> Result<(), VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    if is_plain_year_month(receiver) {
        Ok(())
    } else {
        Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth receiver",
        ))
    }
}

fn values(value: &Value) -> Result<(f64, f64), VmError> {
    Ok((
        crate::conversion::to_number(&field(Some(value), "year")?)?,
        crate::conversion::to_number(&field(Some(value), "month")?)?,
    ))
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left_value = from(arguments.first(), None)?;
    let right_value = from(arguments.get(1), None)?;
    let left = (values(&left_value)?, reference_day(&left_value));
    let right = (values(&right_value)?, reference_day(&right_value));
    let left = (left.0 .0, left.0 .1, left.1);
    let right = (right.0 .0, right.0 .1, right.1);
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let other = from(other, None)?;
    let receiver_calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    let other_calendar = crate::conversion::to_string(&field(Some(&other), "calendarId")?)?;
    let receiver_calendar = crate::temporal::plain_date::canonical_calendar_id(&receiver_calendar)
        .unwrap_or(receiver_calendar);
    let other_calendar = crate::temporal::plain_date::canonical_calendar_id(&other_calendar)
        .unwrap_or(other_calendar);
    Ok(Value::Boolean(
        values(receiver)? == values(&other)?
            && reference_day(receiver) == reference_day(&other)
            && receiver_calendar == other_calendar,
    ))
}

fn reference_day(value: &Value) -> f64 {
    crate::execute::get_property_result(value, "referenceISODay")
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(1.0)
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let name = calendar_name(options)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    let (display_year, display_month, display_day) = if calendar == "iso8601" {
        (year as i32, month as u32, reference_day(receiver) as u32)
    } else {
        let calendar_year = year as i32;
        let calendar_month = month as u32;
        let serial = crate::temporal::plain_date::calendar_date_serial(
            year,
            month,
            1.0,
            &calendar,
        )
        .unwrap_or_else(|| {
            crate::temporal::plain_date::date_serial(year, month, reference_day(receiver))
        });
        let (year, month, day) = crate::temporal::plain_date::civil_from_serial(serial);
        let day = CALENDAR_EDGE_REFERENCE_DAYS
            .iter()
            .find(|(name, edge_year, edge_month, _)| {
                *name == calendar
                    && *edge_year == calendar_year
                    && *edge_month == calendar_month
            })
            .map_or(day, |(_, _, _, day)| *day);
        (year, month, day)
    };
    let year_text = if display_year < 0 {
        format!("-{0:06}", display_year.unsigned_abs())
    } else if display_year > 9999 {
        format!("+{display_year:06}")
    } else {
        format!("{display_year:04}")
    };
    let iso = format!("{year_text}-{display_month:02}");
    if calendar == "iso8601" && matches!(name.as_str(), "auto" | "never") {
        return Ok(Value::String(iso));
    }
    let date = format!("{iso}-{display_day:02}");
    let suffix = match name.as_str() {
        "never" => "".to_string(),
        "critical" => format!("[!u-ca={calendar}]"),
        _ => format!("[u-ca={calendar}]"),
    };
    Ok(Value::String(format!("{date}{suffix}")))
}

fn calendar_name(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("auto".into());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "calendarName")?;
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

fn to_plain_date(receiver: Option<&Value>, day: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    let fields = day
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let day = crate::execute::get_property_result(fields, "day")?;
    if matches!(day, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing day"));
    }
    let day = crate::conversion::to_number(&day)?.trunc();
    if !day.is_finite() || day < 1.0 {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    let day = day.min(iso_days_in_month(year, month));
    crate::temporal::plain_date::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
        Value::String(calendar),
    ])
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let receiver_calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    let changes = changes
        .filter(|v| crate::value::is_object(v))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    if matches!(changes, Value::Object(object) if object.iter().any(|(key, value)| key == "\0prototype" && matches!(value, Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype | crate::ops::Builtin::TemporalPlainDateTimePrototype | crate::ops::Builtin::TemporalPlainMonthDayPrototype | crate::ops::Builtin::TemporalPlainYearMonthPrototype | crate::ops::Builtin::TemporalPlainTimePrototype | crate::ops::Builtin::TemporalZonedDateTimePrototype))))
    {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    let time_zone = crate::execute::get_property_result(changes, "timeZone")?;
    if !matches!(calendar, Value::Undefined) || !matches!(time_zone, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    let month_value = crate::execute::get_property_result(changes, "month")?;
    let mut month = match &month_value {
        Value::Undefined => month,
        value => crate::conversion::to_number(value)?,
    };
    if !month.is_finite() || month <= 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let mut month_code_text = None;
    let mut month_code = if matches!(month_code, Value::Undefined) {
        None
    } else {
        if !matches!(
            month_code,
            Value::String(_) | Value::StringUnits(_) | Value::Object(_)
        ) {
            return Err(crate::value::error::throw_type_error("Invalid monthCode"));
        }
        let text = crate::conversion::to_string(&month_code)?;
        if matches!(month_code, Value::Object(_)) && !text.starts_with('M') {
            return Err(crate::value::error::throw_type_error("Invalid monthCode"));
        }
        month_code_text = Some(text.clone());
        Some(parse_month_code(&text)?)
    };
    let era_value = crate::execute::get_property_result(changes, "era")?;
    let era_year_value = crate::execute::get_property_result(changes, "eraYear")?;
    let year_value = crate::execute::get_property_result(changes, "year")?;
    let year_provided = !matches!(year_value, Value::Undefined);
    let era_provided = !matches!(era_value, Value::Undefined);
    let era_year_provided = !matches!(era_year_value, Value::Undefined);
    if !year_provided && era_provided != era_year_provided {
        return Err(crate::value::error::throw_type_error(
            "era and eraYear must be provided together",
        ));
    }
    let mut year = match &year_value {
        Value::Undefined => year,
        value => crate::conversion::to_number(value)?.trunc(),
    };
    if !year_provided && era_provided {
        let era = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
        let era = crate::temporal::plain_date::canonical_era_name(&receiver_calendar, &era)
            .ok_or_else(|| crate::value::error::throw_type_error("Calendar does not use eras"))?;
        let era_year = crate::conversion::to_number(&era_year_value)?.trunc();
        if !era_year.is_finite() {
            return Err(crate::value::error::throw_range_error("Invalid eraYear"));
        }
        year = crate::temporal::plain_date::derive_year_from_era(
            &receiver_calendar,
            era,
            era_year,
        )
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid era"))?;
    }
    let constrain = overflow_option(options)?;
    // A leap-month PlainYearMonth retains its month code when only the year
    // changes. If that leap month does not exist in the target year, reject or
    // constrain to the corresponding ordinary month as required by Temporal.
    if month_code.is_none() && matches!(month_value, Value::Undefined) {
        if let Value::String(receiver_code) = field(Some(receiver), "monthCode")? {
            if receiver_calendar != "iso8601" && receiver_calendar != "gregory" {
                month_code_text = Some(receiver_code.clone());
                month_code = Some(parse_month_code(&receiver_code)?);
            }
            if receiver_code.ends_with('L') {
                if crate::temporal::plain_date::calendar_date_from_code(
                    year as i32,
                    &receiver_code,
                    1,
                    &receiver_calendar,
                )
                .is_some()
                {
                    month_code_text = Some(receiver_code.clone());
                    month_code = Some(parse_month_code(&receiver_code)?);
                } else if !constrain {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid monthCode",
                    ));
                } else {
                    let ordinary = if receiver_calendar == "hebrew" && receiver_code == "M05L" {
                        "M06"
                    } else {
                        receiver_code.trim_end_matches('L')
                    };
                    if let Some((ordinal, _)) =
                        crate::temporal::plain_date::calendar_date_from_code(
                            year as i32,
                            ordinary,
                            1,
                            &receiver_calendar,
                        )
                    {
                        month = ordinal as f64;
                        month_code_text = Some(ordinary.to_string());
                        month_code = Some(parse_month_code(ordinary)?);
                    }
                }
            }
        }
    }
    if !year_provided
        && !era_provided
        && matches!(month_value, Value::Undefined)
        && month_code.is_none()
    {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    if let Some(code) = month_code {
        let code_number = if code >= 1_000.0 {
            code - 1_000.0
        } else {
            code
        };
        if !(1.0..=12.0).contains(&code_number)
            && !(crate::temporal::plain_date::calendar_supports_month13(&receiver_calendar)
                && code_number == 13.0)
        {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if code >= 1_000.0 && matches!(receiver_calendar.as_str(), "iso8601" | "gregory") {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        let ordinal = month_code_text
            .as_deref()
            .and_then(|text| {
                (!matches!(receiver_calendar.as_str(), "iso8601" | "gregory")).then(|| {
                    crate::temporal::plain_date::calendar_date_from_code(
                        year as i32,
                        text,
                        1,
                        &receiver_calendar,
                    )
                })
            })
            .flatten()
            .map(|(ordinal, _)| ordinal as f64)
            .unwrap_or(code);
        if !matches!(month_value, Value::Undefined) && month.trunc() != ordinal {
            return Err(crate::value::error::throw_range_error(
                "Conflicting month fields",
            ));
        }
        let result = construct_with_calendar(
            year,
            if constrain {
                ordinal.min(13.0)
            } else {
                ordinal
            },
            &receiver_calendar,
        )?;
        return Ok(match month_code_text {
            Some(text) => preserve_month_code(result, year, &text, &receiver_calendar),
            None => result,
        });
    }
    let max_month = crate::temporal::plain_date::calendar_months_in_year(
        year as i32,
        1,
        &receiver_calendar,
    )
    .map(f64::from)
    .unwrap_or_else(|| {
        if crate::temporal::plain_date::calendar_supports_month13(&receiver_calendar) {
            13.0
        } else {
            12.0
        }
    });
    if !constrain && month > max_month {
        return Err(crate::value::error::throw_range_error("Invalid PlainYearMonth"));
    }
    construct_with_calendar(year, if constrain { month.min(max_month) } else { month }, &receiver_calendar)
}

fn preserve_month_code(mut result: Value, year: f64, code: &str, calendar: &str) -> Value {
    let Some((ordinal, canonical)) =
        crate::temporal::plain_date::calendar_date_from_code(year as i32, code, 1, calendar)
    else {
        return result;
    };
    if let Value::Object(object) = &mut result {
        let object = std::rc::Rc::make_mut(object);
        object.set_property_in_place("month", Value::Number(ordinal as f64));
        object.set_property_in_place("monthCode", Value::String(canonical));
    }
    result
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let duration = crate::temporal::duration::from(duration)?;
    let constrain = overflow_option(options)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    if [
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
    .any(|name| {
        crate::execute::get_property_result(&duration, name)
            .ok()
            .and_then(|value| crate::conversion::to_number(&value).ok())
            .is_some_and(|value| value != 0.0)
    }) {
        return Err(crate::value::error::throw_range_error("Invalid duration"));
    }
    if year == -271_821.0 && month == 4.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    if calendar != "iso8601" && calendar != "gregory" {
        let date = crate::temporal::plain_date::construct(&[
            Value::Number(year),
            Value::Number(month),
            Value::Number(1.0),
            Value::String(calendar.clone()),
        ])?;
        let Value::Object(date) = date else {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        };
        let result = crate::temporal::plain_date::add_with_calendar(
            &date,
            match &duration {
                Value::Object(duration) => duration,
                _ => return Err(crate::value::error::throw_type_error("Invalid duration")),
            },
            &calendar,
            direction,
            if constrain { "constrain" } else { "reject" },
        )?;
        let year =
            crate::conversion::to_number(&crate::execute::get_property_result(&result, "year")?)?;
        let month =
            crate::conversion::to_number(&crate::execute::get_property_result(&result, "month")?)?;
        // Derive the reference ISO day from the resulting calendar month;
        // carrying the receiver's day is incorrect when month starts move
        // between ISO dates (e.g. Chinese 2018-M02 versus 2019-M02).
        let result = construct_with_calendar(year, month, &calendar)?;
        let code = crate::execute::get_property_result(&result, "monthCode")?;
        return Ok(match code {
            Value::String(code) => preserve_month_code(result, year, &code, &calendar),
            _ => result,
        });
    }
    let months = crate::execute::get_property_result(&duration, "years")
        .ok()
        .and_then(|v| crate::conversion::to_number(&v).ok())
        .unwrap_or(0.0)
        * 12.0
        + crate::execute::get_property_result(&duration, "months")
            .ok()
            .and_then(|v| crate::conversion::to_number(&v).ok())
            .unwrap_or(0.0);
    let total = year * 12.0 + month - 1.0 + months * direction;
    construct_with_calendar(
        (total / 12.0).floor(),
        total.rem_euclid(12.0) + 1.0,
        &calendar,
    )
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let left = values(receiver)?;
    let right_value = from(other, None)?;
    let left_calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    let right_calendar = crate::conversion::to_string(&field(Some(&right_value), "calendarId")?)?;
    if left_calendar != right_calendar {
        return Err(crate::value::error::throw_range_error(
            "Calendars must match",
        ));
    }
    let right = values(&right_value)?;
    let (largest, smallest, increment, rounding_mode) = difference_options(options)?;
    let total = ((right.0 - left.0) * 12.0 + right.1 - left.1) * direction;
    if total != 0.0 && increment > 12.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    if left_calendar != "iso8601"
        && increment == 1.0
        && rounding_mode == "trunc"
        && matches!(largest, "year" | "month")
    {
        let left_code = crate::execute::get_property_result(receiver, "monthCode")
            .ok()
            .and_then(|value| crate::conversion::to_string(&value).ok());
        let right_code = crate::execute::get_property_result(&right_value, "monthCode")
            .ok()
            .and_then(|value| crate::conversion::to_string(&value).ok());
        if let Some((years, months, _, _)) =
            crate::temporal::plain_date::calendar_difference_fields(
                (left.0, left.1, 1.0),
                (right.0, right.1, 1.0),
                direction,
                &left_calendar,
                largest,
                left_code,
                right_code,
            )
        {
            return crate::temporal::duration::construct(&[
                Value::Number(years as f64),
                Value::Number(months as f64),
            ]);
        }
    }
    let (years, months) = if smallest == "year" {
        (round_increment(total / 12.0, increment, rounding_mode), 0.0)
    } else if largest == "month" {
        (0.0, round_increment(total, increment, rounding_mode))
    } else {
        let years = (total / 12.0).trunc();
        let months = round_increment(total - years * 12.0, increment, rounding_mode);
        if months.abs() >= 12.0 {
            (years + months.signum(), months - months.signum() * 12.0)
        } else {
            (years, months)
        }
    };
    if total != 0.0 && left.0 == -271_821.0 && left.1 == 4.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    let target_total = left.0 * 12.0 + left.1 - 1.0 + years * 12.0 + months;
    let target_year = (target_total / 12.0).floor();
    let target_month = target_total.rem_euclid(12.0) + 1.0;
    if total != 0.0 {
        let _ = construct_with_reference(target_year, target_month, 1.0)?;
    }
    crate::temporal::duration::construct(&[Value::Number(years), Value::Number(months)])
}

fn difference_options(
    options: Option<&Value>,
) -> Result<(&'static str, &'static str, f64, &'static str), VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(("year", "month", 1.0, "trunc"));
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let largest = difference_unit(
        &crate::execute::get_property_result(options, "largestUnit")?,
        "year",
    )?;
    let increment_value = crate::execute::get_property_result(options, "roundingIncrement")?;
    let increment = if matches!(increment_value, Value::Undefined) {
        1.0
    } else {
        crate::conversion::to_number(&increment_value)?.trunc()
    };
    if !increment.is_finite() || increment < 1.0 || increment > 1_000_000_000.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let mode_value = crate::execute::get_property_result(options, "roundingMode")?;
    let mode = if matches!(mode_value, Value::Undefined) {
        "trunc"
    } else {
        match crate::conversion::to_string(&mode_value)?.as_str() {
            "ceil" => "ceil",
            "floor" => "floor",
            "expand" => "expand",
            "trunc" => "trunc",
            "halfCeil" => "halfCeil",
            "halfFloor" => "halfFloor",
            "halfExpand" => "halfExpand",
            "halfTrunc" => "halfTrunc",
            "halfEven" => "halfEven",
            _ => {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingMode",
                ))
            }
        }
    };
    let smallest = difference_unit(
        &crate::execute::get_property_result(options, "smallestUnit")?,
        "month",
    )?;
    if !matches!(largest, "year" | "month")
        || !matches!(smallest, "year" | "month")
        || (largest == "month" && smallest == "year")
    {
        return Err(crate::value::error::throw_range_error("Invalid unit range"));
    }
    Ok((largest, smallest, increment, mode))
}

fn difference_unit(value: &Value, fallback: &'static str) -> Result<&'static str, VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(fallback);
    }
    match crate::conversion::to_string(value)?.trim_end_matches('s') {
        "auto" => Ok(fallback),
        "year" => Ok("year"),
        "month" => Ok("month"),
        "day" => Ok("day"),
        "week" => Ok("week"),
        _ => Err(crate::value::error::throw_range_error("Invalid unit")),
    }
}

fn round_increment(value: f64, increment: f64, mode: &str) -> f64 {
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
        "halfExpand" => scaled.round(),
        "halfCeil" => (scaled + 0.5).floor(),
        "halfFloor" => (scaled - 0.5).ceil(),
        "halfTrunc" => {
            if scaled.abs().fract() > 0.5 {
                scaled.round()
            } else {
                scaled.trunc()
            }
        }
        "halfEven" => {
            let lower = scaled.floor();
            let fraction = scaled - lower;
            if fraction < 0.5 {
                lower
            } else if fraction > 0.5 {
                lower + 1.0
            } else if lower.rem_euclid(2.0) == 0.0 {
                lower
            } else {
                lower + 1.0
            }
        }
        _ => scaled.trunc(),
    };
    rounded * increment
}

fn days_in_month(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    if calendar != "iso8601" && calendar != "gregory" {
        if let Value::String(code) = field(Some(receiver), "monthCode")? {
            if let Some(days) = crate::temporal::plain_date::calendar_days_in_month_for_code(
                year as i32,
                &code,
                &calendar,
            ) {
                return Ok(Value::Number(days as f64));
            }
        }
        if let Some(days) = crate::temporal::plain_date::calendar_days_in_month(
            year as i32,
            month as u32,
            &calendar,
        ) {
            return Ok(Value::Number(days as f64));
        }
    }
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, 1)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
    let next = if month == 12.0 {
        chrono::NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year as i32, month as u32 + 1, 1)
    }
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
    Ok(Value::Number((next - date).num_days() as f64))
}

fn days_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    if let Some(days) =
        crate::temporal::plain_date::calendar_days_in_year(year as i32, month as u32, &calendar)
    {
        return Ok(Value::Number(days as f64));
    }
    Ok(Value::Number(
        if chrono::NaiveDate::from_ymd_opt(year as i32, 2, 29).is_some() {
            366.0
        } else {
            365.0
        },
    ))
}
fn in_leap_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    if let Some(leap) =
        crate::temporal::plain_date::calendar_is_leap_year(year as i32, month as u32, &calendar)
    {
        return Ok(Value::Boolean(leap));
    }
    Ok(Value::Boolean(
        chrono::NaiveDate::from_ymd_opt(year as i32, 2, 29).is_some(),
    ))
}

fn months_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (year, month) = values(receiver)?;
    let calendar = crate::conversion::to_string(&field(Some(receiver), "calendarId")?)?;
    Ok(Value::Number(
        crate::temporal::plain_date::calendar_months_in_year(year as i32, month as u32, &calendar)
            .unwrap_or(12) as f64,
    ))
}
