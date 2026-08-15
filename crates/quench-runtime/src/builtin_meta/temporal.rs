use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Temporal.Duration"),
        Builtin::TemporalDurationFrom => Some("Temporal.Duration.from"),
        Builtin::TemporalDurationCompare => Some("Temporal.Duration.compare"),
        Builtin::TemporalDurationNegated => Some("Temporal.Duration.prototype.negated"),
        Builtin::TemporalDurationAbs => Some("Temporal.Duration.prototype.abs"),
        Builtin::TemporalDurationToJSON => Some("Temporal.Duration.prototype.toJSON"),
        Builtin::TemporalDurationAdd => Some("Temporal.Duration.prototype.add"),
        Builtin::TemporalDurationValueOf => Some("Temporal.Duration.prototype.valueOf"),
        Builtin::TemporalPlainDate => Some("Temporal.PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("Temporal.PlainDate.from"),
        Builtin::TemporalPlainDateToString => Some("Temporal.PlainDate.prototype.toString"),
        Builtin::TemporalPlainDateToJSON => Some("Temporal.PlainDate.prototype.toJSON"),
        Builtin::TemporalPlainDateCalendarIdGetter => {
            Some("Temporal.PlainDate.prototype.calendarId")
        }
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("Temporal.PlainDate.prototype.dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("Temporal.PlainDate.prototype.dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => {
            Some("Temporal.PlainDate.prototype.daysInMonth")
        }
        Builtin::TemporalPlainDateDaysInWeekGetter => {
            Some("Temporal.PlainDate.prototype.daysInWeek")
        }
        Builtin::TemporalPlainDateDaysInYearGetter => {
            Some("Temporal.PlainDate.prototype.daysInYear")
        }
        Builtin::TemporalPlainDateInLeapYearGetter => {
            Some("Temporal.PlainDate.prototype.inLeapYear")
        }
        Builtin::TemporalPlainDateMonthsInYearGetter => {
            Some("Temporal.PlainDate.prototype.monthsInYear")
        }
        Builtin::TemporalPlainDateEquals => Some("Temporal.PlainDate.prototype.equals"),
        Builtin::TemporalPlainDateAdd => Some("Temporal.PlainDate.prototype.add"),
        Builtin::TemporalPlainDateSubtract => Some("Temporal.PlainDate.prototype.subtract"),
        Builtin::TemporalPlainDateUntil => Some("Temporal.PlainDate.prototype.until"),
        Builtin::TemporalPlainDateSince => Some("Temporal.PlainDate.prototype.since"),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Duration"),
        Builtin::TemporalDurationFrom => Some("from"),
        Builtin::TemporalDurationCompare => Some("compare"),
        Builtin::TemporalDurationNegated => Some("negated"),
        Builtin::TemporalDurationAbs => Some("abs"),
        Builtin::TemporalDurationToJSON => Some("toJSON"),
        Builtin::TemporalDurationAdd => Some("add"),
        Builtin::TemporalDurationValueOf => Some("valueOf"),
        Builtin::TemporalPlainDate => Some("PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("from"),
        Builtin::TemporalPlainDateToString => Some("toString"),
        Builtin::TemporalPlainDateToJSON => Some("toJSON"),
        Builtin::TemporalPlainDateCalendarIdGetter => Some("get calendarId"),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("get dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("get dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some("get daysInMonth"),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some("get daysInWeek"),
        Builtin::TemporalPlainDateDaysInYearGetter => Some("get daysInYear"),
        Builtin::TemporalPlainDateInLeapYearGetter => Some("get inLeapYear"),
        Builtin::TemporalPlainDateMonthsInYearGetter => Some("get monthsInYear"),
        Builtin::TemporalPlainDateEquals => Some("equals"),
        Builtin::TemporalPlainDateAdd => Some("add"),
        Builtin::TemporalPlainDateSubtract => Some("subtract"),
        Builtin::TemporalPlainDateUntil => Some("until"),
        Builtin::TemporalPlainDateSince => Some("since"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TemporalDurationFrom => Some(1.0),
        Builtin::TemporalDurationCompare => Some(2.0),
        Builtin::TemporalPlainDateFrom => Some(1.0),
        Builtin::TemporalPlainDateToString | Builtin::TemporalPlainDateToJSON => Some(0.0),
        _ => None,
    }
}
