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
        Builtin::TemporalDurationSubtract => Some("Temporal.Duration.prototype.subtract"),
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
        Builtin::TemporalPlainDateCompare => Some("Temporal.PlainDate.compare"),
        Builtin::TemporalPlainDateValueOf => Some("Temporal.PlainDate.prototype.valueOf"),
        Builtin::TemporalPlainDateToLocaleString => {
            Some("Temporal.PlainDate.prototype.toLocaleString")
        }
        Builtin::TemporalPlainDateAdd => Some("Temporal.PlainDate.prototype.add"),
        Builtin::TemporalPlainDateSubtract => Some("Temporal.PlainDate.prototype.subtract"),
        Builtin::TemporalPlainDateUntil => Some("Temporal.PlainDate.prototype.until"),
        Builtin::TemporalPlainDateSince => Some("Temporal.PlainDate.prototype.since"),
        Builtin::TemporalPlainTime => Some("Temporal.PlainTime"),
        Builtin::TemporalPlainTimeFrom => Some("Temporal.PlainTime.from"),
        Builtin::TemporalPlainTimeCompare => Some("Temporal.PlainTime.compare"),
        Builtin::TemporalPlainTimeHourGetter => Some("Temporal.PlainTime.prototype.hour"),
        Builtin::TemporalPlainTimeMinuteGetter => Some("Temporal.PlainTime.prototype.minute"),
        Builtin::TemporalPlainTimeSecondGetter => Some("Temporal.PlainTime.prototype.second"),
        Builtin::TemporalPlainTimeMillisecondGetter => {
            Some("Temporal.PlainTime.prototype.millisecond")
        }
        Builtin::TemporalPlainTimeMicrosecondGetter => {
            Some("Temporal.PlainTime.prototype.microsecond")
        }
        Builtin::TemporalPlainTimeNanosecondGetter => {
            Some("Temporal.PlainTime.prototype.nanosecond")
        }
        Builtin::TemporalPlainTimeToString => Some("Temporal.PlainTime.prototype.toString"),
        Builtin::TemporalPlainTimeToJSON => Some("Temporal.PlainTime.prototype.toJSON"),
        Builtin::TemporalPlainTimeValueOf => Some("Temporal.PlainTime.prototype.valueOf"),
        Builtin::TemporalPlainTimeEquals => Some("Temporal.PlainTime.prototype.equals"),
        Builtin::TemporalPlainTimeToLocaleString => {
            Some("Temporal.PlainTime.prototype.toLocaleString")
        }
        Builtin::TemporalPlainTimeAdd => Some("Temporal.PlainTime.prototype.add"),
        Builtin::TemporalPlainTimeSubtract => Some("Temporal.PlainTime.prototype.subtract"),
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
        Builtin::TemporalDurationSubtract => Some("subtract"),
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
        Builtin::TemporalPlainDateCompare => Some("compare"),
        Builtin::TemporalPlainDateValueOf => Some("valueOf"),
        Builtin::TemporalPlainDateToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainDateAdd => Some("add"),
        Builtin::TemporalPlainDateSubtract => Some("subtract"),
        Builtin::TemporalPlainDateUntil => Some("until"),
        Builtin::TemporalPlainDateSince => Some("since"),
        Builtin::TemporalPlainTime => Some("PlainTime"),
        Builtin::TemporalPlainTimeFrom => Some("from"),
        Builtin::TemporalPlainTimeCompare => Some("compare"),
        Builtin::TemporalPlainTimeHourGetter => Some("get hour"),
        Builtin::TemporalPlainTimeMinuteGetter => Some("get minute"),
        Builtin::TemporalPlainTimeSecondGetter => Some("get second"),
        Builtin::TemporalPlainTimeMillisecondGetter => Some("get millisecond"),
        Builtin::TemporalPlainTimeMicrosecondGetter => Some("get microsecond"),
        Builtin::TemporalPlainTimeNanosecondGetter => Some("get nanosecond"),
        Builtin::TemporalPlainTimeToString => Some("toString"),
        Builtin::TemporalPlainTimeToJSON => Some("toJSON"),
        Builtin::TemporalPlainTimeValueOf => Some("valueOf"),
        Builtin::TemporalPlainTimeEquals => Some("equals"),
        Builtin::TemporalPlainTimeToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainTimeAdd => Some("add"),
        Builtin::TemporalPlainTimeSubtract => Some("subtract"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TemporalDurationFrom => Some(1.0),
        Builtin::TemporalDurationCompare => Some(2.0),
        Builtin::TemporalPlainDateFrom => Some(1.0),
        Builtin::TemporalPlainDateCompare => Some(2.0),
        Builtin::TemporalPlainTimeFrom => Some(1.0),
        Builtin::TemporalPlainTimeCompare => Some(2.0),
        Builtin::TemporalPlainTimeToString | Builtin::TemporalPlainTimeToJSON => Some(0.0),
        Builtin::TemporalPlainDateToString | Builtin::TemporalPlainDateToJSON => Some(0.0),
        _ => None,
    }
}
