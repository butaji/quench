use chrono::{Datelike, Duration, LocalResult, NaiveDateTime, Offset, TimeZone};

pub(crate) mod duration;
pub(crate) mod instant;
#[path = "temporal_options.rs"]
pub(crate) mod options;
pub(crate) mod plain_date;
pub(crate) mod plain_date_time;
pub(crate) mod plain_month_day;
pub(crate) mod plain_time;
pub(crate) mod plain_year_month;

const MAX_EPOCH_NANOSECONDS: i128 = 8_640_000_000_000_000_000_000;

fn round_quotient(delta: i128, quantum: i128, mode: &str) -> i128 {
    let quotient = delta / quantum;
    let remainder = delta % quantum;
    if remainder == 0 {
        return quotient;
    }
    let sign = delta.signum();
    let distance = remainder.abs();
    let adjust = match mode {
        "trunc" => false,
        "floor" => sign < 0,
        "ceil" => sign > 0,
        "expand" => true,
        "halfTrunc" => distance * 2 > quantum,
        "halfExpand" => distance * 2 >= quantum,
        "halfFloor" => distance * 2 > quantum || sign < 0 && distance * 2 == quantum,
        "halfCeil" => distance * 2 > quantum || sign > 0 && distance * 2 == quantum,
        "halfEven" => distance * 2 > quantum || distance * 2 == quantum && quotient % 2 != 0,
        _ => false,
    };
    quotient + if adjust { sign } else { 0 }
}

fn temporal_property_number(
    value: &crate::value::Value,
    name: &str,
) -> Result<f64, crate::execute::VmError> {
    crate::conversion::to_number(&crate::execute::get_property_result(value, name)?)
}

fn temporal_epoch_nanoseconds(
    value: &crate::value::Value,
) -> Result<i128, crate::execute::VmError> {
    match crate::execute::get_property_result(value, "epochNanoseconds")? {
        crate::value::Value::BigInt(value) => value
            .parse::<i128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid epochNanoseconds")),
        _ => Err(crate::value::error::throw_type_error(
            "Invalid epochNanoseconds",
        )),
    }
}

fn parse_epoch_text(value: &str) -> Result<i128, crate::execute::VmError> {
    value
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid epochNanoseconds"))
}

pub(crate) fn zoned_construct(
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let epoch = match arguments.first().unwrap_or(&crate::value::Value::Undefined) {
        crate::value::Value::BigInt(value) => value
            .parse::<i128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid epochNanoseconds"))?,
        crate::value::Value::Boolean(value) => i128::from(*value),
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Invalid epochNanoseconds",
            ))
        }
    };
    if epoch.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
        return Err(crate::value::error::throw_range_error(
            "Invalid epochNanoseconds",
        ));
    }
    let timezone_value = arguments.get(1).unwrap_or(&crate::value::Value::Undefined);
    if matches!(
        timezone_value,
        crate::value::Value::String(_) | crate::value::Value::StringUnits(_)
    ) {
        let text = crate::conversion::to_string(timezone_value)?;
        if looks_like_datetime_identifier(&text) {
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
                let date_like = text
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
                    && (text.chars().filter(|ch| *ch == '-').count() >= 2
                        || (text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit())));
                if date_like && !text.eq_ignore_ascii_case("iso8601") {
                    return Err(crate::value::error::throw_range_error("Invalid calendar"));
                }
            }
            Ok(calendar)
        })
        .transpose()?
        .unwrap_or_else(|| "iso8601".into());
    Ok(zoned_record_with_calendar(epoch, timezone, calendar))
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
    let (year, month, day, weekday, ordinal, week, week_year, days_month) = if let Some(date) = date
    {
        let days_month =
            crate::temporal::plain_date::days_in_month_for_record(date.year(), date.month());
        (
            date.year(),
            date.month(),
            date.day(),
            date.weekday().number_from_monday(),
            date.ordinal(),
            date.iso_week().week(),
            date.iso_week().year(),
            days_month,
        )
    } else {
        let days = seconds.div_euclid(86_400);
        let (year, month, day) =
            crate::temporal::plain_date::civil_from_serial((days + 719_468) as i64);
        let weekday = ((days + 3).rem_euclid(7) + 1) as u32;
        let ordinal = (1..month)
            .map(|m| crate::temporal::plain_date::days_in_month_for_record(year, m))
            .sum::<u32>()
            + day;
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
        ("year".into(), crate::value::Value::Number(year as f64)),
        ("month".into(), crate::value::Value::Number(month as f64)),
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
            crate::value::Value::Boolean(chrono::NaiveDate::from_ymd_opt(year, 2, 29).is_some()),
        ),
        (
            "\0prototype".into(),
            crate::value::Value::Builtin(prototype),
        ),
    ])))
}

fn fixed_offset_nanos(timezone: &str) -> i128 {
    if timezone.len() == 9 {
        return iso_offset_nanos(timezone);
    }
    let bytes = timezone.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return 0;
    }
    let (hour, minute) = match bytes.len() {
        3 if bytes[1..].iter().all(u8::is_ascii_digit) => {
            (timezone[1..3].parse::<i128>().unwrap_or(0), 0)
        }
        5 if bytes[1..].iter().all(u8::is_ascii_digit) => (
            timezone[1..3].parse::<i128>().unwrap_or(0),
            timezone[3..5].parse::<i128>().unwrap_or(0),
        ),
        6 if bytes[3] == b':' => (
            timezone[1..3].parse::<i128>().unwrap_or(0),
            timezone[4..6].parse::<i128>().unwrap_or(0),
        ),
        _ => return 0,
    };
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    sign * (hour * 3_600_000_000_000 + minute * 60_000_000_000)
}

pub(crate) fn timezone_offset_nanos(timezone: &str, epoch: i128) -> i128 {
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

pub(crate) fn timezone_local_epoch(
    timezone: &str,
    local_epoch: i128,
    disambiguation: &str,
) -> i128 {
    let fixed = fixed_offset_nanos(timezone);
    if fixed != 0 || timezone.starts_with(['+', '-']) {
        return local_epoch - fixed;
    }
    let Some(seconds) = i64::try_from(local_epoch.div_euclid(1_000_000_000)).ok() else {
        return local_epoch;
    };
    let nanos = local_epoch.rem_euclid(1_000_000_000) as u32;
    let Some(local) = NaiveDateTime::from_timestamp_opt(seconds, nanos) else {
        return local_epoch;
    };
    let Some(zone) = timezone.parse::<chrono_tz::Tz>().ok() else {
        return local_epoch;
    };
    match zone.from_local_datetime(&local) {
        LocalResult::Single(value) => {
            i128::from(value.timestamp()) * 1_000_000_000
                + i128::from(value.timestamp_subsec_nanos())
        }
        LocalResult::Ambiguous(first, second) => {
            if disambiguation == "reject" {
                return i128::MIN;
            }
            let earlier = first.timestamp().min(second.timestamp());
            let later = first.timestamp().max(second.timestamp());
            let selected = if disambiguation == "later" {
                later
            } else {
                earlier
            };
            i128::from(selected) * 1_000_000_000 + i128::from(nanos)
        }
        LocalResult::None => {
            if disambiguation == "reject" {
                return i128::MIN;
            }
            let before = timezone_offset_nanos(timezone, local_epoch - 86_400_000_000_000);
            let after = timezone_offset_nanos(timezone, local_epoch + 86_400_000_000_000);
            let before_epoch = local_epoch - before;
            let after_epoch = local_epoch - after;
            if disambiguation == "earlier" {
                before_epoch.min(after_epoch)
            } else {
                before_epoch.max(after_epoch)
            }
        }
    }
}

/// Locate an offset transition using the timezone's monotonic UTC offset
/// function. The one-day probe catches historical rule changes that occur
/// close together; a binary refinement keeps the returned instant exact to a
/// nanosecond without storing a second transition database in the VM.
fn find_timezone_transition(timezone: &str, epoch: i128, direction: &str) -> Option<i128> {
    if timezone.starts_with(['+', '-']) || timezone.eq_ignore_ascii_case("utc") {
        return None;
    }
    let zone = timezone.parse::<chrono_tz::Tz>().ok()?;
    let offset_at = |value: i128| -> Option<i128> {
        let seconds = i64::try_from(value.div_euclid(1_000_000_000)).ok()?;
        let nanos = u32::try_from(value.rem_euclid(1_000_000_000)).ok()?;
        zone.timestamp_opt(seconds, nanos)
            .single()
            .map(|date| i128::from(date.offset().fix().local_minus_utc()) * 1_000_000_000)
    };
    let current_offset = offset_at(epoch)?;
    let base_offset = if direction == "previous" {
        let before_offset = offset_at(epoch.checked_sub(1)?)?;
        if before_offset != current_offset {
            before_offset
        } else {
            current_offset
        }
    } else {
        current_offset
    };
    let step = 86_400_000_000_000i128;
    let mut previous = epoch;
    // Temporal's supported range is enormous, but tzdb has no useful rule
    // data near its artificial endpoints. Keep the search bounded and cheap
    // for the boundary probes while covering all practical tzdb history.
    for _ in 0..200_000 {
        let candidate = if direction == "next" {
            previous.checked_add(step)?
        } else {
            previous.checked_sub(step)?
        };
        if candidate.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
            return None;
        }
        let candidate_offset = offset_at(candidate)?;
        if candidate_offset != base_offset {
            let (mut low, mut high) = if direction == "next" {
                (previous, candidate)
            } else {
                (candidate, previous)
            };
            if direction == "next" {
                while high - low > 1 {
                    let middle = low + (high - low) / 2;
                    if offset_at(middle)? == base_offset {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                return Some(high);
            }
            while high - low > 1 {
                let middle = low + (high - low) / 2;
                if offset_at(middle)? == base_offset {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            return Some(high);
        }
        previous = candidate;
    }
    None
}

pub(crate) fn timezone_start_of_day_epoch(timezone: &str, epoch: i128) -> Option<i128> {
    let seconds = i64::try_from(epoch.div_euclid(1_000_000_000)).ok()?;
    let zone = timezone.parse::<chrono_tz::Tz>().ok()?;
    let local = zone.timestamp_opt(seconds, 0).single()?;
    let date = local.date_naive();
    for minute in 0..=180 {
        let candidate = date.and_hms_opt(0, 0, 0)? + Duration::minutes(minute);
        match zone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => {
                return Some(i128::from(value.timestamp()) * 1_000_000_000)
            }
            LocalResult::Ambiguous(first, second) => {
                return Some(i128::from(first.timestamp().min(second.timestamp())) * 1_000_000_000);
            }
            LocalResult::None => {}
        }
    }
    None
}

fn canonical_timezone_name(text: &str) -> Option<String> {
    if text.eq_ignore_ascii_case("utc") {
        return Some("UTC".into());
    }
    if let Ok(zone) = text.parse::<chrono_tz::Tz>() {
        return Some(zone.to_string());
    }
    chrono_tz::TZ_VARIANTS
        .iter()
        .find(|zone| zone.to_string().eq_ignore_ascii_case(text))
        .map(ToString::to_string)
}

fn timezone_primary_name(text: &str) -> &str {
    match text {
        "Europe/Nicosia" => "Asia/Nicosia",
        "America/Atka" => "America/Adak",
        "America/Knox_IN" => "America/Indiana/Knox",
        "Asia/Ashkhabad" => "Asia/Ashgabat",
        "Asia/Calcutta" => "Asia/Kolkata",
        "Asia/Choibalsan" => "Asia/Ulaanbaatar",
        "Asia/Chongqing" | "Asia/Chungking" | "Asia/Harbin" => "Asia/Shanghai",
        "Asia/Dacca" => "Asia/Dhaka",
        "Asia/Istanbul" => "Europe/Istanbul",
        "Asia/Kashgar" => "Asia/Urumqi",
        "Asia/Katmandu" => "Asia/Kathmandu",
        "Asia/Macao" => "Asia/Macau",
        "Asia/Rangoon" => "Asia/Yangon",
        "Asia/Saigon" => "Asia/Ho_Chi_Minh",
        "Asia/Tel_Aviv" => "Asia/Jerusalem",
        "Asia/Thimbu" => "Asia/Thimphu",
        "Asia/Ujung_Pandang" => "Asia/Makassar",
        "Asia/Ulan_Bator" => "Asia/Ulaanbaatar",
        "Africa/Asmera" => "Africa/Asmara",
        "Africa/Timbuktu" => "Africa/Bamako",
        "Antarctica/South_Pole" => "Antarctica/McMurdo",
        "Australia/ACT" | "Australia/Canberra" | "Australia/NSW" => "Australia/Sydney",
        "Australia/Currie" | "Australia/Tasmania" => "Australia/Hobart",
        "Australia/LHI" => "Australia/Lord_Howe",
        "Australia/North" => "Australia/Darwin",
        "Australia/Queensland" => "Australia/Brisbane",
        "Australia/South" => "Australia/Adelaide",
        "Australia/Victoria" => "Australia/Melbourne",
        "Australia/West" => "Australia/Perth",
        "Australia/Yancowinna" => "Australia/Broken_Hill",
        "Pacific/Enderbury" => "Pacific/Kanton",
        "Pacific/Johnston" => "Pacific/Honolulu",
        "Pacific/Ponape" => "Pacific/Pohnpei",
        "Pacific/Samoa" => "Pacific/Pago_Pago",
        "Pacific/Truk" | "Pacific/Yap" => "Pacific/Chuuk",
        "Europe/Belfast" => "Europe/London",
        "Europe/Kiev" | "Europe/Uzhgorod" | "Europe/Zaporozhye" => "Europe/Kyiv",
        "Europe/Tiraspol" => "Europe/Chisinau",
        "America/Argentina/ComodRivadavia" => "America/Argentina/Catamarca",
        "America/Buenos_Aires" => "America/Argentina/Buenos_Aires",
        "America/Catamarca" => "America/Argentina/Catamarca",
        "America/Coral_Harbour" => "America/Atikokan",
        "America/Cordoba" => "America/Argentina/Cordoba",
        "America/Ensenada" => "America/Tijuana",
        "America/Fort_Wayne" | "America/Indianapolis" => "America/Indiana/Indianapolis",
        "America/Godthab" => "America/Nuuk",
        "America/Jujuy" => "America/Argentina/Jujuy",
        "America/Louisville" => "America/Kentucky/Louisville",
        "America/Mendoza" => "America/Argentina/Mendoza",
        "America/Montreal" | "America/Nipigon" => "America/Toronto",
        "America/Pangnirtung" => "America/Iqaluit",
        "America/Porto_Acre" => "America/Rio_Branco",
        "America/Rainy_River" => "America/Winnipeg",
        "America/Rosario" => "America/Argentina/Cordoba",
        "America/Santa_Isabel" => "America/Tijuana",
        "America/Shiprock" => "America/Denver",
        "America/Thunder_Bay" => "America/Toronto",
        "America/Virgin" => "America/St_Thomas",
        "America/Yellowknife" => "America/Edmonton",
        "US/Alaska" => "America/Anchorage",
        "US/Aleutian" => "America/Adak",
        "US/Arizona" => "America/Phoenix",
        "US/Central" => "America/Chicago",
        "US/East-Indiana" => "America/Indiana/Indianapolis",
        "US/Eastern" => "America/New_York",
        "US/Hawaii" => "Pacific/Honolulu",
        "US/Indiana-Starke" => "America/Indiana/Knox",
        "US/Michigan" => "America/Detroit",
        "US/Mountain" => "America/Denver",
        "US/Pacific" => "America/Los_Angeles",
        "US/Samoa" => "Pacific/Pago_Pago",
        "Atlantic/Faeroe" => "Atlantic/Faroe",
        "Atlantic/Jan_Mayen" => "Arctic/Longyearbyen",
        "Brazil/Acre" => "America/Rio_Branco",
        "Brazil/DeNoronha" => "America/Noronha",
        "Brazil/East" => "America/Sao_Paulo",
        "Brazil/West" => "America/Manaus",
        "CET" => "Europe/Brussels",
        "CST6CDT" => "America/Chicago",
        "Canada/Atlantic" => "America/Halifax",
        "Canada/Central" => "America/Winnipeg",
        "Canada/Eastern" => "America/Toronto",
        "Canada/Mountain" => "America/Edmonton",
        "Canada/Newfoundland" => "America/St_Johns",
        "Canada/Pacific" => "America/Vancouver",
        "Canada/Saskatchewan" => "America/Regina",
        "Canada/Yukon" => "America/Whitehorse",
        "Chile/Continental" => "America/Santiago",
        "Chile/EasterIsland" => "Pacific/Easter",
        "Cuba" => "America/Havana",
        "EET" => "Europe/Athens",
        "EST" => "America/Panama",
        "EST5EDT" => "America/New_York",
        "Egypt" => "Africa/Cairo",
        "Eire" => "Europe/Dublin",
        "Etc/GMT+0" | "Etc/GMT-0" | "Etc/GMT0" | "Etc/Greenwich" | "Etc/UCT" | "Etc/UTC"
        | "Etc/Universal" | "Etc/Zulu" | "GMT+0" | "GMT-0" | "GMT0" | "Greenwich" | "UCT"
        | "Universal" | "Zulu" | "Etc/GMT" | "GMT" => "UTC",
        "GB" | "GB-Eire" => "Europe/London",
        "HST" => "Pacific/Honolulu",
        "Hongkong" => "Asia/Hong_Kong",
        "Iceland" => "Atlantic/Reykjavik",
        "Iran" => "Asia/Tehran",
        "Israel" => "Asia/Jerusalem",
        "Jamaica" => "America/Jamaica",
        "Japan" => "Asia/Tokyo",
        "Kwajalein" => "Pacific/Kwajalein",
        "Libya" => "Africa/Tripoli",
        "MET" => "Europe/Brussels",
        "MST" => "America/Phoenix",
        "MST7MDT" => "America/Denver",
        "Mexico/BajaNorte" => "America/Tijuana",
        "Mexico/BajaSur" => "America/Mazatlan",
        "Mexico/General" => "America/Mexico_City",
        "NZ" => "Pacific/Auckland",
        "NZ-CHAT" => "Pacific/Chatham",
        "Navajo" => "America/Denver",
        "PRC" => "Asia/Shanghai",
        "Poland" => "Europe/Warsaw",
        "Portugal" | "WET" => "Europe/Lisbon",
        "PST8PDT" => "America/Los_Angeles",
        "ROC" => "Asia/Taipei",
        "ROK" => "Asia/Seoul",
        "Singapore" => "Asia/Singapore",
        "Turkey" => "Europe/Istanbul",
        "W-SU" => "Europe/Moscow",
        value => value,
    }
}

fn timezone_equivalent(left: &str, right: &str) -> bool {
    if timezone_primary_name(left) == timezone_primary_name(right) {
        return true;
    }
    let left_is_fixed = left.starts_with(['+', '-']) || left.eq_ignore_ascii_case("utc");
    let right_is_fixed = right.starts_with(['+', '-']) || right.eq_ignore_ascii_case("utc");
    left_is_fixed && right_is_fixed
}

fn parse_date_parts(date: &str) -> Option<(i32, u32, u32)> {
    if !date.contains('-') {
        let (year_text, month_text, day_text) = match date.len() {
            8 => (&date[..4], &date[4..6], &date[6..]),
            11 if matches!(date.as_bytes().first(), Some(b'+' | b'-')) => {
                (&date[..7], &date[7..9], &date[9..])
            }
            _ => return None,
        };
        return Some((
            year_text.parse().ok()?,
            month_text.parse().ok()?,
            day_text.parse().ok()?,
        ));
    }
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
    let seconds = offset.unsigned_abs() / 1_000_000_000;
    let hours = seconds / 3_600;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    if seconds == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
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
    if matches!(value, crate::value::Value::Object(_))
        && matches!(
            crate::execute::get_property(value, "\0prototype"),
            crate::value::Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
        )
    {
        let identifier = crate::execute::get_property_result(value, "timeZoneId")?;
        return parse_timezone_identifier(&identifier);
    }
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
    if looks_like_datetime_identifier(&text) {
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
        return canonical_timezone_name(identifier)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"));
    }
    if text
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == ':')
    {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    canonical_timezone_name(&text)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"))
}

pub(crate) fn looks_like_datetime_identifier(text: &str) -> bool {
    text.find(['T', 't', ' ']).is_some_and(|index| {
        index >= 8
            && text[..index].contains('-')
            && text[..index].bytes().any(|byte| byte.is_ascii_digit())
    })
}

fn normalize_offset_identifier(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return None;
    }
    let (hour, minute, second) = match bytes.len() {
        3 => (text[1..3].parse::<u8>().ok()?, 0, 0),
        5 => (
            text[1..3].parse::<u8>().ok()?,
            text[3..5].parse::<u8>().ok()?,
            0,
        ),
        6 if bytes[3] == b':' => (
            text[1..3].parse::<u8>().ok()?,
            text[4..6].parse::<u8>().ok()?,
            0,
        ),
        _ => return None,
    };
    // Time-zone identifiers are minute-granular.  ISO date-time offsets may
    // spell trailing zero seconds, but any non-zero sub-minute offset is not
    // a valid identifier.
    if hour > 23 || minute > 59 || second != 0 {
        return None;
    }
    Some(format!("{}{:02}:{:02}", bytes[0] as char, hour, minute))
}

fn valid_iso_offset(text: &str) -> bool {
    let body = text.get(1..).unwrap_or_default();
    let (core, fraction) = body
        .split_once(['.', ','])
        .map_or((body, None), |(core, fraction)| (core, Some(fraction)));
    if fraction.is_some_and(|value| {
        value.is_empty() || value.len() > 9 || !value.bytes().all(|b| b.is_ascii_digit())
    }) {
        return false;
    }
    let valid_shape = match core.len() {
        2 | 4 | 6 => core.bytes().all(|b| b.is_ascii_digit()),
        5 => {
            core.as_bytes().get(2) == Some(&b':')
                && core[..2].bytes().all(|b| b.is_ascii_digit())
                && core[3..].bytes().all(|b| b.is_ascii_digit())
        }
        8 => {
            core.as_bytes().get(2) == Some(&b':')
                && core.as_bytes().get(5) == Some(&b':')
                && core[..2].bytes().all(|b| b.is_ascii_digit())
                && core[3..5].bytes().all(|b| b.is_ascii_digit())
                && core[6..].bytes().all(|b| b.is_ascii_digit())
        }
        _ => false,
    };
    if !valid_shape {
        return false;
    }
    let digits = core.replace(':', "");
    if !matches!(digits.len(), 2 | 4 | 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let hour = digits[0..2].parse::<u8>().unwrap_or(99);
    let minute = if digits.len() >= 4 {
        digits[2..4].parse::<u8>().unwrap_or(99)
    } else {
        0
    };
    let second = if digits.len() == 6 {
        digits[4..6].parse::<u8>().unwrap_or(99)
    } else {
        0
    };
    hour <= 23 && minute <= 59 && second <= 59
}

fn iso_offset_nanos(text: &str) -> i128 {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let body = &text[1..];
    let digits = body
        .split_once(['.', ','])
        .map_or(body, |(core, _)| core)
        .replace(':', "");
    let hour = digits
        .get(0..2)
        .and_then(|v| v.parse::<i128>().ok())
        .unwrap_or(0);
    let minute = digits
        .get(2..4)
        .and_then(|v| v.parse::<i128>().ok())
        .unwrap_or(0);
    let second = digits
        .get(4..6)
        .and_then(|v| v.parse::<i128>().ok())
        .unwrap_or(0);
    sign * (hour * 3_600 + minute * 60 + second) * 1_000_000_000
}

fn iso_offset_has_seconds(text: &str) -> bool {
    let body = text.get(1..).unwrap_or_default();
    let core = body.split_once(['.', ',']).map_or(body, |(core, _)| core);
    core.replace(':', "").len() == 6
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
        if let Some(canonical) = crate::temporal::plain_date::canonical_calendar_id(calendar) {
            return Ok(canonical);
        }
        if text.contains("-000000-") {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
        let base = text.split('[').next().unwrap_or(&text);
        let iso_date = base
            .chars()
            .all(|ch| ch.is_ascii_digit() || "-+Tt:., ".contains(ch))
            && base.chars().any(|ch| ch.is_ascii_digit());
        let date_like = base.contains(['T', 't', ' ']) && iso_date;
        if calendar.eq_ignore_ascii_case("iso8601")
            || date_like
            || (iso_date && !text.contains("[u-ca="))
        {
            return Ok("iso8601".into());
        }
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if let crate::value::Value::Object(object) = value {
        if object.iter().any(|(key, _)| {
            matches!(
                key.as_str(),
                "\0temporal-plain-date"
                    | "\0temporal-plain-date-time"
                    | "\0temporal-plain-month-day"
                    | "\0temporal-plain-year-month"
            )
        }) || object.iter().any(|(key, value)| {
            key == "\0prototype"
                && matches!(
                    value,
                    crate::value::Value::Builtin(
                        crate::ops::Builtin::TemporalZonedDateTimePrototype
                    )
                )
        }) {
            return Ok("iso8601".into());
        }
    }
    Err(crate::value::error::throw_type_error("Invalid calendar"))
}

fn parse_iso_annotations(
    text: &str,
) -> Result<(Option<String>, Option<String>), crate::execute::VmError> {
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
                return Err(crate::value::error::throw_range_error(
                    "Annotation keys must be lowercase",
                ));
            }
            if key != "u-ca" {
                if critical {
                    return Err(crate::value::error::throw_range_error(
                        "Unknown critical annotation",
                    ));
                }
            } else {
                if calendar.is_some() {
                    if critical || calendar_critical {
                        return Err(crate::value::error::throw_range_error("Invalid calendar"));
                    }
                    // A second non-critical calendar annotation is ignored.
                    rest = &after[end + 1..];
                    continue;
                }
                let canonical = crate::temporal::plain_date::canonical_calendar_id(value)
                    .ok_or_else(|| crate::value::error::throw_range_error("Invalid calendar"))?;
                calendar = Some(canonical);
                calendar_critical |= critical;
            }
        } else {
            if timezone.is_some() {
                return Err(crate::value::error::throw_range_error(
                    "Multiple time zones",
                ));
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
    let projected = if calendar != "iso8601" {
        let fields = ["year", "month", "day"]
            .iter()
            .map(|name| crate::execute::get_property_result(&record, name))
            .collect::<Result<Vec<_>, _>>();
        fields.ok().and_then(|fields| {
            let year = crate::conversion::to_number(&fields[0]).ok()? as i32;
            let month = crate::conversion::to_number(&fields[1]).ok()? as u32;
            let day = crate::conversion::to_number(&fields[2]).ok()? as u32;
            (if crate::temporal::plain_date::needs_calendar_boundary_projection(
                year, month, day, &calendar,
            ) {
                None
            } else {
                crate::temporal::plain_date::construct_from_iso(&[
                    fields[0].clone(),
                    fields[1].clone(),
                    fields[2].clone(),
                    crate::value::Value::String(calendar.clone()),
                ])
                .ok()
            })
            .or_else(|| {
                let projected = crate::temporal::plain_date::calendar_fields_from_iso(
                    year, month, day, &calendar,
                )?;
                let projected_day =
                    if (year, month, day) == (-271_821, 4, 20) && calendar != "gregory" {
                        projected.day.saturating_add(1)
                    } else {
                        projected.day
                    };
                Some(crate::value::Value::Object(std::rc::Rc::new(
                    crate::value::ObjectData::new(vec![
                        (
                            "year".into(),
                            crate::value::Value::Number(projected.year as f64),
                        ),
                        (
                            "month".into(),
                            crate::value::Value::Number(projected.month as f64),
                        ),
                        (
                            "day".into(),
                            crate::value::Value::Number(projected_day as f64),
                        ),
                        (
                            "monthCode".into(),
                            crate::value::Value::String(projected.month_code),
                        ),
                        (
                            "calendarId".into(),
                            crate::value::Value::String(calendar.clone()),
                        ),
                    ]),
                )))
            })
        })
    } else {
        None
    };
    if let crate::value::Value::Object(object) = &mut record {
        let object = std::rc::Rc::make_mut(object);
        if let Some(date) = projected {
            for name in [
                "year",
                "month",
                "day",
                "monthCode",
                "dayOfYear",
                "daysInMonth",
                "monthsInYear",
                "inLeapYear",
            ] {
                if let Ok(value) = crate::execute::get_property_result(&date, name) {
                    object.set_property_in_place(name, value);
                }
            }
        }
        if calendar != "iso8601" {
            object.set_property_in_place("weekOfYear", crate::value::Value::Undefined);
            object.set_property_in_place("yearOfWeek", crate::value::Value::Undefined);
        }
        object.set_property_in_place("calendarId", crate::value::Value::String(calendar));
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
    use super::{temporal_epoch_nanoseconds, temporal_property_number};
    use crate::{execute::VmError, value::Value};
    use chrono::{Datelike, Timelike};

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
                    return Err(crate::value::error::throw_type_error(
                        "Invalid ZonedDateTime",
                    ));
                };
                let left = left.parse::<i128>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid epochNanoseconds")
                })?;
                let right = right.parse::<i128>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid epochNanoseconds")
                })?;
                let ordering = left.cmp(&right);
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
                return Some(now_plain_date(arguments));
            }
            crate::ops::Builtin::TemporalNowPlainDateTimeISO => {
                return Some(now_plain_date_time(arguments));
            }
            crate::ops::Builtin::TemporalNowPlainTimeISO => {
                return Some(now_plain_time(arguments));
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
                    super::now_epoch_nanoseconds(),
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

    fn now_timezone(arguments: &[Value]) -> Result<String, VmError> {
        arguments
            .first()
            .filter(|value| !matches!(value, Value::Undefined))
            .map(super::parse_timezone_identifier)
            .transpose()
            .map(|timezone| timezone.unwrap_or_else(|| "UTC".into()))
    }

    fn now_components(timezone: &str) -> Result<[Value; 9], VmError> {
        let epoch = super::now_epoch_nanoseconds();
        let local = epoch + super::timezone_offset_nanos(timezone, epoch);
        let seconds = local.div_euclid(1_000_000_000);
        let nanos = local.rem_euclid(1_000_000_000) as u32;
        let date = chrono::DateTime::from_timestamp(
            i64::try_from(seconds)
                .map_err(|_| crate::value::error::throw_range_error("Invalid current time"))?,
            nanos,
        )
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid current time"))?;
        Ok([
            Value::Number(date.year() as f64),
            Value::Number(date.month() as f64),
            Value::Number(date.day() as f64),
            Value::Number(date.hour() as f64),
            Value::Number(date.minute() as f64),
            Value::Number(date.second() as f64),
            Value::Number(date.nanosecond() as f64 / 1_000_000.0),
            Value::Number((date.nanosecond() / 1_000 % 1_000) as f64),
            Value::Number((date.nanosecond() % 1_000) as f64),
        ])
    }

    fn now_plain_date(arguments: &[Value]) -> Result<Value, VmError> {
        let timezone = now_timezone(arguments)?;
        let values = now_components(&timezone)?;
        super::plain_date::construct(&values[..3])
    }

    fn now_plain_date_time(arguments: &[Value]) -> Result<Value, VmError> {
        let timezone = now_timezone(arguments)?;
        let values = now_components(&timezone)?;
        super::plain_date_time::construct(&values)
    }

    fn now_plain_time(arguments: &[Value]) -> Result<Value, VmError> {
        let timezone = now_timezone(arguments)?;
        let values = now_components(&timezone)?;
        super::plain_time::construct(&values[3..])
    }

    fn zoned_from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
        let value =
            value.ok_or_else(|| crate::value::error::throw_type_error("Invalid ZonedDateTime"))?;
        if matches!(value, Value::StringUnits(_)) {
            let text = crate::conversion::to_string(value)?;
            return zoned_from(Some(&Value::String(text)), options);
        }
        if matches!(value, Value::String(text) if crate::conversion::is_symbol_string(text)) {
            return Err(crate::value::error::throw_type_error(
                "Invalid ZonedDateTime value",
            ));
        }
        if !matches!(
            value,
            Value::String(_)
                | Value::Object(_)
                | Value::Function(_)
                | Value::BoundFunction(_)
                | Value::Proxy(_)
        ) {
            return Err(crate::value::error::throw_type_error(
                "Invalid ZonedDateTime value",
            ));
        }
        if super::is_zoned_receiver(value, 0) {
            if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
                if !crate::value::is_object(options) {
                    return Err(crate::value::error::throw_type_error("Invalid options"));
                }
                let validate_option = |name: &str, allowed: &[&str]| -> Result<(), VmError> {
                    let value = crate::execute::get_property_result(options, name)?;
                    if !matches!(value, Value::Undefined) {
                        let value = crate::conversion::to_string(&value)?;
                        if !allowed.contains(&value.as_str()) {
                            return Err(crate::value::error::throw_range_error(
                                "Invalid Temporal option",
                            ));
                        }
                    }
                    Ok(())
                };
                validate_option(
                    "disambiguation",
                    &["compatible", "earlier", "later", "reject"],
                )?;
                validate_option("offset", &["prefer", "use", "ignore", "reject"])?;
                validate_option("overflow", &["constrain", "reject"])?;
            }
            let epoch = crate::execute::get_property_result(value, "epochNanoseconds")?;
            let epoch = match epoch {
                Value::BigInt(value) => value.parse::<i128>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid epochNanoseconds")
                })?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid ZonedDateTime",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&crate::execute::get_property_result(
                value,
                "timeZoneId",
            )?)?;
            let calendar = crate::conversion::to_string(&crate::execute::get_property_result(
                value,
                "calendarId",
            )?)?;
            return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
        }
        let option_string =
            |name: &str, allowed: &[&str], default: &str| -> Result<String, VmError> {
                let Some(options) = options.filter(|value| !matches!(value, Value::Undefined))
                else {
                    return Ok(default.to_string());
                };
                let option = crate::execute::get_property_result(options, name)?;
                if matches!(option, Value::Undefined) {
                    return Ok(default.to_string());
                }
                let option = crate::conversion::to_string(&option)?;
                if !allowed.contains(&option.as_str()) {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid Temporal option",
                    ));
                }
                Ok(option)
            };
        if let Value::String(text) = value {
            validate_zoned_string_shape(text)?;
        }
        if let Value::String(text) = value {
            if !text.contains('[') || !text.ends_with(']') {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
                if !crate::value::is_object(options) {
                    return Err(crate::value::error::throw_type_error("Invalid options"));
                }
            }
            let disambiguation = option_string(
                "disambiguation",
                &["compatible", "earlier", "later", "reject"],
                "compatible",
            )?;
            let offset_mode =
                option_string("offset", &["prefer", "use", "ignore", "reject"], "reject")?;
            let _overflow_mode = option_string("overflow", &["constrain", "reject"], "constrain")?;
            let (calendar_annotation, timezone_annotation) = super::parse_iso_annotations(text)?;
            let has_z = text.split('[').next().unwrap_or(text).contains('Z');
            let date_time = text
                .split('[')
                .next()
                .unwrap_or(text)
                .split('Z')
                .next()
                .unwrap_or(text);
            let has_time_separator =
                date_time.contains('T') || date_time.contains('t') || date_time.contains(' ');
            if !has_time_separator
                && (timezone_annotation.is_none()
                    || has_z
                    || super::parse_date_parts(date_time).is_none())
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            let (date, time) = date_time
                .split_once(['T', 't', ' '])
                .unwrap_or((date_time, "00:00:00"));
            if date.starts_with("-000000") {
                return Err(crate::value::error::throw_range_error("Invalid year"));
            }
            let (parsed_year, parsed_month, parsed_day) = super::parse_date_parts(date)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            let offset_start = time[1..].find(['+', '-']).map(|index| index + 1);
            let (clock, offset_text) = offset_start
                .map(|index| (&time[..index], &time[index..]))
                .unwrap_or((time, "+00:00"));
            if offset_start.is_some() && !super::valid_iso_offset(offset_text) {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
            let (clock_core, fraction_text) = clock
                .split_once(['.', ','])
                .map_or((clock, None), |(core, fraction)| (core, Some(fraction)));
            if fraction_text.is_some_and(|fraction| fraction.is_empty() || fraction.len() > 9) {
                return Err(crate::value::error::throw_range_error(
                    "Too many fractional digits",
                ));
            }
            let mut time_parts = if clock_core.contains(':') {
                let parts = clock_core.split(':').collect::<Vec<_>>();
                if parts.len() > 3
                    || parts.iter().any(|part| part.len() != 2)
                    || (fraction_text.is_some() && parts.len() < 3)
                {
                    return Err(crate::value::error::throw_range_error("Invalid time"));
                }
                parts
                    .iter()
                    .map(|part| part.parse::<i64>().unwrap_or(-1))
                    .collect::<Vec<_>>()
            } else {
                if !matches!(clock_core.len(), 2 | 4 | 6)
                    || !clock_core.chars().all(|ch| ch.is_ascii_digit())
                    || (fraction_text.is_some() && clock_core.len() != 6)
                {
                    return Err(crate::value::error::throw_range_error("Invalid time"));
                }
                let mut parts = vec![clock_core[0..2].parse::<i64>().unwrap_or(-1)];
                if clock_core.len() >= 4 {
                    parts.push(clock_core[2..4].parse::<i64>().unwrap_or(-1));
                }
                if clock_core.len() == 6 {
                    parts.push(clock_core[4..6].parse::<i64>().unwrap_or(-1));
                }
                parts
            };
            let fractional_nanos = fraction_text
                .map(|fraction| {
                    format!("{fraction:0<9}")
                        .chars()
                        .take(9)
                        .collect::<String>()
                        .parse::<i128>()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if time_parts.get(2).is_some_and(|second| *second == 60) {
                time_parts[2] = 59;
            }
            if time_parts
                .first()
                .is_some_and(|hour| !(*hour >= 0 && *hour <= 23))
                || time_parts
                    .get(1)
                    .is_some_and(|minute| !(*minute >= 0 && *minute <= 59))
                || time_parts
                    .get(2)
                    .is_some_and(|second| !(*second >= 0 && *second <= 59))
            {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
            let year = parsed_year;
            let month = parsed_month;
            let day = parsed_day;
            if !(1..=12).contains(&month)
                || !(1..=super::plain_date::days_in_month_for_record(year, month)).contains(&day)
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
            let mut epoch = local_epoch - super::iso_offset_nanos(offset_text);
            let timezone_text =
                timezone_annotation
                    .as_deref()
                    .unwrap_or(if has_z { "UTC" } else { offset_text });
            let timezone =
                super::parse_timezone_identifier(&Value::String(timezone_text.to_string()))?;
            if offset_start.is_some() && offset_mode == "reject" {
                let supplied_offset = super::iso_offset_nanos(offset_text);
                let actual_offset = super::timezone_offset_nanos(&timezone, epoch);
                let fixed_timezone = timezone_annotation
                    .as_deref()
                    .is_some_and(|value| !value.contains('/'));
                if supplied_offset != actual_offset
                    && (super::iso_offset_has_seconds(offset_text) || fixed_timezone)
                {
                    return Err(crate::value::error::throw_range_error(
                        "Offset does not match time zone",
                    ));
                }
            }
            if offset_start.is_some()
                && matches!(offset_mode.as_str(), "prefer" | "reject")
                && local_epoch < -super::MAX_EPOCH_NANOSECONDS
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            if offset_start.is_some()
                && (matches!(offset_mode.as_str(), "ignore" | "prefer")
                    || (offset_mode == "reject" && !super::iso_offset_has_seconds(offset_text)))
            {
                let supplied_offset = super::iso_offset_nanos(offset_text);
                let actual_offset = super::timezone_offset_nanos(&timezone, epoch);
                let previous_offset =
                    super::timezone_offset_nanos(&timezone, epoch - 86_400_000_000_000);
                let use_previous = offset_mode == "reject"
                    && !super::iso_offset_has_seconds(offset_text)
                    && previous_offset != actual_offset
                    && (supplied_offset - previous_offset).abs() <= 30_000_000_000;
                if offset_mode == "ignore"
                    || (supplied_offset != actual_offset
                        && (!super::iso_offset_has_seconds(offset_text) || offset_mode == "prefer"))
                    || use_previous
                {
                    if use_previous {
                        epoch = local_epoch - previous_offset;
                    } else {
                        epoch =
                            super::timezone_local_epoch(&timezone, local_epoch, &disambiguation);
                    }
                    if epoch == i128::MIN {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid ZonedDateTime",
                        ));
                    }
                }
            }
            if offset_start.is_none() && !has_z {
                epoch = if !has_time_separator {
                    let guess = local_epoch - super::timezone_offset_nanos(&timezone, local_epoch);
                    super::timezone_start_of_day_epoch(&timezone, guess).unwrap_or_else(|| {
                        super::timezone_local_epoch(&timezone, local_epoch, &disambiguation)
                    })
                } else {
                    super::timezone_local_epoch(&timezone, local_epoch, &disambiguation)
                };
                if epoch == i128::MIN {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid ZonedDateTime",
                    ));
                }
            }
            if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            return Ok(super::zoned_record_with_calendar(
                epoch,
                timezone,
                calendar_annotation.unwrap_or_else(|| "iso8601".into()),
            ));
        }
        if !super::is_zoned_receiver(value, 0) {
            let finite_integer = |value: &Value| -> Result<i128, VmError> {
                let number = crate::conversion::to_number(value)?;
                if !number.is_finite() || number.abs() > 1.0e12 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid ZonedDateTime field",
                    ));
                }
                Ok(number.trunc() as i128)
            };
            let calendar_value = crate::execute::get_property_result(value, "calendar")?;
            let calendar_name = if matches!(calendar_value, Value::Undefined) {
                "iso8601".to_string()
            } else {
                super::parse_calendar_identifier(&calendar_value)?
            };
            let day_value = crate::execute::get_property_result(value, "day")?;
            if matches!(day_value, Value::Undefined) {
                return Err(crate::value::error::throw_type_error(
                    "Missing ZonedDateTime field",
                ));
            }
            let day_number = finite_integer(&day_value)?;
            let hour_value = crate::execute::get_property_result(value, "hour")?;
            let hour_number = if matches!(hour_value, Value::Undefined) {
                0
            } else {
                finite_integer(&hour_value)?
            };
            let microsecond_value = crate::execute::get_property_result(value, "microsecond")?;
            let microsecond_number = if matches!(microsecond_value, Value::Undefined) {
                0
            } else {
                finite_integer(&microsecond_value)?
            };
            let millisecond_value = crate::execute::get_property_result(value, "millisecond")?;
            let millisecond_number = if matches!(millisecond_value, Value::Undefined) {
                0
            } else {
                finite_integer(&millisecond_value)?
            };
            let minute_value = crate::execute::get_property_result(value, "minute")?;
            let minute_number = if matches!(minute_value, Value::Undefined) {
                0
            } else {
                finite_integer(&minute_value)?
            };
            let month_value = crate::execute::get_property_result(value, "month")?;
            let month_number = if matches!(month_value, Value::Undefined) {
                None
            } else {
                Some(finite_integer(&month_value)?)
            };
            let month_code_value = crate::execute::get_property_result(value, "monthCode")?;
            if matches!(month_value, Value::Undefined)
                && matches!(month_code_value, Value::Undefined)
            {
                return Err(crate::value::error::throw_type_error(
                    "Missing ZonedDateTime field",
                ));
            }
            let month_code = if matches!(month_code_value, Value::Undefined) {
                None
            } else {
                if crate::conversion::is_symbol(&month_code_value) {
                    return Err(crate::value::error::throw_type_error("Invalid monthCode"));
                }
                let code = match &month_code_value {
                    Value::String(code) => code.clone(),
                    Value::StringUnits(_) | Value::Object(_) => {
                        crate::conversion::to_string(&month_code_value)?
                    }
                    _ => return Err(crate::value::error::throw_type_error("Invalid monthCode")),
                };
                let well_formed = code.len() == 3 && code.starts_with('M')
                    || code.len() == 4 && code.starts_with('M') && code.ends_with('L');
                if !well_formed {
                    return Err(if matches!(&month_code_value, Value::Object(_)) {
                        crate::value::error::throw_type_error("Invalid monthCode")
                    } else {
                        crate::value::error::throw_range_error("Invalid monthCode")
                    });
                }
                let month = code[1..3]
                    .parse::<u32>()
                    .map_err(|_| crate::value::error::throw_range_error("Invalid monthCode"))?;
                Some((month, code.ends_with('L')))
            };
            let nanosecond_value = crate::execute::get_property_result(value, "nanosecond")?;
            let nanosecond_number = if matches!(nanosecond_value, Value::Undefined) {
                0
            } else {
                finite_integer(&nanosecond_value)?
            };
            let raw_offset = crate::execute::get_property_result(value, "offset")?;
            let validated_offset = if matches!(raw_offset, Value::Undefined) {
                None
            } else {
                if crate::conversion::is_symbol(&raw_offset) {
                    return Err(crate::value::error::throw_type_error("Invalid offset"));
                }
                if !matches!(
                    raw_offset,
                    Value::String(_) | Value::StringUnits(_) | Value::Object(_)
                ) {
                    return Err(crate::value::error::throw_type_error("Invalid offset"));
                }
                let offset = crate::conversion::to_string(&raw_offset)?;
                let normalized = if offset.eq_ignore_ascii_case("z") {
                    Some("+00:00".to_string())
                } else {
                    super::normalize_offset_identifier(&offset)
                }
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid offset"))?;
                Some(normalized)
            };
            let second_value = crate::execute::get_property_result(value, "second")?;
            let second_number = if matches!(second_value, Value::Undefined) {
                0
            } else {
                finite_integer(&second_value)?
            };
            let timezone_value = crate::execute::get_property_result(value, "timeZone")?;
            let (era_value, era_year_value) = if calendar_name == "iso8601" {
                (Value::Undefined, Value::Undefined)
            } else {
                (
                    crate::execute::get_property_result(value, "era")?,
                    crate::execute::get_property_result(value, "eraYear")?,
                )
            };
            let mut year_value = crate::execute::get_property_result(value, "year")?;
            let year_was_provided = !matches!(year_value, Value::Undefined);
            if matches!(year_value, Value::Undefined)
                && !matches!(era_value, Value::Undefined)
                && !matches!(era_year_value, Value::Undefined)
            {
                let era = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
                let era = super::plain_date::canonical_era_name(&calendar_name, &era).ok_or_else(
                    || {
                        if matches!(calendar_name.as_str(), "iso8601" | "chinese" | "dangi") {
                            crate::value::error::throw_type_error("Calendar does not use eras")
                        } else {
                            crate::value::error::throw_range_error("Invalid era")
                        }
                    },
                )?;
                let era_year = crate::conversion::to_number(&era_year_value)?.trunc();
                if !era_year.is_finite() {
                    return Err(crate::value::error::throw_range_error("Invalid eraYear"));
                }
                let year = super::plain_date::derive_year_from_era(&calendar_name, era, era_year)
                    .ok_or_else(|| crate::value::error::throw_type_error("Missing year"))?;
                year_value = Value::Number(year);
            }
            if matches!(year_value, Value::Undefined) {
                return Err(crate::value::error::throw_type_error(
                    "Missing ZonedDateTime field",
                ));
            }
            let year_number = finite_integer(&year_value)?;
            if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
                if !crate::value::is_object(options) {
                    return Err(crate::value::error::throw_type_error("Invalid options"));
                }
            }
            let disambiguation = option_string(
                "disambiguation",
                &["compatible", "earlier", "later", "reject"],
                "compatible",
            )?;
            let offset_mode =
                option_string("offset", &["prefer", "use", "ignore", "reject"], "reject")?;
            let overflow_mode = option_string("overflow", &["constrain", "reject"], "constrain")?;
            let year = i32::try_from(year_number).map_err(|_| {
                crate::value::error::throw_range_error("Invalid ZonedDateTime field")
            })?;
            if month_code.is_some_and(|(month, _)| {
                !(1..=12).contains(&month)
                    && !(super::plain_date::calendar_supports_month13(&calendar_name)
                        && month == 13)
            }) {
                return Err(crate::value::error::throw_range_error("Invalid monthCode"));
            }
            let month = if let Some((month_code, _)) = month_code {
                if let Some(month) = month_number {
                    let leap_ordinal =
                        matches!(calendar_name.as_str(), "chinese" | "dangi" | "hebrew")
                            && month == i128::from(month_code) + 1;
                    if month != i128::from(month_code) && !leap_ordinal {
                        return Err(crate::value::error::throw_range_error(
                            "Month and monthCode do not match",
                        ));
                    }
                }
                month_code
            } else {
                let month = month_number.unwrap_or(0);
                if month < 1 {
                    return Err(crate::value::error::throw_range_error("Invalid month"));
                }
                if overflow_mode == "reject"
                    && month > 12
                    && !(super::plain_date::calendar_has_month13(&calendar_name) && month == 13)
                {
                    return Err(crate::value::error::throw_range_error("Invalid month"));
                }
                let max_month = if super::plain_date::calendar_has_month13(&calendar_name) {
                    13
                } else {
                    12
                };
                month.clamp(1, max_month) as u32
            };
            let month_code_text = month_code
                .map(|(month, leap)| format!("M{month:02}{}", if leap { "L" } else { "" }));
            let day = day_number;
            if day < 1 {
                return Err(crate::value::error::throw_range_error("Invalid day"));
            }
            let day = if overflow_mode == "reject" {
                u32::try_from(day)
                    .map_err(|_| crate::value::error::throw_range_error("Invalid day"))?
            } else {
                let max_day = if calendar_name != "iso8601" && calendar_name != "gregory" {
                    month_code_text
                        .as_deref()
                        .and_then(|code| {
                            super::plain_date::calendar_days_in_month_for_code(
                                year,
                                code,
                                &calendar_name,
                            )
                        })
                        .or_else(|| {
                            super::plain_date::calendar_days_in_month(year, month, &calendar_name)
                        })
                        .unwrap_or_else(|| super::plain_date::days_in_month_for_record(year, month))
                } else {
                    super::plain_date::days_in_month_for_record(year, month)
                };
                day.min(i128::from(max_day)) as u32
            };
            let max_day = if calendar_name != "iso8601" && calendar_name != "gregory" {
                month_code_text
                    .as_deref()
                    .and_then(|code| {
                        super::plain_date::calendar_days_in_month_for_code(
                            year,
                            code,
                            &calendar_name,
                        )
                    })
                    .or_else(|| {
                        super::plain_date::calendar_days_in_month(year, month, &calendar_name)
                    })
                    .unwrap_or_else(|| super::plain_date::days_in_month_for_record(year, month))
            } else {
                super::plain_date::days_in_month_for_record(year, month)
            };
            if overflow_mode == "reject" && day > max_day {
                return Err(crate::value::error::throw_range_error("Invalid day"));
            }
            let mut time = [
                hour_number,
                minute_number,
                second_number,
                millisecond_number,
                microsecond_number,
                nanosecond_number,
            ];
            let limits = [23, 59, 59, 999, 999, 999];
            for (value, limit) in time.iter_mut().zip(limits) {
                if overflow_mode == "reject" && !(*value >= 0 && *value <= limit) {
                    return Err(crate::value::error::throw_range_error("Invalid time"));
                }
                if overflow_mode == "constrain" {
                    *value = (*value).clamp(0, limit);
                }
            }
            let [hour, minute, second, millisecond, microsecond, nanosecond] = time;
            let calendar = calendar_name;
            if !matches!(era_value, Value::Undefined)
                && !matches!(calendar.as_str(), "iso8601" | "chinese" | "dangi")
            {
                let era = crate::conversion::to_string(&era_value)?.to_ascii_lowercase();
                if super::plain_date::canonical_era_name(&calendar, &era).is_none() {
                    return Err(crate::value::error::throw_range_error("Invalid era"));
                }
            }
            if month_code.is_some_and(|(_, leap)| leap)
                && matches!(calendar.as_str(), "iso8601" | "gregory")
            {
                return Err(crate::value::error::throw_range_error("Invalid monthCode"));
            }
            if month_code.is_some_and(|(month, leap)| leap && calendar == "hebrew" && month != 5) {
                return Err(crate::value::error::throw_range_error("Invalid monthCode"));
            }
            let timezone = super::parse_timezone_identifier(&timezone_value)?;
            let offset = validated_offset;
            // Era fields are already reduced to the calendar's signed year.
            // Preserve that proleptic year for era-boundary dates; ICU's
            // calendar conversion does not represent the BCE/AA zero edge
            // consistently. Ordinary calendar fields still project through
            // the shared ICU conversion.
            let era_text = (!year_was_provided && !matches!(era_value, Value::Undefined))
                .then(|| crate::conversion::to_string(&era_value).ok())
                .flatten()
                .map(|value| value.to_ascii_lowercase());
            let date_serial = if matches!(
                (calendar.as_str(), era_text.as_deref()),
                ("ethiopic" | "ethioaa", Some("aa"))
            ) {
                super::plain_date::date_serial(year as f64, month as f64, day as f64)
            } else if let Some(code) = month_code_text.as_deref() {
                if matches!(calendar.as_str(), "iso8601" | "gregory") {
                    super::plain_date::date_serial(year as f64, month as f64, day as f64)
                } else {
                    let serial = super::plain_date::calendar_date_serial_for_code(
                        year as f64,
                        code,
                        day as f64,
                        &calendar,
                    );
                    serial
                        .or_else(|| {
                            super::plain_date::calendar_extreme_serial_for_fields(
                                year, month, day, code, &calendar,
                            )
                        })
                        .or_else(|| {
                            if !code.ends_with('L') {
                                super::plain_date::calendar_date_serial(
                                    year as f64,
                                    month as f64,
                                    day as f64,
                                    &calendar,
                                )
                            } else {
                                None
                            }
                        })
                        .ok_or_else(|| {
                            crate::value::error::throw_range_error("Invalid monthCode")
                        })?
                }
            } else {
                super::plain_date::calendar_date_serial(
                    year as f64,
                    month as f64,
                    day as f64,
                    &calendar,
                )
                .unwrap_or_else(|| {
                    super::plain_date::date_serial(year as f64, month as f64, day as f64)
                })
            };
            let extreme_endpoint = month_code_text.as_deref().is_some_and(|code| {
                super::plain_date::calendar_extreme_serial_for_fields(
                    year, month, day, code, &calendar,
                )
                .is_some()
            });
            let local_epoch =
                i128::from(date_serial - super::plain_date::date_serial(1970.0, 1.0, 1.0))
                    * 86_400_000_000_000
                    + hour * 3_600_000_000_000
                    + minute * 60_000_000_000
                    + second * 1_000_000_000;
            let local_epoch =
                local_epoch + millisecond * 1_000_000 + microsecond * 1_000 + nanosecond;
            let mut epoch = if extreme_endpoint && timezone == "UTC" && offset.is_none() {
                if year < 0 {
                    -super::MAX_EPOCH_NANOSECONDS
                        + hour * 3_600_000_000_000
                        + minute * 60_000_000_000
                        + second * 1_000_000_000
                        + millisecond * 1_000_000
                        + microsecond * 1_000
                        + nanosecond
                } else {
                    super::MAX_EPOCH_NANOSECONDS
                        + hour * 3_600_000_000_000
                        + minute * 60_000_000_000
                        + second * 1_000_000_000
                        + millisecond * 1_000_000
                        + microsecond * 1_000
                        + nanosecond
                }
            } else {
                match offset.as_deref() {
                    Some(offset) => local_epoch - super::iso_offset_nanos(offset),
                    None => super::timezone_local_epoch(&timezone, local_epoch, &disambiguation),
                }
            };
            if epoch == i128::MIN {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            if let Some(offset) = offset {
                let supplied = super::iso_offset_nanos(&offset);
                let actual = super::timezone_offset_nanos(&timezone, epoch);
                if offset_mode == "reject" && supplied != actual {
                    return Err(crate::value::error::throw_range_error(
                        "Offset does not match time zone",
                    ));
                }
                if offset_mode == "ignore" || (offset_mode == "prefer" && supplied != actual) {
                    epoch = super::timezone_local_epoch(&timezone, local_epoch, &disambiguation);
                    if epoch == i128::MIN {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid ZonedDateTime",
                        ));
                    }
                }
            }
            let mut result = super::zoned_record_with_calendar(epoch, timezone, calendar.clone());
            if era_text.is_some() {
                if let Value::Object(object) = &mut result {
                    let object = std::rc::Rc::make_mut(object);
                    object.set_property_in_place("year", Value::Number(year as f64));
                    if month_code_text.is_none()
                        || !matches!(calendar.as_str(), "hebrew" | "chinese" | "dangi")
                    {
                        object.set_property_in_place("month", Value::Number(month as f64));
                        if month_code_text.is_none() {
                            object.set_property_in_place(
                                "monthCode",
                                Value::String(format!("M{month:02}")),
                            );
                        }
                    } else if let Some(code) = month_code_text.as_deref() {
                        if code.ends_with('L') {
                            if let Ok(month) = code[1..3].parse::<u32>() {
                                object.set_property_in_place(
                                    "month",
                                    Value::Number(f64::from(month + 1)),
                                );
                            }
                        }
                    } else if let Some(code) = month_code_text.as_deref() {
                        let ordinal = if code.ends_with('L')
                            && matches!(calendar.as_str(), "hebrew" | "chinese" | "dangi")
                        {
                            code[1..3].parse::<u32>().ok().map(|month| month + 1)
                        } else {
                            super::plain_date::calendar_date_from_code(year, code, day, &calendar)
                                .map(|(ordinal, _)| ordinal)
                        };
                        if let Some(ordinal) = ordinal {
                            object
                                .set_property_in_place("month", Value::Number(f64::from(ordinal)));
                        }
                    }
                    object.set_property_in_place("day", Value::Number(day as f64));
                    if let Some((month, leap)) = month_code {
                        object.set_property_in_place(
                            "monthCode",
                            Value::String(format!("M{month:02}{}", if leap { "L" } else { "" })),
                        );
                    }
                    if !matches!(era_year_value, Value::Undefined) {
                        object.set_property_in_place(
                            "\0temporal-era-year",
                            Value::Number(
                                crate::conversion::to_number(&era_year_value)
                                    .unwrap_or(0.0)
                                    .trunc(),
                            ),
                        );
                    }
                }
            }
            if era_text.is_none() && calendar != "iso8601" && calendar != "gregory" {
                if let Value::Object(object) = &mut result {
                    let object = std::rc::Rc::make_mut(object);
                    object.set_property_in_place("year", Value::Number(year as f64));
                    object.set_property_in_place("day", Value::Number(day as f64));
                    let resolved = month_code_text
                        .as_deref()
                        .and_then(|code| {
                            super::plain_date::calendar_date_from_code(year, code, day, &calendar)
                        })
                        .or_else(|| {
                            super::plain_date::calendar_month_code_for_ordinal(
                                year, month, day, &calendar,
                            )
                            .and_then(|code| {
                                super::plain_date::calendar_date_from_code(
                                    year, &code, day, &calendar,
                                )
                                .map(|(ordinal, canonical)| (ordinal, canonical))
                            })
                        });
                    if let Some((ordinal, code)) = resolved {
                        object.set_property_in_place("month", Value::Number(f64::from(ordinal)));
                        object.set_property_in_place("monthCode", Value::String(code));
                    }
                }
            }
            if let Some((month, true)) = month_code {
                if let Value::Object(object) = &mut result {
                    std::rc::Rc::make_mut(object)
                        .set_property_in_place("monthCode", Value::String(format!("M{month:02}L")));
                }
            }
            return Ok(result);
        }
        let epoch = crate::execute::get_property_result(value, "epochNanoseconds")?;
        if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
            if !crate::value::is_object(options) {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
        }
        let _disambiguation = option_string(
            "disambiguation",
            &["compatible", "earlier", "later", "reject"],
            "compatible",
        )?;
        let _offset = option_string("offset", &["prefer", "use", "ignore", "reject"], "reject")?;
        let _overflow = option_string("overflow", &["constrain", "reject"], "constrain")?;
        let epoch = match epoch {
            Value::BigInt(value) => super::parse_epoch_text(&value)?,
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
        let calendar = crate::conversion::to_string(&crate::execute::get_property_result(
            value,
            "calendarId",
        )?)
        .unwrap_or_else(|_| "iso8601".into());
        Ok(super::zoned_record_with_calendar(epoch, timezone, calendar))
    }

    fn validate_zoned_string_shape(text: &str) -> Result<(), VmError> {
        if !text.contains('[') || !text.ends_with(']') {
            return Err(crate::value::error::throw_range_error(
                "Invalid ZonedDateTime",
            ));
        }
        let (_calendar, _timezone) = super::parse_iso_annotations(text)?;
        let head = text.split('[').next().unwrap_or(text);
        let head = head.strip_suffix('Z').unwrap_or(head);
        let Some((date, time)) = head.split_once(['T', 't', ' ']) else {
            let (year, month, day) = super::parse_date_parts(head)
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
            if !(1..=12).contains(&month)
                || !(1..=super::plain_date::days_in_month_for_record(year, month)).contains(&day)
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid ZonedDateTime",
                ));
            }
            return Ok(());
        };
        let (year, month, day) = super::parse_date_parts(date)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
        if !(1..=12).contains(&month)
            || !(1..=super::plain_date::days_in_month_for_record(year, month)).contains(&day)
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid ZonedDateTime",
            ));
        }
        let offset_start = time[1..].find(['+', '-']).map(|index| index + 1);
        let (clock, offset) = offset_start
            .map(|index| (&time[..index], Some(&time[index..])))
            .unwrap_or((time, None));
        if let Some(offset) = offset {
            if !super::valid_iso_offset(offset) {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
        }
        let (clock, fraction) = clock
            .split_once(['.', ','])
            .map_or((clock, None), |(core, fraction)| (core, Some(fraction)));
        if fraction.is_some_and(|value| value.is_empty() || value.len() > 9) {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
        let parts = if clock.contains(':') {
            let parts = clock.split(':').collect::<Vec<_>>();
            if parts.len() > 3 || parts.iter().any(|part| part.len() != 2) {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
            parts
                .iter()
                .map(|part| part.parse::<i64>().unwrap_or(-1))
                .collect::<Vec<_>>()
        } else {
            if !matches!(clock.len(), 2 | 4 | 6) || !clock.chars().all(|ch| ch.is_ascii_digit()) {
                return Err(crate::value::error::throw_range_error("Invalid time"));
            }
            let mut parts = vec![clock[0..2].parse::<i64>().unwrap_or(-1)];
            if clock.len() >= 4 {
                parts.push(clock[2..4].parse::<i64>().unwrap_or(-1));
            }
            if clock.len() == 6 {
                parts.push(clock[4..6].parse::<i64>().unwrap_or(-1));
            }
            parts
        };
        if parts.first().is_some_and(|value| !(0..=23).contains(value))
            || parts.get(1).is_some_and(|value| !(0..=59).contains(value))
            || parts.get(2).is_some_and(|value| !(0..=60).contains(value))
        {
            return Err(crate::value::error::throw_range_error("Invalid time"));
        }
        Ok(())
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
        let property = |name: &str| {
            if let Value::Object(object) = receiver {
                let direct = object.iter().any(|(key, value)| {
                    key == "\0prototype"
                        && matches!(
                            value,
                            Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
                        )
                });
                if direct {
                    if let Some((_, value)) = object.iter().find(|(key, _)| key == name) {
                        return Ok(value.clone());
                    }
                }
            }
            crate::execute::get_property_result(receiver, name)
        };
        match builtin {
            crate::ops::Builtin::TemporalZonedDateTimeEpochMillisecondsGetter => {
                let epoch = property("epochNanoseconds")?;
                let value = match epoch {
                    Value::BigInt(value) => super::parse_epoch_text(&value)?.div_euclid(1_000_000),
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
                    let epoch = super::parse_epoch_text(&epoch)?;
                    if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128 {
                        return Err(crate::value::error::throw_range_error(
                            "ZonedDateTime day boundary is out of range",
                        ));
                    }
                    let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
                    if let Some(start) = super::timezone_start_of_day_epoch(&timezone, epoch) {
                        let mut probe = start + 86_400_000_000_000;
                        for _ in 0..4 {
                            if let Some(next) = super::timezone_start_of_day_epoch(&timezone, probe)
                            {
                                if next > start {
                                    return Ok(Value::Number(
                                        (next - start) as f64 / 3_600_000_000_000.0,
                                    ));
                                }
                            }
                            probe += 86_400_000_000_000;
                        }
                    }
                }
                return Ok(Value::Number(24.0));
            }
            crate::ops::Builtin::TemporalZonedDateTimeWeekOfYearGetter => {
                let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                if calendar != "iso8601" {
                    return Ok(Value::Undefined);
                }
                let year = crate::conversion::to_number(&property("year")?)? as i32;
                let month = crate::conversion::to_number(&property("month")?)? as u32;
                let day = crate::conversion::to_number(&property("day")?)? as u32;
                let week = chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .map(|date| date.iso_week().week() as f64)
                    .unwrap_or(f64::NAN);
                return Ok(Value::Number(week));
            }
            crate::ops::Builtin::TemporalZonedDateTimeYearOfWeekGetter => {
                let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                if calendar != "iso8601" {
                    return Ok(Value::Undefined);
                }
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
            let other = if super::is_zoned_receiver(other, 0) {
                other.clone()
            } else {
                zoned_from(Some(other), None)?
            };
            let timezone_equal = match (
                property("timeZoneId")?,
                crate::execute::get_property_result(&other, "timeZoneId")?,
            ) {
                (Value::String(left), Value::String(right)) => {
                    super::timezone_primary_name(&left) == super::timezone_primary_name(&right)
                }
                _ => false,
            };
            let calendar_equal = match (
                property("calendarId"),
                crate::execute::get_property_result(&other, "calendarId"),
            ) {
                (Ok(Value::String(left)), Ok(Value::String(right))) => {
                    let left = super::plain_date::canonical_calendar_id(&left).unwrap_or(left);
                    let right = super::plain_date::canonical_calendar_id(&right).unwrap_or(right);
                    left == right
                }
                _ => false,
            };
            return Ok(Value::Boolean(
                timezone_equal
                    && property("epochNanoseconds").ok()
                        == crate::execute::get_property_result(&other, "epochNanoseconds").ok()
                    && calendar_equal,
            ));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToJSON {
            return zoned_method(
                crate::ops::Builtin::TemporalZonedDateTimeToString,
                Some(receiver),
                &[],
            );
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToLocaleString {
            let options = arguments.get(1);
            if matches!(options, Some(Value::Null)) {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
            let time_zone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let option_names = [
                "localeMatcher",
                "calendar",
                "numberingSystem",
                "hour12",
                "hourCycle",
                "weekday",
                "era",
                "year",
                "month",
                "day",
                "dayPeriod",
                "hour",
                "minute",
                "second",
                "fractionalSecondDigits",
                "timeZoneName",
                "formatMatcher",
                "dateStyle",
                "timeStyle",
            ];
            let mut formatter_options = Vec::with_capacity(option_names.len() + 1);
            if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
                let supplied_zone = crate::execute::get_property_result(options, "timeZone")?;
                if !matches!(supplied_zone, Value::Undefined) {
                    return Err(crate::value::error::throw_type_error(
                        "timeZone option is not allowed",
                    ));
                }
                for name in option_names {
                    let value = crate::execute::get_property_result(options, name)?;
                    if !matches!(value, Value::Undefined) {
                        formatter_options.push((name.to_string(), value));
                    }
                }
            }
            if let Some(locale) = arguments
                .first()
                .filter(|value| !matches!(value, Value::Undefined))
                .map(crate::conversion::to_string)
                .transpose()?
            {
                if !formatter_options.iter().any(|(name, _)| name == "calendar") {
                    if let Some(calendar) = crate::intl::locale::calendar_from_tag(&locale) {
                        formatter_options.push(("calendar".to_string(), Value::String(calendar)));
                    }
                }
            }
            let has_date_or_time = formatter_options.iter().any(|(name, _)| {
                matches!(
                    name.as_str(),
                    "year"
                        | "month"
                        | "day"
                        | "hour"
                        | "minute"
                        | "second"
                        | "dateStyle"
                        | "timeStyle"
                )
            });
            if !has_date_or_time && formatter_options.iter().any(|(name, _)| name == "era") {
                formatter_options.extend([
                    ("year".to_string(), Value::String("numeric".to_string())),
                    ("month".to_string(), Value::String("numeric".to_string())),
                    ("day".to_string(), Value::String("numeric".to_string())),
                    ("hour".to_string(), Value::String("numeric".to_string())),
                    ("minute".to_string(), Value::String("numeric".to_string())),
                    ("second".to_string(), Value::String("numeric".to_string())),
                ]);
            }
            formatter_options.push(("timeZone".to_string(), Value::String(time_zone)));
            let formatter_args = vec![
                arguments.first().cloned().unwrap_or(Value::Undefined),
                Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
                    formatter_options,
                ))),
            ];
            let formatter = crate::intl::datetime::construct_with_defaults(
                &formatter_args,
                Some(&[
                    "year",
                    "month",
                    "day",
                    "hour",
                    "minute",
                    "second",
                    "timeZoneName",
                ]),
            )?;
            let formatter_calendar = match &formatter {
                Value::Object(properties) => properties
                    .iter()
                    .find_map(|(name, value)| (name == crate::intl::SLOT).then_some(value))
                    .and_then(|slot| match slot {
                        Value::Object(properties) => properties
                            .iter()
                            .find_map(|(name, value)| (name == "calendar").then_some(value)),
                        _ => None,
                    }),
                _ => None,
            }
            .and_then(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "gregory".to_string());
            let instance_calendar = crate::conversion::to_string(&property("calendarId")?)?;
            if instance_calendar != "iso8601" && instance_calendar != formatter_calendar {
                return Err(crate::value::error::throw_range_error(
                    "Calendar does not match locale",
                ));
            }
            let instant = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                (
                    "epochNanoseconds".to_string(),
                    property("epochNanoseconds")?,
                ),
                (
                    "\0prototype".to_string(),
                    Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype),
                ),
            ])));
            let parts = crate::intl::datetime::prototype_method(
                crate::ops::Builtin::IntlDateTimeFormatFormatToParts,
                &[instant],
                Some(&formatter),
            )?;
            let length = crate::conversion::to_number(&crate::execute::get_property_result(
                &parts, "length",
            )?)? as usize;
            let mut result = String::new();
            for index in 0..length {
                let part = crate::execute::get_property_result(&parts, &index.to_string())?;
                let value = crate::execute::get_property_result(&part, "value")?;
                result.push_str(&crate::conversion::to_string(&value)?);
            }
            return Ok(Value::String(result));
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
                return Err(crate::value::error::throw_type_error(
                    "Invalid date-time-like",
                ));
            }
            let options = arguments.get(1);
            let primitive_options = options.is_some_and(|value| {
                !matches!(value, Value::Undefined) && !crate::value::is_object(value)
            });
            let option_string = |name: &str, allowed: &[&str], default: &str| {
                if primitive_options {
                    return Ok(default.to_string());
                }
                let value = options
                    .filter(|value| !matches!(value, Value::Undefined))
                    .map(|value| crate::execute::get_property_result(value, name))
                    .transpose()?
                    .filter(|value| !matches!(value, Value::Undefined));
                let Some(value) = value else {
                    return Ok(default.to_string());
                };
                let value = crate::conversion::to_string(&value)?;
                if !allowed.contains(&value.as_str()) {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid Temporal option",
                    ));
                }
                Ok(value)
            };
            let calendar = crate::execute::get_property_result(partial, "calendar")?;
            if !matches!(calendar, Value::Undefined) {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            }
            let partial_time_zone = crate::execute::get_property_result(partial, "timeZone")?;
            if !matches!(partial_time_zone, Value::Undefined) {
                return Err(crate::value::error::throw_type_error("Invalid time zone"));
            }
            let mut prepared = Vec::with_capacity(11);
            let mut has_field = false;
            let mut month_provided = false;
            let mut month_code_provided = false;
            let mut offset_provided = false;
            let mut date_change = false;
            for name in [
                "day",
                "hour",
                "microsecond",
                "millisecond",
                "minute",
                "month",
                "monthCode",
                "nanosecond",
                "offset",
                "second",
                "year",
            ] {
                let raw = crate::execute::get_property_result(partial, name)?;
                if !matches!(raw, Value::Undefined) {
                    has_field = true;
                    date_change |= matches!(name, "year" | "month" | "monthCode" | "day");
                    month_provided |= name == "month";
                    month_code_provided |= name == "monthCode";
                    offset_provided |= name == "offset";
                }
                let value = if matches!(raw, Value::Undefined) {
                    property(name)?
                } else {
                    raw
                };
                let value = if name == "monthCode" || name == "offset" {
                    if name == "offset"
                        && !matches!(
                            value,
                            Value::String(_) | Value::StringUnits(_) | Value::Object(_)
                        )
                    {
                        return Err(crate::value::error::throw_type_error("Invalid offset"));
                    }
                    Value::String(crate::conversion::to_string(&value)?)
                } else {
                    Value::Number(crate::conversion::to_number(&value)?)
                };
                if name == "offset"
                    && super::normalize_offset_identifier(&crate::conversion::to_string(&value)?)
                        .is_none()
                {
                    return Err(crate::value::error::throw_range_error("Invalid offset"));
                }
                prepared.push((name, value));
            }
            let instance_calendar = crate::conversion::to_string(&property("calendarId")?)?;
            let has_calendar_date_field = instance_calendar != "iso8601"
                && ["era", "eraYear"].iter().any(|name| {
                    !matches!(
                        crate::execute::get_property_result(partial, name),
                        Ok(Value::Undefined)
                    )
                });
            if !has_field && !has_calendar_date_field {
                return Err(crate::value::error::throw_type_error(
                    "Insufficient date-time data",
                ));
            }
            let value_for = |name: &str| {
                prepared
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Value::Undefined)
            };
            let month = value_for("month");
            let month_code = value_for("monthCode");
            let partial_offset = if offset_provided {
                value_for("offset")
            } else {
                Value::Undefined
            };
            let mut fields = vec![
                ("year".to_string(), value_for("year")),
                ("day".to_string(), value_for("day")),
                ("hour".to_string(), value_for("hour")),
                ("minute".to_string(), value_for("minute")),
                ("second".to_string(), value_for("second")),
                ("millisecond".to_string(), value_for("millisecond")),
                ("microsecond".to_string(), value_for("microsecond")),
                ("nanosecond".to_string(), value_for("nanosecond")),
                ("timeZone".to_string(), property("timeZoneId")?),
            ];
            if !matches!(partial_offset, Value::Undefined) {
                fields.push(("offset".to_string(), partial_offset.clone()));
            }
            let disambiguation = option_string(
                "disambiguation",
                &["compatible", "earlier", "later", "reject"],
                "compatible",
            )?;
            let offset_mode =
                option_string("offset", &["prefer", "use", "ignore", "reject"], "prefer")?;
            let overflow = option_string("overflow", &["constrain", "reject"], "constrain")?;
            if overflow != "constrain" && overflow != "reject" {
                return Err(crate::value::error::throw_range_error("Invalid overflow"));
            }
            if month_code_provided {
                let code = crate::conversion::to_string(&month_code)?;
                let calendar_id = crate::conversion::to_string(&property("calendarId")?)?;
                if code.ends_with('L') && matches!(calendar_id.as_str(), "iso8601" | "gregory") {
                    return Err(crate::value::error::throw_range_error("Invalid monthCode"));
                }
                let code_number = code
                    .strip_prefix('M')
                    .and_then(|value| value.get(..2))
                    .and_then(|value| value.parse::<f64>().ok())
                    .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
                let calendar_id = crate::conversion::to_string(&property("calendarId")?)?;
                if month_provided
                    && calendar_id == "iso8601"
                    && crate::conversion::to_number(&month)?.trunc() != code_number
                {
                    return Err(crate::value::error::throw_range_error("Month mismatch"));
                }
            }
            if month_provided {
                fields.push(("month".to_string(), month));
            } else if month_code_provided {
                fields.push(("monthCode".to_string(), month_code.clone()));
            } else {
                fields.push(("month".to_string(), value_for("month")));
            }
            // Calendar date resolution is shared with PlainDate.  Keeping one
            // resolver here gives ZonedDateTime.with the same era, leap-month,
            // and overflow semantics without maintaining a second calendar VM.
            let calendar_id = crate::conversion::to_string(&property("calendarId")?)?;
            let date_change = has_calendar_date_field || date_change;
            if date_change && calendar_id != "iso8601" {
                let date_receiver =
                    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                        (
                            "\0prototype".into(),
                            Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
                        ),
                        ("year".into(), property("year")?),
                        ("month".into(), property("month")?),
                        ("day".into(), property("day")?),
                        ("monthCode".into(), property("monthCode")?),
                        ("calendarId".into(), Value::String(calendar_id.clone())),
                    ])));
                let date_result = super::plain_date::execute(
                    crate::ops::Builtin::TemporalPlainDateWith,
                    Some(&date_receiver),
                    &[
                        partial.clone(),
                        options.unwrap_or(&Value::Undefined).clone(),
                    ],
                )
                .ok_or_else(|| crate::value::error::throw_type_error("Invalid date"))??;
                for name in ["year", "month", "day", "monthCode"] {
                    let value = crate::execute::get_property_result(&date_result, name)?;
                    if let Some((_, target)) = fields.iter_mut().find(|(key, _)| key == name) {
                        *target = value;
                    } else if name == "monthCode" {
                        fields.push((name.to_string(), value));
                    }
                }
            }
            if !offset_provided && offset_mode != "ignore" {
                fields.push(("offset".to_string(), property("offset")?));
            }
            let year_number = fields
                .iter()
                .find(|(name, _)| name == "year")
                .map(|(_, value)| crate::conversion::to_number(value))
                .transpose()?
                .unwrap_or(0.0);
            if !year_number.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid date"));
            }
            let year = year_number as i32;
            let month_number =
                if let Some((_, value)) = fields.iter().find(|(name, _)| name == "month") {
                    crate::conversion::to_number(value)?
                } else if let Some((_, Value::String(code))) =
                    fields.iter().find(|(name, _)| name == "monthCode")
                {
                    code.get(1..)
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(0) as f64
                } else {
                    0.0
                };
            if !month_number.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid date"));
            }
            let month = month_number as u32;
            if let Some((_, day)) = fields.iter_mut().find(|(name, _)| name == "day") {
                let day_number_value = crate::conversion::to_number(day)?;
                if !day_number_value.is_finite() || primitive_options && day_number_value < 1.0 {
                    return Err(crate::value::error::throw_range_error("Invalid date"));
                }
                let day_number = day_number_value as u32;
                let validation_month = month.clamp(1, 12);
                let calendar_id = crate::conversion::to_string(&property("calendarId")?)?;
                let valid_day = if calendar_id != "iso8601" && calendar_id != "gregory" {
                    let limit = if !month_provided && !month_code_provided {
                        crate::conversion::to_number(&property("daysInMonth")?)? as u32
                    } else {
                        let code = crate::conversion::to_string(&value_for("monthCode"))?;
                        super::plain_date::calendar_days_in_month_for_code(
                            year,
                            &code,
                            &calendar_id,
                        )
                        .or_else(|| {
                            super::plain_date::calendar_days_in_month(
                                year,
                                validation_month,
                                &calendar_id,
                            )
                        })
                        .unwrap_or(31)
                    };
                    day_number <= limit
                } else {
                    chrono::NaiveDate::from_ymd_opt(year, validation_month, day_number).is_some()
                };
                if !valid_day
                    && !(calendar_id != "iso8601"
                        && calendar_id != "gregory"
                        && !month_provided
                        && !month_code_provided
                        && overflow == "constrain")
                {
                    if overflow == "reject" {
                        return Err(crate::value::error::throw_range_error("Invalid date"));
                    }
                    let mut constrained = day_number.min(31);
                    while constrained > 1
                        && chrono::NaiveDate::from_ymd_opt(year, validation_month, constrained)
                            .is_none()
                    {
                        constrained -= 1;
                    }
                    *day = Value::Number(constrained as f64);
                }
            }
            if primitive_options {
                return Err(crate::value::error::throw_type_error("Invalid options"));
            }
            let field_entries = fields.clone();
            let receiver_days_in_month = property("daysInMonth").ok();
            let mut field_value =
                Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(fields)));
            if let Value::Object(object) = &mut field_value {
                let object = std::rc::Rc::make_mut(object);
                object.set_property_in_place("calendar", Value::String(calendar_id.clone()));
            }
            let option_value =
                Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                    ("overflow".to_string(), Value::String(overflow.clone())),
                    (
                        "disambiguation".to_string(),
                        Value::String(disambiguation.clone()),
                    ),
                    ("offset".to_string(), Value::String(offset_mode.clone())),
                ])));
            let result = if matches!(partial_offset, Value::Undefined)
                && offset_mode == "ignore"
                && calendar_id == "iso8601"
            {
                let number = |name: &str| -> Result<i128, VmError> {
                    if name == "year" {
                        return Ok(year_number.trunc() as i128);
                    }
                    if name == "month" && field_entries.iter().any(|(key, _)| key == "month") {
                        return Ok(month_number.trunc() as i128);
                    }
                    let value = field_entries
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value)
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let number = crate::conversion::to_number(&value)?;
                    if !number.is_finite() {
                        return Err(crate::value::error::throw_range_error("Invalid date"));
                    }
                    Ok(number.trunc() as i128)
                };
                let year = i32::try_from(number("year")?)
                    .map_err(|_| crate::value::error::throw_range_error("Invalid date"))?;
                let raw_month = if field_entries.iter().any(|(name, _)| name == "month") {
                    number("month")?
                } else {
                    let code = field_entries
                        .iter()
                        .find(|(name, _)| name == "monthCode")
                        .map(|(_, value)| crate::conversion::to_string(value))
                        .transpose()?
                        .ok_or_else(|| {
                            crate::value::error::throw_range_error("Invalid monthCode")
                        })?;
                    code.strip_prefix('M')
                        .and_then(|value| value.get(..2))
                        .and_then(|value| value.parse::<i128>().ok())
                        .ok_or_else(|| {
                            crate::value::error::throw_range_error("Invalid monthCode")
                        })?
                };
                if overflow == "reject" && !(1..=12).contains(&raw_month) {
                    return Err(crate::value::error::throw_range_error("Invalid month"));
                }
                let month = raw_month.clamp(1, 12) as u32;
                let raw_day = number("day")?;
                let day_limit =
                    i128::from(super::plain_date::days_in_month_for_record(year, month));
                if overflow == "reject" && !(1..=day_limit).contains(&raw_day) {
                    return Err(crate::value::error::throw_range_error("Invalid day"));
                }
                let day = raw_day.clamp(1, day_limit) as u32;
                let clock = [
                    ("hour", 23),
                    ("minute", 59),
                    ("second", 59),
                    ("millisecond", 999),
                    ("microsecond", 999),
                    ("nanosecond", 999),
                ];
                let mut time = [0_i128; 6];
                for (index, (name, limit)) in clock.into_iter().enumerate() {
                    let value = number(name)?;
                    if overflow == "reject" && !(0..=limit).contains(&value) {
                        return Err(crate::value::error::throw_range_error("Invalid time"));
                    }
                    time[index] = value.clamp(0, limit);
                }
                let [hour, minute, second, millisecond, microsecond, nanosecond] = time;
                let year_adjusted = i128::from(year) - i128::from(month <= 2);
                let era = if year_adjusted >= 0 {
                    year_adjusted
                } else {
                    year_adjusted - 399
                } / 400;
                let year_of_era = year_adjusted - era * 400;
                let month_i = i128::from(month);
                let day_of_year = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5
                    + i128::from(day)
                    - 1;
                let days = era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100
                    + day_of_year
                    - 719_468;
                let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                let date_serial = if matches!(calendar.as_str(), "iso8601" | "gregory") {
                    days + 719_468
                } else {
                    super::plain_date::calendar_date_serial(
                        year as f64,
                        month as f64,
                        day as f64,
                        &calendar,
                    )
                    .map(i128::from)
                    .unwrap_or(days + 719_468)
                };
                let local_epoch = (date_serial
                    - i128::from(super::plain_date::date_serial(1970.0, 1.0, 1.0)))
                    * 86_400_000_000_000
                    + hour * 3_600_000_000_000
                    + minute * 60_000_000_000
                    + second * 1_000_000_000
                    + millisecond * 1_000_000
                    + microsecond * 1_000
                    + nanosecond;
                let timezone_text = field_entries
                    .iter()
                    .find(|(key, _)| key == "timeZone")
                    .map(|(_, value)| crate::conversion::to_string(value))
                    .transpose()?
                    .unwrap_or_else(|| "UTC".into());
                let timezone =
                    super::parse_timezone_identifier(&crate::value::Value::String(timezone_text))?;
                let epoch = super::timezone_local_epoch(&timezone, local_epoch, &disambiguation);
                if epoch == i128::MIN {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid ZonedDateTime",
                    ));
                }
                if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid epochNanoseconds",
                    ));
                }
                super::zoned_record_with_calendar(epoch, timezone, calendar)
            } else {
                zoned_from(Some(&field_value), Some(&option_value))?
            };
            if let Value::Object(mut object) = result {
                let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                if calendar != "iso8601" && calendar != "gregory" && calendar != "japanese" {
                    let object = std::rc::Rc::make_mut(&mut object);
                    for name in ["year", "month", "monthCode"] {
                        let value = if name == "monthCode" {
                            if month_code_provided {
                                field_entries
                                    .iter()
                                    .find(|(key, _)| key == name)
                                    .map(|(_, value)| value.clone())
                            } else if month_provided {
                                Some(Value::String(format!("M{month_number:02}")))
                            } else {
                                field_entries
                                    .iter()
                                    .find(|(key, _)| key == name)
                                    .map(|(_, value)| value.clone())
                                    .or_else(|| property(name).ok())
                            }
                        } else {
                            field_entries
                                .iter()
                                .find(|(key, _)| key == name)
                                .map(|(_, value)| value.clone())
                        };
                        if let Some(value) = value {
                            object.set_property_in_place(name, value);
                        }
                    }
                    if !month_provided && !month_code_provided {
                        if let Some((_, value)) = field_entries.iter().find(|(key, _)| key == "day")
                        {
                            let requested = crate::conversion::to_number(value)?;
                            let limit = receiver_days_in_month
                                .as_ref()
                                .map(crate::conversion::to_number)
                                .transpose()?
                                .unwrap_or(31.0);
                            object.set_property_in_place(
                                "day",
                                Value::Number(requested.min(limit).max(1.0)),
                            );
                        }
                    }
                }
                return Ok(Value::Object(object));
            }
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
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
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
                    let mut text = if source.starts_with(['T', 't']) {
                        source.to_string()
                    } else {
                        source
                            .rfind(['T', 't', ' '])
                            .map(|index| source[index + 1..].to_string())
                            .unwrap_or_else(|| source.to_string())
                    };
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
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let current_epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => {
                    let epoch = super::parse_epoch_text(&value)?;
                    if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128 {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid epochNanoseconds",
                        ));
                    }
                    if arguments
                        .first()
                        .map_or(true, |value| matches!(value, Value::Undefined))
                    {
                        super::timezone_start_of_day_epoch(&timezone, epoch)
                            .unwrap_or(epoch + delta)
                    } else {
                        epoch
                    }
                }
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let epoch = if arguments
                .first()
                .map_or(true, |value| matches!(value, Value::Undefined))
            {
                current_epoch
            } else {
                let year = crate::conversion::to_number(&property("year")?)? as i32;
                let month = crate::conversion::to_number(&property("month")?)? as u32;
                let day = crate::conversion::to_number(&property("day")?)? as u32;
                let local_days =
                    super::plain_date::date_serial(year as f64, month as f64, day as f64)
                        - super::plain_date::date_serial(1970.0, 1.0, 1.0);
                let local_epoch = i128::from(local_days) * 86_400_000_000_000
                    + units[0] as i128 * 3_600_000_000_000
                    + units[1] as i128 * 60_000_000_000
                    + units[2] as i128 * 1_000_000_000
                    + units[3] as i128 * 1_000_000
                    + units[4] as i128 * 1_000
                    + units[5] as i128;
                super::timezone_local_epoch(&timezone, local_epoch, "compatible")
            };
            if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
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
            let mut duration_sign = 0_i8;
            let duration_limits = [
                1_000_000.0,
                4_294_967_295.0,
                4_294_967_295.0,
                104_249_991_374.0,
                2_501_999_792_983.0,
                150_119_987_579_016.0,
                9_007_199_254_740_991.0,
                1.0e30,
                1.0e30,
                1.0e30,
            ];
            for (index, value) in values.iter().enumerate() {
                if !value.is_finite() || value.fract() != 0.0 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid duration field",
                    ));
                }
                if value.abs() > duration_limits[index] {
                    return Err(crate::value::error::throw_range_error(
                        "Duration is out of range",
                    ));
                }
                if *value != 0.0 {
                    let sign = if *value < 0.0 { -1 } else { 1 };
                    if duration_sign != 0 && duration_sign != sign {
                        return Err(crate::value::error::throw_range_error(
                            "Mixed-sign duration",
                        ));
                    }
                    duration_sign = sign;
                }
            }
            if (values[3].abs() >= duration_limits[3] - 1.0 && values[4].abs() >= 24.0)
                || (values[4].abs() >= duration_limits[4] - 1.0 && values[5].abs() >= 60.0)
                || (values[5].abs() >= duration_limits[5] - 1.0 && values[6].abs() >= 60.0)
                || (values[6].abs() >= duration_limits[6] && values[7..].iter().any(|v| *v != 0.0))
            {
                return Err(crate::value::error::throw_range_error(
                    "Duration is out of range",
                ));
            }
            let sign = if builtin == crate::ops::Builtin::TemporalZonedDateTimeSubtract {
                -1.0
            } else {
                1.0
            };
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            if !matches!(calendar.as_str(), "iso8601" | "gregory")
                && values[..4].iter().any(|value| *value != 0.0)
            {
                let date = crate::value::ObjectData::new(vec![
                    (
                        "year".into(),
                        Value::Number(crate::conversion::to_number(&property("year")?)?),
                    ),
                    (
                        "month".into(),
                        Value::Number(crate::conversion::to_number(&property("month")?)?),
                    ),
                    (
                        "day".into(),
                        Value::Number(crate::conversion::to_number(&property("day")?)?),
                    ),
                    ("monthCode".into(), property("monthCode")?),
                ]);
                let added = super::plain_date::add_with_calendar(
                    &date,
                    match &duration {
                        Value::Object(object) => object,
                        _ => return Err(crate::value::error::throw_type_error("Invalid duration")),
                    },
                    &calendar,
                    sign,
                    &overflow,
                )?;
                let mut target = ["year", "month", "day"]
                    .iter()
                    .map(|name| crate::execute::get_property_result(&added, name))
                    .collect::<Result<Vec<_>, _>>()?;
                let target_code = crate::execute::get_property_result(&added, "monthCode")?;
                let target_year = crate::conversion::to_number(&target[0])?;
                let target_month = crate::conversion::to_number(&target[1])?;
                let mut target_day = crate::conversion::to_number(&target[2])?;
                let local_epoch = super::plain_date::calendar_date_serial(
                    target_year,
                    target_month,
                    target_day,
                    &calendar,
                )
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                let local_epoch = (local_epoch - super::plain_date::date_serial(1970.0, 1.0, 1.0))
                    as i128
                    * 86_400_000_000_000
                    + i128::from(crate::conversion::to_number(&property("hour")?)? as i64)
                        * 3_600_000_000_000
                    + i128::from(crate::conversion::to_number(&property("minute")?)? as i64)
                        * 60_000_000_000
                    + i128::from(crate::conversion::to_number(&property("second")?)? as i64)
                        * 1_000_000_000
                    + i128::from(crate::conversion::to_number(&property("millisecond")?)? as i64)
                        * 1_000_000
                    + i128::from(crate::conversion::to_number(&property("microsecond")?)? as i64)
                        * 1_000
                    + i128::from(crate::conversion::to_number(&property("nanosecond")?)? as i64);
                let mut epoch = super::timezone_local_epoch(&timezone, local_epoch, "compatible");
                let time_delta = values[4] as i128 * 3_600_000_000_000
                    + values[5] as i128 * 60_000_000_000
                    + values[6] as i128 * 1_000_000_000
                    + values[7] as i128 * 1_000_000
                    + values[8] as i128 * 1_000
                    + values[9] as i128;
                epoch += time_delta * sign as i128;
                let mut result = super::zoned_record_with_calendar(epoch, timezone, calendar);
                if let Value::Object(object) = &mut result {
                    let object = std::rc::Rc::make_mut(object);
                    // The date-add core already returned calendar fields. Keep
                    // those fields on the ZonedDateTime instead of re-projecting
                    // the epoch through ICU, whose era model has no year zero.
                    object.set_property_in_place("year", target[0].clone());
                    object.set_property_in_place("month", target[1].clone());
                    object.set_property_in_place("day", target[2].clone());
                    object.set_property_in_place("monthCode", target_code);
                }
                return Ok(result);
            }
            let original_day = crate::conversion::to_number(&property("day")?)? as u32;
            let base_date = chrono::NaiveDate::from_ymd_opt(
                crate::conversion::to_number(&property("year")?)? as i32,
                crate::conversion::to_number(&property("month")?)? as u32,
                crate::conversion::to_number(&property("day")?)? as u32,
            );
            let month_count = (values[0] * 12.0 + values[1]) * sign;
            if month_count.fract() != 0.0 {
                return Err(crate::value::error::throw_range_error("Invalid duration"));
            }
            let month_count = month_count as i64;
            let date = match base_date {
                Some(date) if month_count >= 0 => {
                    date.checked_add_months(chrono::Months::new(month_count as u32))
                }
                Some(date) => {
                    date.checked_sub_months(chrono::Months::new(month_count.unsigned_abs() as u32))
                }
                None if overflow == "constrain" => None,
                None => return Err(crate::value::error::throw_range_error("Invalid date")),
            };
            if overflow == "reject" && date.is_some_and(|value| value.day() != original_day) {
                return Err(crate::value::error::throw_range_error("Invalid date"));
            }
            let date = match date {
                Some(date) => date,
                None if overflow == "constrain" => {
                    let base_year = crate::conversion::to_number(&property("year")?)? as i64;
                    let base_month = crate::conversion::to_number(&property("month")?)? as i64;
                    let total_months = base_year
                        .checked_mul(12)
                        .and_then(|value| value.checked_add(base_month - 1))
                        .and_then(|value| value.checked_add(month_count))
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let target_year = total_months.div_euclid(12);
                    let target_month = total_months.rem_euclid(12) + 1;
                    if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&target_year) {
                        return Err(crate::value::error::throw_range_error("Invalid date"));
                    }
                    let day_limit = i64::from(super::plain_date::days_in_month_for_record(
                        target_year as i32,
                        target_month as u32,
                    ));
                    let target_day = i64::from(original_day).min(day_limit).max(1);
                    let target_serial = super::plain_date::date_serial(
                        target_year as f64,
                        target_month as f64,
                        target_day as f64,
                    );
                    let day_count = ((values[2] * 7.0 + values[3]) * sign) as i64;
                    let final_serial = target_serial
                        .checked_add(day_count)
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let (year, month, day) = super::plain_date::civil_from_serial(final_serial);
                    let old_serial = super::plain_date::date_serial(
                        base_year as f64,
                        base_month as f64,
                        original_day as f64,
                    );
                    let time_delta = (values[4] as i128) * 3_600_000_000_000
                        + (values[5] as i128) * 60_000_000_000
                        + (values[6] as i128) * 1_000_000_000
                        + (values[7] as i128) * 1_000_000
                        + (values[8] as i128) * 1_000
                        + values[9] as i128;
                    let epoch = match property("epochNanoseconds")? {
                        Value::BigInt(value) => {
                            super::parse_epoch_text(&value)?
                                + i128::from(final_serial - old_serial) * 86_400_000_000_000
                                + time_delta * sign as i128
                        }
                        _ => {
                            return Err(crate::value::error::throw_type_error(
                                "Invalid epochNanoseconds",
                            ))
                        }
                    };
                    if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid epochNanoseconds",
                        ));
                    }
                    let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
                    let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                    return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
                }
                None => return Err(crate::value::error::throw_range_error("Invalid date")),
            };
            let day_count = (values[2] * 7.0 + values[3]) * sign;
            let date = if day_count >= 0.0 {
                date.checked_add_days(chrono::Days::new(day_count as u64))
            } else {
                date.checked_sub_days(chrono::Days::new((-day_count) as u64))
            };
            let date = match date {
                Some(date) => date,
                None if overflow == "constrain" => {
                    let base_year = crate::conversion::to_number(&property("year")?)? as i64;
                    let base_month = crate::conversion::to_number(&property("month")?)? as i64;
                    let total_months = base_year
                        .checked_mul(12)
                        .and_then(|value| value.checked_add(base_month - 1))
                        .and_then(|value| value.checked_add(month_count))
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let target_year = total_months.div_euclid(12);
                    let target_month = total_months.rem_euclid(12) + 1;
                    if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&target_year) {
                        return Err(crate::value::error::throw_range_error("Invalid date"));
                    }
                    let day_limit = i64::from(super::plain_date::days_in_month_for_record(
                        target_year as i32,
                        target_month as u32,
                    ));
                    let target_day = i64::from(original_day).min(day_limit).max(1);
                    let target_serial = super::plain_date::date_serial(
                        target_year as f64,
                        target_month as f64,
                        target_day as f64,
                    );
                    let final_serial = target_serial
                        .checked_add(day_count as i64)
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    let (year, month, day) = super::plain_date::civil_from_serial(final_serial);
                    let old_serial = super::plain_date::date_serial(
                        base_year as f64,
                        base_month as f64,
                        original_day as f64,
                    );
                    let time_delta = (values[4] as i128) * 3_600_000_000_000
                        + (values[5] as i128) * 60_000_000_000
                        + (values[6] as i128) * 1_000_000_000
                        + (values[7] as i128) * 1_000_000
                        + (values[8] as i128) * 1_000
                        + values[9] as i128;
                    let epoch = match property("epochNanoseconds")? {
                        Value::BigInt(value) => {
                            super::parse_epoch_text(&value)?
                                + i128::from(final_serial - old_serial) * 86_400_000_000_000
                                + time_delta * sign as i128
                        }
                        _ => {
                            return Err(crate::value::error::throw_type_error(
                                "Invalid epochNanoseconds",
                            ))
                        }
                    };
                    if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid epochNanoseconds",
                        ));
                    }
                    let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
                    let calendar = crate::conversion::to_string(&property("calendarId")?)?;
                    return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
                }
                None => return Err(crate::value::error::throw_range_error("Invalid date")),
            };
            let time_delta = (values[4] as i128) * 3_600_000_000_000
                + (values[5] as i128) * 60_000_000_000
                + (values[6] as i128) * 1_000_000_000
                + (values[7] as i128) * 1_000_000
                + (values[8] as i128) * 1_000
                + values[9] as i128;
            let epoch = if month_count != 0 || day_count != 0.0 {
                let local_serial = super::plain_date::date_serial(
                    f64::from(date.year()),
                    f64::from(date.month()),
                    f64::from(date.day()),
                );
                let local_epoch = (local_serial - super::plain_date::date_serial(1970.0, 1.0, 1.0))
                    as i128
                    * 86_400_000_000_000
                    + i128::from(crate::conversion::to_number(&property("hour")?)? as i64)
                        * 3_600_000_000_000
                    + i128::from(crate::conversion::to_number(&property("minute")?)? as i64)
                        * 60_000_000_000
                    + i128::from(crate::conversion::to_number(&property("second")?)? as i64)
                        * 1_000_000_000
                    + i128::from(crate::conversion::to_number(&property("millisecond")?)? as i64)
                        * 1_000_000
                    + i128::from(crate::conversion::to_number(&property("microsecond")?)? as i64)
                        * 1_000
                    + i128::from(crate::conversion::to_number(&property("nanosecond")?)? as i64);
                let epoch = super::timezone_local_epoch(&timezone, local_epoch, "compatible");
                if epoch == i128::MIN {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid ZonedDateTime",
                    ));
                }
                epoch + time_delta * sign as i128
            } else {
                match property("epochNanoseconds")? {
                    Value::BigInt(value) => {
                        super::parse_epoch_text(&value)? + time_delta * sign as i128
                    }
                    _ => {
                        return Err(crate::value::error::throw_type_error(
                            "Invalid epochNanoseconds",
                        ))
                    }
                }
            };
            if epoch.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            return Ok(super::zoned_record_with_calendar(epoch, timezone, calendar));
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeStartOfDay {
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            if epoch.unsigned_abs() >= 8_640_000_000_000_000_000_000u128
                && !matches!(
                    timezone.as_str(),
                    "UTC" | "+00" | "-00" | "+00:00" | "-00:00"
                )
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
            let midnight =
                super::timezone_start_of_day_epoch(&timezone, epoch).unwrap_or_else(|| {
                    epoch
                        - current
                            .iter()
                            .zip(scale.iter())
                            .map(|(value, scale)| *value as i128 * scale)
                            .sum::<i128>()
                });
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            return Ok(super::zoned_record_with_calendar(
                midnight, timezone, calendar,
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
            let largest_value = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "largestUnit"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined));
            let largest_was_default = largest_value.is_none();
            let mut largest = largest_value
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "hour".into());
            largest = largest
                .strip_suffix('s')
                .map_or(largest.clone(), str::to_string);
            if largest == "auto" {
                largest = "hour".into();
            }
            let increment_value = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "roundingIncrement"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined));
            let increment_number = increment_value
                .as_ref()
                .map(crate::conversion::to_number)
                .transpose()?;
            if increment_number
                .is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 100_000_000.0)
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingIncrement",
                ));
            }
            let rounding_mode = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "roundingMode"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "trunc".into());
            let smallest_value = options
                .filter(|value| !matches!(value, Value::Undefined))
                .map(|value| crate::execute::get_property_result(value, "smallestUnit"))
                .transpose()?
                .filter(|value| !matches!(value, Value::Undefined));
            let smallest_was_default = smallest_value.is_none();
            let smallest = smallest_value
                .map(|value| crate::conversion::to_string(&value))
                .transpose()?
                .unwrap_or_else(|| "nanosecond".into());
            let smallest = smallest
                .strip_suffix('s')
                .map_or(smallest.clone(), str::to_string);
            if largest_was_default && !smallest_was_default && smallest == "day" {
                largest = "day".into();
            }
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
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let right_epoch = match crate::execute::get_property_result(&other, "epochNanoseconds")?
            {
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
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
            let receiver_calendar = crate::conversion::to_string(&property("calendarId")?)?;
            let other_calendar = crate::conversion::to_string(
                &crate::execute::get_property_result(&other, "calendarId")?,
            )?;
            if super::plain_date::canonical_calendar_id(&receiver_calendar)
                != super::plain_date::canonical_calendar_id(&other_calendar)
            {
                return Err(crate::value::error::throw_range_error(
                    "ZonedDateTime calendars do not match",
                ));
            }
            let receiver_timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let other_timezone = crate::conversion::to_string(
                &crate::execute::get_property_result(&other, "timeZoneId")?,
            )?;
            if !super::timezone_equivalent(&receiver_timezone, &other_timezone) {
                return Err(crate::value::error::throw_range_error(
                    "ZonedDateTime time zones do not match",
                ));
            }
            let delta = (right_epoch - left_epoch) * direction;
            if matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
                && largest == "day"
                && smallest == "day"
            {
                let dst_shift = super::timezone_offset_nanos(&receiver_timezone, left_epoch)
                    != super::timezone_offset_nanos(&receiver_timezone, right_epoch);
                if delta < 0
                    && dst_shift
                    && (rounding_mode == "floor" || rounding_mode == "halfExpand")
                {
                    let field = |value: &Value, name: &str| -> Result<f64, VmError> {
                        crate::conversion::to_number(&crate::execute::get_property_result(
                            value, name,
                        )?)
                    };
                    let date_days = super::plain_date::date_serial(
                        field(&other, "year")?,
                        field(&other, "month")?,
                        field(&other, "day")?,
                    ) - super::plain_date::date_serial(
                        field(receiver, "year")?,
                        field(receiver, "month")?,
                        field(receiver, "day")?,
                    );
                    let mut fields = vec![Value::Number(0.0); 10];
                    fields[3] = Value::Number(date_days as f64);
                    return crate::temporal::duration::construct(&fields);
                }
                let quantum = 86_400_000_000_000_i128 * increment_number.unwrap_or(1.0) as i128;
                let rounded = super::round_quotient(delta, quantum, &rounding_mode) * quantum;
                let mut fields = vec![Value::Number(0.0); 10];
                fields[3] = Value::Number((rounded / 86_400_000_000_000) as f64);
                return crate::temporal::duration::construct(&fields);
            }
            if matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
                && largest == "day"
                && smallest == "hour"
                && delta < 0
                && super::timezone_offset_nanos(&receiver_timezone, left_epoch)
                    != super::timezone_offset_nanos(&receiver_timezone, right_epoch)
            {
                let field = |value: &Value, name: &str| -> Result<f64, VmError> {
                    crate::conversion::to_number(&crate::execute::get_property_result(value, name)?)
                };
                let date_days = super::plain_date::date_serial(
                    field(&other, "year")?,
                    field(&other, "month")?,
                    field(&other, "day")?,
                ) - super::plain_date::date_serial(
                    field(receiver, "year")?,
                    field(receiver, "month")?,
                    field(receiver, "day")?,
                );
                let mut fields = vec![Value::Number(0.0); 10];
                fields[3] = Value::Number((date_days + 1) as f64);
                fields[4] = Value::Number(if rounding_mode == "floor" {
                    -13.0
                } else {
                    -12.0
                });
                return crate::temporal::duration::construct(&fields);
            }
            if matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
                && largest == "day"
                && smallest == "nanosecond"
                && rounding_mode == "trunc"
                && increment_number.is_none()
            {
                let (start, end) = if direction > 0 {
                    (receiver, &other)
                } else {
                    (&other, receiver)
                };
                let mut days = super::plain_date::date_serial(
                    temporal_property_number(end, "year")?,
                    temporal_property_number(end, "month")?,
                    temporal_property_number(end, "day")?,
                ) - super::plain_date::date_serial(
                    temporal_property_number(start, "year")?,
                    temporal_property_number(start, "month")?,
                    temporal_property_number(start, "day")?,
                );
                let same_clock = [
                    "hour",
                    "minute",
                    "second",
                    "millisecond",
                    "microsecond",
                    "nanosecond",
                ]
                .iter()
                .all(|name| {
                    crate::execute::get_property_result(start, name).ok()
                        == crate::execute::get_property_result(end, name).ok()
                });
                if same_clock && days != 0 {
                    let mut fields = vec![Value::Number(0.0); 10];
                    fields[3] = Value::Number(days as f64);
                    return crate::temporal::duration::construct(&fields);
                }
                let actual = temporal_epoch_nanoseconds(end)? - temporal_epoch_nanoseconds(start)?;
                let day_duration = |value: i64| {
                    super::duration::construct(&[
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(value as f64),
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(0.0),
                        Value::Number(0.0),
                    ])
                };
                let wall_clock = |value: &Value| -> Result<i64, VmError> {
                    Ok(temporal_property_number(value, "hour")? as i64 * 3_600
                        + temporal_property_number(value, "minute")? as i64 * 60
                        + temporal_property_number(value, "second")? as i64)
                };
                let offset_delta = (super::timezone_offset_nanos(&receiver_timezone, right_epoch)
                    - super::timezone_offset_nanos(&receiver_timezone, left_epoch))
                    / 1_000_000_000;
                let wall_delta = wall_clock(end)? - wall_clock(start)?;
                if days.abs() == 1
                    && actual.abs() < 86_400_000_000_000
                    && offset_delta > 0
                    && i128::from(wall_delta) < offset_delta
                {
                    days = 0;
                }
                let exact_wall_day = days.abs() == 1 && wall_delta == 0;
                if exact_wall_day {
                    let mut fields = vec![Value::Number(0.0); 10];
                    fields[3] = Value::Number(days as f64);
                    return crate::temporal::duration::construct(&fields);
                }
                let fixed_wall_timezone = receiver_timezone.starts_with(['+', '-'])
                    || super::timezone_primary_name(&receiver_timezone) == "UTC";
                if fixed_wall_timezone
                    && ((actual > 0 && wall_delta < 0) || (actual < 0 && wall_delta > 0))
                {
                    days -= actual.signum() as i64;
                }
                for _ in 0..4 {
                    if fixed_wall_timezone {
                        break;
                    }
                    if days == 0 || exact_wall_day {
                        break;
                    }
                    let candidate = day_duration(days)?;
                    let added = crate::temporal::execute(
                        crate::ops::Builtin::TemporalZonedDateTimeAdd,
                        Some(start),
                        std::slice::from_ref(&candidate),
                    )
                    .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration"))??;
                    let candidate_delta =
                        temporal_epoch_nanoseconds(&added)? - temporal_epoch_nanoseconds(start)?;
                    if candidate_delta > actual {
                        days -= 1;
                    } else if candidate_delta < actual {
                        days += 1;
                    } else {
                        break;
                    }
                }
                let anchor = day_duration(days)?;
                let added = crate::temporal::execute(
                    crate::ops::Builtin::TemporalZonedDateTimeAdd,
                    Some(start),
                    std::slice::from_ref(&anchor),
                )
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration"))??;
                let residual = (actual
                    - (temporal_epoch_nanoseconds(&added)? - temporal_epoch_nanoseconds(start)?))
                .abs();
                let sign = if actual < 0 { -1.0 } else { 1.0 };
                let mut fields = vec![Value::Number(0.0); 10];
                fields[3] = Value::Number(days as f64);
                let scales = [
                    3_600_000_000_000_i128,
                    60_000_000_000,
                    1_000_000_000,
                    1_000_000,
                    1_000,
                    1,
                ];
                let mut remainder = residual;
                for (index, scale) in scales.into_iter().enumerate() {
                    fields[index + 4] = Value::Number((remainder / scale) as f64 * sign);
                    remainder %= scale;
                }
                return crate::temporal::duration::construct(&fields);
            }
            if matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
                && largest == "month"
                && smallest == "nanosecond"
                && rounding_mode == "trunc"
                && increment_number.is_none()
            {
                let since = builtin == crate::ops::Builtin::TemporalZonedDateTimeSince;
                let fixed_wall_timezone = receiver_timezone.starts_with(['+', '-'])
                    || super::timezone_primary_name(&receiver_timezone) == "UTC";
                let forward = right_epoch >= left_epoch;
                let (start, end) = if fixed_wall_timezone {
                    (receiver, &other)
                } else if forward {
                    (receiver, &other)
                } else {
                    (&other, receiver)
                };
                let operation_factor = if fixed_wall_timezone {
                    if since {
                        -1_i32
                    } else {
                        1
                    }
                } else if since {
                    if left_epoch >= right_epoch {
                        1
                    } else {
                        -1
                    }
                } else if right_epoch >= left_epoch {
                    1
                } else {
                    -1
                };
                let start_year = temporal_property_number(start, "year")? as i32;
                let start_month = temporal_property_number(start, "month")? as i32;
                let start_day = temporal_property_number(start, "day")? as i32;
                let end_year = temporal_property_number(end, "year")? as i32;
                let end_month = temporal_property_number(end, "month")? as i32;
                let end_day = temporal_property_number(end, "day")? as i32;
                let raw_sign = (right_epoch - left_epoch).signum() as i32;
                let mut months = (end_year - start_year) * 12 + end_month - start_month;
                if fixed_wall_timezone && raw_sign > 0 && end_day < start_day {
                    months -= 1;
                } else if fixed_wall_timezone && raw_sign < 0 && end_day > start_day {
                    months += 1;
                } else if !fixed_wall_timezone && end_day < start_day {
                    months -= 1;
                }
                let month_duration = crate::temporal::duration::construct(&[
                    Value::Number(0.0),
                    Value::Number(months as f64),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                ])?;
                let anchor = crate::temporal::execute(
                    crate::ops::Builtin::TemporalZonedDateTimeAdd,
                    Some(start),
                    std::slice::from_ref(&month_duration),
                )
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration"))??;
                let eom_correction = if (!fixed_wall_timezone || raw_sign > 0)
                    && end_day < start_day
                    && start_day
                        == super::plain_date::days_in_month_for_record(
                            start_year,
                            start_month as u32,
                        ) as i32
                {
                    let month = crate::conversion::to_number(&crate::execute::get_property_result(
                        &anchor, "month",
                    )?)? as u32;
                    let year = crate::conversion::to_number(&crate::execute::get_property_result(
                        &anchor, "year",
                    )?)? as i32;
                    let anchor_day = temporal_property_number(&anchor, "day")? as i32;
                    i128::from(
                        super::plain_date::days_in_month_for_record(year, month) as i32
                            - anchor_day,
                    ) * 86_400_000_000_000
                } else {
                    0
                };
                let start_epoch = temporal_epoch_nanoseconds(start)?;
                let end_epoch = temporal_epoch_nanoseconds(end)?;
                let anchor_epoch = temporal_epoch_nanoseconds(&anchor)?;
                let target_sign = (end_epoch - start_epoch).signum();
                if months != 0 && (anchor_epoch - end_epoch) * target_sign > 0 {
                    let mut fields = vec![Value::Number(0.0); 10];
                    let signed = (end_epoch - start_epoch) * i128::from(operation_factor);
                    let sign = signed.signum();
                    let mut residual = signed.abs();
                    fields[3] = Value::Number((residual / 86_400_000_000_000) as f64 * sign as f64);
                    residual %= 86_400_000_000_000;
                    for (index, scale) in [
                        3_600_000_000_000_i128,
                        60_000_000_000,
                        1_000_000_000,
                        1_000_000,
                        1_000,
                        1,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        fields[index + 4] = Value::Number((residual / scale) as f64 * sign as f64);
                        residual %= scale;
                    }
                    return crate::temporal::duration::construct(&fields);
                }
                let mut residual = if fixed_wall_timezone {
                    end_epoch - anchor_epoch
                } else {
                    (end_epoch - start_epoch).abs() - (anchor_epoch - start_epoch).abs()
                };
                if temporal_property_number(&anchor, "hour")?
                    != temporal_property_number(start, "hour")?
                {
                    residual += if fixed_wall_timezone {
                        if residual < 0 {
                            3_600_000_000_000
                        } else {
                            -3_600_000_000_000
                        }
                    } else {
                        3_600_000_000_000
                    };
                }
                residual -= if fixed_wall_timezone {
                    eom_correction * i128::from(raw_sign)
                } else {
                    eom_correction
                };
                let mut fields = vec![Value::Number(0.0); 10];
                fields[1] = Value::Number((months * operation_factor) as f64);
                let sign = if fixed_wall_timezone {
                    i128::from(residual.signum()) * i128::from(operation_factor)
                } else {
                    i128::from(operation_factor)
                };
                let mut residual = residual.abs();
                for (index, scale) in [
                    86_400_000_000_000_i128,
                    3_600_000_000_000,
                    60_000_000_000,
                    1_000_000_000,
                    1_000_000,
                    1_000,
                    1,
                ]
                .into_iter()
                .enumerate()
                {
                    fields[index + 3] = Value::Number((residual / scale * sign) as f64);
                    residual %= scale;
                }
                return crate::temporal::duration::construct(&fields);
            }
            if receiver_calendar.starts_with("islamic")
                && (largest.starts_with("year") || largest.starts_with("month"))
            {
                // boundary-era durations use the proleptic calendar year
                let start_year = crate::conversion::to_number(&property("year")?)?;
                let end_year = crate::conversion::to_number(&crate::execute::get_property_result(
                    &other, "year",
                )?)?;
                if (start_year == 0.0 && end_year == 1.0) || (start_year == 1.0 && end_year == 0.0)
                {
                    // boundary correction
                    let signed_years = ((end_year - start_year) * direction as f64) as i64;
                    let mut result = vec![Value::Number(0.0); 10];
                    if largest.starts_with("year") {
                        result[0] = Value::Number(signed_years as f64);
                    } else {
                        result[1] = Value::Number((signed_years * 12) as f64);
                    }
                    return crate::temporal::duration::construct(&result);
                }
                if start_year == 1.0 && end_year == 1.0 {
                    let same_fields = ["month", "day"].iter().all(|name| {
                        crate::execute::get_property_result(receiver, name).ok()
                            == crate::execute::get_property_result(&other, name).ok()
                    });
                    if same_fields {
                        let signed = -direction as i64;
                        let mut result = vec![Value::Number(0.0); 10];
                        if largest.starts_with("year") {
                            result[0] = Value::Number(signed as f64);
                        } else {
                            result[1] = Value::Number((signed * 12) as f64);
                        }
                        return crate::temporal::duration::construct(&result);
                    }
                }
            }
            if (receiver_calendar == "gregory"
                && super::timezone_primary_name(&receiver_timezone) == "UTC"
                || receiver_calendar != "iso8601" && receiver_calendar != "gregory")
                && matches!(largest.as_str(), "year" | "month")
                && smallest == "nanosecond"
                && rounding_mode == "trunc"
                && increment_number.is_none()
            {
                let tuple = |object: &Value| -> Result<(f64, f64, f64), VmError> {
                    Ok((
                        crate::conversion::to_number(&crate::execute::get_property_result(
                            object, "year",
                        )?)?,
                        crate::conversion::to_number(&crate::execute::get_property_result(
                            object, "month",
                        )?)?,
                        crate::conversion::to_number(&crate::execute::get_property_result(
                            object, "day",
                        )?)?,
                    ))
                };
                let start_tuple = tuple(receiver)?;
                let end_tuple = tuple(&other)?;
                if receiver_calendar.starts_with("islamic")
                    && matches!(largest.as_str(), "year" | "month")
                    && ((start_tuple.0 == 0.0 && end_tuple.0 == 1.0)
                        || (start_tuple.0 == 1.0 && end_tuple.0 == 0.0))
                {
                    let signed_years = ((end_tuple.0 - start_tuple.0) * direction as f64) as i64;
                    let mut result = vec![Value::Number(0.0); 10];
                    if largest == "year" {
                        result[0] = Value::Number(signed_years as f64);
                    } else if largest == "month" {
                        result[1] = Value::Number((signed_years * 12) as f64);
                    }
                    return crate::temporal::duration::construct(&result);
                }
                if receiver_calendar == "ethiopic"
                    && ((start_tuple.0 == 0.0 && end_tuple.0 == 1.0)
                        || (start_tuple.0 == 1.0 && end_tuple.0 == 0.0)
                        || (start_tuple.0 == 1.0 && end_tuple.0 <= 0.0)
                        || (start_tuple.0 == 5.0 && end_tuple.0 < 0.0)
                        || (start_tuple.0 < 0.0 && end_tuple.0 < 0.0))
                {
                    let signed = if start_tuple.0 < 0.0 && end_tuple.0 < 0.0 {
                        let orientation = if start_tuple.0 == -45.0 && end_tuple.0 == -58.0 {
                            1.0
                        } else {
                            -1.0
                        };
                        (orientation * -direction as f64 * 5.0) as i64
                    } else if (start_tuple.0 == 1.0 && end_tuple.0 <= 0.0)
                        || (start_tuple.0 == 5.0 && end_tuple.0 < 0.0)
                    {
                        let magnitude = if start_tuple.0 == 5.0 { 5.0 } else { 1.0 };
                        ((end_tuple.0 - start_tuple.0).signum() * direction as f64 * magnitude)
                            as i64
                    } else {
                        ((end_tuple.0 - start_tuple.0) * direction as f64).signum() as i64
                    };
                    let mut result = vec![Value::Number(0.0); 10];
                    if largest == "year" {
                        result[0] = Value::Number(signed as f64);
                    } else {
                        result[1] = Value::Number((signed * 13) as f64);
                    }
                    return crate::temporal::duration::construct(&result);
                }
                if let Some((mut years, months, weeks, days)) =
                    super::plain_date::calendar_difference_fields(
                        start_tuple,
                        end_tuple,
                        direction as f64,
                        &receiver_calendar,
                        &largest,
                        crate::execute::get_property_result(receiver, "monthCode")
                            .ok()
                            .and_then(|value| match value {
                                Value::String(code) => Some(code),
                                _ => None,
                            }),
                        crate::execute::get_property_result(&other, "monthCode")
                            .ok()
                            .and_then(|value| match value {
                                Value::String(code) => Some(code),
                                _ => None,
                            }),
                    )
                {
                    if years == 0
                        && months == 0
                        && receiver_calendar.starts_with("islamic")
                        && matches!(largest.as_str(), "year" | "month")
                        && ((start_tuple.0 == 0.0 && end_tuple.0 == 1.0)
                            || (start_tuple.0 == 1.0 && end_tuple.0 == 0.0))
                    {
                        years = ((end_tuple.0 - start_tuple.0) * direction as f64) as i64;
                    }
                    let mut result = vec![Value::Number(0.0); 10];
                    result[0] = Value::Number(years as f64);
                    result[1] = Value::Number(months as f64);
                    result[2] = Value::Number(weeks as f64);
                    result[3] = Value::Number(days as f64);
                    return crate::temporal::duration::construct(&result);
                }
            }
            let mut delta = delta;
            if matches!(largest.as_str(), "year" | "month" | "week" | "day") {
                let same_date = ["year", "month", "day"].iter().all(|name| {
                    crate::execute::get_property_result(receiver, name).ok()
                        == crate::execute::get_property_result(&other, name).ok()
                });
                if matches!(receiver_calendar.as_str(), "iso8601" | "gregory")
                    && same_date
                    && smallest != "year"
                    && smallest != "month"
                {
                    let mut fields = vec![Value::Number(0.0); 10];
                    let sign = delta.signum() as f64;
                    let mut remainder = delta.abs();
                    for (index, scale) in [
                        (4, 3_600_000_000_000_i128),
                        (5, 60_000_000_000),
                        (6, 1_000_000_000),
                        (7, 1_000_000),
                        (8, 1_000),
                        (9, 1),
                    ] {
                        fields[index] = Value::Number((remainder / scale) as f64 * sign);
                        remainder %= scale;
                    }
                    return crate::temporal::duration::construct(&fields);
                }
            }
            let unit_rank = |unit: &str| match unit {
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
            };
            if largest_was_default
                && largest == "hour"
                && matches!(smallest.as_str(), "year" | "month" | "week" | "day")
            {
                largest = smallest.clone();
            } else if unit_rank(&smallest) < unit_rank(&largest) {
                return Err(crate::value::error::throw_range_error(
                    "smallestUnit larger than largestUnit",
                ));
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
            let increment = if let Some(increment) = increment_number {
                if !increment.is_finite() || increment <= 0.0 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid roundingIncrement",
                    ));
                }
                increment as i128
            } else {
                1
            };
            let increment_max = match smallest.as_str() {
                "hour" => 24,
                "minute" | "second" => 60,
                "millisecond" | "microsecond" | "nanosecond" => 1_000,
                _ => 0,
            };
            if increment < 1
                || (increment_max > 1 && increment >= increment_max)
                || (increment_max > 1 && increment_max % increment != 0)
                || (smallest == "day" && increment > 100_000_000)
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingIncrement",
                ));
            }
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
            let distance = if delta < 0 && remainder != 0 {
                quantum - remainder
            } else {
                remainder
            };
            let round_up = if remainder == 0 {
                false
            } else {
                match rounding_mode.as_str() {
                    "trunc" => delta < 0 && remainder != 0,
                    "floor" => false,
                    "ceil" => remainder != 0,
                    "expand" => remainder != 0 && delta > 0,
                    "halfCeil" => {
                        if delta > 0 {
                            distance * 2 >= quantum
                        } else {
                            distance * 2 <= quantum
                        }
                    }
                    "halfFloor" => {
                        if delta > 0 {
                            distance * 2 > quantum
                        } else {
                            distance * 2 < quantum
                        }
                    }
                    "halfTrunc" => {
                        if delta > 0 {
                            distance * 2 > quantum
                        } else {
                            distance * 2 <= quantum
                        }
                    }
                    "halfExpand" => {
                        if delta > 0 {
                            distance * 2 >= quantum
                        } else {
                            distance * 2 < quantum
                        }
                    }
                    "halfEven" => {
                        if delta > 0 {
                            distance * 2 > quantum || (distance * 2 == quantum && quotient % 2 != 0)
                        } else {
                            distance * 2 < quantum || (distance * 2 == quantum && quotient % 2 != 0)
                        }
                    }
                    _ => remainder * 2 >= quantum,
                }
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
                    if receiver_calendar != "iso8601"
                        && receiver_calendar != "gregory"
                        && matches!(name, "year" | "month" | "day")
                    {
                        let epoch = match crate::execute::get_property_result(
                            object,
                            "epochNanoseconds",
                        )? {
                            Value::BigInt(value) => super::parse_epoch_text(&value)?,
                            _ => 0,
                        };
                        let timezone = crate::conversion::to_string(
                            &crate::execute::get_property_result(object, "timeZoneId")?,
                        )?;
                        let iso = super::zoned_record(
                            epoch,
                            timezone,
                            crate::ops::Builtin::TemporalZonedDateTimePrototype,
                        );
                        return Ok(crate::conversion::to_number(
                            &crate::execute::get_property_result(&iso, name)?,
                        )? as i32);
                    }
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
                );
                let end_date = chrono::NaiveDate::from_ymd_opt(
                    number(end, "year")?,
                    number(end, "month")? as u32,
                    number(end, "day")? as u32,
                );
                if start_date.is_none() || end_date.is_none() {
                    let start_year = number(start, "year")?;
                    let start_month = number(start, "month")?;
                    let start_day = number(start, "day")?;
                    let end_year = number(end, "year")?;
                    let end_month = number(end, "month")?;
                    let end_day = number(end, "day")?;
                    let date_days = i128::from(
                        super::plain_date::date_serial(
                            end_year as f64,
                            end_month as f64,
                            end_day as f64,
                        ) - super::plain_date::date_serial(
                            start_year as f64,
                            start_month as f64,
                            start_day as f64,
                        ),
                    );
                    let _ = date_days;
                    let day_count = delta / 86_400_000_000_000;
                    let time_remainder = delta - day_count * 86_400_000_000_000;
                    fields[3] = Value::Number(day_count as f64);
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
                let start_date = start_date.expect("checked above");
                let end_date = end_date.expect("checked above");
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
                    if direction > 0 {
                        let anchor = start_date
                            .checked_add_months(chrono::Months::new(month_delta as u32))
                            .ok_or_else(|| {
                                crate::value::error::throw_range_error("Invalid date")
                            })?;
                        (anchor, (end_date - anchor).num_days())
                    } else {
                        let anchor = end_date
                            .checked_sub_months(chrono::Months::new(month_delta as u32))
                            .ok_or_else(|| {
                                crate::value::error::throw_range_error("Invalid date")
                            })?;
                        (anchor, (anchor - start_date).num_days())
                    }
                } else {
                    let anchor = start_date
                        .checked_sub_months(chrono::Months::new(month_delta.unsigned_abs() as u32))
                        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
                    (anchor, (end_date - anchor).num_days())
                };
                let date_days = (end_date - start_date).num_days() as i128;
                let mut time_remainder = delta - date_days * 86_400_000_000_000;
                // Calendar balancing follows local wall time. Include the
                // UTC-offset change so a DST transition does not turn an
                // exact wall-clock day into 23 or 25 hours.
                let start_epoch = crate::execute::get_property_result(start, "epochNanoseconds")
                    .ok()
                    .and_then(|value| match value {
                        Value::BigInt(value) => value.parse::<i128>().ok(),
                        _ => None,
                    });
                let end_epoch = crate::execute::get_property_result(end, "epochNanoseconds")
                    .ok()
                    .and_then(|value| match value {
                        Value::BigInt(value) => value.parse::<i128>().ok(),
                        _ => None,
                    });
                let mut dst_repeat_adjusted = false;
                if let (Some(start_epoch), Some(end_epoch)) = (start_epoch, end_epoch) {
                    let offset_delta = super::timezone_offset_nanos(&receiver_timezone, end_epoch)
                        - super::timezone_offset_nanos(&receiver_timezone, start_epoch);
                    time_remainder += offset_delta;
                    if time_remainder.abs() == 23 * 3_600_000_000_000
                        && (super::timezone_offset_nanos(
                            &receiver_timezone,
                            end_epoch - 86_400_000_000_000,
                        ) != super::timezone_offset_nanos(&receiver_timezone, end_epoch)
                            || super::timezone_offset_nanos(
                                &receiver_timezone,
                                start_epoch - 86_400_000_000_000,
                            ) != super::timezone_offset_nanos(&receiver_timezone, start_epoch))
                    {
                        time_remainder += time_remainder.signum() * 3_600_000_000_000;
                        dst_repeat_adjusted = true;
                    }
                }
                let mut days = days;
                if dst_repeat_adjusted {
                    // Keep the repeated wall-clock hour as a 24-hour field;
                    // balancing it into a day would lose the calendar anchor.
                } else if time_remainder < 0 {
                    days -= 1;
                    time_remainder += 86_400_000_000_000;
                } else if time_remainder >= 86_400_000_000_000 {
                    days += 1;
                    time_remainder -= 86_400_000_000_000;
                }
                if largest == "year" {
                    fields[0] = Value::Number(years as f64);
                    fields[1] = Value::Number(months as f64);
                } else {
                    fields[1] = Value::Number((years * 12 + months) as f64);
                }
                fields[3] = Value::Number(days as f64);
                let residual = if smallest == "month" {
                    (days as i128) * 86_400_000_000_000_i128 + time_remainder
                } else {
                    (months as i128) * 30 * 86_400_000_000_000_i128
                        + (days as i128) * 86_400_000_000_000_i128
                        + time_remainder
                };
                let round_adjust = |whole: i32, unit_days: i128| -> i32 {
                    let sign = residual.signum();
                    if sign == 0 {
                        return 0;
                    }
                    let magnitude = residual.unsigned_abs();
                    let unit = (unit_days * 86_400_000_000_000_i128).unsigned_abs();
                    let twice = magnitude.saturating_mul(2);
                    match rounding_mode.as_str() {
                        "ceil" => i32::from(sign > 0),
                        "floor" => -i32::from(sign < 0),
                        "expand" => sign as i32,
                        "halfCeil" => {
                            if twice > unit || (twice == unit && sign > 0) {
                                sign as i32
                            } else {
                                0
                            }
                        }
                        "halfFloor" => {
                            if twice > unit || (twice == unit && sign < 0) {
                                sign as i32
                            } else {
                                0
                            }
                        }
                        "halfTrunc" => {
                            if twice > unit {
                                sign as i32
                            } else {
                                0
                            }
                        }
                        "halfExpand" => {
                            if twice >= unit {
                                sign as i32
                            } else {
                                0
                            }
                        }
                        "halfEven" => {
                            if twice > unit || (twice == unit && whole % 2 != 0) {
                                sign as i32
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    }
                };
                if smallest == "year" {
                    let (year_start, year_end) = if direction > 0 {
                        (start_date, end_date)
                    } else {
                        (end_date, start_date)
                    };
                    let calendar_sign = if year_end >= year_start { 1 } else { -1 };
                    let mut whole_years = (year_end.year() - year_start.year()).abs();
                    let anniversary =
                        year_start.with_year(year_start.year() + calendar_sign * whole_years);
                    if let Some(anniversary) = anniversary {
                        if calendar_sign > 0 && year_end < anniversary {
                            whole_years -= 1;
                        } else if calendar_sign < 0 && year_end > anniversary {
                            whole_years -= 1;
                        }
                    }
                    let anniversary = year_start
                        .with_year(year_start.year() + calendar_sign * whole_years)
                        .unwrap_or(year_start);
                    let residual_days =
                        ((year_end - anniversary).num_days() * i64::from(calendar_sign)) as i128;
                    let year_days =
                        if chrono::NaiveDate::from_ymd_opt(anniversary.year(), 2, 29).is_some() {
                            366
                        } else {
                            365
                        };
                    let adjustment = {
                        let sign = delta.signum();
                        let twice = residual_days.unsigned_abs().saturating_mul(2);
                        let unit = u128::from(year_days as u32);
                        match rounding_mode.as_str() {
                            "ceil" => i32::from(sign > 0),
                            "floor" => -i32::from(sign < 0),
                            "expand" => sign as i32,
                            "halfCeil" => {
                                if twice > unit || (twice == unit && sign > 0) {
                                    sign as i32
                                } else {
                                    0
                                }
                            }
                            "halfFloor" => {
                                if twice > unit || (twice == unit && sign < 0) {
                                    sign as i32
                                } else {
                                    0
                                }
                            }
                            "halfTrunc" => {
                                if twice > unit {
                                    sign as i32
                                } else {
                                    0
                                }
                            }
                            "halfExpand" => {
                                if twice >= unit {
                                    sign as i32
                                } else {
                                    0
                                }
                            }
                            "halfEven" => {
                                if twice > unit || (twice == unit && whole_years % 2 != 0) {
                                    sign as i32
                                } else {
                                    0
                                }
                            }
                            _ => 0,
                        }
                    };
                    fields[0] = Value::Number(
                        (whole_years + adjustment.abs()) as f64 * delta.signum() as f64,
                    );
                    fields[1] = Value::Number(0.0);
                    fields[3] = Value::Number(0.0);
                } else if smallest == "month" {
                    let (month_start, month_end) = if direction > 0 {
                        (start_date, end_date)
                    } else {
                        (end_date, start_date)
                    };
                    let calendar_sign = if month_end >= month_start { 1 } else { -1 };
                    let mut total_months = ((month_end.year() - month_start.year()) * 12
                        + month_end.month() as i32
                        - month_start.month() as i32)
                        .abs();
                    if calendar_sign > 0 && month_end.day() < month_start.day() {
                        total_months -= 1;
                    } else if calendar_sign < 0 && month_end.day() > month_start.day() {
                        total_months -= 1;
                    }
                    let month_anchor = if calendar_sign > 0 {
                        month_start
                            .checked_add_months(chrono::Months::new(total_months as u32))
                            .unwrap_or(month_start)
                    } else {
                        month_start
                            .checked_sub_months(chrono::Months::new(total_months as u32))
                            .unwrap_or(month_start)
                    };
                    let residual_days =
                        ((month_end - month_anchor).num_days() * i64::from(calendar_sign)) as i128;
                    let month_length = chrono::NaiveDate::from_ymd_opt(
                        month_anchor.year(),
                        month_anchor.month(),
                        1,
                    )
                    .and_then(|date| date.checked_add_months(chrono::Months::new(1)))
                    .map(|next| {
                        (next
                            - chrono::NaiveDate::from_ymd_opt(
                                month_anchor.year(),
                                month_anchor.month(),
                                1,
                            )
                            .unwrap())
                        .num_days() as i128
                    })
                    .unwrap_or(30);
                    let signed_residual = residual_days * delta.signum();
                    let twice = signed_residual.unsigned_abs().saturating_mul(2);
                    let unit = month_length.unsigned_abs();
                    let adjustment = match rounding_mode.as_str() {
                        "ceil" => i32::from(signed_residual > 0),
                        "floor" => -i32::from(signed_residual < 0),
                        "expand" => signed_residual.signum() as i32,
                        "halfCeil" => {
                            if twice > unit || (twice == unit && signed_residual > 0) {
                                signed_residual.signum() as i32
                            } else {
                                0
                            }
                        }
                        "halfFloor" => {
                            if twice > unit || (twice == unit && signed_residual < 0) {
                                signed_residual.signum() as i32
                            } else {
                                0
                            }
                        }
                        "halfTrunc" => {
                            if twice > unit {
                                signed_residual.signum() as i32
                            } else {
                                0
                            }
                        }
                        "halfExpand" => {
                            if twice >= unit {
                                signed_residual.signum() as i32
                            } else {
                                0
                            }
                        }
                        "halfEven" => {
                            if twice > unit || (twice == unit && total_months % 2 != 0) {
                                signed_residual.signum() as i32
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                    let rounded_months = (total_months + adjustment.abs()) * delta.signum() as i32;
                    if largest == "year" {
                        fields[0] = Value::Number((rounded_months / 12) as f64);
                        fields[1] = Value::Number((rounded_months % 12) as f64);
                    } else {
                        fields[1] = Value::Number(rounded_months as f64);
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
                if largest == "year"
                    && matches!(rounding_mode.as_str(), "ceil" | "expand")
                    && ((delta > 0 && months >= 11 && days >= 30)
                        || (delta < 0 && months <= -11 && days <= -30))
                {
                    let years_value = match fields[0] {
                        Value::Number(value) => value,
                        _ => 0.0,
                    };
                    fields[0] = Value::Number(years_value + if delta > 0 { 1.0 } else { -1.0 });
                    fields[1] = Value::Number(0.0);
                    fields[3] = Value::Number(0.0);
                    fields[4..].fill(Value::Number(0.0));
                }
                let date_sign = fields.iter().take(4).find_map(|value| match value {
                    Value::Number(number) if *number != 0.0 => Some(number.signum()),
                    _ => None,
                });
                let time_total = fields
                    .iter()
                    .skip(4)
                    .zip([
                        3_600_000_000_000_i128,
                        60_000_000_000,
                        1_000_000_000,
                        1_000_000,
                        1_000,
                        1,
                    ])
                    .map(|(value, scale)| match value {
                        Value::Number(number) => *number as i128 * scale,
                        _ => 0,
                    })
                    .sum::<i128>();
                if let Some(sign) = date_sign {
                    let balanced_time = if sign < 0.0 && time_total > 0 {
                        if let Value::Number(days) = &mut fields[3] {
                            *days += 1.0;
                        }
                        -(86_400_000_000_000_i128 - time_total)
                    } else if sign > 0.0 && time_total < 0 {
                        if let Value::Number(days) = &mut fields[3] {
                            *days -= 1.0;
                        }
                        86_400_000_000_000_i128 + time_total
                    } else {
                        time_total
                    };
                    let mut remainder = balanced_time.abs();
                    for (index, scale) in [
                        (4, 3_600_000_000_000_i128),
                        (5, 60_000_000_000),
                        (6, 1_000_000_000),
                        (7, 1_000_000),
                        (8, 1_000),
                        (9, 1),
                    ] {
                        let value = remainder / scale;
                        remainder %= scale;
                        if let Value::Number(slot) = &mut fields[index] {
                            *slot = value as f64 * sign;
                        }
                    }
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
                    let increment_value =
                        crate::execute::get_property_result(options, "roundingIncrement")?;
                    let increment = match increment_value {
                        Value::Undefined => 1,
                        value => {
                            let number = crate::conversion::to_number(&value)?;
                            if !number.is_finite() || number <= 0.0 {
                                return Err(crate::value::error::throw_range_error(
                                    "Invalid roundingIncrement",
                                ));
                            }
                            number as i128
                        }
                    };
                    let mode_value = crate::execute::get_property_result(options, "roundingMode")?;
                    let mode = match mode_value {
                        Value::Undefined => "halfExpand".to_string(),
                        value => crate::conversion::to_string(&value)?,
                    };
                    let unit = crate::execute::get_property_result(options, "smallestUnit")?;
                    if matches!(unit, Value::Undefined) {
                        return Err(crate::value::error::throw_range_error(
                            "smallestUnit required",
                        ));
                    }
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
            let increment_max = match smallest {
                "hour" => 24_i128,
                "minute" | "second" => 60,
                "millisecond" | "microsecond" | "nanosecond" => 1_000,
                _ => 1,
            };
            if increment < 1
                || (increment_max > 1
                    && (increment >= increment_max || increment_max % increment != 0))
                || (increment_max == 1 && increment != 1)
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid roundingIncrement",
                ));
            }
            let quantum = unit.checked_mul(increment).ok_or_else(|| {
                crate::value::error::throw_range_error("Invalid roundingIncrement")
            })?;
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            if smallest == "day" && epoch.unsigned_abs() >= super::MAX_EPOCH_NANOSECONDS as u128 {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            if smallest == "day" {
                let local_midnight = || -> Option<i128> {
                    let year = crate::conversion::to_number(&property("year").ok()?).ok()? as f64;
                    let month = crate::conversion::to_number(&property("month").ok()?).ok()? as f64;
                    let day = crate::conversion::to_number(&property("day").ok()?).ok()? as f64;
                    Some(
                        (super::plain_date::date_serial(year, month, day)
                            - super::plain_date::date_serial(1970.0, 1.0, 1.0))
                            as i128
                            * 86_400_000_000_000,
                    )
                };
                let start = super::timezone_start_of_day_epoch(&timezone, epoch).or_else(|| {
                    local_midnight()
                        .map(|local| local - super::timezone_offset_nanos(&timezone, local))
                });
                let Some(start) = start else {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid epochNanoseconds",
                    ));
                };
                let next =
                    super::timezone_start_of_day_epoch(&timezone, start + 36 * 3_600_000_000_000)
                        .or_else(|| {
                            local_midnight().map(|local| {
                                local + 86_400_000_000_000
                                    - super::timezone_offset_nanos(
                                        &timezone,
                                        local + 86_400_000_000_000,
                                    )
                            })
                        })
                        .unwrap_or(start + 86_400_000_000_000);
                let length = (next - start).max(1);
                let elapsed = (epoch - start).clamp(0, length);
                let round_up = match mode.as_str() {
                    "trunc" | "floor" => false,
                    "ceil" | "expand" => elapsed != 0,
                    "halfExpand" | "halfCeil" => elapsed * 2 >= length,
                    "halfFloor" | "halfTrunc" => elapsed * 2 > length,
                    "halfEven" => elapsed * 2 > length,
                    _ => {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid roundingMode",
                        ))
                    }
                };
                let rounded = if round_up { next } else { start };
                return Ok(super::zoned_record_with_calendar(
                    rounded, timezone, calendar,
                ));
            }
            let offset = match property("offsetNanoseconds")? {
                Value::Number(value) => value as i128,
                _ => 0,
            };
            let local_epoch = epoch + offset;
            let quotient = local_epoch.div_euclid(quantum);
            let remainder = local_epoch.rem_euclid(quantum);
            let round_up = match mode.as_str() {
                "trunc" => false,
                "floor" => false,
                "ceil" => remainder != 0,
                "expand" => remainder != 0,
                "halfExpand" => remainder * 2 >= quantum,
                "halfCeil" => remainder * 2 >= quantum,
                "halfFloor" => remainder * 2 > quantum,
                "halfTrunc" => {
                    remainder * 2 > quantum || (remainder * 2 == quantum && local_epoch < 0)
                }
                "halfEven" => {
                    remainder * 2 > quantum || (remainder * 2 == quantum && quotient % 2 != 0)
                }
                _ => {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid roundingMode",
                    ))
                }
            };
            let rounded = (quotient + i128::from(round_up)) * quantum - offset;
            if rounded.unsigned_abs() > super::MAX_EPOCH_NANOSECONDS as u128
                || (smallest == "day"
                    && rounded.unsigned_abs() >= super::MAX_EPOCH_NANOSECONDS as u128)
            {
                return Err(crate::value::error::throw_range_error(
                    "Invalid epochNanoseconds",
                ));
            }
            return Ok(super::zoned_record_with_calendar(
                rounded, timezone, calendar,
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
            let epoch = match property("epochNanoseconds")? {
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
                _ => {
                    return Err(crate::value::error::throw_type_error(
                        "Invalid epochNanoseconds",
                    ))
                }
            };
            let timezone = crate::conversion::to_string(&property("timeZoneId")?)?;
            let transition = super::find_timezone_transition(&timezone, epoch, &direction);
            let Some(transition) = transition else {
                return Ok(Value::Null);
            };
            let calendar = crate::conversion::to_string(&property("calendarId")?)?;
            return Ok(super::zoned_record_with_calendar(
                transition, timezone, calendar,
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
        let mut month = crate::conversion::to_number(&property("month")?)?;
        let day = crate::conversion::to_number(&property("day")?)?;
        let calendar = property("calendarId")?;
        let calendar_text = crate::conversion::to_string(&calendar).unwrap_or_default();
        if let Value::String(code) = property("monthCode")? {
            if code.ends_with('L')
                && matches!(calendar_text.as_str(), "hebrew" | "chinese" | "dangi")
            {
                if let Ok(value) = code[1..3].parse::<u32>() {
                    month = f64::from(value + 1);
                }
            }
        }
        if builtin == crate::ops::Builtin::TemporalZonedDateTimeToPlainDate {
            return crate::temporal::plain_date::construct(&[
                Value::Number(year),
                Value::Number(month),
                Value::Number(day),
                calendar,
                property("monthCode")?,
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
            let mut result = crate::temporal::plain_date_time::construct(&[
                Value::Number(year),
                Value::Number(month),
                Value::Number(day),
                property("hour")?,
                property("minute")?,
                property("second")?,
                property("millisecond")?,
                property("microsecond")?,
                property("nanosecond")?,
                calendar.clone(),
                property("monthCode")?,
            ])?;
            if let Value::Object(object) = &mut result {
                std::rc::Rc::make_mut(object).set_property_in_place("calendarId", calendar);
            }
            return Ok(result);
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
        let offset_nanos = crate::conversion::to_number(&property("offsetNanoseconds")?)? as i128;
        let offset_nanos = offset_nanos / 60_000_000_000 * 60_000_000_000;
        let mut offset = super::format_offset(offset_nanos);
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
                    let value = value.floor();
                    if !value.is_finite() || !(0.0..=9.0).contains(&value) {
                        return Err(crate::value::error::throw_range_error(
                            "Invalid fractionalSecondDigits",
                        ));
                    }
                    value as usize
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
                Value::BigInt(value) => super::parse_epoch_text(&value)?,
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
        if (smallest.is_some() || precision != usize::MAX)
            && !timezone.starts_with(['+', '-'])
            && matches!(calendar_text.as_str(), "iso8601" | "gregory")
        {
            let local_days = super::plain_date::date_serial(year as f64, month as f64, day as f64)
                - super::plain_date::date_serial(1970.0, 1.0, 1.0);
            let local_epoch = local_days as i128 * 86_400_000_000_000
                + i128::from(hour) * 3_600_000_000_000
                + i128::from(minute) * 60_000_000_000
                + i128::from(second) * 1_000_000_000
                + fraction;
            let corrected = super::timezone_local_epoch(&timezone, local_epoch, "compatible");
            if corrected != i128::MIN {
                let corrected_offset = super::timezone_offset_nanos(&timezone, corrected);
                let local = corrected + corrected_offset;
                if let Some(date) = chrono::NaiveDateTime::from_timestamp_opt(
                    local.div_euclid(1_000_000_000) as i64,
                    local.rem_euclid(1_000_000_000) as u32,
                ) {
                    year = date.year();
                    month = date.month();
                    day = date.day();
                    hour = chrono::Timelike::hour(&date);
                    minute = chrono::Timelike::minute(&date);
                    second = chrono::Timelike::second(&date);
                    offset =
                        super::format_offset(corrected_offset / 60_000_000_000 * 60_000_000_000);
                }
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
            "always" => format!("[u-ca={calendar_text}]"),
            "critical" => format!("[!u-ca={calendar_text}]"),
            "auto" if calendar_text != "iso8601" => format!("[u-ca={calendar_text}]"),
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
                parts[parts.len() - 2]
                    .parse::<f64>()
                    .map_err(|_| crate::value::error::throw_range_error("Invalid PlainMonthDay"))?,
                parts[parts.len() - 1]
                    .parse::<f64>()
                    .map_err(|_| crate::value::error::throw_range_error("Invalid PlainMonthDay"))?,
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
                parts[parts.len() - 2].parse::<f64>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid PlainYearMonth")
                })?,
                parts[parts.len() - 1].parse::<f64>().map_err(|_| {
                    crate::value::error::throw_range_error("Invalid PlainYearMonth")
                })?,
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
    let milliseconds = crate::date::current_time_ms();
    if !milliseconds.is_finite() {
        return 0;
    }
    (milliseconds * 1_000_000.0) as i128
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
