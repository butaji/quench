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
const SHORT_MONTH_LENGTH: usize = 3;
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
const CYCLIC_STEM_COUNT: usize = 10;
const CYCLIC_BRANCH_COUNT: usize = 12;
const LEAP_MONTH_SUFFIX: &str = "bis";
const HEBREW_MONTHS_EN: &[(&str, &str)] = &[
    ("M01", "Tishri"),
    ("M02", "Heshvan"),
    ("M03", "Kislev"),
    ("M04", "Tevet"),
    ("M05", "Shevat"),
    ("M05L", "Adar I"),
    ("M06", "Adar"),
    ("M07", "Nisan"),
    ("M08", "Iyar"),
    ("M09", "Sivan"),
    ("M10", "Tamuz"),
    ("M11", "Av"),
    ("M12", "Elul"),
];
const ISLAMIC_MONTHS_EN: &[&str] = &[
    "Muharram",
    "Safar",
    "Rabiʻ I",
    "Rabiʻ II",
    "Jumada I",
    "Jumada II",
    "Rajab",
    "Shaʻban",
    "Ramadan",
    "Shawwal",
    "Dhuʻl-Qiʻdah",
    "Dhuʻl-Hijjah",
];
const FORMAT_STYLE_FULL: &str = "full";
const FORMAT_STYLE_LONG: &str = "long";
const FORMAT_STYLE_MEDIUM: &str = "medium";
const FORMAT_STYLE_SHORT: &str = "short";

#[derive(Clone)]
pub(super) struct DateTimeFields {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub month_code: Option<String>,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
    pub weekday: u32,
    pub has_date: bool,
    pub has_time: bool,
    pub is_temporal: bool,
    pub temporal_kind: Option<TemporalKind>,
    pub calendar: Option<String>,
    pub related_year: Option<i32>,
    pub cyclic_year: Option<u8>,
    pub era: Option<String>,
    pub era_year: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TemporalKind {
    PlainDate,
    PlainDateTime,
    PlainMonthDay,
    PlainYearMonth,
    PlainTime,
    Instant,
    ZonedDateTime,
}

impl DateTimeFields {
    pub(super) fn from_date(date: &chrono::DateTime<chrono::FixedOffset>) -> Self {
        Self::from_components(
            date.year(),
            date.month(),
            date.day(),
            date.hour(),
            date.minute(),
            date.second(),
            date.timestamp_subsec_millis(),
        )
    }

    pub(super) fn from_components(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        millisecond: u32,
    ) -> Self {
        Self {
            year,
            month,
            day,
            month_code: None,
            hour,
            minute,
            second,
            millisecond,
            weekday: super::temporal_date::iso_day_of_week(super::temporal_date::IsoDate {
                year,
                month,
                day,
            }),
            has_date: true,
            has_time: true,
            is_temporal: false,
            temporal_kind: None,
            calendar: None,
            related_year: None,
            cyclic_year: None,
            era: None,
            era_year: None,
        }
    }
}

#[derive(Default)]
pub(super) struct DateTimePartOptions {
    pub locale: Option<String>,
    pub weekday: Option<String>,
    pub era: Option<String>,
    pub year: Option<String>,
    pub month: Option<String>,
    pub day: Option<String>,
    pub hour: Option<String>,
    pub minute: Option<String>,
    pub second: Option<String>,
    pub day_period: Option<String>,
    pub hour_cycle: Option<String>,
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
    fields: &DateTimeFields,
    options: &DateTimePartOptions,
) -> Vec<(String, String)> {
    let has_date = fields.has_date
        && (options.year.is_some()
            || options.month.is_some()
            || options.day.is_some()
            || options.weekday.is_some()
            || options.era.is_some());
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
    fields: &DateTimeFields,
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
        let value = localized_calendar_month_name(fields, style, options.locale.as_deref())
            .unwrap_or_else(|| match (style, month) {
                ("long" | "short" | "narrow", Some(name)) => name.to_string(),
                ("2-digit", Some(prefix)) => format!("{prefix}{}", fields.month),
                _ => fields.month.to_string(),
            });
        let value = if fields
            .month_code
            .as_deref()
            .is_some_and(|code| code.ends_with('L'))
            && fields.calendar.as_deref() != Some("hebrew")
            && matches!(style, "numeric" | "2-digit")
        {
            format!("{value}{LEAP_MONTH_SUFFIX}")
        } else {
            value
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
        let display_year = if options.era.is_some() && fields.year <= 0 {
            1 - fields.year
        } else {
            fields.year
        };
        let value = if style == "2-digit" {
            format!(
                "{:0width$}",
                display_year.rem_euclid(YEARS_PER_CENTURY),
                width = TWO_DIGIT_WIDTH
            )
        } else {
            display_year.to_string()
        };
        if let Some(related_year) = fields.related_year {
            push(parts, "relatedYear", related_year.to_string());
            if let Some(cyclic_year) = fields.cyclic_year {
                if options
                    .month
                    .as_deref()
                    .is_none_or(|style| !matches!(style, "long" | "short" | "narrow"))
                {
                    push(
                        parts,
                        "yearName",
                        cyclic_year_name(cyclic_year, options.locale.as_deref()),
                    );
                    if options
                        .locale
                        .as_deref()
                        .is_some_and(|locale| locale.starts_with("zh"))
                    {
                        push(parts, "literal", "年");
                    }
                }
            }
        } else {
            push(parts, "year", value);
        }
    }
    if let (Some(style), Some(code)) = (&options.era, fields.era.as_deref()) {
        push(parts, "literal", " ");
        push(parts, "era", era_code_value(code, style));
    }
}

fn localized_calendar_month_name(
    fields: &DateTimeFields,
    style: &str,
    locale: Option<&str>,
) -> Option<String> {
    if !locale.is_some_and(|locale| locale.starts_with("en")) {
        return None;
    }
    let name = match fields.calendar.as_deref()? {
        "hebrew" => {
            let code = fields.month_code.as_deref()?;
            HEBREW_MONTHS_EN
                .iter()
                .find_map(|(month_code, name)| (*month_code == code).then_some(*name))?
        }
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" => {
            ISLAMIC_MONTHS_EN.get(fields.month.checked_sub(FIRST_MONTH_NUMBER)? as usize)?
        }
        _ => return None,
    };
    Some(match style {
        "long" => name.to_string(),
        "short" => name.chars().take(SHORT_MONTH_LENGTH).collect(),
        "narrow" => name.chars().next()?.to_string(),
        _ => return None,
    })
}

fn era_code_value(code: &str, style: &str) -> String {
    let (long, short) = match code {
        "ce" => ("Anno Domini", "AD"),
        "bce" => ("Before Christ", "BC"),
        "be" => ("Buddhist Era", "BE"),
        "ah" => ("Anno Hegirae", "AH"),
        "am" => ("Anno Mundi", "AM"),
        other => (other, other),
    };
    match style {
        "long" => long.to_string(),
        "narrow" => short.chars().next().unwrap_or_default().to_string(),
        _ => short.to_string(),
    }
}

fn cyclic_year_name(year: u8, locale: Option<&str>) -> String {
    if locale.is_some_and(|locale| locale.starts_with("zh")) {
        const STEMS: [&str; CYCLIC_STEM_COUNT] =
            ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
        const BRANCHES: [&str; CYCLIC_BRANCH_COUNT] = [
            "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
        ];
        let index = usize::from(year.saturating_sub(1));
        format!(
            "{}{}",
            STEMS[index % STEMS.len()],
            BRANCHES[index % BRANCHES.len()]
        )
    } else {
        "1".into()
    }
}

fn append_time(
    parts: &mut Vec<(String, String)>,
    fields: &DateTimeFields,
    options: &DateTimePartOptions,
) {
    if options.hour.is_none() && options.minute.is_none() && options.second.is_none() {
        if let Some(style) = &options.day_period {
            push(parts, "dayPeriod", day_period_value(fields.hour, style));
        }
        return;
    }
    let hour_cycle = options.hour_cycle.as_deref().unwrap_or("h12");
    let hour12 = matches!(hour_cycle, "h11" | "h12");
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
        let value = if style == "2-digit" || matches!(hour_cycle, "h23" | "h24") {
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

fn format_weekday(fields: &DateTimeFields, style: &str) -> String {
    const WEEKDAYS: [&str; DAYS_PER_WEEK] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let weekday = fields.weekday;
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
