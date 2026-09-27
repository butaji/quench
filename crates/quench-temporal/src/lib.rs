//! Runtime-independent calendrical algorithms shared by both Quench engines.

mod duration;
mod duration_total;
mod offset;

pub use duration::parse_duration;
pub use duration_total::{relative_duration_nanoseconds, total_duration};
pub use offset::{
    offset_minutes, offset_seconds, valid_date_time_offset, valid_string_offset,
    valid_timezone_offset,
};

const DAYS_PER_400_YEAR_CYCLE: i64 = 146_097;
const YEARS_PER_GREGORIAN_CYCLE: i64 = 400;
const ISO_EPOCH_OFFSET_DAYS: i64 = 719_468;
const DAYS_PER_COMMON_YEAR: i64 = 365;
const DAYS_PER_4_YEAR_CYCLE: i64 = 1_460;
const DAYS_PER_CENTURY: i64 = 36_524;
const DAYS_BEFORE_LAST_400_YEAR_DAY: i64 = 146_096;
const DAYS_PER_MONTH_TRANSFORM_CYCLE: i64 = 153;
const MONTH_TRANSFORM_DIVISOR: i64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

pub fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

pub fn days_in_month(year: i32, month: u32) -> Option<u32> {
    Some(match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    })
}

pub fn days_from_civil(date: IsoDate) -> i64 {
    let year = i64::from(date.year) - i64::from(date.month <= 2);
    let era = year.div_euclid(YEARS_PER_GREGORIAN_CYCLE);
    let year_of_era = year - era * YEARS_PER_GREGORIAN_CYCLE;
    let adjusted_month = i64::from(date.month) + if date.month > 2 { -3 } else { 9 };
    let day_of_year = (DAYS_PER_MONTH_TRANSFORM_CYCLE * adjusted_month + 2)
        / MONTH_TRANSFORM_DIVISOR
        + i64::from(date.day)
        - 1;
    let day_of_era =
        year_of_era * DAYS_PER_COMMON_YEAR + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_400_YEAR_CYCLE + day_of_era - ISO_EPOCH_OFFSET_DAYS
}

pub fn civil_from_days(days: i64) -> Option<IsoDate> {
    let adjusted_days = days.checked_add(ISO_EPOCH_OFFSET_DAYS)?;
    let era = adjusted_days.div_euclid(DAYS_PER_400_YEAR_CYCLE);
    let day_of_era = adjusted_days - era * DAYS_PER_400_YEAR_CYCLE;
    let year_of_era = (day_of_era - day_of_era / DAYS_PER_4_YEAR_CYCLE
        + day_of_era / DAYS_PER_CENTURY
        - day_of_era / DAYS_BEFORE_LAST_400_YEAR_DAY)
        / DAYS_PER_COMMON_YEAR;
    let year = i32::try_from(year_of_era + era * YEARS_PER_GREGORIAN_CYCLE).ok()?;
    let day_of_year =
        day_of_era - (DAYS_PER_COMMON_YEAR * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (MONTH_TRANSFORM_DIVISOR * day_of_year + 2) / DAYS_PER_MONTH_TRANSFORM_CYCLE;
    let day = u32::try_from(
        day_of_year - (DAYS_PER_MONTH_TRANSFORM_CYCLE * month_part + 2) / MONTH_TRANSFORM_DIVISOR
            + 1,
    )
    .ok()?;
    let month = u32::try_from(month_part + if month_part < 10 { 3 } else { -9 }).ok()?;
    let year = year + i32::from(month <= 2);
    Some(IsoDate { year, month, day })
}
