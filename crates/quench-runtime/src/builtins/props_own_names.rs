pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::BigInt => &["length", "name", "prototype", "asIntN", "asUintN"],
        Builtin::BigIntPrototype => {
            &["constructor", "toString", "valueOf", "Symbol.toStringTag"]
        }
        Builtin::Error => &[
            "length",
            "name",
            "prototype",
            "isError",
        ],
        Builtin::ErrorPrototype => {
            &["constructor", "name", "message", "cause", "stack", "toString"]
        }
        Builtin::SuppressedErrorPrototype => &["constructor", "name", "message", "toString"],
        Builtin::DisposableStack => &["length", "name", "prototype"],
        Builtin::DisposableStackPrototype => &[
            "constructor", "use", "adopt", "defer", "move", "dispose", "disposed",
            "Symbol.dispose", "Symbol.toStringTag",
        ],
        Builtin::AsyncDisposableStack => &["length", "name", "prototype"],
        Builtin::FinalizationRegistry => &["length", "name", "prototype"],
        Builtin::FinalizationRegistryPrototype => &[
            "constructor", "register", "unregister", "Symbol.toStringTag",
        ],
        Builtin::AsyncDisposableStackPrototype => &[
            "constructor", "use", "adopt", "defer", "move", "disposeAsync", "disposed",
            "Symbol.asyncDispose", "Symbol.toStringTag",
        ],
        Builtin::Math => &[
            "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt",
            "ceil", "clz32", "cos", "cosh", "exp", "expm1", "f16round", "floor", "fround",
            "hypot", "imul", "log", "log1p", "log2", "log10", "max", "min", "pow", "random",
            "round", "sign", "sin", "sinh", "sqrt", "sumPrecise", "tan", "tanh", "trunc",
            "E", "LN2", "LN10", "LOG2E", "LOG10E", "PI", "SQRT1_2", "SQRT2", "Symbol.toStringTag",
        ],
        Builtin::Reflect => &[
            "apply", "construct", "defineProperty", "deleteProperty", "get",
            "getOwnPropertyDescriptor", "getPrototypeOf", "has", "isExtensible", "ownKeys",
            "preventExtensions", "set", "setPrototypeOf", "Symbol.toStringTag",
        ],
        Builtin::Intl => &[
            "getCanonicalLocales", "supportedValuesOf", "Collator", "DateTimeFormat",
            "DisplayNames", "ListFormat", "Locale", "NumberFormat", "PluralRules",
            "RelativeTimeFormat", "Segmenter", "Symbol.toStringTag",
        ],
        Builtin::IntlCollator
        | Builtin::IntlDateTimeFormat
        | Builtin::IntlDisplayNames
        | Builtin::IntlListFormat
        | Builtin::IntlLocale
        | Builtin::IntlNumberFormat
        | Builtin::IntlPluralRules
        | Builtin::IntlRelativeTimeFormat
        | Builtin::IntlSegmenter => &["length", "name", "prototype"],
        Builtin::IntlCollatorPrototype => &["constructor", "compare", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlDateTimeFormatPrototype => &["constructor", "format", "formatToParts", "formatRange", "formatRangeToParts", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlDisplayNamesPrototype => &["constructor", "of", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlListFormatPrototype => &["constructor", "format", "formatToParts", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlLocalePrototype => &["constructor", "toString", "maximize", "minimize", "baseName", "calendar", "caseFirst", "collation", "firstDayOfWeek", "hourCycle", "language", "numberingSystem", "numeric", "region", "script", "textInfo", "variants", "getCalendars", "getCollations", "getHourCycles", "getNumberingSystems", "getTimeZones", "getTextInfo", "getWeekInfo", "Symbol.toStringTag"],
        Builtin::IntlNumberFormatPrototype => &["constructor", "format", "formatToParts", "formatRange", "formatRangeToParts", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlPluralRulesPrototype => &["constructor", "select", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlRelativeTimeFormatPrototype => &["constructor", "format", "formatToParts", "resolvedOptions", "Symbol.toStringTag"],
        Builtin::IntlSegmenterPrototype => &["constructor", "segment", "resolvedOptions", "Symbol.toStringTag"],
        _ => &[],
    }
}
