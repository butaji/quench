use icu_calendar::{
    cal::Iso,
    options::{DateAddOptions, DateDifferenceOptions, DateDurationUnit, Overflow},
    types::{DateDuration, Month},
    AnyCalendar, AnyCalendarKind, Date,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub month_code: String,
    pub related_year: Option<i32>,
    pub cyclic_year: Option<u8>,
    pub era: Option<String>,
    pub era_year: Option<i32>,
    pub day_of_year: u32,
    pub days_in_month: u32,
    pub days_in_year: u32,
    pub months_in_year: u32,
    pub is_leap_year: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarDifferenceUnit {
    Years,
    Months,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarDifferenceDirection {
    Until,
    Since,
}

pub fn calendar_fields_from_iso(
    year: i32,
    month: u32,
    day: u32,
    calendar: &str,
) -> Option<CalendarDate> {
    if matches!(calendar, "iso8601" | "gregory") {
        let has_era = calendar == "gregory";
        return Some(CalendarDate {
            year,
            month,
            day,
            month_code: format!("M{month:02}"),
            related_year: None,
            cyclic_year: None,
            era: has_era.then(|| if year > 0 { "ce" } else { "bce" }.into()),
            era_year: has_era.then_some(if year > 0 { year } else { 1 - year }),
            day_of_year: iso_day_of_year(year, month, day)?,
            days_in_month: quench_temporal::days_in_month(year, month)?,
            days_in_year: if quench_temporal::is_leap_year(year) {
                GREGORIAN_LEAP_YEAR_DAYS
            } else {
                GREGORIAN_COMMON_YEAR_DAYS
            },
            months_in_year: MONTHS_PER_YEAR,
            is_leap_year: quench_temporal::is_leap_year(year),
        });
    }
    let kind = calendar_kind(calendar)?;
    let date = Date::try_new_iso(year, month.try_into().ok()?, day.try_into().ok()?)
        .ok()?
        .to_calendar(AnyCalendar::new(kind));
    let (year, related_year, cyclic_year, era, era_year) = match date.year() {
        icu_calendar::types::YearInfo::Era(value) => {
            let year = if calendar == "ethiopic" && value.era.as_str() == "aa" {
                value.year - ETHIOPIC_AMETE_ALEM_YEAR_OFFSET
            } else if calendar == "gregory" && value.era.as_str() == "bce"
                || calendar == "roc" && value.era.as_str() == "broc"
                || calendar.starts_with("islamic") && value.era.as_str() == "bh"
            {
                1 - value.year
            } else if calendar == "japanese" {
                year
            } else {
                value.year
            };
            (
                year,
                None,
                None,
                Some(value.era.as_str().to_owned()),
                Some(value.year),
            )
        }
        icu_calendar::types::YearInfo::Cyclic(value) => (
            value.related_iso,
            Some(value.related_iso),
            Some(value.year),
            None,
            None,
        ),
        _ => (year, None, None, None, None),
    };
    Some(CalendarDate {
        year,
        month: u32::from(date.month().ordinal),
        day: u32::from(date.day_of_month().0),
        month_code: date.month().to_input().code().0.to_string(),
        related_year,
        cyclic_year,
        era,
        era_year,
        day_of_year: u32::from(date.day_of_year().0),
        days_in_month: u32::from(date.days_in_month()),
        days_in_year: u32::from(date.days_in_year()),
        months_in_year: u32::from(date.months_in_year()),
        is_leap_year: date.is_in_leap_year(),
    })
}

fn iso_day_of_year(year: i32, month: u32, day: u32) -> Option<u32> {
    let current = quench_temporal::days_from_civil(quench_temporal::IsoDate { year, month, day });
    let start = quench_temporal::days_from_civil(quench_temporal::IsoDate {
        year,
        month: FIRST_MONTH_OF_YEAR,
        day: FIRST_DAY_OF_MONTH_VALUE,
    });
    u32::try_from(current.checked_sub(start)?.checked_add(i64::from(FIRST_DAY_OF_MONTH_VALUE))?)
        .ok()
}

pub fn calendar_date_to_iso(
    year: i32,
    month: u32,
    day: u32,
    calendar: &str,
) -> Option<(i32, u32, u32)> {
    calendar_date_to_iso_with_overflow(year, month, day, calendar, false)
}

pub fn calendar_month_from_code(year: i32, code: &str, calendar: &str) -> Option<u32> {
    let kind = calendar_kind(calendar)?;
    let digits = code.strip_prefix('M')?;
    let (digits, leap) = digits
        .strip_suffix('L')
        .map_or((digits, false), |digits| (digits, true));
    if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse::<u8>().ok()?;
    if number == 0 || number > 13 || leap && !matches!(calendar, "chinese" | "dangi" | "hebrew") {
        return None;
    }
    if number == 13 && !leap && matches!(calendar, "coptic" | "ethiopic" | "ethioaa") {
        return Some(13);
    }
    let month_code = if leap {
        Month::leap(number)
    } else {
        Month::new(number)
    }
    .code();
    #[allow(deprecated)]
    let date = Date::try_new_from_codes(
        None,
        year,
        month_code,
        FIRST_DAY_OF_MONTH,
        AnyCalendar::new(kind),
    )
    .ok()?;
    (date.month().to_input().code().0 == code).then_some(u32::from(date.month().ordinal))
}

pub fn calendar_reference_date_from_code(
    code: &str,
    day: u32,
    calendar: &str,
    constrain: bool,
) -> Option<(i32, u32, u32)> {
    if let Some(year) = lunisolar_reference_year(code, day, calendar) {
        if let Some(date) = calendar_iso_date_for_code(year, code, day, calendar) {
            return Some(date);
        }
    }
    let years = (REFERENCE_YEAR_START..=REFERENCE_YEAR_END).rev();
    let exact = years.clone().find_map(|year| {
        (FIRST_MONTH_OF_YEAR..=MONTHS_PER_YEAR).find_map(|month| {
            (FIRST_DAY_OF_MONTH_VALUE..=REFERENCE_MONTH_DAY_LIMIT).find_map(|iso_day| {
                let fields = calendar_fields_from_iso(year, month, iso_day, calendar)?;
                (fields.month_code == code && fields.day == day)
                    .then_some((year, month, iso_day))
            })
        })
    });
    if exact.is_some() || !constrain {
        return exact;
    }
    let reference = years
        .flat_map(|year| {
            (FIRST_MONTH_OF_YEAR..=MONTHS_PER_YEAR).filter_map(move |month| {
                (FIRST_DAY_OF_MONTH_VALUE..=REFERENCE_MONTH_DAY_LIMIT).find_map(|iso_day| {
                    let fields = calendar_fields_from_iso(year, month, iso_day, calendar)?;
                    (fields.month_code == code && fields.day == FIRST_DAY_OF_MONTH_VALUE)
                        .then_some((
                            fields.days_in_month,
                            fields.year,
                            fields.month,
                            year,
                            month,
                            iso_day,
                        ))
                })
            })
        })
        .max_by_key(|(days, year, _, _, _, _)| (*days, *year));
    let Some(reference) = reference else {
        return constrained_regular_month_reference(code, day, calendar, constrain);
    };
    if day >= THIRTY_DAY_MONTH_LENGTH
        && reference.0 < THIRTY_DAY_MONTH_LENGTH
        && matches!(calendar, "chinese" | "dangi")
        && code.ends_with('L')
    {
        return constrained_regular_month_reference(code, day, calendar, constrain);
    }
    let days_in_month = fixed_month_length(calendar, code).unwrap_or(reference.0);
    let target_day = day.min(days_in_month);
    let exact_constrained = (REFERENCE_YEAR_START..=REFERENCE_YEAR_END)
        .rev()
        .find_map(|year| {
            (FIRST_MONTH_OF_YEAR..=MONTHS_PER_YEAR).find_map(|month| {
                (FIRST_DAY_OF_MONTH_VALUE..=REFERENCE_MONTH_DAY_LIMIT).find_map(|iso_day| {
                    let fields = calendar_fields_from_iso(year, month, iso_day, calendar)?;
                    (fields.month_code == code && fields.day == target_day)
                        .then_some((year, month, iso_day))
                })
            })
        });
    if exact_constrained.is_some() {
        return exact_constrained;
    }
    let first_day = quench_temporal::days_from_civil(quench_temporal::IsoDate {
        year: reference.3,
        month: reference.4,
        day: reference.5,
    });
    let date = quench_temporal::civil_from_days(
        first_day + i64::from(target_day.saturating_sub(FIRST_DAY_OF_MONTH_VALUE)),
    )?;
    Some((date.year, date.month, date.day))
}

fn calendar_iso_date_for_code(
    year: i32,
    code: &str,
    day: u32,
    calendar: &str,
) -> Option<(i32, u32, u32)> {
    (FIRST_MONTH_OF_YEAR..=MONTHS_PER_YEAR).find_map(|month| {
        (FIRST_DAY_OF_MONTH_VALUE..=REFERENCE_MONTH_DAY_LIMIT).find_map(|iso_day| {
            let fields = calendar_fields_from_iso(year, month, iso_day, calendar)?;
            (fields.month_code == code && fields.day == day).then_some((year, month, iso_day))
        })
    })
}

fn lunisolar_reference_year(code: &str, day: u32, calendar: &str) -> Option<i32> {
    matches!(calendar, "chinese" | "dangi")
        .then(|| {
            LUNISOLAR_REFERENCE_YEARS
                .iter()
                .find_map(|(candidate_code, candidate_day, year)| {
                    (*candidate_code == code && *candidate_day == day).then_some(*year)
                })
        })
        .flatten()
}

fn constrained_regular_month_reference(
    code: &str,
    day: u32,
    calendar: &str,
    constrain: bool,
) -> Option<(i32, u32, u32)> {
    (constrain && matches!(calendar, "chinese" | "dangi"))
        .then(|| code.strip_suffix('L'))
        .flatten()
        .and_then(|regular| calendar_reference_date_from_code(regular, day, calendar, true))
}

fn fixed_month_length(calendar: &str, code: &str) -> Option<u32> {
    let month = code.strip_prefix('M')?.parse::<u32>().ok()?;
    (matches!(calendar, "coptic" | "ethiopic" | "ethioaa") && month <= 12)
        .then_some(THIRTY_DAY_MONTH_LENGTH)
}

pub fn calendar_year_from_era(era: &str, year: i32, calendar: &str) -> Option<i32> {
    let era = era.to_ascii_lowercase();
    Some(match (calendar, era.as_str()) {
        ("gregory", "ad" | "ce") | ("buddhist", "be") | ("hebrew", "am")
        | ("coptic", "am") | ("ethiopic", "am") | ("ethioaa", "aa")
        | ("indian", "shaka") | ("persian", "ap") | ("roc", "roc")
        | ("islamic-civil" | "islamic-tbla" | "islamic-umalqura", "ah") => year,
        ("gregory", "bc" | "bce") | ("roc", "broc")
        | ("islamic-civil" | "islamic-tbla" | "islamic-umalqura", "bh") => 1 - year,
        ("ethiopic", "aa") => year - ETHIOPIC_AMETE_ALEM_YEAR_OFFSET,
        ("japanese", "ad" | "ce") => year,
        ("japanese", "bc" | "bce") => 1 - year,
        ("japanese", "reiwa") => year + JAPANESE_REIWA_YEAR_OFFSET,
        ("japanese", "heisei") => year + JAPANESE_HEISEI_YEAR_OFFSET,
        ("japanese", "showa") => year + JAPANESE_SHOWA_YEAR_OFFSET,
        ("japanese", "taisho") => year + JAPANESE_TAISHO_YEAR_OFFSET,
        ("japanese", "meiji") => year + JAPANESE_MEIJI_YEAR_OFFSET,
        _ => return None,
    })
}

pub fn calendar_uses_eras(calendar: &str) -> bool {
    calendar_kind(calendar).is_some() && !matches!(calendar, "chinese" | "dangi")
}

pub fn calendar_date_add(
    date: (i32, u32, u32),
    duration: (i64, i64, i64, i64),
    calendar: &str,
    constrain: bool,
) -> Option<(i32, u32, u32)> {
    let kind = if calendar == "iso8601" {
        AnyCalendarKind::Iso
    } else {
        calendar_kind(calendar)?
    };
    let values = [duration.0, duration.1, duration.2, duration.3];
    let Some(first_nonzero) = values.iter().copied().find(|value| *value != 0) else {
        return Some(date);
    };
    let is_negative = first_nonzero < 0;
    if values
        .iter()
        .any(|value| *value != 0 && (*value < 0) != is_negative)
    {
        return None;
    }
    let [years, months, weeks, days] = values.map(|value| u32::try_from(value.unsigned_abs()).ok());
    let duration = DateDuration {
        is_negative,
        years: years?,
        months: months?,
        weeks: weeks?,
        days: days?,
    };
    let date = Date::try_new_iso(date.0, date.1.try_into().ok()?, date.2.try_into().ok()?)
        .ok()?
        .to_calendar(AnyCalendar::new(kind));
    let mut options = DateAddOptions::default();
    options.overflow = Some(if constrain {
            Overflow::Constrain
        } else {
            Overflow::Reject
        });
    let date = date.try_added_with_options(duration, options).ok()?.to_calendar(Iso);
    Some((
        date.year().extended_year(),
        u32::from(date.month().ordinal),
        u32::from(date.day_of_month().0),
    ))
}

pub fn calendar_date_difference(
    start: (i32, u32, u32),
    end: (i32, u32, u32),
    calendar: &str,
    largest_unit: CalendarDifferenceUnit,
    direction: CalendarDifferenceDirection,
) -> Option<(i64, i64, i64, i64)> {
    let kind = if calendar == "iso8601" {
        AnyCalendarKind::Iso
    } else {
        calendar_kind(calendar)?
    };
    let start = Date::try_new_iso(start.0, start.1.try_into().ok()?, start.2.try_into().ok()?)
        .ok()?
        .to_calendar(AnyCalendar::new(kind));
    let end = Date::try_new_iso(end.0, end.1.try_into().ok()?, end.2.try_into().ok()?)
        .ok()?
        .to_calendar(AnyCalendar::new(kind));
    let mut options = DateDifferenceOptions::default();
    options.largest_unit = Some(match largest_unit {
        CalendarDifferenceUnit::Years => DateDurationUnit::Years,
        CalendarDifferenceUnit::Months => DateDurationUnit::Months,
    });
    let duration = match start.try_until_with_options(&end, options) {
        Ok(duration) => duration,
        Err(_) => {
            let mut duration = end.try_until_with_options(&start, options).ok()?;
            duration.is_negative = !duration.is_negative;
            duration
        }
    };
    let nonzero = duration.years != 0
        || duration.months != 0
        || duration.weeks != 0
        || duration.days != 0;
    let negative = nonzero
        && match direction {
            CalendarDifferenceDirection::Until => duration.is_negative,
            CalendarDifferenceDirection::Since => !duration.is_negative,
        };
    let sign = if negative { -1 } else { 1 };
    Some((
        i64::from(duration.years) * sign,
        i64::from(duration.months) * sign,
        i64::from(duration.weeks) * sign,
        i64::from(duration.days) * sign,
    ))
}

pub fn calendar_date_to_iso_with_overflow(
    year: i32,
    month: u32,
    day: u32,
    calendar: &str,
    constrain: bool,
) -> Option<(i32, u32, u32)> {
    if matches!(calendar, "iso8601" | "gregory") {
        let month = if constrain {
            month.clamp(FIRST_MONTH_OF_YEAR, MONTHS_PER_YEAR)
        } else {
            month
        };
        let last_day = quench_temporal::days_in_month(year, month)?;
        let day = if constrain {
            day.clamp(FIRST_DAY_OF_MONTH.into(), last_day)
        } else {
            day
        };
        return (day >= FIRST_DAY_OF_MONTH.into()).then_some((year, month, day));
    }
    let kind = calendar_kind(calendar)?;
    let first_month = Date::try_new(
        year.into(),
        Month::new(1),
        FIRST_DAY_OF_MONTH,
        AnyCalendar::new(kind),
    )
    .ok()?;
    let month = if constrain {
        month.clamp(FIRST_MONTH_OF_YEAR, u32::from(first_month.months_in_year()))
    } else {
        month
    };
    let input_month = calendar_input_month(year, month, kind);
    let first_day = Date::try_new(
        year.into(),
        input_month,
        FIRST_DAY_OF_MONTH,
        AnyCalendar::new(kind),
    )
    .ok()?;
    let day = if constrain {
        day.clamp(
            FIRST_DAY_OF_MONTH.into(),
            u32::from(first_day.days_in_month()),
        )
    } else {
        day
    };
    let date = Date::try_new(
        year.into(),
        input_month,
        day.try_into().ok()?,
        AnyCalendar::new(kind),
    )
    .ok()?
    .to_calendar(Iso);
    Some((
        date.year().extended_year(),
        u32::from(date.month().ordinal),
        u32::from(date.day_of_month().0),
    ))
}

fn calendar_input_month(year: i32, month: u32, kind: AnyCalendarKind) -> Month {
    if !matches!(
        kind,
        AnyCalendarKind::Chinese | AnyCalendarKind::Dangi | AnyCalendarKind::Hebrew
    ) {
        return Month::new(month as u8);
    }
    let leap_ordinal = (1..=ISO_MONTHS_PER_YEAR).find_map(|base| {
        Date::try_new(
            year.into(),
            Month::leap(base as u8),
            FIRST_DAY_OF_MONTH,
            AnyCalendar::new(kind),
        )
        .ok()
        .filter(|date| date.month().leap_status() != icu_calendar::types::LeapStatus::Normal)
        .map(|date| u32::from(date.month().ordinal))
    });
    match leap_ordinal {
        Some(ordinal) if month == ordinal => Month::leap(month.saturating_sub(1) as u8),
        Some(ordinal) if month > ordinal => Month::new(month.saturating_sub(1) as u8),
        _ => Month::new(month as u8),
    }
}

fn calendar_kind(calendar: &str) -> Option<AnyCalendarKind> {
    Some(match calendar {
        "buddhist" => AnyCalendarKind::Buddhist,
        "chinese" => AnyCalendarKind::Chinese,
        "coptic" => AnyCalendarKind::Coptic,
        "dangi" => AnyCalendarKind::Dangi,
        "ethiopic" => AnyCalendarKind::Ethiopian,
        "ethioaa" => AnyCalendarKind::EthiopianAmeteAlem,
        "gregory" => AnyCalendarKind::Gregorian,
        "hebrew" => AnyCalendarKind::Hebrew,
        "indian" => AnyCalendarKind::Indian,
        "islamic-civil" => AnyCalendarKind::HijriTabularTypeIIFriday,
        "islamic-tbla" => AnyCalendarKind::HijriTabularTypeIIThursday,
        "islamic-umalqura" => AnyCalendarKind::HijriUmmAlQura,
        "japanese" => AnyCalendarKind::Japanese,
        "persian" => AnyCalendarKind::Persian,
        "roc" => AnyCalendarKind::Roc,
        _ => return None,
    })
}

const FIRST_DAY_OF_MONTH: u8 = 1;
const FIRST_MONTH_OF_YEAR: u32 = 1;
const ISO_MONTHS_PER_YEAR: u32 = 12;
const ETHIOPIC_AMETE_ALEM_YEAR_OFFSET: i32 = 5_500;
const JAPANESE_REIWA_YEAR_OFFSET: i32 = 2_018;
const JAPANESE_HEISEI_YEAR_OFFSET: i32 = 1_988;
const JAPANESE_SHOWA_YEAR_OFFSET: i32 = 1_925;
const JAPANESE_TAISHO_YEAR_OFFSET: i32 = 1_911;
const JAPANESE_MEIJI_YEAR_OFFSET: i32 = 1_867;
const REFERENCE_YEAR_START: i32 = 1_932;
const REFERENCE_YEAR_END: i32 = 1_972;
const FIRST_DAY_OF_MONTH_VALUE: u32 = 1;
const REFERENCE_MONTH_DAY_LIMIT: u32 = 31;
const THIRTY_DAY_MONTH_LENGTH: u32 = 30;
const LUNISOLAR_REFERENCE_YEARS: &[(&str, u32, i32)] = &[
    ("M03L", 30, 1_955),
    ("M04L", 30, 1_944),
    ("M05L", 30, 1_952),
    ("M06L", 30, 1_941),
    ("M07L", 30, 1_938),
    ("M09L", 1, 2_014),
    ("M09L", 29, 2_014),
    ("M10L", 1, 1_984),
    ("M10L", 29, 1_984),
    ("M11L", 1, 2_033),
    ("M11L", 29, 2_034),
];
const MONTHS_PER_YEAR: u32 = 12;
const GREGORIAN_COMMON_YEAR_DAYS: u32 = 365;
const GREGORIAN_LEAP_YEAR_DAYS: u32 = 366;
