const OFFSET_HOUR_DIGITS: usize = 2;
const OFFSET_MINUTE_DIGITS: usize = 2;
const OFFSET_SECOND_DIGITS: usize = 2;
const COMPACT_HOUR_DIGITS: usize = OFFSET_HOUR_DIGITS;
const COMPACT_MINUTE_DIGITS: usize = COMPACT_HOUR_DIGITS + OFFSET_MINUTE_DIGITS;
const COMPACT_SECOND_DIGITS: usize = COMPACT_MINUTE_DIGITS + OFFSET_SECOND_DIGITS;
const SECONDS_PER_MINUTE: i32 = 60;
const MINUTES_PER_HOUR: i32 = SECONDS_PER_MINUTE;
const SECONDS_PER_HOUR: i32 = MINUTES_PER_HOUR * SECONDS_PER_MINUTE;
const OFFSET_MAX_HOUR: u8 = 23;
const OFFSET_MAX_MINUTE: u8 = 59;
const OFFSET_MAX_SECOND: u8 = 59;
const FRACTIONAL_OFFSET_DIGIT_LIMIT: usize = 9;

pub fn valid_timezone_offset(value: &str) -> bool {
    (valid_offset(value) || valid_hour_only_offset(value)) && value.matches(':').count() <= 1
}

pub fn valid_string_offset(value: &str) -> bool {
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
        .map_or((*second, None), |(second, fraction)| {
            (second, Some(fraction))
        });
    hour.len() == OFFSET_HOUR_DIGITS
        && minute.len() == OFFSET_MINUTE_DIGITS
        && second.len() == OFFSET_SECOND_DIGITS
        && hour.bytes().all(|byte| byte.is_ascii_digit())
        && minute.bytes().all(|byte| byte.is_ascii_digit())
        && second.bytes().all(|byte| byte.is_ascii_digit())
        && hour.parse::<u8>().is_ok_and(|hour| hour <= OFFSET_MAX_HOUR)
        && minute
            .parse::<u8>()
            .is_ok_and(|minute| minute <= OFFSET_MAX_MINUTE)
        && second
            .parse::<u8>()
            .is_ok_and(|second| second <= OFFSET_MAX_SECOND)
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty()
                && fraction.len() <= FRACTIONAL_OFFSET_DIGIT_LIMIT
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn valid_date_time_offset(value: &str) -> bool {
    if valid_string_offset(value) {
        return true;
    }
    let Some(unsigned) = value.strip_prefix(['+', '-']) else {
        return false;
    };
    let (clock, fraction) = unsigned
        .split_once(['.', ','])
        .map_or((unsigned, None), |(clock, fraction)| {
            (clock, Some(fraction))
        });
    if clock.len() == COMPACT_HOUR_DIGITS {
        return fraction.is_none() && clock.bytes().all(|byte| byte.is_ascii_digit());
    }
    if clock.len() != COMPACT_SECOND_DIGITS
        || !clock.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|fraction| {
            fraction.is_empty()
                || fraction.len() > FRACTIONAL_OFFSET_DIGIT_LIMIT
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return false;
    }
    let hour = clock[..COMPACT_HOUR_DIGITS].parse::<u8>().ok();
    let minute = clock[COMPACT_HOUR_DIGITS..COMPACT_MINUTE_DIGITS]
        .parse::<u8>()
        .ok();
    let second = clock[COMPACT_MINUTE_DIGITS..COMPACT_SECOND_DIGITS]
        .parse::<u8>()
        .ok();
    hour.is_some_and(|hour| hour <= OFFSET_MAX_HOUR)
        && minute.is_some_and(|minute| minute <= OFFSET_MAX_MINUTE)
        && second.is_some_and(|second| second <= OFFSET_MAX_SECOND)
}

pub fn offset_seconds(value: &str) -> i32 {
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let digits = value.get(1..).unwrap_or_default().replace(':', "");
    let hour = digits
        .get(..OFFSET_HOUR_DIGITS)
        .and_then(|field| field.parse::<i32>().ok())
        .unwrap_or(0);
    let minute_start = OFFSET_HOUR_DIGITS;
    let second_start = minute_start + OFFSET_MINUTE_DIGITS;
    let minute = digits
        .get(minute_start..second_start)
        .and_then(|field| field.parse::<i32>().ok())
        .unwrap_or(0);
    let second = digits
        .get(second_start..second_start + OFFSET_SECOND_DIGITS)
        .and_then(|field| field.parse::<i32>().ok())
        .unwrap_or(0);
    sign * (hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + second)
}

pub fn offset_minutes(value: &str) -> Option<i32> {
    if matches!(value, "UTC" | "Z") {
        return Some(0);
    }
    let tail = value.get(1..)?;
    let start = tail.find(['+', '-']).map_or(0, |index| index + 1);
    let value = value.get(start..)?;
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = &value[1..];
    let (hour, minute) = digits.split_once(':').map_or(
        (
            digits.get(..OFFSET_HOUR_DIGITS)?,
            digits.get(OFFSET_HOUR_DIGITS..OFFSET_HOUR_DIGITS + OFFSET_MINUTE_DIGITS)?,
        ),
        |(hour, rest)| (hour, rest.get(..OFFSET_MINUTE_DIGITS).unwrap_or(rest)),
    );
    Some(sign * (hour.parse::<i32>().ok()? * MINUTES_PER_HOUR + minute.parse::<i32>().ok()?))
}

fn valid_offset(value: &str) -> bool {
    let Some(value) = value.strip_prefix(['+', '-']) else {
        return false;
    };
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [compact] => {
            compact.len() == COMPACT_MINUTE_DIGITS
                && compact.bytes().all(|byte| byte.is_ascii_digit())
                && valid_hour_minute(
                    &compact[..OFFSET_HOUR_DIGITS],
                    &compact[OFFSET_HOUR_DIGITS..],
                )
        }
        [hour, minute] => {
            hour.len() == OFFSET_HOUR_DIGITS
                && minute.len() == OFFSET_MINUTE_DIGITS
                && hour.bytes().all(|byte| byte.is_ascii_digit())
                && minute.bytes().all(|byte| byte.is_ascii_digit())
                && valid_hour_minute(hour, minute)
        }
        [hour, minute, second] => {
            let (seconds, fraction) = second
                .split_once('.')
                .map_or((*second, None), |(seconds, fraction)| {
                    (seconds, Some(fraction))
                });
            hour.len() == OFFSET_HOUR_DIGITS
                && minute.len() == OFFSET_MINUTE_DIGITS
                && hour.bytes().all(|byte| byte.is_ascii_digit())
                && minute.bytes().all(|byte| byte.is_ascii_digit())
                && valid_hour_minute(hour, minute)
                && seconds.len() == OFFSET_SECOND_DIGITS
                && seconds.bytes().all(|byte| byte.is_ascii_digit())
                && seconds
                    .parse::<u8>()
                    .is_ok_and(|second| second <= OFFSET_MAX_SECOND)
                && fraction.is_none_or(|fraction| {
                    !fraction.is_empty() && fraction.bytes().all(|byte| byte == b'0')
                })
        }
        _ => false,
    }
}

fn valid_hour_only_offset(value: &str) -> bool {
    let Some(hour) = value.strip_prefix(['+', '-']) else {
        return false;
    };
    hour.len() == OFFSET_HOUR_DIGITS
        && hour.bytes().all(|byte| byte.is_ascii_digit())
        && hour.parse::<u8>().is_ok_and(|hour| hour <= OFFSET_MAX_HOUR)
}

fn valid_hour_minute(hour: &str, minute: &str) -> bool {
    hour.parse::<u8>().is_ok_and(|hour| hour <= OFFSET_MAX_HOUR)
        && minute
            .parse::<u8>()
            .is_ok_and(|minute| minute <= OFFSET_MAX_MINUTE)
}
