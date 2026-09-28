use chrono::{Datelike, Timelike};

const FIRST_MONTH_NUMBER: u32 = 1;
const MONTHS_PER_YEAR: usize = 12;
const HOURS_PER_DAY: u32 = 24;
const HOURS_PER_HALF_DAY: u32 = 12;
const YEARS_PER_CENTURY: i32 = 100;
const DECIMAL_RADIX: u32 = 10;
const FULL_FRACTION_DIGITS: u32 = 3;
const TWO_DIGIT_WIDTH: usize = 2;
const FIRST_WEEKDAY_CHAR: usize = 1;
const SHORT_WEEKDAY_LENGTH: usize = 3;
const DAYS_PER_WEEK: usize = 7;
const DAYS_PER_WEEK_U32: u32 = DAYS_PER_WEEK as u32;
const EVENING_START_HOUR: u32 = 18;
const NIGHT_END_HOUR: u32 = 5;
const MORNING_START_HOUR: u32 = 6;
const MORNING_END_HOUR: u32 = 11;
const NOON_HOUR: u32 = 12;
const EVENING_END_HOUR: u32 = 21;
const LATE_NIGHT_START_HOUR: u32 = EVENING_END_HOUR + 1;
const PERIOD_NOON: &str = "noon";
const PERIOD_NARROW_NOON: &str = "n";
const FORMAT_STYLE_FULL: &str = "full";
const FORMAT_STYLE_LONG: &str = "long";
const FORMAT_STYLE_MEDIUM: &str = "medium";
const FORMAT_STYLE_SHORT: &str = "short";

#[derive(Clone, Copy)]
pub(super) struct DateTimeFields {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
    pub has_date: bool,
    pub has_time: bool,
    pub is_temporal: bool,
}

impl DateTimeFields {
    pub(super) fn from_date(date: &chrono::DateTime<chrono::FixedOffset>) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
            hour: date.hour(),
            minute: date.minute(),
            second: date.second(),
            millisecond: date.timestamp_subsec_millis(),
            has_date: true,
            has_time: true,
            is_temporal: false,
        }
    }
}

#[derive(Default)]
pub(super) struct DateTimePartOptions {
    pub weekday: Option<String>,
    pub year: Option<String>,
    pub month: Option<String>,
    pub day: Option<String>,
    pub hour: Option<String>,
    pub minute: Option<String>,
    pub second: Option<String>,
    pub day_period: Option<String>,
    pub hour_cycle: Option<String>,
    pub hour12: Option<bool>,
    pub fractional_second_digits: Option<u32>,
    pub time_zone_name: Option<String>,
}

pub(super) fn apply_date_style(options: &mut DateTimePartOptions, style: &str) {
    match style {
        FORMAT_STYLE_FULL => options.weekday = Some("long".into()),
        _ => {}
    }
    match style {
        FORMAT_STYLE_FULL | FORMAT_STYLE_LONG => {
            options.month = Some("long".into());
            options.day = Some("numeric".into());
            options.year = Some("numeric".into());
        }
        FORMAT_STYLE_MEDIUM => {
            options.month = Some("short".into());
            options.day = Some("numeric".into());
            options.year = Some("numeric".into());
        }
        FORMAT_STYLE_SHORT => {
            options.month = Some("numeric".into());
            options.day = Some("numeric".into());
            options.year = Some("2-digit".into());
        }
        _ => {}
    }
}

pub(super) fn apply_time_style(options: &mut DateTimePartOptions, style: &str) {
    options.hour.get_or_insert_with(|| "numeric".into());
    options.minute.get_or_insert_with(|| "2-digit".into());
    if style != FORMAT_STYLE_SHORT {
        options.second.get_or_insert_with(|| "2-digit".into());
    }
}

pub(super) fn format_parts(
    fields: DateTimeFields,
    options: &DateTimePartOptions,
) -> Vec<(String, String)> {
    let has_date = fields.has_date
        && (options.year.is_some()
            || options.month.is_some()
            || options.day.is_some()
            || options.weekday.is_some());
    let has_time = fields.has_time
        && (options.hour.is_some()
            || options.minute.is_some()
            || options.second.is_some()
            || options.day_period.is_some());
    let mut parts = Vec::new();
    if has_date {
        append_date(&mut parts, fields, options);
    }
    if has_date && has_time {
        push(&mut parts, "literal", ", ");
    }
    if has_time {
        append_time(&mut parts, fields, options);
    }
    if !fields.is_temporal {
        if let Some(name) = &options.time_zone_name {
            if !parts.is_empty() {
                push(&mut parts, "literal", " ");
            }
            push(&mut parts, "timeZoneName", name.clone());
        }
    }
    parts
}

fn append_date(
    parts: &mut Vec<(String, String)>,
    fields: DateTimeFields,
    options: &DateTimePartOptions,
) {
    if let Some(style) = &options.weekday {
        let value = format_weekday(fields, style);
        push(parts, "weekday", value);
        if options.month.is_some() || options.day.is_some() || options.year.is_some() {
            push(parts, "literal", ", ");
        }
    }
    let month_style = options.month.as_deref();
    let textual_month =
        month_style.is_some_and(|style| matches!(style, "long" | "short" | "narrow"));
    if let Some(style) = month_style {
        let month = match style {
            "long" => MONTH_NAMES
                .get(fields.month.saturating_sub(FIRST_MONTH_NUMBER) as usize)
                .copied(),
            "short" => MONTH_ABBREVIATIONS
                .get(fields.month.saturating_sub(FIRST_MONTH_NUMBER) as usize)
                .copied(),
            "narrow" => MONTH_NAMES
                .get(fields.month.saturating_sub(FIRST_MONTH_NUMBER) as usize)
                .map(|name| &name[..FIRST_WEEKDAY_CHAR]),
            "2-digit" => Some(if fields.month < DECIMAL_RADIX {
                "0"
            } else {
                ""
            }),
            _ => None,
        };
        let value = match (style, month) {
            ("long" | "short" | "narrow", Some(name)) => name.to_string(),
            ("2-digit", Some(prefix)) => format!("{prefix}{}", fields.month),
            _ => fields.month.to_string(),
        };
        push(parts, "month", value);
    }
    if let Some(style) = &options.day {
        if options.month.is_some() {
            push(parts, "literal", if textual_month { " " } else { "/" });
        }
        let value = if style == "2-digit" {
            format!("{:0width$}", fields.day, width = TWO_DIGIT_WIDTH)
        } else {
            fields.day.to_string()
        };
        push(parts, "day", value);
    }
    if let Some(style) = &options.year {
        if options.month.is_some() {
            push(parts, "literal", if textual_month { ", " } else { "/" });
        } else if options.day.is_some() {
            push(parts, "literal", " ");
        }
        let value = if style == "2-digit" {
            format!(
                "{:0width$}",
                fields.year.rem_euclid(YEARS_PER_CENTURY),
                width = TWO_DIGIT_WIDTH
            )
        } else {
            fields.year.to_string()
        };
        push(parts, "year", value);
    }
}

fn append_time(
    parts: &mut Vec<(String, String)>,
    fields: DateTimeFields,
    options: &DateTimePartOptions,
) {
    if options.hour.is_none() && options.minute.is_none() && options.second.is_none() {
        if let Some(style) = &options.day_period {
            push(parts, "dayPeriod", day_period_value(fields.hour, style));
        }
        return;
    }
    let hour_cycle = options.hour_cycle.as_deref().unwrap_or("h12");
    let hour12 = options
        .hour12
        .unwrap_or_else(|| matches!(hour_cycle, "h11" | "h12"));
    let hour = match hour_cycle {
        "h11" => fields.hour % HOURS_PER_HALF_DAY,
        "h12" => match fields.hour % HOURS_PER_HALF_DAY {
            0 => HOURS_PER_HALF_DAY,
            hour => hour,
        },
        "h24" => match fields.hour {
            0 => HOURS_PER_DAY,
            hour => hour,
        },
        _ => fields.hour,
    };
    if let Some(style) = &options.hour {
        let value = if style == "2-digit" {
            format!("{hour:0width$}", width = TWO_DIGIT_WIDTH)
        } else {
            hour.to_string()
        };
        push(parts, "hour", value);
    }
    if options.minute.is_some() {
        if options.hour.is_some() {
            push(parts, "literal", ":");
        }
        let value = format!("{:0width$}", fields.minute, width = TWO_DIGIT_WIDTH);
        push(parts, "minute", value);
    }
    if options.second.is_some() {
        if options.hour.is_some() || options.minute.is_some() {
            push(parts, "literal", ":");
        }
        let value = format!("{:0width$}", fields.second, width = TWO_DIGIT_WIDTH);
        push(parts, "second", value);
        if let Some(digits) = options.fractional_second_digits {
            if (1..=FULL_FRACTION_DIGITS).contains(&digits) {
                let divisor = DECIMAL_RADIX.pow(FULL_FRACTION_DIGITS - digits);
                push(parts, "literal", ".");
                push(
                    parts,
                    "fractionalSecond",
                    format!(
                        "{:0width$}",
                        fields.millisecond / divisor,
                        width = digits as usize
                    ),
                );
            }
        }
    }
    if options.hour.is_some() && (hour12 || options.day_period.is_some()) {
        push(parts, "literal", " ");
        let value = options.day_period.as_deref().map_or_else(
            || {
                if fields.hour < HOURS_PER_HALF_DAY {
                    "AM"
                } else {
                    "PM"
                }
            },
            |style| day_period_value(fields.hour, style),
        );
        push(parts, "dayPeriod", value);
    }
}

fn day_period_value(hour: u32, style: &str) -> &'static str {
    let (long, short, narrow) = match hour {
        0..=NIGHT_END_HOUR | LATE_NIGHT_START_HOUR..=HOURS_PER_DAY => {
            ("at night", "at night", "at night")
        }
        MORNING_START_HOUR..=MORNING_END_HOUR => {
            ("in the morning", "in the morning", "in the morning")
        }
        NOON_HOUR => (PERIOD_NOON, PERIOD_NOON, PERIOD_NARROW_NOON),
        EVENING_START_HOUR..=EVENING_END_HOUR => {
            ("in the evening", "in the evening", "in the evening")
        }
        _ => ("in the afternoon", "in the afternoon", "in the afternoon"),
    };
    match style {
        "narrow" => narrow,
        "short" => short,
        _ => long,
    }
}

fn format_weekday(fields: DateTimeFields, style: &str) -> String {
    const WEEKDAYS: [&str; DAYS_PER_WEEK] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let weekday = super::temporal_date::iso_day_of_week(super::temporal_date::IsoDate {
        year: fields.year,
        month: fields.month,
        day: fields.day,
    });
    let index = (weekday % DAYS_PER_WEEK_U32) as usize;
    let name = WEEKDAYS[index];
    match style {
        "short" => name[..SHORT_WEEKDAY_LENGTH].to_string(),
        "narrow" => name[..FIRST_WEEKDAY_CHAR].to_string(),
        _ => name.to_string(),
    }
}

fn push(parts: &mut Vec<(String, String)>, kind: &str, value: impl Into<String>) {
    parts.push((kind.to_string(), value.into()));
}

const MONTH_NAMES: [&str; MONTHS_PER_YEAR] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_ABBREVIATIONS: [&str; MONTHS_PER_YEAR] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
