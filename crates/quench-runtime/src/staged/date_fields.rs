//! The calendar fields of a Date's time value, derived in one place for the Date getters, the
//! setters and `toISOString` (`vm::date`). Plain functions over `f64` milliseconds: the proleptic
//! Gregorian calendar of the spec's Day, MonthFromTime, DateFromTime and their companions, by
//! integer arithmetic (`quench_temporal::civil_from_days`), with the local fields moved by the host's offset.

/// The largest magnitude a time value holds (`TimeClip`'s limit).
pub const DATE_TIME_CLIP_LIMIT_MS: f64 = 8.64e15;
const DATE_MILLISECONDS_PER_DAY: i64 = 86_400_000;
const DATE_SECONDS_PER_MINUTE_I64: i64 = 60;
const DATE_MINUTES_PER_HOUR_I64: i64 = 60;
const DATE_EPOCH_WEEKDAY_SUNDAY_INDEX: i64 = 4;
const DATE_DAYS_PER_WEEK: i64 = 7;
const DATE_MILLISECONDS_PER_SECOND: i64 = 1_000;
const DATE_DAYS_PER_CYCLE: i64 = 146_097;
const DATE_YEARS_PER_CYCLE: i64 = 400;
const DATE_DAYS_PER_YEAR: i64 = 365;
const DATE_MONTHS_PER_CYCLE_SEGMENT: i64 = 153;
const DATE_EPOCH_DAY_OFFSET: i64 = 719_468;

/// The fields of a time value: the year, the zero-based month, the day of the month, the weekday
/// (Sunday is zero) and the time of day.
#[cfg(not(stencil_build))]
pub(crate) struct DateParts {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) weekday: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
    pub(crate) millisecond: u32,
}

/// The UTC fields of `milliseconds` (`Date.prototype.getUTC*`, `toISOString`), or `None` for a
/// time value that is not finite or lies outside the time clip.
#[cfg(not(stencil_build))]
pub(crate) fn date_parts_utc(milliseconds: f64) -> Option<DateParts> {
    if !milliseconds.is_finite() || milliseconds.abs() > DATE_TIME_CLIP_LIMIT_MS {
        return None;
    }
    date_parts_of(milliseconds.trunc() as i64)
}

/// The local fields of `milliseconds`: its UTC time moved by the local offset in seconds that
/// `local_offset_seconds` answers for the timestamp (the host's zone rules, read by the caller).
#[cfg(not(stencil_build))]
pub(crate) fn date_parts_local(milliseconds: f64, local_offset_seconds: impl FnOnce(i64) -> i32) -> Option<DateParts> {
    if !milliseconds.is_finite() || milliseconds.abs() > DATE_TIME_CLIP_LIMIT_MS {
        return None;
    }
    let timestamp = milliseconds.trunc() as i64;
    let offset_seconds = local_offset_seconds(timestamp);
    date_parts_of(timestamp + i64::from(offset_seconds) * DATE_MILLISECONDS_PER_SECOND)
}

/// The fields of the time `whole` milliseconds after the epoch, by integer arithmetic
/// (`quench_temporal::civil_from_days`): the proleptic Gregorian calendar chrono computes, for every time a
/// Date holds.
#[cfg(not(stencil_build))]
pub(crate) fn date_parts_of(whole: i64) -> Option<DateParts> {
    let days = whole.div_euclid(DATE_MILLISECONDS_PER_DAY);
    let time = whole.rem_euclid(DATE_MILLISECONDS_PER_DAY);
    let date = quench_temporal::civil_from_days(days)?;
    Some(DateParts {
        year: date.year,
        month: date.month - 1,
        day: date.day,
        weekday: (days + DATE_EPOCH_WEEKDAY_SUNDAY_INDEX).rem_euclid(DATE_DAYS_PER_WEEK) as u32,
        hour: (time / (DATE_MILLISECONDS_PER_SECOND * DATE_SECONDS_PER_MINUTE_I64 * DATE_MINUTES_PER_HOUR_I64)) as u32,
        minute: ((time / (DATE_MILLISECONDS_PER_SECOND * DATE_SECONDS_PER_MINUTE_I64))
            % DATE_MINUTES_PER_HOUR_I64) as u32,
        second: ((time / DATE_MILLISECONDS_PER_SECOND) % DATE_SECONDS_PER_MINUTE_I64) as u32,
        millisecond: (time % DATE_MILLISECONDS_PER_SECOND) as u32,
    })
}

/// The day number (days from the epoch) of the Gregorian `year`, one-based `month` and `day`,
/// by the cycle arithmetic of `MakeDay`'s civil form, over the spec's floating-point Numbers.
#[cfg(not(stencil_build))]
pub(crate) fn days_from_civil(year: f64, month: f64, day: f64) -> f64 {
    let year = year - f64::from(month <= 2.0);
    let years_per_cycle = DATE_YEARS_PER_CYCLE as f64;
    let days_per_cycle = DATE_DAYS_PER_CYCLE as f64;
    let era = (year / years_per_cycle).floor();
    let year_of_era = year - era * years_per_cycle;
    let month = month + if month > 2.0 { -3.0 } else { 9.0 };
    let day_of_year =
        ((DATE_MONTHS_PER_CYCLE_SEGMENT as f64 * month + 2.0) / 5.0).floor() + day - 1.0;
    era * days_per_cycle + year_of_era * DATE_DAYS_PER_YEAR as f64 + (year_of_era / 4.0).floor()
        - (year_of_era / 100.0).floor()
        + day_of_year
        - DATE_EPOCH_DAY_OFFSET as f64
}
