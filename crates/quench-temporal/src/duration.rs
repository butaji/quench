const YEARS_FIELD: usize = 0;
const MONTHS_FIELD: usize = 1;
const WEEKS_FIELD: usize = 2;
const DAYS_FIELD: usize = 3;
const HOURS_FIELD: usize = 4;
const MINUTES_FIELD: usize = 5;
const SECONDS_FIELD: usize = 6;
const MILLISECONDS_FIELD: usize = 7;
const MICROSECONDS_FIELD: usize = 8;
const NANOSECONDS_FIELD: usize = 9;
const DURATION_FIELD_COUNT: usize = NANOSECONDS_FIELD + 1;
const DATE_FIELD_COUNT: usize = 4;
const TIME_FIELD_COUNT: usize = 3;
const DATE_FIELDS: [(char, usize); DATE_FIELD_COUNT] = [
    ('Y', YEARS_FIELD),
    ('M', MONTHS_FIELD),
    ('W', WEEKS_FIELD),
    ('D', DAYS_FIELD),
];
const TIME_FIELDS: [(char, usize); TIME_FIELD_COUNT] = [
    ('H', HOURS_FIELD),
    ('M', MINUTES_FIELD),
    ('S', SECONDS_FIELD),
];
const TIME_UNIT_NANOSECOND_SCALES: [f64; TIME_FIELDS.len()] = [3_600.0, 60.0, 1.0];
const NANOSECONDS_PER_SECOND: f64 = 1_000_000_000.0;
const NANOSECONDS_PER_MINUTE: i64 = 60_000_000_000;
const NANOSECONDS_PER_SECOND_INTEGER: i64 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i64 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: i64 = 1_000;
const SUBUNITS_PER_SECOND: i64 = 1_000;
const FRACTIONAL_SECOND_DIGIT_LIMIT: usize = 9;

/// Parses an ISO 8601 duration into Temporal's year-to-nanosecond field order.
/// Guest values and errors stay with each runtime; this shared operation is pure.
pub fn parse_duration(text: &str) -> Option<[f64; DURATION_FIELD_COUNT]> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let body = body.strip_prefix('P').or_else(|| body.strip_prefix('p'))?;
    let mut fields = [0.0; DURATION_FIELD_COUNT];
    let (date, time) = body.split_once(['T', 't']).unwrap_or((body, ""));
    let date_seen = parse_duration_section(date, false, &mut fields)?;
    let time_seen = parse_duration_section(time, true, &mut fields)?;
    if !date_seen && !time_seen {
        return None;
    }
    if negative {
        fields.iter_mut().for_each(|value| {
            if *value != 0.0 {
                *value = -*value;
            }
        });
    }
    Some(fields)
}

fn parse_duration_section(
    section: &str,
    is_time: bool,
    fields: &mut [f64; DURATION_FIELD_COUNT],
) -> Option<bool> {
    let mut rest = section;
    let mut seen = false;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find_map(|(index, character)| character.is_ascii_alphabetic().then_some(index))?;
        let (number, suffix) = rest.split_at(end);
        let unit = suffix.chars().next()?;
        let unit_end = unit.len_utf8();
        validate_duration_number(number, suffix, is_time, unit)?;
        let (whole, fraction) = parse_duration_number(number)?;
        let field = duration_field_index(is_time, unit)?;
        fields[field] += whole;
        if is_time && fraction != 0.0 {
            add_fractional_time(fields, field, fraction);
        }
        seen = true;
        rest = &suffix[unit_end..];
    }
    Some(seen)
}

fn duration_field_index(is_time: bool, unit: char) -> Option<usize> {
    let unit = unit.to_ascii_uppercase();
    let fields = if is_time {
        &TIME_FIELDS[..]
    } else {
        &DATE_FIELDS[..]
    };
    fields
        .iter()
        .find_map(|(candidate, index)| (*candidate == unit).then_some(*index))
}

fn validate_duration_number(number: &str, suffix: &str, is_time: bool, unit: char) -> Option<()> {
    let separators = number.matches(['.', ',']).count();
    let digits = number.split(['.', ',']).collect::<Vec<_>>();
    if number.is_empty()
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ','))
        || separators > 1
        || digits.first().is_some_and(|part| part.is_empty())
        || digits.get(1).is_some_and(|part| part.is_empty())
        || separators > 0 && (!is_time || !suffix[unit.len_utf8()..].is_empty())
        || unit.eq_ignore_ascii_case(&'S')
            && digits
                .get(1)
                .is_some_and(|part| part.len() > FRACTIONAL_SECOND_DIGIT_LIMIT)
    {
        return None;
    }
    Some(())
}

fn parse_duration_number(number: &str) -> Option<(f64, f64)> {
    let (whole, fraction) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let whole = whole.parse::<f64>().ok()?;
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let scale = 10_f64.powi(fraction.len().try_into().ok()?);
        fraction.parse::<f64>().ok()? / scale
    };
    Some((whole, fraction))
}

fn add_fractional_time(fields: &mut [f64; DURATION_FIELD_COUNT], field: usize, fraction: f64) {
    let time_unit = field - HOURS_FIELD;
    let mut nanoseconds =
        (fraction * TIME_UNIT_NANOSECOND_SCALES[time_unit] * NANOSECONDS_PER_SECOND).round() as i64;
    if field == HOURS_FIELD {
        fields[MINUTES_FIELD] += (nanoseconds / NANOSECONDS_PER_MINUTE) as f64;
        nanoseconds %= NANOSECONDS_PER_MINUTE;
    }
    fields[SECONDS_FIELD] += (nanoseconds / NANOSECONDS_PER_SECOND_INTEGER) as f64;
    nanoseconds %= NANOSECONDS_PER_SECOND_INTEGER;
    fields[MILLISECONDS_FIELD] += (nanoseconds / NANOSECONDS_PER_MILLISECOND) as f64;
    fields[MICROSECONDS_FIELD] +=
        (nanoseconds / NANOSECONDS_PER_MICROSECOND % SUBUNITS_PER_SECOND) as f64;
    fields[NANOSECONDS_FIELD] += (nanoseconds % NANOSECONDS_PER_MICROSECOND) as f64;
}
