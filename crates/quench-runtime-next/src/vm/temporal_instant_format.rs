use chrono::{Datelike, TimeZone, Timelike, Utc};

const MAX_BASIC_ISO_YEAR: i32 = 9_999;
const BASIC_YEAR_WIDTH: usize = 4;
const EXTENDED_YEAR_WIDTH: usize = 6;
const FRACTION_WIDTH: usize = 9;
const TIME_FIELD_WIDTH: usize = 2;

pub(super) fn format_instant(epoch: i128) -> Option<String> {
    let seconds = epoch.div_euclid(super::temporal_instant::INSTANT_NANOSECONDS_PER_SECOND);
    let nanoseconds =
        epoch.rem_euclid(super::temporal_instant::INSTANT_NANOSECONDS_PER_SECOND) as u32;
    let date = Utc
        .timestamp_opt(i64::try_from(seconds).ok()?, nanoseconds)
        .single()?;
    let year = format_iso_year(date.year());
    let fraction = format_fraction(date.nanosecond());
    Some(format!(
        "{year}-{:0width$}-{:0width$}T{:0width$}:{:0width$}:{:0width$}{fraction}Z",
        date.month(),
        date.day(),
        date.hour(),
        date.minute(),
        date.second(),
        width = TIME_FIELD_WIDTH,
    ))
}

fn format_iso_year(year: i32) -> String {
    if (0..=MAX_BASIC_ISO_YEAR).contains(&year) {
        format!("{year:0width$}", width = BASIC_YEAR_WIDTH)
    } else if year < 0 {
        format!(
            "-{year:0width$}",
            year = year.unsigned_abs(),
            width = EXTENDED_YEAR_WIDTH
        )
    } else {
        format!("+{year:0width$}", width = EXTENDED_YEAR_WIDTH)
    }
}

fn format_fraction(nanoseconds: u32) -> String {
    let fraction = format!("{nanoseconds:0width$}", width = FRACTION_WIDTH)
        .trim_end_matches('0')
        .to_owned();
    if fraction.is_empty() {
        String::new()
    } else {
        format!(".{fraction}")
    }
}
