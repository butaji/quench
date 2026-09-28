use icu_calendar::{AnyCalendar, AnyCalendarKind, Date, cal::Iso, types::Month};

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
    pub days_in_month: u32,
    pub days_in_year: u32,
    pub months_in_year: u32,
    pub is_leap_year: bool,
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
            let year = if calendar == "ethiopic" && value.year > ETHIOPIC_AMETE_ALEM_YEAR_OFFSET {
                value.year - ETHIOPIC_AMETE_ALEM_YEAR_OFFSET
            } else if calendar.starts_with("islamic") && year < ISLAMIC_ERA_START_YEAR
                || calendar == "roc" && year < ROC_ERA_START_YEAR
            {
                1 - value.year
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
        month: u32::from(date.month().number()),
        day: u32::from(date.day_of_month().0),
        month_code: date.month().to_input().code().0.to_string(),
        related_year,
        cyclic_year,
        era,
        era_year,
        days_in_month: u32::from(date.days_in_month()),
        days_in_year: u32::from(date.days_in_year()),
        months_in_year: u32::from(date.months_in_year()),
        is_leap_year: date.is_in_leap_year(),
    })
}

pub fn calendar_date_to_iso(
    year: i32,
    month: u32,
    day: u32,
    calendar: &str,
) -> Option<(i32, u32, u32)> {
    calendar_date_to_iso_with_overflow(year, month, day, calendar, false)
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
const ISLAMIC_ERA_START_YEAR: i32 = 622;
const ROC_ERA_START_YEAR: i32 = 1_912;
const MONTHS_PER_YEAR: u32 = 12;
const GREGORIAN_COMMON_YEAR_DAYS: u32 = 365;
const GREGORIAN_LEAP_YEAR_DAYS: u32 = 366;
