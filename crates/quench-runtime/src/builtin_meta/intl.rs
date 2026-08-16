//! Intl builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    intl_name_group_a(b)
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    intl_fn_len(b)
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    intl_short_name(b)
}

const fn intl_name_group_a(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlGetCanonicalLocales => Some("Intl.getCanonicalLocales"),
        Builtin::IntlSupportedValuesOf => Some("Intl.supportedValuesOf"),
        Builtin::IntlLocaleToString => Some("Intl.Locale.prototype.toString"),
        Builtin::IntlLocaleMaximize => Some("Intl.Locale.prototype.maximize"),
        Builtin::IntlLocaleMinimize => Some("Intl.Locale.prototype.minimize"),
        Builtin::IntlLocaleGetCalendars => Some("Intl.Locale.prototype.getCalendars"),
        Builtin::IntlLocaleGetCollations => Some("Intl.Locale.prototype.getCollations"),
        Builtin::IntlLocaleGetHourCycles => Some("Intl.Locale.prototype.getHourCycles"),
        Builtin::IntlLocaleGetNumberingSystems => Some("Intl.Locale.prototype.getNumberingSystems"),
        Builtin::IntlLocaleGetTimeZones => Some("Intl.Locale.prototype.getTimeZones"),
        Builtin::IntlLocaleGetTextInfo => Some("Intl.Locale.prototype.getTextInfo"),
        Builtin::IntlLocaleGetWeekInfo => Some("Intl.Locale.prototype.getWeekInfo"),
        Builtin::IntlDateTimeFormatSupportedLocalesOf => {
            Some("Intl.DateTimeFormat.supportedLocalesOf")
        }
        Builtin::IntlSegmenterSupportedLocalesOf => Some("Intl.Segmenter.supportedLocalesOf"),
        Builtin::IntlListFormatSupportedLocalesOf => Some("Intl.ListFormat.supportedLocalesOf"),
        Builtin::IntlLocaleBaseNameGetter => Some("get baseName"),
        Builtin::IntlLocaleCalendarGetter => Some("get calendar"),
        Builtin::IntlLocaleCaseFirstGetter => Some("get caseFirst"),
        Builtin::IntlLocaleCollationGetter => Some("get collation"),
        Builtin::IntlLocaleFirstDayOfWeekGetter => Some("get firstDayOfWeek"),
        Builtin::IntlLocaleHourCycleGetter => Some("get hourCycle"),
        Builtin::IntlLocaleLanguageGetter => Some("get language"),
        Builtin::IntlLocaleNumberingSystemGetter => Some("get numberingSystem"),
        Builtin::IntlLocaleNumericGetter => Some("get numeric"),
        Builtin::IntlLocaleRegionGetter => Some("get region"),
        Builtin::IntlLocaleScriptGetter => Some("get script"),
        Builtin::IntlLocaleTextInfoGetter => Some("get textInfo"),
        Builtin::IntlLocaleVariantsGetter => Some("get variants"),
        _ => intl_name_group_a_tail(b),
    }
}

const fn intl_name_group_a_tail(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlNumberFormatFormat => Some(""),
        Builtin::IntlNumberFormatFormatToParts => Some("Intl.NumberFormat.prototype.formatToParts"),
        Builtin::IntlNumberFormatFormatRange => Some("Intl.NumberFormat.prototype.formatRange"),
        Builtin::IntlNumberFormatFormatRangeToParts => {
            Some("Intl.NumberFormat.prototype.formatRangeToParts")
        }
        Builtin::IntlNumberFormatResolvedOptions => {
            Some("Intl.NumberFormat.prototype.resolvedOptions")
        }
        Builtin::IntlPluralRulesSelect => Some("Intl.PluralRules.prototype.select"),
        Builtin::IntlPluralRulesResolvedOptions => {
            Some("Intl.PluralRules.prototype.resolvedOptions")
        }
        Builtin::IntlDateTimeFormatFormat => Some("Intl.DateTimeFormat.prototype.format"),
        Builtin::IntlDateTimeFormatFormatToParts => {
            Some("Intl.DateTimeFormat.prototype.formatToParts")
        }
        Builtin::IntlDateTimeFormatFormatRange => Some("Intl.DateTimeFormat.prototype.formatRange"),
        Builtin::IntlDateTimeFormatFormatRangeToParts => {
            Some("Intl.DateTimeFormat.prototype.formatRangeToParts")
        }
        Builtin::IntlDateTimeFormatResolvedOptions => {
            Some("Intl.DateTimeFormat.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::IntlGetCanonicalLocales => Some(1.0),
        Builtin::IntlSupportedValuesOf => Some(1.0),
        Builtin::IntlLocaleToString
        | Builtin::IntlLocaleMaximize
        | Builtin::IntlLocaleMinimize
        | Builtin::IntlLocaleGetCalendars
        | Builtin::IntlLocaleGetCollations
        | Builtin::IntlLocaleGetHourCycles
        | Builtin::IntlLocaleGetNumberingSystems
        | Builtin::IntlLocaleGetTimeZones
        | Builtin::IntlLocaleGetTextInfo
        | Builtin::IntlLocaleGetWeekInfo => Some(0.0),
        Builtin::IntlLocaleBaseNameGetter
        | Builtin::IntlLocaleCalendarGetter
        | Builtin::IntlLocaleCaseFirstGetter
        | Builtin::IntlLocaleCollationGetter
        | Builtin::IntlLocaleFirstDayOfWeekGetter
        | Builtin::IntlLocaleHourCycleGetter
        | Builtin::IntlLocaleLanguageGetter
        | Builtin::IntlLocaleNumberingSystemGetter
        | Builtin::IntlLocaleNumericGetter
        | Builtin::IntlLocaleRegionGetter
        | Builtin::IntlLocaleScriptGetter
        | Builtin::IntlLocaleTextInfoGetter
        | Builtin::IntlLocaleVariantsGetter => Some(0.0),
        Builtin::IntlDateTimeFormatSupportedLocalesOf => Some(1.0),
        Builtin::IntlSegmenterSupportedLocalesOf => Some(1.0),
        Builtin::IntlListFormatSupportedLocalesOf => Some(1.0),
        Builtin::IntlNumberFormatFormat => Some(1.0),
        _ => intl_fn_len_tail(b),
    }
}

const fn intl_fn_len_tail(b: Builtin) -> Option<f64> {
    match b {
        Builtin::IntlNumberFormatResolvedOptions
        | Builtin::IntlPluralRulesSelect
        | Builtin::IntlPluralRulesResolvedOptions
        | Builtin::IntlDateTimeFormatFormat
        | Builtin::IntlDateTimeFormatFormatToParts
        | Builtin::IntlDateTimeFormatResolvedOptions
        | Builtin::IntlCollatorCompare
        | Builtin::IntlCollatorResolvedOptions
        | Builtin::IntlSegmenterSegment
        | Builtin::IntlSegmenterSegmentsIterator
        | Builtin::IntlSegmenterSegmentsContaining
        | Builtin::IntlSegmenterResolvedOptions
        | Builtin::IntlDisplayNamesOf
        | Builtin::IntlDisplayNamesResolvedOptions => Some(0.0),
        Builtin::IntlNumberFormatFormatToParts => Some(1.0),
        Builtin::IntlNumberFormatFormatRange
        | Builtin::IntlNumberFormatFormatRangeToParts
        | Builtin::IntlDateTimeFormatFormatRange
        | Builtin::IntlDateTimeFormatFormatRangeToParts => Some(2.0),
        Builtin::IntlListFormatFormat
        | Builtin::IntlListFormatFormatToParts
        | Builtin::IntlListFormatResolvedOptions
        | Builtin::IntlRelativeTimeFormatFormat
        | Builtin::IntlRelativeTimeFormatFormatToParts
        | Builtin::IntlRelativeTimeFormatResolvedOptions => Some(1.0),
        _ => None,
    }
}

const fn intl_short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlSegmenter => Some("Segmenter"),
        Builtin::IntlLocaleToString => Some("toString"),
        Builtin::IntlLocaleMaximize => Some("maximize"),
        Builtin::IntlLocaleMinimize => Some("minimize"),
        Builtin::IntlLocaleGetCalendars => Some("getCalendars"),
        Builtin::IntlLocaleGetCollations => Some("getCollations"),
        Builtin::IntlLocaleGetHourCycles => Some("getHourCycles"),
        Builtin::IntlLocaleGetNumberingSystems => Some("getNumberingSystems"),
        Builtin::IntlLocaleGetTimeZones => Some("getTimeZones"),
        Builtin::IntlLocaleGetTextInfo => Some("getTextInfo"),
        Builtin::IntlLocaleGetWeekInfo => Some("getWeekInfo"),
        Builtin::IntlLocaleBaseNameGetter => Some("baseName"),
        Builtin::IntlLocaleCalendarGetter => Some("calendar"),
        Builtin::IntlLocaleCaseFirstGetter => Some("caseFirst"),
        Builtin::IntlLocaleCollationGetter => Some("collation"),
        Builtin::IntlLocaleFirstDayOfWeekGetter => Some("firstDayOfWeek"),
        Builtin::IntlLocaleHourCycleGetter => Some("hourCycle"),
        Builtin::IntlLocaleLanguageGetter => Some("language"),
        Builtin::IntlLocaleNumberingSystemGetter => Some("numberingSystem"),
        Builtin::IntlLocaleNumericGetter => Some("numeric"),
        Builtin::IntlLocaleRegionGetter => Some("region"),
        Builtin::IntlLocaleScriptGetter => Some("script"),
        Builtin::IntlLocaleTextInfoGetter => Some("textInfo"),
        Builtin::IntlLocaleVariantsGetter => Some("variants"),
        Builtin::IntlDateTimeFormatSupportedLocalesOf => Some("supportedLocalesOf"),
        Builtin::IntlSegmenterSupportedLocalesOf => Some("supportedLocalesOf"),
        Builtin::IntlListFormatSupportedLocalesOf => Some("supportedLocalesOf"),
        _ => intl_short_name_formats(b),
    }
}

const fn intl_short_name_formats(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlNumberFormatFormat => Some(""),
        Builtin::IntlNumberFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlNumberFormatFormatRange => Some("formatRange"),
        Builtin::IntlNumberFormatFormatRangeToParts => Some("formatRangeToParts"),
        Builtin::IntlNumberFormatResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlPluralRulesSelect => Some("select"),
        Builtin::IntlPluralRulesResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlDateTimeFormatFormat => Some("format"),
        Builtin::IntlDateTimeFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlDateTimeFormatFormatRange => Some("formatRange"),
        Builtin::IntlDateTimeFormatFormatRangeToParts => Some("formatRangeToParts"),
        Builtin::IntlDateTimeFormatResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlCollatorCompare => Some("compare"),
        Builtin::IntlCollatorResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlListFormatFormat => Some("format"),
        Builtin::IntlListFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlListFormatResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlRelativeTimeFormatFormat => Some("format"),
        Builtin::IntlRelativeTimeFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlRelativeTimeFormatResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlSegmenterSegment => Some("segment"),
        Builtin::IntlSegmenterSegmentsIterator => Some("[Symbol.iterator]"),
        Builtin::IntlSegmenterSegmentsContaining => Some("containing"),
        Builtin::IntlSegmenterResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlDisplayNamesOf => Some("of"),
        Builtin::IntlDisplayNamesResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}
