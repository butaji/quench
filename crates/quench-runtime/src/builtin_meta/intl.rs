//! Intl builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    intl_static_name(b)
        .or_else(|| intl_locale_name(b))
        .or_else(|| intl_number_format_name(b))
        .or_else(|| intl_plural_rules_name(b))
        .or_else(|| intl_date_time_format_name(b))
        .or_else(|| intl_collator_name(b))
        .or_else(|| intl_list_format_name(b))
        .or_else(|| intl_relative_time_format_name(b))
        .or_else(|| intl_segmenter_name(b))
        .or_else(|| intl_display_names_name(b))
}

const fn intl_static_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlGetCanonicalLocales => Some("Intl.getCanonicalLocales"),
        Builtin::IntlSupportedValuesOf => Some("Intl.supportedValuesOf"),
        _ => None,
    }
}

const fn intl_locale_name(b: Builtin) -> Option<&'static str> {
    match b {
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
        _ => None,
    }
}

const fn intl_number_format_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlNumberFormatFormat => Some("Intl.NumberFormat.prototype.format"),
        Builtin::IntlNumberFormatFormatToParts => Some("Intl.NumberFormat.prototype.formatToParts"),
        Builtin::IntlNumberFormatResolvedOptions => {
            Some("Intl.NumberFormat.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_plural_rules_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlPluralRulesSelect => Some("Intl.PluralRules.prototype.select"),
        Builtin::IntlPluralRulesResolvedOptions => {
            Some("Intl.PluralRules.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_date_time_format_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlDateTimeFormatFormat => Some("Intl.DateTimeFormat.prototype.format"),
        Builtin::IntlDateTimeFormatFormatToParts => {
            Some("Intl.DateTimeFormat.prototype.formatToParts")
        }
        Builtin::IntlDateTimeFormatResolvedOptions => {
            Some("Intl.DateTimeFormat.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_collator_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlCollatorCompare => Some("Intl.Collator.prototype.compare"),
        Builtin::IntlCollatorResolvedOptions => Some("Intl.Collator.prototype.resolvedOptions"),
        _ => None,
    }
}

const fn intl_list_format_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlListFormatFormat => Some("Intl.ListFormat.prototype.format"),
        Builtin::IntlListFormatFormatToParts => Some("Intl.ListFormat.prototype.formatToParts"),
        Builtin::IntlListFormatResolvedOptions => {
            Some("Intl.ListFormat.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_relative_time_format_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlRelativeTimeFormatFormat => {
            Some("Intl.RelativeTimeFormat.prototype.format")
        }
        Builtin::IntlRelativeTimeFormatFormatToParts => {
            Some("Intl.RelativeTimeFormat.prototype.formatToParts")
        }
        Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some("Intl.RelativeTimeFormat.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_segmenter_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlSegmenterSegment => Some("Intl.Segmenter.prototype.segment"),
        Builtin::IntlSegmenterResolvedOptions => {
            Some("Intl.Segmenter.prototype.resolvedOptions")
        }
        _ => None,
    }
}

const fn intl_display_names_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlDisplayNamesOf => Some("Intl.DisplayNames.prototype.of"),
        Builtin::IntlDisplayNamesResolvedOptions => {
            Some("Intl.DisplayNames.prototype.resolvedOptions")
        }
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
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
        Builtin::IntlNumberFormatFormat
        | Builtin::IntlNumberFormatFormatToParts
        | Builtin::IntlNumberFormatResolvedOptions
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
        Builtin::IntlListFormatFormat
        | Builtin::IntlListFormatFormatToParts
        | Builtin::IntlListFormatResolvedOptions
        | Builtin::IntlRelativeTimeFormatFormat
        | Builtin::IntlRelativeTimeFormatFormatToParts
        | Builtin::IntlRelativeTimeFormatResolvedOptions => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    intl_locale_short(b)
        .or_else(|| intl_number_format_short(b))
        .or_else(|| intl_plural_rules_short(b))
        .or_else(|| intl_date_time_format_short(b))
        .or_else(|| intl_collator_short(b))
        .or_else(|| intl_list_format_short(b))
        .or_else(|| intl_relative_time_format_short(b))
        .or_else(|| intl_segmenter_short(b))
        .or_else(|| intl_display_names_short(b))
}

const fn intl_locale_short(b: Builtin) -> Option<&'static str> {
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
        _ => None,
    }
}

const fn intl_number_format_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlNumberFormatFormat => Some("format"),
        Builtin::IntlNumberFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlNumberFormatResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_plural_rules_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlPluralRulesSelect => Some("select"),
        Builtin::IntlPluralRulesResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_date_time_format_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlDateTimeFormatFormat => Some("format"),
        Builtin::IntlDateTimeFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlDateTimeFormatResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_collator_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlCollatorCompare => Some("compare"),
        Builtin::IntlCollatorResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_list_format_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlListFormatFormat => Some("format"),
        Builtin::IntlListFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlListFormatResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_relative_time_format_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlRelativeTimeFormatFormat => Some("format"),
        Builtin::IntlRelativeTimeFormatFormatToParts => Some("formatToParts"),
        Builtin::IntlRelativeTimeFormatResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_segmenter_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlSegmenterSegment => Some("segment"),
        Builtin::IntlSegmenterResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}

const fn intl_display_names_short(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::IntlDisplayNamesOf => Some("of"),
        Builtin::IntlDisplayNamesResolvedOptions => Some("resolvedOptions"),
        _ => None,
    }
}
