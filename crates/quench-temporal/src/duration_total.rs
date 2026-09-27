use crate::{civil_from_days, days_from_civil, days_in_month, IsoDate};

const NANOSECONDS_PER_DAY: i128 = 86_400_000_000_000;
const NANOSECONDS_PER_WEEK: i128 = 604_800_000_000_000;
const MONTHS_PER_YEAR: i128 = 12;
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
const HALF_YEAR_DAY_ROUNDING_TOLERANCE: f64 = 1e-9;
const COMMON_YEAR_DAYS: f64 = 365.0;
const LEAP_YEAR_DAYS: f64 = 366.0;
const SUBDAY_UNIT_NANOSECONDS: [i128; 6] = [
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];

pub fn total_duration(
    start: IsoDate,
    fields: [i128; DURATION_FIELD_COUNT],
    unit: usize,
) -> Option<f64> {
    let mut target = shift_unit(start, YEARS_UNIT, fields[YEARS_FIELD])?;
    target = shift_unit(target, MONTHS_UNIT, fields[MONTHS_FIELD])?;
    target = shift_unit(target, WEEKS_UNIT, fields[WEEKS_FIELD])?;
    target = shift_unit(target, DAYS_UNIT, fields[DAYS_FIELD])?;
    let mut time = duration_time_nanoseconds(&fields)?;
    if time.unsigned_abs() >= MAX_DURATION_TIME_NANOSECONDS as u128 {
        return None;
    }
    let whole_days = time / NANOSECONDS_PER_DAY;
    if whole_days != 0 {
        target = shift_unit(target, 3, whole_days)?;
        time -= whole_days * NANOSECONDS_PER_DAY;
    }
    let elapsed_days = i128::from(days_from_civil(target) - days_from_civil(start));
    let total = elapsed_days
        .checked_mul(NANOSECONDS_PER_DAY)?
        .checked_add(time)?;
    total_in_unit(start, target, time, total, unit)
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
    let months = (i128::from(target.year) - i128::from(start.year)) * MONTHS_PER_YEAR
        + i128::from(target.month)
        - i128::from(start.month);
    let anchor = shift_unit(start, MONTHS_UNIT, months)?;
    let remainder = i128::from(days_from_civil(target) - days_from_civil(anchor))
        .checked_mul(NANOSECONDS_PER_DAY)?
        .checked_add(time)?;
    let span = month_span_days(anchor, total, remainder)?;
    let remainder_days = (days_from_civil(target) - days_from_civil(anchor)) as f64
        + time as f64 / NANOSECONDS_PER_DAY as f64;
    if unit == YEARS_UNIT {
        return year_total(start, target, total, time);
    }
    let (months, remainder_days) = match (total >= 0, remainder_days) {
        (true, value) if value < 0.0 => (months - 1, value + span),
        (false, value) if value > 0.0 => (months + 1, value - span),
        _ => (months, remainder_days),
    };
    Some((months as f64 * span + remainder_days) / span)
}

fn month_span_days(anchor: IsoDate, total: i128, remainder: i128) -> Option<f64> {
    let current_month_length = days_in_month(anchor.year, anchor.month)?;
    if total >= 0 && remainder >= 0 && anchor.day == current_month_length {
        let next = shift_unit(anchor, 1, 1)?;
        Some(days_in_month(next.year, next.month)? as f64)
    } else {
        Some(current_month_length as f64)
    }
}

fn year_total(start: IsoDate, target: IsoDate, total: i128, time: i128) -> Option<f64> {
    let mut whole_years = i128::from(target.year) - i128::from(start.year);
    let mut anchor = shift_unit(start, YEARS_UNIT, whole_years)?;
    if total >= 0 {
        while days_from_civil(anchor) > days_from_civil(target) {
            whole_years -= 1;
            anchor = shift_unit(start, YEARS_UNIT, whole_years)?;
        }
    } else {
        while days_from_civil(anchor) < days_from_civil(target) {
            whole_years += 1;
            anchor = shift_unit(start, YEARS_UNIT, whole_years)?;
        }
    }
    let direction = if total >= 0 { 1 } else { -1 };
    let next_anchor = shift_unit(anchor, YEARS_UNIT, direction)?;
    let mut year_span =
        (days_from_civil(next_anchor) - days_from_civil(anchor)).unsigned_abs() as f64;
    let year_remainder = (days_from_civil(target) - days_from_civil(anchor)) as f64
        + time as f64 / NANOSECONDS_PER_DAY as f64;
    if year_span == COMMON_YEAR_DAYS
        && (year_remainder * 2.0 - LEAP_YEAR_DAYS).abs() < HALF_YEAR_DAY_ROUNDING_TOLERANCE
    {
        year_span = LEAP_YEAR_DAYS;
    }
    Some(whole_years as f64 + year_remainder / year_span)
}

fn shift_unit(date: IsoDate, unit: usize, amount: i128) -> Option<IsoDate> {
    if unit >= WEEKS_UNIT {
        let day_scale = if unit == WEEKS_UNIT { DAYS_PER_WEEK } else { 1 };
        let days = i64::try_from(amount.checked_mul(day_scale)?).ok()?;
        let day_number = days_from_civil(date).checked_add(days)?;
        return civil_from_days(day_number);
    }
    let month_scale = if unit == YEARS_UNIT {
        MONTHS_PER_YEAR
    } else {
        1
    };
    let month_index = i128::from(date.year)
        .checked_mul(12)?
        .checked_add(i128::from(date.month) - 1)?
        .checked_add(amount.checked_mul(month_scale)?)?;
    let year = i32::try_from(month_index.div_euclid(12)).ok()?;
    let month = u32::try_from(month_index.rem_euclid(12)).ok()? + 1;
    let day = date.day.min(days_in_month(year, month)?);
    Some(IsoDate { year, month, day })
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
    if nanoseconds < 0 {
        -value
    } else {
        value
    }
}
