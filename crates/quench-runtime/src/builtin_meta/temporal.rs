use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalPlainMonthDay => Some("Temporal.PlainMonthDay"),
        Builtin::TemporalPlainMonthDayFrom => Some("Temporal.PlainMonthDay.from"),
        Builtin::TemporalPlainMonthDayCompare => Some("Temporal.PlainMonthDay.compare"),
        Builtin::TemporalPlainMonthDayCalendarIdGetter => Some("get calendarId"),
        Builtin::TemporalPlainMonthDayDayGetter => Some("get day"),
        Builtin::TemporalPlainMonthDayMonthCodeGetter => Some("get monthCode"),
        Builtin::TemporalPlainMonthDayEquals => Some("Temporal.PlainMonthDay.prototype.equals"),
        Builtin::TemporalPlainMonthDayToString => Some("Temporal.PlainMonthDay.prototype.toString"),
        Builtin::TemporalPlainMonthDayToJSON => Some("Temporal.PlainMonthDay.prototype.toJSON"),
        Builtin::TemporalPlainMonthDayToLocaleString => {
            Some("Temporal.PlainMonthDay.prototype.toLocaleString")
        }
        Builtin::TemporalPlainMonthDayToPlainDate => {
            Some("Temporal.PlainMonthDay.prototype.toPlainDate")
        }
        Builtin::TemporalPlainMonthDayValueOf => Some("Temporal.PlainMonthDay.prototype.valueOf"),
        Builtin::TemporalPlainMonthDayWith => Some("Temporal.PlainMonthDay.prototype.with"),
        Builtin::TemporalPlainYearMonth => Some("Temporal.PlainYearMonth"),
        Builtin::TemporalPlainYearMonthFrom => Some("Temporal.PlainYearMonth.from"),
        Builtin::TemporalPlainYearMonthCompare => Some("Temporal.PlainYearMonth.compare"),
        Builtin::TemporalPlainYearMonthCalendarIdGetter => Some("get calendarId"),
        Builtin::TemporalPlainYearMonthYearGetter => Some("get year"),
        Builtin::TemporalPlainYearMonthMonthGetter => Some("get month"),
        Builtin::TemporalPlainYearMonthMonthCodeGetter => Some("get monthCode"),
        Builtin::TemporalPlainYearMonthEquals => Some("Temporal.PlainYearMonth.prototype.equals"),
        Builtin::TemporalPlainYearMonthToString => {
            Some("Temporal.PlainYearMonth.prototype.toString")
        }
        Builtin::TemporalPlainYearMonthToJSON => Some("Temporal.PlainYearMonth.prototype.toJSON"),
        Builtin::TemporalPlainYearMonthToLocaleString => {
            Some("Temporal.PlainYearMonth.prototype.toLocaleString")
        }
        Builtin::TemporalPlainYearMonthToPlainDate => {
            Some("Temporal.PlainYearMonth.prototype.toPlainDate")
        }
        Builtin::TemporalPlainYearMonthValueOf => Some("Temporal.PlainYearMonth.prototype.valueOf"),
        Builtin::TemporalPlainYearMonthWith => Some("Temporal.PlainYearMonth.prototype.with"),
        Builtin::TemporalPlainYearMonthAdd => Some("Temporal.PlainYearMonth.prototype.add"),
        Builtin::TemporalPlainYearMonthSubtract => {
            Some("Temporal.PlainYearMonth.prototype.subtract")
        }
        Builtin::TemporalPlainYearMonthUntil => Some("Temporal.PlainYearMonth.prototype.until"),
        Builtin::TemporalPlainYearMonthSince => Some("Temporal.PlainYearMonth.prototype.since"),
        Builtin::TemporalPlainYearMonthDaysInMonthGetter => Some("get daysInMonth"),
        Builtin::TemporalPlainYearMonthDaysInYearGetter => Some("get daysInYear"),
        Builtin::TemporalPlainYearMonthInLeapYearGetter => Some("get inLeapYear"),
        Builtin::TemporalPlainYearMonthMonthsInYearGetter => Some("get monthsInYear"),
        Builtin::TemporalPlainYearMonthEraGetter => Some("get era"),
        Builtin::TemporalPlainYearMonthEraYearGetter => Some("get eraYear"),
        Builtin::TemporalZonedDateTimeFrom => Some("Temporal.ZonedDateTime.from"),
        Builtin::TemporalZonedDateTimeCompare => Some("Temporal.ZonedDateTime.compare"),
        Builtin::TemporalZonedDateTimeToString => Some("Temporal.ZonedDateTime.prototype.toString"),
        Builtin::TemporalZonedDateTimeToJSON => Some("Temporal.ZonedDateTime.prototype.toJSON"),
        Builtin::TemporalZonedDateTimeToLocaleString => {
            Some("Temporal.ZonedDateTime.prototype.toLocaleString")
        }
        Builtin::TemporalZonedDateTimeToInstant => {
            Some("Temporal.ZonedDateTime.prototype.toInstant")
        }
        Builtin::TemporalZonedDateTimeToPlainDateTime => {
            Some("Temporal.ZonedDateTime.prototype.toPlainDateTime")
        }
        Builtin::TemporalZonedDateTimeToPlainDate => {
            Some("Temporal.ZonedDateTime.prototype.toPlainDate")
        }
        Builtin::TemporalZonedDateTimeToPlainTime => {
            Some("Temporal.ZonedDateTime.prototype.toPlainTime")
        }
        Builtin::TemporalZonedDateTimeEquals => Some("Temporal.ZonedDateTime.prototype.equals"),
        Builtin::TemporalZonedDateTimeEpochMillisecondsGetter => Some("get epochMilliseconds"),
        Builtin::TemporalZonedDateTimeTimeZoneIdGetter => Some("get timeZoneId"),
        Builtin::TemporalZonedDateTimeOffsetGetter => Some("get offset"),
        Builtin::TemporalZonedDateTimeOffsetNanosecondsGetter => Some("get offsetNanoseconds"),
        Builtin::TemporalZonedDateTimeHoursInDayGetter => Some("get hoursInDay"),
        Builtin::TemporalNowInstant => Some("Temporal.Now.instant"),
        Builtin::TemporalNowPlainDateISO => Some("Temporal.Now.plainDateISO"),
        Builtin::TemporalNowPlainDateTimeISO => Some("Temporal.Now.plainDateTimeISO"),
        Builtin::TemporalNowPlainTimeISO => Some("Temporal.Now.plainTimeISO"),
        Builtin::TemporalNowTimeZoneId => Some("Temporal.Now.timeZoneId"),
        Builtin::TemporalNowZonedDateTimeISO => Some("Temporal.Now.zonedDateTimeISO"),
        Builtin::TemporalInstant => Some("Temporal.Instant"),
        Builtin::TemporalInstantFrom => Some("Temporal.Instant.from"),
        Builtin::TemporalInstantEpochNanosecondsGetter => Some("get epochNanoseconds"),
        Builtin::TemporalInstantEpochMillisecondsGetter => Some("get epochMilliseconds"),
        Builtin::TemporalInstantToString => Some("Temporal.Instant.prototype.toString"),
        Builtin::TemporalInstantToJSON => Some("Temporal.Instant.prototype.toJSON"),
        Builtin::TemporalInstantToLocaleString => Some("Temporal.Instant.prototype.toLocaleString"),
        Builtin::TemporalInstantToZonedDateTimeISO => {
            Some("Temporal.Instant.prototype.toZonedDateTimeISO")
        }
        Builtin::TemporalInstantEquals => Some("Temporal.Instant.prototype.equals"),
        Builtin::TemporalInstantAdd => Some("Temporal.Instant.prototype.add"),
        Builtin::TemporalInstantSubtract => Some("Temporal.Instant.prototype.subtract"),
        Builtin::TemporalInstantUntil => Some("Temporal.Instant.prototype.until"),
        Builtin::TemporalInstantSince => Some("Temporal.Instant.prototype.since"),
        Builtin::TemporalInstantRound => Some("Temporal.Instant.prototype.round"),
        Builtin::TemporalPlainDateTime => Some("Temporal.PlainDateTime"),
        Builtin::TemporalPlainDateTimeFrom => Some("Temporal.PlainDateTime.from"),
        Builtin::TemporalPlainDateTimeCompare => Some("Temporal.PlainDateTime.compare"),
        Builtin::TemporalPlainDateTimeAdd => Some("Temporal.PlainDateTime.prototype.add"),
        Builtin::TemporalPlainDateTimeSubtract => Some("Temporal.PlainDateTime.prototype.subtract"),
        Builtin::TemporalPlainDateTimeWith => Some("Temporal.PlainDateTime.prototype.with"),
        Builtin::TemporalPlainDateTimeRound => Some("Temporal.PlainDateTime.prototype.round"),
        Builtin::TemporalPlainDateTimeEquals => Some("Temporal.PlainDateTime.prototype.equals"),
        Builtin::TemporalPlainDateTimeToString => Some("Temporal.PlainDateTime.prototype.toString"),
        Builtin::TemporalPlainDateTimeToJSON => Some("Temporal.PlainDateTime.prototype.toJSON"),
        Builtin::TemporalPlainDateTimeToLocaleString => {
            Some("Temporal.PlainDateTime.prototype.toLocaleString")
        }
        Builtin::TemporalPlainDateTimeValueOf => Some("Temporal.PlainDateTime.prototype.valueOf"),
        Builtin::TemporalPlainDateTimeUntil => Some("Temporal.PlainDateTime.prototype.until"),
        Builtin::TemporalPlainDateTimeSince => Some("Temporal.PlainDateTime.prototype.since"),
        Builtin::TemporalPlainDateTimeToPlainDate => {
            Some("Temporal.PlainDateTime.prototype.toPlainDate")
        }
        Builtin::TemporalPlainDateTimeToPlainTime => {
            Some("Temporal.PlainDateTime.prototype.toPlainTime")
        }
        Builtin::TemporalPlainDateTimeToZonedDateTime => {
            Some("Temporal.PlainDateTime.prototype.toZonedDateTime")
        }
        Builtin::TemporalPlainDateTimeWithCalendar => {
            Some("Temporal.PlainDateTime.prototype.withCalendar")
        }
        Builtin::TemporalPlainDateTimeWithPlainTime => {
            Some("Temporal.PlainDateTime.prototype.withPlainTime")
        }
        Builtin::TemporalPlainDateTimeDayOfWeekGetter => Some("get dayOfWeek"),
        Builtin::TemporalPlainDateTimeDayOfYearGetter => Some("get dayOfYear"),
        Builtin::TemporalPlainDateTimeDaysInMonthGetter => Some("get daysInMonth"),
        Builtin::TemporalPlainDateTimeDaysInWeekGetter => Some("get daysInWeek"),
        Builtin::TemporalPlainDateTimeDaysInYearGetter => Some("get daysInYear"),
        Builtin::TemporalPlainDateTimeMonthsInYearGetter => Some("get monthsInYear"),
        Builtin::TemporalPlainDateTimeInLeapYearGetter => Some("get inLeapYear"),
        Builtin::TemporalPlainDateTimeEraGetter => Some("get era"),
        Builtin::TemporalPlainDateTimeEraYearGetter => Some("get eraYear"),
        Builtin::TemporalPlainDateTimeWeekOfYearGetter => Some("get weekOfYear"),
        Builtin::TemporalPlainDateTimeYearOfWeekGetter => Some("get yearOfWeek"),
        Builtin::TemporalPlainTime => Some("Temporal.PlainTime"),
        Builtin::TemporalPlainTimeFrom => Some("Temporal.PlainTime.from"),
        Builtin::TemporalPlainTimeCompare => Some("Temporal.PlainTime.compare"),
        Builtin::TemporalPlainTimeAdd => Some("Temporal.PlainTime.prototype.add"),
        Builtin::TemporalPlainTimeSubtract => Some("Temporal.PlainTime.prototype.subtract"),
        Builtin::TemporalPlainTimeWith => Some("Temporal.PlainTime.prototype.with"),
        Builtin::TemporalPlainTimeRound => Some("Temporal.PlainTime.prototype.round"),
        Builtin::TemporalPlainTimeEquals => Some("Temporal.PlainTime.prototype.equals"),
        Builtin::TemporalPlainTimeUntil => Some("Temporal.PlainTime.prototype.until"),
        Builtin::TemporalPlainTimeSince => Some("Temporal.PlainTime.prototype.since"),
        Builtin::TemporalPlainTimeToString => Some("Temporal.PlainTime.prototype.toString"),
        Builtin::TemporalPlainTimeToJSON => Some("Temporal.PlainTime.prototype.toJSON"),
        Builtin::TemporalPlainTimeToLocaleString => {
            Some("Temporal.PlainTime.prototype.toLocaleString")
        }
        Builtin::TemporalPlainTimeValueOf => Some("Temporal.PlainTime.prototype.valueOf"),
        Builtin::TemporalPlainDateTimeCalendarIdGetter
        | Builtin::TemporalPlainDateTimeYearGetter
        | Builtin::TemporalPlainDateTimeMonthGetter
        | Builtin::TemporalPlainDateTimeMonthCodeGetter
        | Builtin::TemporalPlainDateTimeDayGetter
        | Builtin::TemporalPlainDateTimeHourGetter
        | Builtin::TemporalPlainDateTimeMinuteGetter
        | Builtin::TemporalPlainDateTimeSecondGetter
        | Builtin::TemporalPlainDateTimeMillisecondGetter
        | Builtin::TemporalPlainDateTimeMicrosecondGetter
        | Builtin::TemporalPlainDateTimeNanosecondGetter
        | Builtin::TemporalPlainTimeHourGetter
        | Builtin::TemporalPlainTimeMinuteGetter
        | Builtin::TemporalPlainTimeSecondGetter
        | Builtin::TemporalPlainTimeMillisecondGetter
        | Builtin::TemporalPlainTimeMicrosecondGetter
        | Builtin::TemporalPlainTimeNanosecondGetter => Some("get Temporal field"),
        Builtin::TemporalDuration => Some("Temporal.Duration"),
        Builtin::TemporalDurationFrom => Some("Temporal.Duration.from"),
        Builtin::TemporalDurationCompare => Some("Temporal.Duration.compare"),
        Builtin::TemporalDurationAdd => Some("Temporal.Duration.prototype.add"),
        Builtin::TemporalDurationSubtract => Some("Temporal.Duration.prototype.subtract"),
        Builtin::TemporalDurationWith => Some("Temporal.Duration.prototype.with"),
        Builtin::TemporalDurationAbs => Some("Temporal.Duration.prototype.abs"),
        Builtin::TemporalDurationNegated => Some("Temporal.Duration.prototype.negated"),
        Builtin::TemporalDurationRound => Some("Temporal.Duration.prototype.round"),
        Builtin::TemporalDurationTotal => Some("Temporal.Duration.prototype.total"),
        Builtin::TemporalDurationToLocaleString => {
            Some("Temporal.Duration.prototype.toLocaleString")
        }
        Builtin::TemporalDurationToString => Some("Temporal.Duration.prototype.toString"),
        Builtin::TemporalDurationToJSON => Some("Temporal.Duration.prototype.toJSON"),
        Builtin::TemporalDurationSignGetter => Some("get sign"),
        Builtin::TemporalDurationBlankGetter => Some("get blank"),
        Builtin::TemporalDurationValueOf => Some("Temporal.Duration.prototype.valueOf"),
        Builtin::TemporalPlainDate => Some("Temporal.PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("Temporal.PlainDate.from"),
        Builtin::TemporalPlainDateCompare => Some("Temporal.PlainDate.compare"),
        Builtin::TemporalPlainDateWith => Some("Temporal.PlainDate.prototype.with"),
        Builtin::TemporalPlainDateAdd => Some("Temporal.PlainDate.prototype.add"),
        Builtin::TemporalPlainDateSubtract => Some("Temporal.PlainDate.prototype.subtract"),
        Builtin::TemporalPlainDateEquals => Some("Temporal.PlainDate.prototype.equals"),
        Builtin::TemporalPlainDateUntil => Some("Temporal.PlainDate.prototype.until"),
        Builtin::TemporalPlainDateSince => Some("Temporal.PlainDate.prototype.since"),
        Builtin::TemporalPlainDateToLocaleString => {
            Some("Temporal.PlainDate.prototype.toLocaleString")
        }
        Builtin::TemporalPlainDateToPlainDateTime => {
            Some("Temporal.PlainDate.prototype.toPlainDateTime")
        }
        Builtin::TemporalPlainDateToPlainMonthDay => {
            Some("Temporal.PlainDate.prototype.toPlainMonthDay")
        }
        Builtin::TemporalPlainDateToPlainYearMonth => {
            Some("Temporal.PlainDate.prototype.toPlainYearMonth")
        }
        Builtin::TemporalPlainDateToZonedDateTime => {
            Some("Temporal.PlainDate.prototype.toZonedDateTime")
        }
        Builtin::TemporalPlainDateWithCalendar => Some("Temporal.PlainDate.prototype.withCalendar"),
        Builtin::TemporalPlainDateToString => Some("Temporal.PlainDate.prototype.toString"),
        Builtin::TemporalPlainDateToJSON => Some("Temporal.PlainDate.prototype.toJSON"),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("get dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("get dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some("get daysInMonth"),
        Builtin::TemporalPlainDateDaysInYearGetter => Some("get daysInYear"),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some("get daysInWeek"),
        Builtin::TemporalPlainDateMonthsInYearGetter => Some("get monthsInYear"),
        Builtin::TemporalPlainDateInLeapYearGetter => Some("get inLeapYear"),
        Builtin::TemporalPlainDateEraGetter => Some("get era"),
        Builtin::TemporalPlainDateEraYearGetter => Some("get eraYear"),
        Builtin::TemporalPlainDateCalendarIdGetter => Some("get calendarId"),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some("get weekOfYear"),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some("get yearOfWeek"),
        Builtin::TemporalPlainDateDayGetter => Some("get day"),
        Builtin::TemporalPlainDateYearGetter => Some("get year"),
        Builtin::TemporalPlainDateMonthCodeGetter => Some("get monthCode"),
        Builtin::TemporalPlainDateMonthGetter => Some("get month"),
        Builtin::TemporalPlainDateValueOf => Some("Temporal.PlainDate.prototype.valueOf"),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalPlainMonthDay => Some("PlainMonthDay"),
        Builtin::TemporalPlainMonthDayFrom => Some("from"),
        Builtin::TemporalPlainMonthDayCompare => Some("compare"),
        Builtin::TemporalPlainMonthDayCalendarIdGetter => Some("calendarId"),
        Builtin::TemporalPlainMonthDayDayGetter => Some("day"),
        Builtin::TemporalPlainMonthDayMonthCodeGetter => Some("monthCode"),
        Builtin::TemporalPlainMonthDayEquals => Some("equals"),
        Builtin::TemporalPlainMonthDayToString => Some("toString"),
        Builtin::TemporalPlainMonthDayToJSON => Some("toJSON"),
        Builtin::TemporalPlainMonthDayToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainMonthDayToPlainDate => Some("toPlainDate"),
        Builtin::TemporalPlainMonthDayValueOf => Some("valueOf"),
        Builtin::TemporalPlainMonthDayWith => Some("with"),
        Builtin::TemporalPlainYearMonth => Some("PlainYearMonth"),
        Builtin::TemporalPlainYearMonthFrom => Some("from"),
        Builtin::TemporalPlainYearMonthCompare => Some("compare"),
        Builtin::TemporalPlainYearMonthCalendarIdGetter => Some("calendarId"),
        Builtin::TemporalPlainYearMonthYearGetter => Some("year"),
        Builtin::TemporalPlainYearMonthMonthGetter => Some("month"),
        Builtin::TemporalPlainYearMonthMonthCodeGetter => Some("monthCode"),
        Builtin::TemporalPlainYearMonthEquals => Some("equals"),
        Builtin::TemporalPlainYearMonthToString => Some("toString"),
        Builtin::TemporalPlainYearMonthToJSON => Some("toJSON"),
        Builtin::TemporalPlainYearMonthToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainYearMonthToPlainDate => Some("toPlainDate"),
        Builtin::TemporalPlainYearMonthValueOf => Some("valueOf"),
        Builtin::TemporalPlainYearMonthWith => Some("with"),
        Builtin::TemporalPlainYearMonthAdd => Some("add"),
        Builtin::TemporalPlainYearMonthSubtract => Some("subtract"),
        Builtin::TemporalPlainYearMonthUntil => Some("until"),
        Builtin::TemporalPlainYearMonthSince => Some("since"),
        Builtin::TemporalPlainYearMonthDaysInMonthGetter => Some("daysInMonth"),
        Builtin::TemporalPlainYearMonthDaysInYearGetter => Some("daysInYear"),
        Builtin::TemporalPlainYearMonthInLeapYearGetter => Some("inLeapYear"),
        Builtin::TemporalPlainYearMonthMonthsInYearGetter => Some("monthsInYear"),
        Builtin::TemporalPlainYearMonthEraGetter => Some("era"),
        Builtin::TemporalPlainYearMonthEraYearGetter => Some("eraYear"),
        Builtin::TemporalZonedDateTimeFrom => Some("from"),
        Builtin::TemporalZonedDateTimeCompare => Some("compare"),
        Builtin::TemporalZonedDateTimeToString => Some("toString"),
        Builtin::TemporalZonedDateTimeToJSON => Some("toJSON"),
        Builtin::TemporalZonedDateTimeToLocaleString => Some("toLocaleString"),
        Builtin::TemporalZonedDateTimeToInstant => Some("toInstant"),
        Builtin::TemporalZonedDateTimeToPlainDateTime => Some("toPlainDateTime"),
        Builtin::TemporalZonedDateTimeToPlainDate => Some("toPlainDate"),
        Builtin::TemporalZonedDateTimeToPlainTime => Some("toPlainTime"),
        Builtin::TemporalZonedDateTimeEquals => Some("equals"),
        Builtin::TemporalZonedDateTimeEpochMillisecondsGetter => Some("epochMilliseconds"),
        Builtin::TemporalZonedDateTimeTimeZoneIdGetter => Some("timeZoneId"),
        Builtin::TemporalZonedDateTimeOffsetGetter => Some("offset"),
        Builtin::TemporalZonedDateTimeOffsetNanosecondsGetter => Some("offsetNanoseconds"),
        Builtin::TemporalZonedDateTimeHoursInDayGetter => Some("hoursInDay"),
        Builtin::TemporalNowInstant => Some("instant"),
        Builtin::TemporalNowPlainDateISO => Some("plainDateISO"),
        Builtin::TemporalNowPlainDateTimeISO => Some("plainDateTimeISO"),
        Builtin::TemporalNowPlainTimeISO => Some("plainTimeISO"),
        Builtin::TemporalNowTimeZoneId => Some("timeZoneId"),
        Builtin::TemporalNowZonedDateTimeISO => Some("zonedDateTimeISO"),
        Builtin::TemporalInstant => Some("Instant"),
        Builtin::TemporalInstantFrom => Some("from"),
        Builtin::TemporalInstantEpochNanosecondsGetter => Some("epochNanoseconds"),
        Builtin::TemporalInstantEpochMillisecondsGetter => Some("epochMilliseconds"),
        Builtin::TemporalInstantToString => Some("toString"),
        Builtin::TemporalInstantToJSON => Some("toJSON"),
        Builtin::TemporalInstantToLocaleString => Some("toLocaleString"),
        Builtin::TemporalInstantToZonedDateTimeISO => Some("toZonedDateTimeISO"),
        Builtin::TemporalInstantEquals => Some("equals"),
        Builtin::TemporalInstantAdd => Some("add"),
        Builtin::TemporalInstantSubtract => Some("subtract"),
        Builtin::TemporalInstantUntil => Some("until"),
        Builtin::TemporalInstantSince => Some("since"),
        Builtin::TemporalInstantRound => Some("round"),
        Builtin::TemporalPlainDateTime => Some("PlainDateTime"),
        Builtin::TemporalPlainDateTimeFrom => Some("from"),
        Builtin::TemporalPlainDateTimeCompare => Some("compare"),
        Builtin::TemporalPlainDateTimeAdd => Some("add"),
        Builtin::TemporalPlainDateTimeSubtract => Some("subtract"),
        Builtin::TemporalPlainDateTimeWith => Some("with"),
        Builtin::TemporalPlainDateTimeRound => Some("round"),
        Builtin::TemporalPlainDateTimeEquals => Some("equals"),
        Builtin::TemporalPlainDateTimeToString => Some("toString"),
        Builtin::TemporalPlainDateTimeToJSON => Some("toJSON"),
        Builtin::TemporalPlainDateTimeToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainDateTimeValueOf => Some("valueOf"),
        Builtin::TemporalPlainDateTimeUntil => Some("until"),
        Builtin::TemporalPlainDateTimeSince => Some("since"),
        Builtin::TemporalPlainDateTimeToPlainDate => Some("toPlainDate"),
        Builtin::TemporalPlainDateTimeToPlainTime => Some("toPlainTime"),
        Builtin::TemporalPlainDateTimeToZonedDateTime => Some("toZonedDateTime"),
        Builtin::TemporalPlainDateTimeWithCalendar => Some("withCalendar"),
        Builtin::TemporalPlainDateTimeWithPlainTime => Some("withPlainTime"),
        Builtin::TemporalPlainDateTimeDayOfWeekGetter => Some("dayOfWeek"),
        Builtin::TemporalPlainDateTimeDayOfYearGetter => Some("dayOfYear"),
        Builtin::TemporalPlainDateTimeDaysInMonthGetter => Some("daysInMonth"),
        Builtin::TemporalPlainDateTimeDaysInWeekGetter => Some("daysInWeek"),
        Builtin::TemporalPlainDateTimeDaysInYearGetter => Some("daysInYear"),
        Builtin::TemporalPlainDateTimeMonthsInYearGetter => Some("monthsInYear"),
        Builtin::TemporalPlainDateTimeInLeapYearGetter => Some("inLeapYear"),
        Builtin::TemporalPlainDateTimeEraGetter => Some("era"),
        Builtin::TemporalPlainDateTimeEraYearGetter => Some("eraYear"),
        Builtin::TemporalPlainDateTimeWeekOfYearGetter => Some("weekOfYear"),
        Builtin::TemporalPlainDateTimeYearOfWeekGetter => Some("yearOfWeek"),
        Builtin::TemporalPlainTime => Some("PlainTime"),
        Builtin::TemporalPlainTimeFrom => Some("from"),
        Builtin::TemporalPlainTimeCompare => Some("compare"),
        Builtin::TemporalPlainTimeAdd => Some("add"),
        Builtin::TemporalPlainTimeSubtract => Some("subtract"),
        Builtin::TemporalPlainTimeWith => Some("with"),
        Builtin::TemporalPlainTimeRound => Some("round"),
        Builtin::TemporalPlainTimeEquals => Some("equals"),
        Builtin::TemporalPlainTimeUntil => Some("until"),
        Builtin::TemporalPlainTimeSince => Some("since"),
        Builtin::TemporalPlainTimeToString => Some("toString"),
        Builtin::TemporalPlainTimeToJSON => Some("toJSON"),
        Builtin::TemporalPlainTimeToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainTimeValueOf => Some("valueOf"),
        Builtin::TemporalDuration => Some("Duration"),
        Builtin::TemporalDurationFrom => Some("from"),
        Builtin::TemporalDurationCompare => Some("compare"),
        Builtin::TemporalDurationAdd => Some("add"),
        Builtin::TemporalDurationSubtract => Some("subtract"),
        Builtin::TemporalDurationWith => Some("with"),
        Builtin::TemporalDurationAbs => Some("abs"),
        Builtin::TemporalDurationNegated => Some("negated"),
        Builtin::TemporalDurationRound => Some("round"),
        Builtin::TemporalDurationTotal => Some("total"),
        Builtin::TemporalDurationToLocaleString => Some("toLocaleString"),
        Builtin::TemporalDurationToString => Some("toString"),
        Builtin::TemporalDurationToJSON => Some("toJSON"),
        Builtin::TemporalDurationSignGetter => Some("sign"),
        Builtin::TemporalDurationBlankGetter => Some("blank"),
        Builtin::TemporalDurationValueOf => Some("valueOf"),
        Builtin::TemporalPlainDate => Some("PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("from"),
        Builtin::TemporalPlainDateCompare => Some("compare"),
        Builtin::TemporalPlainDateWith => Some("with"),
        Builtin::TemporalPlainDateAdd => Some("add"),
        Builtin::TemporalPlainDateSubtract => Some("subtract"),
        Builtin::TemporalPlainDateEquals => Some("equals"),
        Builtin::TemporalPlainDateUntil => Some("until"),
        Builtin::TemporalPlainDateSince => Some("since"),
        Builtin::TemporalPlainDateToLocaleString => Some("toLocaleString"),
        Builtin::TemporalPlainDateToPlainDateTime => Some("toPlainDateTime"),
        Builtin::TemporalPlainDateToPlainMonthDay => Some("toPlainMonthDay"),
        Builtin::TemporalPlainDateToPlainYearMonth => Some("toPlainYearMonth"),
        Builtin::TemporalPlainDateToZonedDateTime => Some("toZonedDateTime"),
        Builtin::TemporalPlainDateWithCalendar => Some("withCalendar"),
        Builtin::TemporalPlainDateToString => Some("toString"),
        Builtin::TemporalPlainDateToJSON => Some("toJSON"),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some("dayOfWeek"),
        Builtin::TemporalPlainDateDayOfYearGetter => Some("dayOfYear"),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some("daysInMonth"),
        Builtin::TemporalPlainDateDaysInYearGetter => Some("daysInYear"),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some("daysInWeek"),
        Builtin::TemporalPlainDateMonthsInYearGetter => Some("monthsInYear"),
        Builtin::TemporalPlainDateInLeapYearGetter => Some("inLeapYear"),
        Builtin::TemporalPlainDateEraGetter => Some("era"),
        Builtin::TemporalPlainDateEraYearGetter => Some("eraYear"),
        Builtin::TemporalPlainDateCalendarIdGetter => Some("calendarId"),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some("weekOfYear"),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some("yearOfWeek"),
        Builtin::TemporalPlainDateDayGetter => Some("day"),
        Builtin::TemporalPlainDateYearGetter => Some("year"),
        Builtin::TemporalPlainDateMonthCodeGetter => Some("monthCode"),
        Builtin::TemporalPlainDateMonthGetter => Some("month"),
        Builtin::TemporalPlainDateValueOf => Some("valueOf"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TemporalPlainMonthDay => Some(2.0),
        Builtin::TemporalPlainMonthDayFrom => Some(1.0),
        Builtin::TemporalPlainMonthDayCompare => Some(2.0),
        Builtin::TemporalPlainMonthDayCalendarIdGetter
        | Builtin::TemporalPlainMonthDayDayGetter
        | Builtin::TemporalPlainMonthDayMonthCodeGetter
        | Builtin::TemporalPlainMonthDayToString
        | Builtin::TemporalPlainMonthDayToJSON
        | Builtin::TemporalPlainMonthDayToLocaleString
        | Builtin::TemporalPlainMonthDayValueOf => Some(0.0),
        Builtin::TemporalPlainMonthDayEquals | Builtin::TemporalPlainMonthDayWith => Some(1.0),
        Builtin::TemporalPlainMonthDayToPlainDate => Some(1.0),
        Builtin::TemporalPlainYearMonth => Some(2.0),
        Builtin::TemporalPlainYearMonthFrom => Some(1.0),
        Builtin::TemporalPlainYearMonthCompare => Some(2.0),
        Builtin::TemporalPlainYearMonthCalendarIdGetter
        | Builtin::TemporalPlainYearMonthYearGetter
        | Builtin::TemporalPlainYearMonthMonthGetter
        | Builtin::TemporalPlainYearMonthMonthCodeGetter
        | Builtin::TemporalPlainYearMonthToString
        | Builtin::TemporalPlainYearMonthToJSON
        | Builtin::TemporalPlainYearMonthToLocaleString
        | Builtin::TemporalPlainYearMonthValueOf
        | Builtin::TemporalPlainYearMonthDaysInMonthGetter
        | Builtin::TemporalPlainYearMonthDaysInYearGetter
        | Builtin::TemporalPlainYearMonthInLeapYearGetter
        | Builtin::TemporalPlainYearMonthMonthsInYearGetter
        | Builtin::TemporalPlainYearMonthEraGetter
        | Builtin::TemporalPlainYearMonthEraYearGetter => Some(0.0),
        Builtin::TemporalPlainYearMonthEquals
        | Builtin::TemporalPlainYearMonthWith
        | Builtin::TemporalPlainYearMonthAdd
        | Builtin::TemporalPlainYearMonthSubtract
        | Builtin::TemporalPlainYearMonthUntil
        | Builtin::TemporalPlainYearMonthSince
        | Builtin::TemporalPlainYearMonthToPlainDate => Some(1.0),
        Builtin::TemporalZonedDateTimeFrom => Some(1.0),
        Builtin::TemporalZonedDateTimeCompare => Some(2.0),
        Builtin::TemporalZonedDateTimeToString | Builtin::TemporalZonedDateTimeToLocaleString => {
            Some(0.0)
        }
        Builtin::TemporalZonedDateTimeToJSON
        | Builtin::TemporalZonedDateTimeToInstant
        | Builtin::TemporalZonedDateTimeToPlainDateTime
        | Builtin::TemporalZonedDateTimeToPlainDate
        | Builtin::TemporalZonedDateTimeToPlainTime => Some(0.0),
        Builtin::TemporalZonedDateTimeEquals => Some(1.0),
        Builtin::TemporalZonedDateTimeEpochMillisecondsGetter
        | Builtin::TemporalZonedDateTimeTimeZoneIdGetter
        | Builtin::TemporalZonedDateTimeOffsetGetter
        | Builtin::TemporalZonedDateTimeOffsetNanosecondsGetter
        | Builtin::TemporalZonedDateTimeHoursInDayGetter => Some(0.0),
        Builtin::TemporalNowInstant
        | Builtin::TemporalNowPlainDateISO
        | Builtin::TemporalNowPlainDateTimeISO
        | Builtin::TemporalNowPlainTimeISO
        | Builtin::TemporalNowTimeZoneId
        | Builtin::TemporalNowZonedDateTimeISO => Some(0.0),
        Builtin::TemporalInstant => Some(1.0),
        Builtin::TemporalInstantFrom => Some(1.0),
        Builtin::TemporalInstantEpochNanosecondsGetter
        | Builtin::TemporalInstantEpochMillisecondsGetter => Some(0.0),
        Builtin::TemporalInstantToString
        | Builtin::TemporalInstantToJSON
        | Builtin::TemporalInstantToLocaleString
        | Builtin::TemporalInstantToZonedDateTimeISO => Some(0.0),
        Builtin::TemporalInstantEquals
        | Builtin::TemporalInstantAdd
        | Builtin::TemporalInstantSubtract
        | Builtin::TemporalInstantRound => Some(1.0),
        Builtin::TemporalInstantUntil | Builtin::TemporalInstantSince => Some(2.0),
        Builtin::TemporalPlainDateTime => Some(3.0),
        Builtin::TemporalPlainDateTimeFrom => Some(1.0),
        Builtin::TemporalPlainDateTimeCompare => Some(2.0),
        Builtin::TemporalPlainDateTimeAdd => Some(1.0),
        Builtin::TemporalPlainDateTimeSubtract => Some(1.0),
        Builtin::TemporalPlainDateTimeWith => Some(1.0),
        Builtin::TemporalPlainDateTimeRound => Some(1.0),
        Builtin::TemporalPlainDateTimeEquals => Some(1.0),
        Builtin::TemporalPlainDateTimeToString
        | Builtin::TemporalPlainDateTimeToJSON
        | Builtin::TemporalPlainDateTimeToLocaleString
        | Builtin::TemporalPlainDateTimeValueOf => Some(0.0),
        Builtin::TemporalPlainDateTimeUntil => Some(2.0),
        Builtin::TemporalPlainDateTimeSince => Some(2.0),
        Builtin::TemporalPlainDateTimeToPlainDate | Builtin::TemporalPlainDateTimeToPlainTime => {
            Some(0.0)
        }
        Builtin::TemporalPlainDateTimeToZonedDateTime
        | Builtin::TemporalPlainDateTimeWithCalendar
        | Builtin::TemporalPlainDateTimeWithPlainTime => Some(1.0),
        Builtin::TemporalPlainDateTimeDayOfWeekGetter
        | Builtin::TemporalPlainDateTimeDayOfYearGetter
        | Builtin::TemporalPlainDateTimeDaysInMonthGetter
        | Builtin::TemporalPlainDateTimeDaysInWeekGetter
        | Builtin::TemporalPlainDateTimeDaysInYearGetter
        | Builtin::TemporalPlainDateTimeMonthsInYearGetter
        | Builtin::TemporalPlainDateTimeInLeapYearGetter
        | Builtin::TemporalPlainDateTimeEraGetter
        | Builtin::TemporalPlainDateTimeEraYearGetter
        | Builtin::TemporalPlainDateTimeWeekOfYearGetter
        | Builtin::TemporalPlainDateTimeYearOfWeekGetter => Some(0.0),
        Builtin::TemporalPlainTime => Some(0.0),
        Builtin::TemporalPlainTimeFrom => Some(1.0),
        Builtin::TemporalPlainTimeCompare => Some(2.0),
        Builtin::TemporalPlainTimeAdd
        | Builtin::TemporalPlainTimeSubtract
        | Builtin::TemporalPlainTimeWith
        | Builtin::TemporalPlainTimeRound
        | Builtin::TemporalPlainTimeEquals
        | Builtin::TemporalPlainTimeUntil
        | Builtin::TemporalPlainTimeSince => Some(1.0),
        Builtin::TemporalPlainTimeToString
        | Builtin::TemporalPlainTimeToJSON
        | Builtin::TemporalPlainTimeToLocaleString
        | Builtin::TemporalPlainTimeValueOf => Some(0.0),
        Builtin::TemporalPlainDateTimeCalendarIdGetter
        | Builtin::TemporalPlainDateTimeYearGetter
        | Builtin::TemporalPlainDateTimeMonthGetter
        | Builtin::TemporalPlainDateTimeMonthCodeGetter
        | Builtin::TemporalPlainDateTimeDayGetter
        | Builtin::TemporalPlainDateTimeHourGetter
        | Builtin::TemporalPlainDateTimeMinuteGetter
        | Builtin::TemporalPlainDateTimeSecondGetter
        | Builtin::TemporalPlainDateTimeMillisecondGetter
        | Builtin::TemporalPlainDateTimeMicrosecondGetter
        | Builtin::TemporalPlainDateTimeNanosecondGetter
        | Builtin::TemporalPlainTimeHourGetter
        | Builtin::TemporalPlainTimeMinuteGetter
        | Builtin::TemporalPlainTimeSecondGetter
        | Builtin::TemporalPlainTimeMillisecondGetter
        | Builtin::TemporalPlainTimeMicrosecondGetter
        | Builtin::TemporalPlainTimeNanosecondGetter
        | Builtin::TemporalInstantEpochNanosecondsGetter
        | Builtin::TemporalInstantEpochMillisecondsGetter => Some(0.0),
        Builtin::TemporalPlainDate => Some(3.0),
        Builtin::TemporalDurationFrom => Some(1.0),
        Builtin::TemporalDurationCompare => Some(2.0),
        Builtin::TemporalDurationAdd => Some(1.0),
        Builtin::TemporalDurationSubtract => Some(1.0),
        Builtin::TemporalDurationWith => Some(1.0),
        Builtin::TemporalDurationAbs => Some(0.0),
        Builtin::TemporalDurationNegated => Some(0.0),
        Builtin::TemporalDurationRound => Some(1.0),
        Builtin::TemporalDurationTotal => Some(1.0),
        Builtin::TemporalDurationToLocaleString => Some(0.0),
        Builtin::TemporalDurationToString => Some(0.0),
        Builtin::TemporalDurationToJSON => Some(0.0),
        Builtin::TemporalDurationSignGetter => Some(0.0),
        Builtin::TemporalDurationBlankGetter => Some(0.0),
        Builtin::TemporalDurationValueOf => Some(0.0),
        Builtin::TemporalPlainDateFrom => Some(1.0),
        Builtin::TemporalPlainDateCompare => Some(2.0),
        Builtin::TemporalPlainDateWith => Some(1.0),
        Builtin::TemporalPlainDateAdd
        | Builtin::TemporalPlainDateSubtract
        | Builtin::TemporalPlainDateEquals
        | Builtin::TemporalPlainDateToPlainDateTime
        | Builtin::TemporalPlainDateToPlainMonthDay
        | Builtin::TemporalPlainDateToPlainYearMonth
        | Builtin::TemporalPlainDateToZonedDateTime => Some(1.0),
        Builtin::TemporalPlainDateUntil | Builtin::TemporalPlainDateSince => Some(2.0),
        Builtin::TemporalPlainDateToLocaleString => Some(0.0),
        Builtin::TemporalPlainDateWithCalendar => Some(1.0),
        Builtin::TemporalPlainDateToString => Some(0.0),
        Builtin::TemporalPlainDateToJSON => Some(0.0),
        Builtin::TemporalPlainDateDayOfWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateDayOfYearGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInMonthGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInYearGetter => Some(0.0),
        Builtin::TemporalPlainDateDaysInWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateMonthsInYearGetter => Some(0.0),
        Builtin::TemporalPlainDateInLeapYearGetter => Some(0.0),
        Builtin::TemporalPlainDateEraGetter => Some(0.0),
        Builtin::TemporalPlainDateEraYearGetter => Some(0.0),
        Builtin::TemporalPlainDateCalendarIdGetter => Some(0.0),
        Builtin::TemporalPlainDateWeekOfYearGetter => Some(0.0),
        Builtin::TemporalPlainDateYearOfWeekGetter => Some(0.0),
        Builtin::TemporalPlainDateDayGetter => Some(0.0),
        Builtin::TemporalPlainDateYearGetter => Some(0.0),
        Builtin::TemporalPlainDateMonthCodeGetter => Some(0.0),
        Builtin::TemporalPlainDateMonthGetter => Some(0.0),
        Builtin::TemporalPlainDateValueOf => Some(0.0),
        _ => None,
    }
}
