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
        Builtin::IntlNumberFormatFormat => Some(""),
        Builtin::IntlNumberFormatFormatToParts => Some("Intl.NumberFormat.prototype.formatToParts"),
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
        Builtin::IntlDateTimeFormatResolvedOptions => {
            Some("Intl.DateTimeFormat.prototype.resolvedOptions")
        }
        _ => intl_name_group_b(b),
    }
}

const fn intl_name_group_b(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlCollatorCompare => Some("Intl.Collator.prototype.compare"),
        Builtin::IntlCollatorResolvedOptions => Some("Intl.Collator.prototype.resolvedOptions"),
        Builtin::IntlListFormatFormat => Some("Intl.ListFormat.prototype.format"),
        Builtin::IntlListFormatFormatToParts => Some("Intl.ListFormat.prototype.formatToParts"),
        Builtin::IntlListFormatResolvedOptions => Some("Intl.ListFormat.prototype.resolvedOptions"),
        Builtin::IntlRelativeTimeFormatFormat => Some("Intl.RelativeTimeFormat.prototype.format"),
        Builtin::IntlRelativeTimeFormatFormatToParts => {
            Some("Intl.RelativeTimeFormat.prototype.formatToParts")
        }
        Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some("Intl.RelativeTimeFormat.prototype.resolvedOptions")
        }
        Builtin::IntlSegmenterSegment => Some("Intl.Segmenter.prototype.segment"),
        Builtin::IntlSegmenterResolvedOptions => Some("Intl.Segmenter.prototype.resolvedOptions"),
        Builtin::IntlDisplayNamesOf => Some("Intl.DisplayNames.prototype.of"),
        Builtin::IntlDisplayNamesResolvedOptions => {
            Some("Intl.DisplayNames.prototype.resolvedOptions")
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
        Builtin::IntlNumberFormatFormat => Some(1.0),
        Builtin::IntlNumberFormatResolvedOptions
        | Builtin::IntlPluralRulesSelect
        | Builtin::IntlPluralRulesResolvedOptions
        | Builtin::IntlDateTimeFormatFormat
        | Builtin::IntlDateTimeFormatFormatToParts
        | Builtin::IntlDateTimeFormatResolvedOptions
        | Builtin::IntlCollatorCompare
        | Builtin::IntlCollatorResolvedOptions
        | Builtin::IntlSegmenterSegment
        | Builtin::IntlSegmenterResolvedOptions
        | Builtin::IntlDisplayNamesOf
        | Builtin::IntlDisplayNamesResolvedOptions => Some(0.0),
        Builtin::IntlNumberFormatFormatToParts => Some(1.0),
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
        Builtin::IntlNumberFormatFormat => Some(""),
        Builtin::IntlNumberFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlNumberFormatResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlPluralRulesSelect => Some("select"),
        Builtin::IntlPluralRulesResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlDateTimeFormatFormat => Some("format"),
        Builtin::IntlDateTimeFormatFormatToParts => Some("formatToParts"),
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
        Builtin::IntlSegmenterResolvedOptions => Some("resolvedOptions"),
        Builtin::IntlDisplayNamesOf => Some("of"),
        Builtin::IntlDisplayNamesResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}
