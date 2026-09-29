use crate::{IsoDate, civil_from_days, days_from_civil, days_in_month};

const NANOSECONDS_PER_DAY: i128 = 86_400_000_000_000;
const NANOSECONDS_PER_WEEK: i128 = 604_800_000_000_000;
const DAYS_PER_WEEK: i128 = 7;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const DURATION_FIELD_COUNT: usize = 10;
const YEARS_FIELD: usize = 0;
const MONTHS_FIELD: usize = 1;
const WEEKS_FIELD: usize = 2;
const DAYS_FIELD: usize = 3;
const HOURS_FIELD: usize = 4;
const YEARS_UNIT: usize = YEARS_FIELD;
const MONTHS_UNIT: usize = MONTHS_FIELD;
const WEEKS_UNIT: usize = WEEKS_FIELD;
const DAYS_UNIT: usize = DAYS_FIELD;
const HOURS_UNIT: usize = HOURS_FIELD;
const FIRST_TIME_UNIT: usize = HOURS_UNIT;
const NANOSECONDS_UNIT: usize = 9;
const MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;
const MAX_DURATION_TIME_NANOSECONDS: i128 = MAX_SAFE_INTEGER * NANOSECONDS_PER_SECOND;
const DURATION_DECIMAL_DIGITS: usize = 32;
const MIN_TEMPORAL_DATE: IsoDate = IsoDate {
    year: -271_821,
    month: 4,
    day: 19,
};
const MAX_TEMPORAL_DATE: IsoDate = IsoDate {
    year: 275_760,
    month: 9,
    day: 13,
};
const SUBDAY_UNIT_NANOSECONDS: [i128; 6] = [
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];

struct RelativeDuration {
    target: IsoDate,
    time_nanoseconds: i128,
    total_nanoseconds: i128,
}

pub fn total_duration(
    start: IsoDate,
    fields: [i128; DURATION_FIELD_COUNT],
    unit: usize,
) -> Option<f64> {
    let relative = relative_duration(start, fields)?;
    total_in_unit(
        start,
        relative.target,
        relative.time_nanoseconds,
        relative.total_nanoseconds,
        unit,
    )
}

pub fn relative_duration_nanoseconds(
    start: IsoDate,
    fields: [i128; DURATION_FIELD_COUNT],
) -> Option<i128> {
    Some(relative_duration(start, fields)?.total_nanoseconds)
}

fn relative_duration(
    start: IsoDate,
    fields: [i128; DURATION_FIELD_COUNT],
) -> Option<RelativeDuration> {
    if !temporal_date_in_range(start) {
        return None;
    }
    let mut target = crate::add_iso_date(
        start,
        (
            i64::try_from(fields[YEARS_FIELD]).ok()?,
            i64::try_from(fields[MONTHS_FIELD]).ok()?,
            i64::try_from(fields[WEEKS_FIELD]).ok()?,
            i64::try_from(fields[DAYS_FIELD]).ok()?,
        ),
        true,
    )?;
    if !temporal_date_in_range(target) {
        return None;
    }
    let mut time = duration_time_nanoseconds(&fields)?;
    if time.unsigned_abs() >= MAX_DURATION_TIME_NANOSECONDS as u128 {
        return None;
    }
    let whole_days = time / NANOSECONDS_PER_DAY;
    if whole_days != 0 {
        target = shift_unit(target, 3, whole_days)?;
        time -= whole_days * NANOSECONDS_PER_DAY;
    }
    if !temporal_datetime_in_range(target, time) {
        return None;
    }
    let elapsed_days = i128::from(days_from_civil(target) - days_from_civil(start));
    let total = elapsed_days
        .checked_mul(NANOSECONDS_PER_DAY)?
        .checked_add(time)?;
    Some(RelativeDuration {
        target,
        time_nanoseconds: time,
        total_nanoseconds: total,
    })
}

fn temporal_date_in_range(date: IsoDate) -> bool {
    (
        MIN_TEMPORAL_DATE.year,
        MIN_TEMPORAL_DATE.month,
        MIN_TEMPORAL_DATE.day,
    ) <= (date.year, date.month, date.day)
        && (date.year, date.month, date.day)
            <= (
                MAX_TEMPORAL_DATE.year,
                MAX_TEMPORAL_DATE.month,
                MAX_TEMPORAL_DATE.day,
            )
        && days_in_month(date.year, date.month).is_some_and(|days| (1..=days).contains(&date.day))
}

fn temporal_datetime_in_range(date: IsoDate, time: i128) -> bool {
    let start = i128::from(days_from_civil(MIN_TEMPORAL_DATE)) * NANOSECONDS_PER_DAY;
    let end = i128::from(days_from_civil(MAX_TEMPORAL_DATE)) * NANOSECONDS_PER_DAY;
    let value = i128::from(days_from_civil(date)) * NANOSECONDS_PER_DAY + time;
    (start..=end).contains(&value)
}

fn duration_time_nanoseconds(fields: &[i128; DURATION_FIELD_COUNT]) -> Option<i128> {
    fields[FIRST_TIME_UNIT..]
        .iter()
        .zip(SUBDAY_UNIT_NANOSECONDS)
        .try_fold(0_i128, |total, (value, scale)| {
            total.checked_add(value.checked_mul(scale)?)
        })
}

fn total_in_unit(
    start: IsoDate,
    target: IsoDate,
    time: i128,
    total: i128,
    unit: usize,
) -> Option<f64> {
    match unit {
        YEARS_UNIT | MONTHS_UNIT => calendar_total(start, target, time, total, unit),
        WEEKS_UNIT => Some(divide_duration(total, NANOSECONDS_PER_WEEK)),
        DAYS_UNIT => Some(divide_duration(total, NANOSECONDS_PER_DAY)),
        FIRST_TIME_UNIT..=NANOSECONDS_UNIT => Some(divide_duration(
            total,
            SUBDAY_UNIT_NANOSECONDS[unit - FIRST_TIME_UNIT],
        )),
        _ => None,
    }
}

fn calendar_total(
    start: IsoDate,
    target: IsoDate,
    time: i128,
    total: i128,
    unit: usize,
) -> Option<f64> {
    if total == 0 {
        return Some(0.0);
    }
    let mut count = i128::from(target.year) - i128::from(start.year);
    if unit == MONTHS_UNIT {
        count = count.checked_mul(crate::ISO_MONTHS_PER_YEAR as i128)? + i128::from(target.month)
            - i128::from(start.month);
    }
    let endpoint = i128::from(days_from_civil(target))
        .checked_mul(NANOSECONDS_PER_DAY)?
        .checked_add(time)?;
    let anchor_epoch = |count| {
        let date = shift_unit(start, unit, count)?;
        i128::from(days_from_civil(date)).checked_mul(NANOSECONDS_PER_DAY)
    };
    let direction = total.signum();
    let mut anchor = anchor_epoch(count)?;
    while (endpoint - anchor) * direction < 0 {
        count -= direction;
        anchor = anchor_epoch(count)?;
    }
    let next = anchor_epoch(count + direction)?;
    let span = (next - anchor).abs();
    let numerator = count.checked_mul(span)?.checked_add(endpoint - anchor)?;
    Some(divide_duration(numerator, span))
}

fn shift_unit(date: IsoDate, unit: usize, amount: i128) -> Option<IsoDate> {
    if unit >= WEEKS_UNIT {
        let day_scale = if unit == WEEKS_UNIT { DAYS_PER_WEEK } else { 1 };
        let days = i64::try_from(amount.checked_mul(day_scale)?).ok()?;
        let day_number = days_from_civil(date).checked_add(days)?;
        return civil_from_days(day_number);
    }
    let amount = i64::try_from(amount).ok()?;
    let (years, months) = if unit == YEARS_UNIT {
        (amount, 0)
    } else {
        (0, amount)
    };
    crate::add_iso_date(date, (years, months, 0, 0), true)
}

fn divide_duration(nanoseconds: i128, divisor: i128) -> f64 {
    let whole = nanoseconds / divisor;
    let remainder = nanoseconds % divisor;
    if remainder == 0 {
        return whole as f64;
    }
    let mut digits = format!("{}.", whole.abs());
    let mut remainder = remainder.abs();
    for _ in 0..DURATION_DECIMAL_DIGITS {
        remainder *= 10;
        digits.push(char::from(b'0' + (remainder / divisor) as u8));
        remainder %= divisor;
        if remainder == 0 {
            break;
        }
    }
    let value = digits.parse::<f64>().unwrap_or(f64::INFINITY);
    if nanoseconds < 0 { -value } else { value }
}
