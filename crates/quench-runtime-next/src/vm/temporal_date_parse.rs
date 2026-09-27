use super::temporal_date::{IsoDate, checked_iso_date};

const ISO_CALENDAR: &str = "iso8601";
const GREGORIAN_CALENDAR: &str = "gregory";
const ISO_YEAR_DIGITS: usize = 4;
const EXTENDED_YEAR_DIGITS: usize = 6;
const ISO_MONTH_DIGITS: usize = 2;
const ISO_DAY_DIGITS: usize = 2;
const MAX_FRACTION_DIGITS: usize = 9;

pub(super) fn parse_plain_date_string(text: &str) -> Option<(IsoDate, String)> {
    if !valid_plain_date_string(text) {
        return None;
    }
    let calendar = parse_calendar_annotation(text)?;
    let date = parse_iso_date_part(date_part(text))?;
    Some((date, calendar))
}

pub(super) fn parse_calendar_identifier(text: &str) -> Option<String> {
    let calendar = parse_calendar_annotation(text)?;
    if calendar == ISO_CALENDAR || calendar == GREGORIAN_CALENDAR {
        Some(calendar)
    } else {
        None
    }
}

fn parse_calendar_annotation(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if matches!(lower.as_str(), ISO_CALENDAR | GREGORIAN_CALENDAR) {
        return Some(lower);
    }
    if !valid_calendar_source(text) {
        return None;
    }
    let calendar = first_calendar_annotation(&lower).unwrap_or(ISO_CALENDAR);
    matches!(calendar, ISO_CALENDAR | GREGORIAN_CALENDAR).then(|| calendar.to_owned())
}

fn first_calendar_annotation(text: &str) -> Option<&str> {
    text.split('[').skip(1).find_map(|annotation| {
        annotation
            .strip_prefix("u-ca=")
            .or_else(|| annotation.strip_prefix("!u-ca="))
            .map(|value| value.split(']').next().unwrap_or(""))
    })
}

fn valid_calendar_source(text: &str) -> bool {
    let base = text.split('[').next().unwrap_or("");
    let date = date_part(base);
    parse_iso_date_part(date).is_some()
        || parse_calendar_partial_date(date).is_some()
        || matches!(
            text.to_ascii_lowercase().as_str(),
            ISO_CALENDAR | GREGORIAN_CALENDAR
        )
}

fn valid_plain_date_string(text: &str) -> bool {
    let annotations_well_formed = !has_annotation_junk(text);
    annotations_well_formed
        && !has_uppercase_annotation_key(text)
        && !has_unknown_critical_annotation(text)
        && !has_invalid_calendar_annotation(text)
        && !has_multiple_time_zones(text)
        && !(text.contains("[!u-ca=") && text.matches("[u-ca=").count() > 0)
        && !text.contains('−')
        && !has_time_junk(text)
        && !has_empty_time_designator(text)
        && !has_fractional_minutes(text)
        && !has_invalid_time(text)
        && !has_utc_designator(text)
        && !has_excess_fraction(text)
        && !has_invalid_offset(text)
        && parse_iso_date_part(date_part(text)).is_some()
}

fn has_invalid_offset(text: &str) -> bool {
    let base = text.split('[').next().unwrap_or(text);
    let Some((_, time)) = base.split_once(['T', 't', ' ']) else {
        return false;
    };
    let offset_index = time
        .get(1..)
        .and_then(|tail| tail.find(['+', '-']).map(|index| index + 1));
    if let Some(index) = offset_index {
        let offset = &time[index..];
        if !quench_temporal::valid_date_time_offset(offset) {
            return true;
        }
    }
    text.split('[').skip(1).any(|annotation| {
        let annotation = annotation.split(']').next().unwrap_or(annotation);
        annotation.starts_with(['+', '-']) && !quench_temporal::valid_timezone_offset(annotation)
    })
}

fn parse_iso_date_part(date: &str) -> Option<IsoDate> {
    if date.len() == ISO_YEAR_DIGITS + ISO_MONTH_DIGITS + ISO_DAY_DIGITS
        && date.bytes().all(|byte| byte.is_ascii_digit())
    {
        return checked_iso_date(
            date[..ISO_YEAR_DIGITS].parse().ok()?,
            date[ISO_YEAR_DIGITS..ISO_YEAR_DIGITS + ISO_MONTH_DIGITS]
                .parse()
                .ok()?,
            date[ISO_YEAR_DIGITS + ISO_MONTH_DIGITS..].parse().ok()?,
        );
    }
    let fields = date.split('-').collect::<Vec<_>>();
    let (year, month, day) = match fields.as_slice() {
        [year, month, day]
            if year.len() == ISO_YEAR_DIGITS
                && month.len() == ISO_MONTH_DIGITS
                && day.len() == ISO_DAY_DIGITS =>
        {
            (
                (*year).parse().ok()?,
                (*month).parse().ok()?,
                (*day).parse().ok()?,
            )
        }
        [year, month, day]
            if year.len() == EXTENDED_YEAR_DIGITS + 1
                && year.starts_with('+')
                && month.len() == ISO_MONTH_DIGITS
                && day.len() == ISO_DAY_DIGITS =>
        {
            (
                (*year).parse().ok()?,
                (*month).parse().ok()?,
                (*day).parse().ok()?,
            )
        }
        [year, month, day]
            if year.len() == EXTENDED_YEAR_DIGITS + 1
                && year.starts_with('-')
                && month.len() == ISO_MONTH_DIGITS
                && day.len() == ISO_DAY_DIGITS =>
        {
            let year = year.parse::<i32>().ok()?;
            if year == 0 {
                return None;
            }
            (year, (*month).parse().ok()?, (*day).parse().ok()?)
        }
        ["", year, month, day]
            if year.len() == EXTENDED_YEAR_DIGITS
                && month.len() == ISO_MONTH_DIGITS
                && day.len() == ISO_DAY_DIGITS =>
        {
            let year = format!("-{year}").parse::<i32>().ok()?;
            if year == 0 {
                return None;
            }
            (year, (*month).parse().ok()?, (*day).parse().ok()?)
        }
        _ => return None,
    };
    checked_iso_date(year, month, day)
}

fn parse_calendar_partial_date(date: &str) -> Option<()> {
    let fields = date.split('-').collect::<Vec<_>>();
    match fields.as_slice() {
        [month, day] if month.len() == ISO_MONTH_DIGITS && day.len() == ISO_DAY_DIGITS => {
            let month = month.parse::<u32>().ok()?;
            let day = day.parse::<u32>().ok()?;
            if (1..=12).contains(&month) && (1..=31).contains(&day) {
                Some(())
            } else {
                None
            }
        }
        [year, month] if year.len() == ISO_YEAR_DIGITS && month.len() == ISO_MONTH_DIGITS => {
            let month = month.parse::<u32>().ok()?;
            (1..=12).contains(&month).then_some(())
        }
        _ => None,
    }
}

fn date_part(text: &str) -> &str {
    text.split(['T', 't', ' ', '[']).next().unwrap_or(text)
}

fn has_uppercase_annotation_key(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|part| part.contains('='))
        .any(|part| {
            part.split('=')
                .next()
                .is_some_and(|key| key.chars().any(|ch| ch.is_ascii_uppercase()))
        })
}

fn has_unknown_critical_annotation(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .any(|part| part.starts_with('!') && part.contains('=') && !part.starts_with("!u-ca="))
}

fn has_invalid_calendar_annotation(text: &str) -> bool {
    first_calendar_annotation(text)
        .is_some_and(|value| !matches!(value, ISO_CALENDAR | GREGORIAN_CALENDAR))
}

fn has_time_junk(text: &str) -> bool {
    let base = text.split('[').next().unwrap_or("");
    base.split_once(['T', 't']).is_some_and(|(_, time)| {
        time.chars()
            .any(|ch| !ch.is_ascii_digit() && !":.,+-Zz".contains(ch))
    })
}

fn has_annotation_junk(text: &str) -> bool {
    let mut remainder = text;
    while let Some(open) = remainder.find('[') {
        let after_open = &remainder[open + 1..];
        let Some(close) = after_open.find(']') else {
            return true;
        };
        let after_close = &after_open[close + 1..];
        if !after_close.is_empty() && !after_close.starts_with('[') {
            return true;
        }
        remainder = after_close;
    }
    false
}

fn has_multiple_time_zones(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|part| !part.contains('=') && !part.is_empty())
        .count()
        > 1
}

fn has_excess_fraction(text: &str) -> bool {
    text.split('[')
        .next()
        .and_then(|base| base.find('.').map(|index| &base[index + 1..]))
        .is_some_and(|digits| {
            digits.bytes().take_while(u8::is_ascii_digit).count() > MAX_FRACTION_DIGITS
        })
}

fn has_utc_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|base| base.ends_with(['Z', 'z']))
}

fn has_empty_time_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|base| base.ends_with(['T', 't']))
}

fn has_fractional_minutes(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
        .and_then(|time| time.split('[').next())
    else {
        return false;
    };
    let time = time
        .get(1..)
        .and_then(|tail| tail.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    let mut fields = time.split(':');
    let hours = fields.next().unwrap_or("");
    let minutes = fields.next().unwrap_or("");
    hours.contains(['.', ',']) || minutes.contains(['.', ','])
}

fn has_invalid_time(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
    else {
        return false;
    };
    let time = time.trim_end_matches(['Z', 'z']);
    let clock = time
        .get(1..)
        .and_then(|tail| tail.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    let fields = clock.split(':').collect::<Vec<_>>();
    let parse_field = |field: &str| field.parse::<u32>().ok();
    if fields.len() == 1 {
        return invalid_compact_time(fields[0]);
    }
    if fields.len() > 1 && (fields[0].len() != 2 || fields[1].len() != 2) {
        return true;
    }
    let hour_text = fields[0].split(['.', ',']).next().unwrap_or(fields[0]);
    let Some(hour) = parse_field(hour_text) else {
        return true;
    };
    if hour > 23 {
        return true;
    }
    let minute_text = fields[1].split(['.', ',']).next().unwrap_or(fields[1]);
    let Some(minute) = parse_field(minute_text) else {
        return true;
    };
    if minute > 59 {
        return true;
    }
    fields.get(2).is_some_and(|second| {
        let second = second.split(['.', ',']).next().unwrap_or(second);
        second.len() != 2 || parse_field(second).is_none_or(|second| second > 60)
    })
}

fn invalid_compact_time(value: &str) -> bool {
    let clock = value.split(['.', ',']).next().unwrap_or(value);
    let valid_length = matches!(clock.len(), 2 | 4 | 6);
    if !valid_length || !clock.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let hour = clock[..2].parse::<u32>().unwrap_or(u32::MAX);
    let minute = clock
        .get(2..4)
        .and_then(|field| field.parse::<u32>().ok())
        .unwrap_or(0);
    let second = clock
        .get(4..6)
        .and_then(|field| field.parse::<u32>().ok())
        .unwrap_or(0);
    hour > 23 || minute > 59 || second > 60
}
