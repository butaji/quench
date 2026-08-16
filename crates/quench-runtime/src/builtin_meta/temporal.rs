use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Temporal.Duration"),
        Builtin::TemporalDurationFrom => Some("Temporal.Duration.from"),
        Builtin::TemporalDurationCompare => Some("Temporal.Duration.compare"),
        Builtin::TemporalDurationAbs => Some("Temporal.Duration.prototype.abs"),
        Builtin::TemporalDurationToLocaleString => {
            Some("Temporal.Duration.prototype.toLocaleString")
        }
        Builtin::TemporalPlainDate => Some("Temporal.PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("Temporal.PlainDate.from"),
        Builtin::TemporalPlainDateWithCalendar => Some("Temporal.PlainDate.prototype.withCalendar"),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("get dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("get dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some("get daysInMonth"),
        Builtin::TemporalPlainDateDaysInYearGetter => Some("get daysInYear"),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some("get daysInWeek"),
        Builtin::TemporalPlainDateInLeapYearGetter => Some("get inLeapYear"),
        Builtin::TemporalPlainDateEraGetter => Some("get era"),
        Builtin::TemporalPlainDateEraYearGetter => Some("get eraYear"),
        Builtin::TemporalPlainDateCalendarIdGetter => Some("get calendarId"),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some("get weekOfYear"),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some("get yearOfWeek"),
        Builtin::TemporalPlainDateDayGetter => Some("get day"),
        Builtin::TemporalPlainDateValueOf => Some("Temporal.PlainDate.prototype.valueOf"),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Duration"),
        Builtin::TemporalDurationFrom => Some("from"),
        Builtin::TemporalDurationCompare => Some("compare"),
        Builtin::TemporalDurationAbs => Some("abs"),
        Builtin::TemporalDurationToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainDate => Some("PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("from"),
        Builtin::TemporalPlainDateWithCalendar => Some("withCalendar"),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some("daysInMonth"),
        Builtin::TemporalPlainDateDaysInYearGetter => Some("daysInYear"),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some("daysInWeek"),
        Builtin::TemporalPlainDateInLeapYearGetter => Some("inLeapYear"),
        Builtin::TemporalPlainDateEraGetter => Some("era"),
        Builtin::TemporalPlainDateEraYearGetter => Some("eraYear"),
        Builtin::TemporalPlainDateCalendarIdGetter => Some("calendarId"),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some("weekOfYear"),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some("yearOfWeek"),
        Builtin::TemporalPlainDateDayGetter => Some("day"),
        Builtin::TemporalPlainDateValueOf => Some("valueOf"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TemporalPlainDate => Some(3.0),
        Builtin::TemporalDurationFrom => Some(1.0),
        Builtin::TemporalDurationCompare => Some(2.0),
        Builtin::TemporalDurationAbs => Some(0.0),
        Builtin::TemporalDurationToLocaleString => Some(0.0),
        Builtin::TemporalPlainDateFrom => Some(1.0),
        Builtin::TemporalPlainDateWithCalendar => Some(1.0),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateDayOfYearGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInYearGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateInLeapYearGetter => Some(0.0),
        Builtin::TemporalPlainDateEraGetter => Some(0.0),
        Builtin::TemporalPlainDateEraYearGetter => Some(0.0),
        Builtin::TemporalPlainDateCalendarIdGetter => Some(0.0),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some(0.0),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateDayGetter => Some(0.0),
        Builtin::TemporalPlainDateValueOf => Some(0.0),
        _ => None,
    }
}
