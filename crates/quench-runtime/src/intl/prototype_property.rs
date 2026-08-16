fn prototype_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    if builtin == Builtin::IntlRelativeTimeFormatPrototype && key == "constructor" {
        return Some(Builtin::IntlRelativeTimeFormat);
    }
    prototype_locale_property(builtin, key)
}

fn prototype_locale_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    Some(match (builtin, key) {
        (Builtin::IntlLocalePrototype, "toString") => Builtin::IntlLocaleToString,
        (Builtin::IntlLocalePrototype, "maximize") => Builtin::IntlLocaleMaximize,
        (Builtin::IntlLocalePrototype, "minimize") => Builtin::IntlLocaleMinimize,
        (Builtin::IntlLocalePrototype, "getCalendars") => Builtin::IntlLocaleGetCalendars,
        (Builtin::IntlLocalePrototype, "getCollations") => Builtin::IntlLocaleGetCollations,
        (Builtin::IntlLocalePrototype, "getHourCycles") => Builtin::IntlLocaleGetHourCycles,
        (Builtin::IntlLocalePrototype, "getNumberingSystems") => {
            Builtin::IntlLocaleGetNumberingSystems
        }
        (Builtin::IntlLocalePrototype, "getTimeZones") => Builtin::IntlLocaleGetTimeZones,
        (Builtin::IntlLocalePrototype, "getTextInfo") => Builtin::IntlLocaleGetTextInfo,
        (Builtin::IntlLocalePrototype, "getWeekInfo") => Builtin::IntlLocaleGetWeekInfo,
        (Builtin::IntlLocalePrototype, "baseName") => Builtin::IntlLocaleBaseNameGetter,
        (Builtin::IntlLocalePrototype, "calendar") => Builtin::IntlLocaleCalendarGetter,
        (Builtin::IntlLocalePrototype, "caseFirst") => Builtin::IntlLocaleCaseFirstGetter,
        (Builtin::IntlLocalePrototype, "collation") => Builtin::IntlLocaleCollationGetter,
        (Builtin::IntlLocalePrototype, "firstDayOfWeek") => Builtin::IntlLocaleFirstDayOfWeekGetter,
        (Builtin::IntlLocalePrototype, "hourCycle") => Builtin::IntlLocaleHourCycleGetter,
        (Builtin::IntlLocalePrototype, "language") => Builtin::IntlLocaleLanguageGetter,
        (Builtin::IntlLocalePrototype, "numberingSystem") => {
            Builtin::IntlLocaleNumberingSystemGetter
        }
        (Builtin::IntlLocalePrototype, "numeric") => Builtin::IntlLocaleNumericGetter,
        (Builtin::IntlLocalePrototype, "region") => Builtin::IntlLocaleRegionGetter,
        (Builtin::IntlLocalePrototype, "script") => Builtin::IntlLocaleScriptGetter,
        (Builtin::IntlLocalePrototype, "textInfo") => Builtin::IntlLocaleTextInfoGetter,
        (Builtin::IntlLocalePrototype, "variants") => Builtin::IntlLocaleVariantsGetter,
        _ => return prototype_number_property(builtin, key),
    })
}

fn prototype_number_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    Some(match (builtin, key) {
        (Builtin::IntlNumberFormatPrototype, "format") => Builtin::IntlNumberFormatFormat,
        (Builtin::IntlNumberFormatPrototype, "formatToParts") => {
            Builtin::IntlNumberFormatFormatToParts
        }
        (Builtin::IntlNumberFormatPrototype, "formatRange") => Builtin::IntlNumberFormatFormatRange,
        (Builtin::IntlNumberFormatPrototype, "formatRangeToParts") => {
            Builtin::IntlNumberFormatFormatRangeToParts
        }
        (Builtin::IntlNumberFormatPrototype, "resolvedOptions") => {
            Builtin::IntlNumberFormatResolvedOptions
        }
        _ => return prototype_property_tail(builtin, key),
    })
}

fn prototype_property_tail(builtin: Builtin, key: &str) -> Option<Builtin> {
    Some(match (builtin, key) {
        (Builtin::IntlCollatorPrototype, "constructor") => Builtin::IntlCollator,
        (Builtin::IntlCollatorPrototype, "resolvedOptions") => Builtin::IntlCollatorResolvedOptions,
        (Builtin::IntlDateTimeFormatPrototype, "constructor") => Builtin::IntlDateTimeFormat,
        (Builtin::IntlSegmenterPrototype, "constructor") => Builtin::IntlSegmenter,
        (Builtin::IntlSegmenterPrototype, "segment") => Builtin::IntlSegmenterSegment,
        (Builtin::IntlSegmenterPrototype, "resolvedOptions") => {
            Builtin::IntlSegmenterResolvedOptions
        }
        (Builtin::IntlDateTimeFormatPrototype, "format") => Builtin::IntlDateTimeFormatFormat,
        (Builtin::IntlDateTimeFormatPrototype, "formatToParts") => {
            Builtin::IntlDateTimeFormatFormatToParts
        }
        (Builtin::IntlDateTimeFormatPrototype, "formatRange") => {
            Builtin::IntlDateTimeFormatFormatRange
        }
        (Builtin::IntlDateTimeFormatPrototype, "formatRangeToParts") => {
            Builtin::IntlDateTimeFormatFormatRangeToParts
        }
        (Builtin::IntlDateTimeFormatPrototype, "resolvedOptions") => {
            Builtin::IntlDateTimeFormatResolvedOptions
        }
        (Builtin::IntlPluralRulesPrototype, "select") => Builtin::IntlPluralRulesSelect,
        (Builtin::IntlPluralRulesPrototype, "constructor") => Builtin::IntlPluralRules,
        (Builtin::IntlPluralRulesPrototype, "resolvedOptions") => {
            Builtin::IntlPluralRulesResolvedOptions
        }
        (Builtin::IntlRelativeTimeFormatPrototype, "format") => {
            Builtin::IntlRelativeTimeFormatFormat
        }
        (Builtin::IntlRelativeTimeFormatPrototype, "formatToParts") => {
            Builtin::IntlRelativeTimeFormatFormatToParts
        }
        (Builtin::IntlRelativeTimeFormatPrototype, "resolvedOptions") => {
            Builtin::IntlRelativeTimeFormatResolvedOptions
        }
        (Builtin::IntlListFormatPrototype, "constructor") => Builtin::IntlListFormat,
        (Builtin::IntlListFormatPrototype, "format") => Builtin::IntlListFormatFormat,
        (Builtin::IntlListFormatPrototype, "formatToParts") => Builtin::IntlListFormatFormatToParts,
        (Builtin::IntlListFormatPrototype, "resolvedOptions") => {
            Builtin::IntlListFormatResolvedOptions
        }
        (Builtin::IntlDisplayNamesPrototype, "of") => Builtin::IntlDisplayNamesOf,
        (Builtin::IntlDisplayNamesPrototype, "resolvedOptions") => {
            Builtin::IntlDisplayNamesResolvedOptions
        }
        _ => return None,
    })
}
