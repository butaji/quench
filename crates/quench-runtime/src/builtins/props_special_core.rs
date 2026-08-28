fn special(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == Number {
        return props_number::constant(key).or_else(|| special_match(builtin, key));
    }
    if builtin == Math {
        return crate::math::constant(key)
            .or_else(|| crate::math::property(key).map(Value::Builtin))
            .or_else(|| special_match(builtin, key));
    }
    if builtin == Json && key == "stringify" {
        return Some(Value::Builtin(JsonStringify));
    }
    special_match(builtin, key)
}
fn special_match(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if let Some(value) = special_match_prefix(builtin, key) {
        return Some(value);
    }
    match (builtin, key) {
        (Temporal, "Duration") => Some(Value::Builtin(TemporalDuration)),
        (Temporal, "Instant") => Some(Value::Builtin(TemporalInstant)),
        (Temporal, "PlainDateTime") => Some(Value::Builtin(TemporalPlainDateTime)),
        (Temporal, "PlainTime") => Some(Value::Builtin(TemporalPlainTime)),
        (Temporal, "PlainMonthDay") => Some(Value::Builtin(TemporalPlainMonthDay)),
        (Temporal, "PlainYearMonth") => Some(Value::Builtin(TemporalPlainYearMonth)),
        (Temporal, "ZonedDateTime") => Some(Value::Builtin(TemporalZonedDateTime)),
        (Temporal, "Now") => Some(Value::Builtin(TemporalNow)),
        (Temporal, "toString") => Some(Value::Builtin(ObjectPrototypeToString)),
        (IntlSegmenterPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.Segmenter".into()))
        }
        (IntlCollatorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.Collator".into()))
        }
        (Intl, "Symbol.toStringTag") => Some(Value::String("Intl".into())),
        (IntlDisplayNamesPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.DisplayNames".into()))
        }
        (IntlListFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.ListFormat".into()))
        }
        (IntlNumberFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.NumberFormat".into()))
        }
        (IntlDateTimeFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.DateTimeFormat".into()))
        }
        (IntlLocalePrototype, "Symbol.toStringTag") => Some(Value::String("Intl.Locale".into())),
        (IntlPluralRulesPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.PluralRules".into()))
        }
        (IntlRelativeTimeFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.RelativeTimeFormat".into()))
        }
        (PromisePrototype, "Symbol.toStringTag") => Some(Value::String("Promise".into())),
        (IntlDurationFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.DurationFormat".into()))
        }
        (Temporal, "PlainDate") => Some(Value::Builtin(TemporalPlainDate)),
        (Temporal, "Symbol.toStringTag") => Some(Value::String("Temporal".into())),
        (TemporalDuration, "prototype") => Some(Value::Builtin(TemporalDurationPrototype)),
        (TemporalInstant, "prototype") => Some(Value::Builtin(TemporalInstantPrototype)),
        (TemporalInstant, "from") => Some(Value::Builtin(TemporalInstantFrom)),
        (TemporalInstant, "compare") => Some(Value::Builtin(TemporalInstantCompare)),
        (TemporalInstant, "fromEpochMilliseconds") => {
            Some(Value::Builtin(TemporalInstantFromEpochMilliseconds))
        }
        (TemporalInstant, "fromEpochNanoseconds") => {
            Some(Value::Builtin(TemporalInstantFromEpochNanoseconds))
        }
        (TemporalInstantPrototype, "constructor") => Some(Value::Builtin(TemporalInstant)),
        (TemporalInstantPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.Instant".into()))
        }
        (TemporalInstantPrototype, "epochNanoseconds") => {
            Some(Value::Builtin(TemporalInstantEpochNanosecondsGetter))
        }
        (TemporalInstantPrototype, "epochMilliseconds") => {
            Some(Value::Builtin(TemporalInstantEpochMillisecondsGetter))
        }
        (TemporalInstantPrototype, "toString") => Some(Value::Builtin(TemporalInstantToString)),
        (TemporalInstantPrototype, "toJSON") => Some(Value::Builtin(TemporalInstantToJSON)),
        (TemporalInstantPrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalInstantToLocaleString))
        }
        (TemporalInstantPrototype, "valueOf") => Some(Value::Builtin(TemporalInstantValueOf)),
        (TemporalInstantPrototype, "toZonedDateTimeISO") => {
            Some(Value::Builtin(TemporalInstantToZonedDateTimeISO))
        }
        (TemporalZonedDateTimePrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.ZonedDateTime".into()))
        }
        (TemporalInstantPrototype, "equals") => Some(Value::Builtin(TemporalInstantEquals)),
        (TemporalInstantPrototype, "add") => Some(Value::Builtin(TemporalInstantAdd)),
        (TemporalInstantPrototype, "subtract") => Some(Value::Builtin(TemporalInstantSubtract)),
        (TemporalInstantPrototype, "until") => Some(Value::Builtin(TemporalInstantUntil)),
        (TemporalInstantPrototype, "since") => Some(Value::Builtin(TemporalInstantSince)),
        (TemporalInstantPrototype, "round") => Some(Value::Builtin(TemporalInstantRound)),
        (TemporalDuration, "from") => Some(Value::Builtin(TemporalDurationFrom)),
        (TemporalDuration, "compare") => Some(Value::Builtin(TemporalDurationCompare)),
        (TemporalDurationPrototype, "constructor") => Some(Value::Builtin(TemporalDuration)),
        (TemporalDurationPrototype, "add") => Some(Value::Builtin(TemporalDurationAdd)),
        (TemporalDurationPrototype, "subtract") => Some(Value::Builtin(TemporalDurationSubtract)),
        (TemporalDurationPrototype, "with") => Some(Value::Builtin(TemporalDurationWith)),
        (TemporalDurationPrototype, "abs") => Some(Value::Builtin(TemporalDurationAbs)),
        (TemporalDurationPrototype, "negated") => Some(Value::Builtin(TemporalDurationNegated)),
        (TemporalDurationPrototype, "round") => Some(Value::Builtin(TemporalDurationRound)),
        (TemporalDurationPrototype, "total") => Some(Value::Builtin(TemporalDurationTotal)),
        (TemporalDurationPrototype, "sign") => Some(Value::Builtin(TemporalDurationSignGetter)),
        (TemporalDurationPrototype, "blank") => Some(Value::Builtin(TemporalDurationBlankGetter)),
        (TemporalDurationPrototype, "valueOf") => Some(Value::Builtin(TemporalDurationValueOf)),
        (TemporalDurationPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.Duration".into()))
        }
        (TemporalDurationPrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalDurationToLocaleString))
        }
        (TemporalDurationPrototype, "toString") => Some(Value::Builtin(TemporalDurationToString)),
        (TemporalDurationPrototype, "toJSON") => Some(Value::Builtin(TemporalDurationToJSON)),
        (TemporalPlainDate, "prototype") => Some(Value::Builtin(TemporalPlainDatePrototype)),
        (TemporalPlainDate, "from") => Some(Value::Builtin(TemporalPlainDateFrom)),
        (TemporalPlainDate, "compare") => Some(Value::Builtin(TemporalPlainDateCompare)),
        (TemporalPlainDatePrototype, "constructor") => Some(Value::Builtin(TemporalPlainDate)),
        (TemporalPlainDatePrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.PlainDate".into()))
        }
        (TemporalPlainDatePrototype, "withCalendar") => {
            Some(Value::Builtin(TemporalPlainDateWithCalendar))
        }
        (TemporalPlainDatePrototype, "with") => Some(Value::Builtin(TemporalPlainDateWith)),
        (TemporalPlainDatePrototype, "add") => Some(Value::Builtin(TemporalPlainDateAdd)),
        (TemporalPlainDatePrototype, "subtract") => Some(Value::Builtin(TemporalPlainDateSubtract)),
        (TemporalPlainDatePrototype, "equals") => Some(Value::Builtin(TemporalPlainDateEquals)),
        (TemporalPlainDatePrototype, "until") => Some(Value::Builtin(TemporalPlainDateUntil)),
        (TemporalPlainDatePrototype, "since") => Some(Value::Builtin(TemporalPlainDateSince)),
        (TemporalPlainDatePrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalPlainDateToLocaleString))
        }
        (TemporalPlainDatePrototype, "toPlainDateTime") => {
            Some(Value::Builtin(TemporalPlainDateToPlainDateTime))
        }
        (TemporalPlainDatePrototype, "toPlainMonthDay") => {
            Some(Value::Builtin(TemporalPlainDateToPlainMonthDay))
        }
        (TemporalPlainDatePrototype, "toPlainYearMonth") => {
            Some(Value::Builtin(TemporalPlainDateToPlainYearMonth))
        }
        (TemporalPlainDatePrototype, "toZonedDateTime") => {
            Some(Value::Builtin(TemporalPlainDateToZonedDateTime))
        }
        (TemporalPlainDatePrototype, "monthsInYear") => {
            Some(Value::Builtin(TemporalPlainDateMonthsInYearGetter))
        }
        (TemporalPlainDatePrototype, "toString") => Some(Value::Builtin(TemporalPlainDateToString)),
        (TemporalPlainDatePrototype, "toJSON") => Some(Value::Builtin(TemporalPlainDateToJSON)),
        (TemporalPlainDatePrototype, "valueOf") => Some(Value::Builtin(TemporalPlainDateValueOf)),
        (TemporalPlainDateTime, "prototype") => {
            Some(Value::Builtin(TemporalPlainDateTimePrototype))
        }
        (TemporalPlainDateTime, "from") => Some(Value::Builtin(TemporalPlainDateTimeFrom)),
        (TemporalPlainDateTime, "compare") => Some(Value::Builtin(TemporalPlainDateTimeCompare)),
        (TemporalPlainDateTimePrototype, "constructor") => {
            Some(Value::Builtin(TemporalPlainDateTime))
        }
        (TemporalPlainDateTimePrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.PlainDateTime".into()))
        }
        (TemporalPlainDateTimePrototype, "calendarId") => {
            Some(Value::Builtin(TemporalPlainDateTimeCalendarIdGetter))
        }
        (TemporalPlainDateTimePrototype, "year") => {
            Some(Value::Builtin(TemporalPlainDateTimeYearGetter))
        }
        (TemporalPlainDateTimePrototype, "month") => {
            Some(Value::Builtin(TemporalPlainDateTimeMonthGetter))
        }
        (TemporalPlainDateTimePrototype, "monthCode") => {
            Some(Value::Builtin(TemporalPlainDateTimeMonthCodeGetter))
        }
        (TemporalPlainDateTimePrototype, "day") => {
            Some(Value::Builtin(TemporalPlainDateTimeDayGetter))
        }
        (TemporalPlainDateTimePrototype, "hour") => {
            Some(Value::Builtin(TemporalPlainDateTimeHourGetter))
        }
        (TemporalPlainDateTimePrototype, "minute") => {
            Some(Value::Builtin(TemporalPlainDateTimeMinuteGetter))
        }
        (TemporalPlainDateTimePrototype, "second") => {
            Some(Value::Builtin(TemporalPlainDateTimeSecondGetter))
        }
        (TemporalPlainDateTimePrototype, "millisecond") => {
            Some(Value::Builtin(TemporalPlainDateTimeMillisecondGetter))
        }
        (TemporalPlainDateTimePrototype, "microsecond") => {
            Some(Value::Builtin(TemporalPlainDateTimeMicrosecondGetter))
        }
        (TemporalPlainDateTimePrototype, "nanosecond") => {
            Some(Value::Builtin(TemporalPlainDateTimeNanosecondGetter))
        }
        (TemporalPlainDateTimePrototype, "add") => Some(Value::Builtin(TemporalPlainDateTimeAdd)),
        (TemporalPlainDateTimePrototype, "subtract") => {
            Some(Value::Builtin(TemporalPlainDateTimeSubtract))
        }
        (TemporalPlainDateTimePrototype, "with") => Some(Value::Builtin(TemporalPlainDateTimeWith)),
        (TemporalPlainDateTimePrototype, "round") => {
            Some(Value::Builtin(TemporalPlainDateTimeRound))
        }
        (TemporalPlainDateTimePrototype, "equals") => {
            Some(Value::Builtin(TemporalPlainDateTimeEquals))
        }
        (TemporalPlainDateTimePrototype, "until") => {
            Some(Value::Builtin(TemporalPlainDateTimeUntil))
        }
        (TemporalPlainDateTimePrototype, "since") => {
            Some(Value::Builtin(TemporalPlainDateTimeSince))
        }
        (TemporalPlainDateTimePrototype, "toPlainDate") => {
            Some(Value::Builtin(TemporalPlainDateTimeToPlainDate))
        }
        (TemporalPlainDateTimePrototype, "toPlainTime") => {
            Some(Value::Builtin(TemporalPlainDateTimeToPlainTime))
        }
        (TemporalPlainDateTimePrototype, "toZonedDateTime") => {
            Some(Value::Builtin(TemporalPlainDateTimeToZonedDateTime))
        }
        (TemporalPlainDateTimePrototype, "withCalendar") => {
            Some(Value::Builtin(TemporalPlainDateTimeWithCalendar))
        }
        (TemporalPlainDateTimePrototype, "withPlainTime") => {
            Some(Value::Builtin(TemporalPlainDateTimeWithPlainTime))
        }
        (TemporalPlainDateTimePrototype, "dayOfWeek") => {
            Some(Value::Builtin(TemporalPlainDateTimeDayOfWeekGetter))
        }
        (TemporalPlainDateTimePrototype, "dayOfYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeDayOfYearGetter))
        }
        (TemporalPlainDateTimePrototype, "daysInMonth") => {
            Some(Value::Builtin(TemporalPlainDateTimeDaysInMonthGetter))
        }
        (TemporalPlainDateTimePrototype, "daysInWeek") => {
            Some(Value::Builtin(TemporalPlainDateTimeDaysInWeekGetter))
        }
        (TemporalPlainDateTimePrototype, "daysInYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeDaysInYearGetter))
        }
        (TemporalPlainDateTimePrototype, "monthsInYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeMonthsInYearGetter))
        }
        (TemporalPlainDateTimePrototype, "inLeapYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeInLeapYearGetter))
        }
        (TemporalPlainDateTimePrototype, "era") => {
            Some(Value::Builtin(TemporalPlainDateTimeEraGetter))
        }
        (TemporalPlainDateTimePrototype, "eraYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeEraYearGetter))
        }
        (TemporalPlainDateTimePrototype, "weekOfYear") => {
            Some(Value::Builtin(TemporalPlainDateTimeWeekOfYearGetter))
        }
        (TemporalPlainDateTimePrototype, "yearOfWeek") => {
            Some(Value::Builtin(TemporalPlainDateTimeYearOfWeekGetter))
        }
        (TemporalPlainDateTimePrototype, "toString") => {
            Some(Value::Builtin(TemporalPlainDateTimeToString))
        }
        (TemporalPlainDateTimePrototype, "toJSON") => {
            Some(Value::Builtin(TemporalPlainDateTimeToJSON))
        }
        (TemporalPlainDateTimePrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalPlainDateTimeToLocaleString))
        }
        (TemporalPlainDateTimePrototype, "valueOf") => {
            Some(Value::Builtin(TemporalPlainDateTimeValueOf))
        }
        (TemporalPlainTime, "prototype") => Some(Value::Builtin(TemporalPlainTimePrototype)),
        (TemporalPlainTime, "from") => Some(Value::Builtin(TemporalPlainTimeFrom)),
        (TemporalPlainTime, "compare") => Some(Value::Builtin(TemporalPlainTimeCompare)),
        (TemporalPlainTimePrototype, "constructor") => Some(Value::Builtin(TemporalPlainTime)),
        (TemporalPlainTimePrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.PlainTime".into()))
        }
        (TemporalPlainTimePrototype, "hour") => Some(Value::Builtin(TemporalPlainTimeHourGetter)),
        (TemporalPlainTimePrototype, "minute") => {
            Some(Value::Builtin(TemporalPlainTimeMinuteGetter))
        }
        (TemporalPlainTimePrototype, "second") => {
            Some(Value::Builtin(TemporalPlainTimeSecondGetter))
        }
        (TemporalPlainTimePrototype, "millisecond") => {
            Some(Value::Builtin(TemporalPlainTimeMillisecondGetter))
        }
        (TemporalPlainTimePrototype, "microsecond") => {
            Some(Value::Builtin(TemporalPlainTimeMicrosecondGetter))
        }
        (TemporalPlainTimePrototype, "nanosecond") => {
            Some(Value::Builtin(TemporalPlainTimeNanosecondGetter))
        }
        (TemporalPlainMonthDayPrototype, "calendarId") => {
            Some(Value::Builtin(TemporalPlainMonthDayCalendarIdGetter))
        }
        (TemporalPlainMonthDayPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.PlainMonthDay".into()))
        }
        (TemporalPlainMonthDayPrototype, "day") => {
            Some(Value::Builtin(TemporalPlainMonthDayDayGetter))
        }
        (TemporalPlainMonthDayPrototype, "monthCode") => {
            Some(Value::Builtin(TemporalPlainMonthDayMonthCodeGetter))
        }
        (TemporalPlainMonthDayPrototype, "equals") => {
            Some(Value::Builtin(TemporalPlainMonthDayEquals))
        }
        (TemporalPlainMonthDayPrototype, "toString") => {
            Some(Value::Builtin(TemporalPlainMonthDayToString))
        }
        (TemporalPlainMonthDayPrototype, "toJSON") => {
            Some(Value::Builtin(TemporalPlainMonthDayToJSON))
        }
        (TemporalPlainMonthDayPrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalPlainMonthDayToLocaleString))
        }
        (TemporalPlainMonthDayPrototype, "toPlainDate") => {
            Some(Value::Builtin(TemporalPlainMonthDayToPlainDate))
        }
        (TemporalPlainMonthDayPrototype, "with") => Some(Value::Builtin(TemporalPlainMonthDayWith)),
        (TemporalPlainMonthDayPrototype, "valueOf") => {
            Some(Value::Builtin(TemporalPlainMonthDayValueOf))
        }
        (TemporalPlainYearMonthPrototype, "calendarId") => {
            Some(Value::Builtin(TemporalPlainYearMonthCalendarIdGetter))
        }
        (TemporalPlainYearMonthPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Temporal.PlainYearMonth".into()))
        }
        (TemporalPlainYearMonthPrototype, "year") => {
            Some(Value::Builtin(TemporalPlainYearMonthYearGetter))
        }
        (TemporalPlainYearMonthPrototype, "month") => {
            Some(Value::Builtin(TemporalPlainYearMonthMonthGetter))
        }
        (TemporalPlainYearMonthPrototype, "monthCode") => {
            Some(Value::Builtin(TemporalPlainYearMonthMonthCodeGetter))
        }
        (TemporalPlainYearMonthPrototype, "equals") => {
            Some(Value::Builtin(TemporalPlainYearMonthEquals))
        }
        (TemporalPlainYearMonthPrototype, "toString") => {
            Some(Value::Builtin(TemporalPlainYearMonthToString))
        }
        (TemporalPlainYearMonthPrototype, "toJSON") => {
            Some(Value::Builtin(TemporalPlainYearMonthToJSON))
        }
        (TemporalPlainYearMonthPrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalPlainYearMonthToLocaleString))
        }
        (TemporalPlainYearMonthPrototype, "toPlainDate") => {
            Some(Value::Builtin(TemporalPlainYearMonthToPlainDate))
        }
        (TemporalPlainYearMonthPrototype, "with") => {
            Some(Value::Builtin(TemporalPlainYearMonthWith))
        }
        (TemporalPlainYearMonthPrototype, "add") => Some(Value::Builtin(TemporalPlainYearMonthAdd)),
        (TemporalPlainYearMonthPrototype, "subtract") => {
            Some(Value::Builtin(TemporalPlainYearMonthSubtract))
        }
        (TemporalPlainYearMonthPrototype, "until") => {
            Some(Value::Builtin(TemporalPlainYearMonthUntil))
        }
        (TemporalPlainYearMonthPrototype, "since") => {
            Some(Value::Builtin(TemporalPlainYearMonthSince))
        }
        (TemporalPlainYearMonthPrototype, "daysInMonth") => {
            Some(Value::Builtin(TemporalPlainYearMonthDaysInMonthGetter))
        }
        (TemporalPlainYearMonthPrototype, "daysInYear") => {
            Some(Value::Builtin(TemporalPlainYearMonthDaysInYearGetter))
        }
        (TemporalPlainYearMonthPrototype, "inLeapYear") => {
            Some(Value::Builtin(TemporalPlainYearMonthInLeapYearGetter))
        }
        (TemporalPlainYearMonthPrototype, "monthsInYear") => {
            Some(Value::Builtin(TemporalPlainYearMonthMonthsInYearGetter))
        }
        (TemporalPlainYearMonthPrototype, "era") => {
            Some(Value::Builtin(TemporalPlainYearMonthEraGetter))
        }
        (TemporalPlainYearMonthPrototype, "eraYear") => {
            Some(Value::Builtin(TemporalPlainYearMonthEraYearGetter))
        }
        (TemporalPlainYearMonthPrototype, "valueOf") => {
            Some(Value::Builtin(TemporalPlainYearMonthValueOf))
        }
        (TemporalZonedDateTimePrototype, "toString") => {
            Some(Value::Builtin(TemporalZonedDateTimeToString))
        }
        (TemporalZonedDateTimePrototype, "toJSON") => {
            Some(Value::Builtin(TemporalZonedDateTimeToJSON))
        }
        (TemporalZonedDateTimePrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalZonedDateTimeToLocaleString))
        }
        (TemporalZonedDateTimePrototype, "toInstant") => {
            Some(Value::Builtin(TemporalZonedDateTimeToInstant))
        }
        (TemporalZonedDateTimePrototype, "toPlainDateTime") => {
            Some(Value::Builtin(TemporalZonedDateTimeToPlainDateTime))
        }
        (TemporalZonedDateTimePrototype, "toPlainDate") => {
            Some(Value::Builtin(TemporalZonedDateTimeToPlainDate))
        }
        (TemporalZonedDateTimePrototype, "toPlainTime") => {
            Some(Value::Builtin(TemporalZonedDateTimeToPlainTime))
        }
        (TemporalZonedDateTimePrototype, "equals") => {
            Some(Value::Builtin(TemporalZonedDateTimeEquals))
        }
        (TemporalZonedDateTimePrototype, "withTimeZone") => {
            Some(Value::Builtin(TemporalZonedDateTimeWithTimeZone))
        }
        (TemporalZonedDateTimePrototype, "withCalendar") => {
            Some(Value::Builtin(TemporalZonedDateTimeWithCalendar))
        }
        (TemporalZonedDateTimePrototype, "withPlainTime") => {
            Some(Value::Builtin(TemporalZonedDateTimeWithPlainTime))
        }
        (TemporalZonedDateTimePrototype, "with") => Some(Value::Builtin(TemporalZonedDateTimeWith)),
        (TemporalZonedDateTimePrototype, "startOfDay") => {
            Some(Value::Builtin(TemporalZonedDateTimeStartOfDay))
        }
        (TemporalZonedDateTimePrototype, "getTimeZoneTransition") => {
            Some(Value::Builtin(TemporalZonedDateTimeGetTimeZoneTransition))
        }
        (TemporalZonedDateTimePrototype, "add") => Some(Value::Builtin(TemporalZonedDateTimeAdd)),
        (TemporalZonedDateTimePrototype, "subtract") => {
            Some(Value::Builtin(TemporalZonedDateTimeSubtract))
        }
        (TemporalZonedDateTimePrototype, "until") => {
            Some(Value::Builtin(TemporalZonedDateTimeUntil))
        }
        (TemporalZonedDateTimePrototype, "since") => {
            Some(Value::Builtin(TemporalZonedDateTimeSince))
        }
        (TemporalZonedDateTimePrototype, "round") => {
            Some(Value::Builtin(TemporalZonedDateTimeRound))
        }
        (TemporalZonedDateTimePrototype, "valueOf") => {
            Some(Value::Builtin(TemporalDurationValueOf))
        }
        (TemporalZonedDateTimePrototype, "epochMilliseconds") => {
            Some(Value::Builtin(TemporalZonedDateTimeEpochMillisecondsGetter))
        }
        (TemporalZonedDateTimePrototype, "timeZoneId") => {
            Some(Value::Builtin(TemporalZonedDateTimeTimeZoneIdGetter))
        }
        (TemporalZonedDateTimePrototype, "offset") => {
            Some(Value::Builtin(TemporalZonedDateTimeOffsetGetter))
        }
        (TemporalZonedDateTimePrototype, "offsetNanoseconds") => {
            Some(Value::Builtin(TemporalZonedDateTimeOffsetNanosecondsGetter))
        }
        (TemporalZonedDateTimePrototype, "hoursInDay") => {
            Some(Value::Builtin(TemporalZonedDateTimeHoursInDayGetter))
        }
        (TemporalZonedDateTimePrototype, "weekOfYear") => {
            Some(Value::Builtin(TemporalZonedDateTimeWeekOfYearGetter))
        }
        (TemporalZonedDateTimePrototype, "yearOfWeek") => {
            Some(Value::Builtin(TemporalZonedDateTimeYearOfWeekGetter))
        }
        (TemporalPlainTimePrototype, "toString") => Some(Value::Builtin(TemporalPlainTimeToString)),
        (TemporalPlainTimePrototype, "toJSON") => Some(Value::Builtin(TemporalPlainTimeToJSON)),
        (TemporalPlainTimePrototype, "toLocaleString") => {
            Some(Value::Builtin(TemporalPlainTimeToLocaleString))
        }
        (TemporalPlainTimePrototype, "valueOf") => Some(Value::Builtin(TemporalPlainTimeValueOf)),
        (TemporalPlainTimePrototype, "equals") => Some(Value::Builtin(TemporalPlainTimeEquals)),
        (TemporalPlainTimePrototype, "add") => Some(Value::Builtin(TemporalPlainTimeAdd)),
        (TemporalPlainTimePrototype, "subtract") => Some(Value::Builtin(TemporalPlainTimeSubtract)),
        (TemporalPlainTimePrototype, "with") => Some(Value::Builtin(TemporalPlainTimeWith)),
        (TemporalPlainTimePrototype, "round") => Some(Value::Builtin(TemporalPlainTimeRound)),
        (TemporalPlainTimePrototype, "until") => Some(Value::Builtin(TemporalPlainTimeUntil)),
        (TemporalPlainTimePrototype, "since") => Some(Value::Builtin(TemporalPlainTimeSince)),
        (TemporalPlainMonthDay, "prototype") => {
            Some(Value::Builtin(TemporalPlainMonthDayPrototype))
        }
        (TemporalPlainMonthDay, "from") => Some(Value::Builtin(TemporalPlainMonthDayFrom)),
        (TemporalPlainMonthDay, "compare") => Some(Value::Builtin(TemporalPlainMonthDayCompare)),
        (TemporalPlainMonthDayPrototype, "constructor") => {
            Some(Value::Builtin(TemporalPlainMonthDay))
        }
        (TemporalPlainYearMonth, "prototype") => {
            Some(Value::Builtin(TemporalPlainYearMonthPrototype))
        }
        (TemporalPlainYearMonth, "from") => Some(Value::Builtin(TemporalPlainYearMonthFrom)),
        (TemporalPlainYearMonth, "compare") => Some(Value::Builtin(TemporalPlainYearMonthCompare)),
        (TemporalPlainYearMonthPrototype, "constructor") => {
            Some(Value::Builtin(TemporalPlainYearMonth))
        }
        (TemporalZonedDateTime, "prototype") => {
            Some(Value::Builtin(TemporalZonedDateTimePrototype))
        }
        (TemporalZonedDateTime, "from") => Some(Value::Builtin(TemporalZonedDateTimeFrom)),
        (TemporalZonedDateTime, "compare") => Some(Value::Builtin(TemporalZonedDateTimeCompare)),
        (TemporalZonedDateTimePrototype, "constructor") => {
            Some(Value::Builtin(TemporalZonedDateTime))
        }
        (TemporalNow, "instant") => Some(Value::Builtin(TemporalNowInstant)),
        (TemporalNow, "Symbol.toStringTag") => Some(Value::String("Temporal.Now".into())),
        (TemporalNow, "plainDateISO") => Some(Value::Builtin(TemporalNowPlainDateISO)),
        (TemporalNow, "plainDateTimeISO") => Some(Value::Builtin(TemporalNowPlainDateTimeISO)),
        (TemporalNow, "plainTimeISO") => Some(Value::Builtin(TemporalNowPlainTimeISO)),
        (TemporalNow, "timeZoneId") => Some(Value::Builtin(TemporalNowTimeZoneId)),
        (TemporalNow, "zonedDateTimeISO") => Some(Value::Builtin(TemporalNowZonedDateTimeISO)),
        (AbstractModuleSource, "prototype") => Some(Value::Builtin(AbstractModuleSourcePrototype)),
        (AbstractModuleSourcePrototype, "constructor") => {
            Some(Value::Builtin(AbstractModuleSource))
        }
        (ShadowRealmPrototype, "constructor") => Some(Value::Builtin(ShadowRealm)),
        (ShadowRealm, "prototype") => Some(Value::Builtin(ShadowRealmPrototype)),
        (ShadowRealmPrototype, "evaluate") => Some(Value::Builtin(ShadowRealmEvaluate)),
        (ShadowRealmPrototype, "importValue") => Some(Value::Builtin(ShadowRealmImportValue)),
        (ShadowRealmPrototype, "Symbol.toStringTag") => Some(Value::String("ShadowRealm".into())),
        (String, "prototype") => Some(Value::Builtin(StringPrototype)),
        (StringPrototype, "constructor") => Some(Value::Builtin(String)),
        _ => special_match_middle(builtin, key),
    }
}

fn special_match_prefix(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == TypedArrayPrototype && key == "slice" {
        return Some(Value::Builtin(TypedArraySlice));
    }
    if builtin == ArrayIteratorPrototype && key == "constructor" {
        return Some(Value::Builtin(Array));
    }
    if builtin == ArrayIteratorPrototype && key == "next" {
        return Some(Value::Builtin(IteratorNext));
    }
    if builtin == ArrayIteratorPrototype && key == "Symbol.toStringTag" {
        return Some(Value::String("Array Iterator".into()));
    }
    if builtin == IteratorPrototype && key == "Symbol.iterator" {
        return Some(Value::Builtin(IteratorSelf));
    }
    if builtin == IteratorPrototype && key == "toArray" {
        return Some(Value::Builtin(IteratorToArray));
    }
    if builtin == IteratorPrototype && key == "map" {
        return Some(Value::Builtin(IteratorMap));
    }
    if builtin == IteratorPrototype && key == "filter" {
        return Some(Value::Builtin(IteratorFilter));
    }
    if builtin == IteratorPrototype && key == "flatMap" {
        return Some(Value::Builtin(IteratorFlatMap));
    }
    if builtin == IteratorPrototype && key == "drop" {
        return Some(Value::Builtin(IteratorDrop));
    }
    if builtin == IteratorPrototype && key == "take" {
        return Some(Value::Builtin(IteratorTake));
    }
    if builtin == IteratorPrototype && key == "reduce" {
        return Some(Value::Builtin(IteratorReduce));
    }
    if builtin == IteratorPrototype && key == "find" {
        return Some(Value::Builtin(IteratorFind));
    }
    if builtin == IteratorPrototype && key == "forEach" {
        return Some(Value::Builtin(IteratorForEach));
    }
    if builtin == IteratorPrototype && key == "some" {
        return Some(Value::Builtin(IteratorSome));
    }
    if builtin == IteratorPrototype && key == "every" {
        return Some(Value::Builtin(IteratorEvery));
    }
    if let Some(value) = typed_array_static_property(builtin, key) {
        return Some(value);
    }
    if let Some(value) = weak_special(builtin, key) {
        return Some(value);
    }
    if builtin == Builtin::Error && key == "isError" {
        return Some(Value::Builtin(Builtin::ErrorIsError));
    }
    if builtin == Builtin::Error && key == "captureStackTrace" {
        return Some(Value::Builtin(Builtin::ErrorCaptureStackTrace));
    }
    None
}

fn special_match_middle(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (ArrayBufferPrototype, "Symbol.toStringTag") => Some(Value::String("ArrayBuffer".into())),
        (RegExpStringIteratorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("RegExp String Iterator".into()))
        }
        (SharedArrayBufferPrototype, "Symbol.toStringTag") => {
            Some(Value::String("SharedArrayBuffer".into()))
        }
        (StringIteratorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("String Iterator".into()))
        }
        (Math, "Symbol.toStringTag") => Some(Value::String("Math".into())),
        (Atomics, "Symbol.toStringTag") => Some(Value::String("Atomics".into())),
        (Atomics, "add") => Some(Value::Builtin(AtomicsAdd)),
        (Atomics, "and") => Some(Value::Builtin(AtomicsAnd)),
        (Atomics, "or") => Some(Value::Builtin(AtomicsOr)),
        (Atomics, "sub") => Some(Value::Builtin(AtomicsSub)),
        (Atomics, "xor") => Some(Value::Builtin(AtomicsXor)),
        (Atomics, "compareExchange") => Some(Value::Builtin(AtomicsCompareExchange)),
        (Atomics, "isLockFree") => Some(Value::Builtin(AtomicsIsLockFree)),
        (Atomics, "notify") => Some(Value::Builtin(AtomicsNotify)),
        (Atomics, "wait") => Some(Value::Builtin(AtomicsWait)),
        (Atomics, "load") => Some(Value::Builtin(AtomicsLoad)),
        (Atomics, "store") => Some(Value::Builtin(AtomicsStore)),
        (Atomics, "exchange") => Some(Value::Builtin(AtomicsExchange)),
        (Atomics, "waitAsync") => Some(Value::Builtin(AtomicsWaitAsync)),
        (Atomics, "pause") => Some(Value::Builtin(AtomicsPause)),
        (Reflect, "Symbol.toStringTag") => Some(Value::String("Reflect".into())),
        (SymbolPrototype, "Symbol.toStringTag") => Some(Value::String("Symbol".into())),
        (Symbol, "prototype") => Some(Value::Builtin(SymbolPrototype)),
        (Symbol, "unscopables") => Some(Value::String("Symbol.unscopables\0".to_string())),
        (ArrayPrototype, "constructor") => Some(Value::Builtin(Array)),
        (ArrayPrototype, "length") => Some(Value::Number(0.0)),
        (ArrayPrototype, "Symbol.unscopables") => Some(array_unscopables()),
        (Symbol, k) => crate::builtin_meta::symbol::symbol_prop(k).map(Value::Builtin),
        (Map, "groupBy") => Some(Value::Builtin(MapGroupBy)),
        (Set, "Symbol.species") => Some(Value::Builtin(Set)),
        (Map, "Symbol.species") => Some(Value::Builtin(Map)),
        (MapPrototype | SetPrototype | SetIteratorPrototype | MapIteratorPrototype, k) => {
            collections_prop(builtin, k)
        }
        (BigIntPrototype, "Symbol.toStringTag") => Some(Value::String("BigInt".to_string())),
        (AsyncFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncFunction".to_string()))
        }
        (GeneratorFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("GeneratorFunction".to_string()))
        }
        (GeneratorFunctionPrototype, "prototype") => Some(crate::builtins::generator_prototype()),
        (DataViewPrototype, "Symbol.toStringTag") => Some(Value::String("DataView".into())),
        (AsyncGeneratorFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncGeneratorFunction".into()))
        }
        (AsyncGeneratorFunctionPrototype, "prototype") => {
            Some(crate::builtins::async_generator_prototype())
        }
        _ => special_match_tail(builtin, key),
    }
}

fn special_match_tail(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (FinalizationRegistry, "prototype") => Some(Value::Builtin(FinalizationRegistryPrototype)),
        (FinalizationRegistryPrototype, "Symbol.toStringTag") => {
            Some(Value::String("FinalizationRegistry".into()))
        }
        (FinalizationRegistryPrototype, k) => {
            crate::builtin_meta::finalization_registry::property(k).map(Value::Builtin)
        }
        (ObjectPrototype, "constructor") => Some(Value::Builtin(Object)),
        (DatePrototype, k) => crate::builtin_meta::date::date_prop(k).map(Value::Builtin),
        (DisposableStack, "prototype") => Some(Value::Builtin(DisposableStackPrototype)),
        (AsyncDisposableStack, "prototype") => Some(Value::Builtin(AsyncDisposableStackPrototype)),
        (AsyncDisposableStackPrototype, "constructor") => {
            Some(Value::Builtin(AsyncDisposableStack))
        }
        (AsyncDisposableStackPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncDisposableStack".into()))
        }
        (AsyncDisposableStackPrototype, k) => {
            crate::builtin_meta::disposable::async_property(k).map(Value::Builtin)
        }
        (DisposableStackPrototype, "Symbol.dispose") => {
            Some(Value::Builtin(DisposableStackDispose))
        }
        _ => special_match_error_tail(builtin, key),
    }
}

fn special_match_error_tail(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (ErrorPrototype, "toString") => Some(Value::Builtin(ErrorPrototypeToString)),
        (ErrorPrototype, "name") => Some(Value::String("Error".to_string())),
        (ErrorPrototype, "message") => Some(Value::String("".to_string())),
        (ErrorPrototype, "constructor") => Some(Value::Builtin(Error)),
        (RangeErrorPrototype, "name") => Some(Value::String("RangeError".to_string())),
        (RangeErrorPrototype, "message") => Some(Value::String("".to_string())),
        (RangeErrorPrototype, "constructor") => Some(Value::Builtin(RangeError)),
        (TypeErrorPrototype, "name") => Some(Value::String("TypeError".to_string())),
        (TypeErrorPrototype, "message") => Some(Value::String("".to_string())),
        (TypeErrorPrototype, "constructor") => Some(Value::Builtin(TypeError)),
        (ReferenceErrorPrototype, "name") => Some(Value::String("ReferenceError".to_string())),
        (ReferenceErrorPrototype, "message") => Some(Value::String("".to_string())),
        (ReferenceErrorPrototype, "constructor") => Some(Value::Builtin(ReferenceError)),
        (SyntaxErrorPrototype, "name") => Some(Value::String("SyntaxError".to_string())),
        (SyntaxErrorPrototype, "message") => Some(Value::String("".to_string())),
        (SyntaxErrorPrototype, "constructor") => Some(Value::Builtin(SyntaxError)),
        (EvalErrorPrototype, "name") => Some(Value::String("EvalError".to_string())),
        (EvalErrorPrototype, "message") => Some(Value::String("".to_string())),
        (EvalErrorPrototype, "constructor") => Some(Value::Builtin(EvalError)),
        (URIErrorPrototype, "name") => Some(Value::String("URIError".to_string())),
        (URIErrorPrototype, "message") => Some(Value::String("".to_string())),
        (URIErrorPrototype, "constructor") => Some(Value::Builtin(URIError)),
        (AggregateError, "prototype") => Some(Value::Builtin(AggregateErrorPrototype)),
        (AggregateErrorPrototype, "name") => Some(Value::String("AggregateError".to_string())),
        (AggregateErrorPrototype, "message") => Some(Value::String("".to_string())),
        (AggregateErrorPrototype, "constructor") => Some(Value::Builtin(AggregateError)),
        (SuppressedError, "prototype") => Some(Value::Builtin(SuppressedErrorPrototype)),
        (SuppressedErrorPrototype, "name") => Some(Value::String("SuppressedError".to_string())),
        (SuppressedErrorPrototype, "message") => Some(Value::String("".to_string())),
        (SuppressedErrorPrototype, "constructor") => Some(Value::Builtin(SuppressedError)),
        (DisposableStackPrototype, "Symbol.toStringTag") => {
            Some(Value::String("DisposableStack".into()))
        }
        (DisposableStackPrototype, k) => {
            crate::builtin_meta::disposable::property(k).map(Value::Builtin)
        }
        _ => builtin_method(builtin, key).map(Value::Builtin),
    }
}

fn weak_special(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (WeakMapPrototype, "constructor") => Some(Value::Builtin(WeakMap)),
        (WeakMapPrototype, "Symbol.toStringTag") => Some(Value::String("WeakMap".into())),
        (WeakMapPrototype, k) => match crate::collections::map::weak_property(k) {
            Value::Builtin(value) => Some(Value::Builtin(value)),
            _ => None,
        },
        (WeakMap, "prototype") => Some(Value::Builtin(WeakMapPrototype)),
        (WeakSetPrototype, "constructor") => Some(Value::Builtin(WeakSet)),
        (WeakSetPrototype, "Symbol.toStringTag") => Some(Value::String("WeakSet".into())),
        (WeakSetPrototype, k) => match crate::collections::set::weak_property(k) {
            Value::Builtin(value) => Some(Value::Builtin(value)),
            _ => None,
        },
        (WeakSet, "prototype") => Some(Value::Builtin(WeakSetPrototype)),
        (WeakRef, "prototype") => Some(Value::Builtin(WeakRefPrototype)),
        (WeakRefPrototype, "constructor") => Some(Value::Builtin(WeakRef)),
        (WeakRefPrototype, "deref") => Some(Value::Builtin(WeakRefDeref)),
        (WeakRefPrototype, "Symbol.toStringTag") => Some(Value::String("WeakRef".into())),
        _ => None,
    }
}

fn array_unscopables() -> Value {
    use crate::value::ObjectData;
    use std::rc::Rc;
    const NAMES: &[&str] = &[
        "at",
        "copyWithin",
        "entries",
        "fill",
        "find",
        "findIndex",
        "findLast",
        "findLastIndex",
        "flat",
        "flatMap",
        "includes",
        "keys",
        "toReversed",
        "toSorted",
        "toSpliced",
        "values",
    ];
    let mut properties = vec![("\0prototype".to_string(), Value::Null)];
    properties.extend(
        NAMES
            .iter()
            .map(|name| ((*name).to_string(), Value::Boolean(true))),
    );
    Value::Object(Rc::new(ObjectData::new(properties)))
}
