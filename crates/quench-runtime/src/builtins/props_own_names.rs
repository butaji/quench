const TEMPORAL_DURATION_PROTOTYPE_NAMES: &[&str] = &[
    "constructor",
    "Symbol.toStringTag",
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
    "sign",
    "blank",
    "abs",
    "negated",
    "round",
    "total",
    "add",
    "subtract",
    "toString",
    "toJSON",
    "valueOf",
];

const TEMPORAL_PLAIN_DATE_PROTOTYPE_NAMES: &[&str] = &[
    "constructor",
    "calendarId",
    "era",
    "eraYear",
    "year",
    "month",
    "monthCode",
    "day",
    "dayOfWeek",
    "dayOfYear",
    "weekOfYear",
    "daysInWeek",
    "daysInMonth",
    "daysInYear",
    "monthsInYear",
    "inLeapYear",
    "toString",
    "toJSON",
    "toLocaleString",
    "equals",
    "with",
    "add",
    "subtract",
    "until",
    "since",
    "toPlainDateTime",
    "toPlainMonthDay",
    "toPlainYearMonth",
    "toZonedDateTime",
    "toLocaleString",
    "valueOf",
];

const MATH_NAMES: &[&str] = &[
    "abs",
    "acos",
    "acosh",
    "asin",
    "asinh",
    "atan",
    "atan2",
    "atanh",
    "cbrt",
    "ceil",
    "clz32",
    "cos",
    "cosh",
    "exp",
    "expm1",
    "f16round",
    "floor",
    "fround",
    "hypot",
    "imul",
    "log",
    "log1p",
    "log2",
    "log10",
    "max",
    "min",
    "pow",
    "random",
    "round",
    "sign",
    "sin",
    "sinh",
    "sqrt",
    "sumPrecise",
    "tan",
    "tanh",
    "trunc",
    "E",
    "LN2",
    "LN10",
    "LOG2E",
    "LOG10E",
    "PI",
    "SQRT1_2",
    "SQRT2",
    "Symbol.toStringTag",
];

const REFLECT_NAMES: &[&str] = &[
    "apply",
    "construct",
    "defineProperty",
    "deleteProperty",
    "get",
    "getOwnPropertyDescriptor",
    "getPrototypeOf",
    "has",
    "isExtensible",
    "ownKeys",
    "preventExtensions",
    "set",
    "setPrototypeOf",
    "Symbol.toStringTag",
];

pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::Temporal
        | Builtin::TemporalInstant
        | Builtin::TemporalInstantPrototype
        | Builtin::TemporalDuration
        | Builtin::TemporalDurationPrototype
        | Builtin::TemporalPlainDate
        | Builtin::TemporalPlainDatePrototype
        | Builtin::TemporalPlainDateTime
        | Builtin::TemporalPlainDateTimePrototype
        | Builtin::TemporalPlainTime
        | Builtin::TemporalPlainTimePrototype
        | Builtin::TemporalPlainMonthDay
        | Builtin::TemporalPlainMonthDayPrototype
        | Builtin::TemporalPlainYearMonth
        | Builtin::TemporalPlainYearMonthPrototype
        | Builtin::TemporalZonedDateTime
        | Builtin::TemporalZonedDateTimePrototype
        | Builtin::TemporalNow => own_property_names_temporal(builtin),
        _ => own_property_names_standard(builtin),
    }
}

fn own_property_names_temporal(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::Temporal => &["Duration", "Instant", "ZonedDateTime", "PlainDate", "PlainDateTime", "PlainTime", "PlainMonthDay", "PlainYearMonth", "Now", "Symbol.toStringTag"],
        Builtin::TemporalDuration => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalDurationPrototype => TEMPORAL_DURATION_PROTOTYPE_NAMES,
        Builtin::TemporalInstant => &["length", "name", "prototype", "from"],
        Builtin::TemporalInstantPrototype => &["constructor", "epochNanoseconds", "toString", "toJSON", "toLocaleString", "toZonedDateTimeISO", "equals", "add", "subtract", "until", "since", "round"],
        Builtin::TemporalPlainDate => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainDatePrototype => TEMPORAL_PLAIN_DATE_PROTOTYPE_NAMES,
        Builtin::TemporalPlainDateTime => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainDateTimePrototype => &["constructor", "calendarId", "era", "eraYear", "year", "month", "monthCode", "day", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "toString", "toJSON", "toLocaleString", "equals", "valueOf", "add", "subtract", "with", "round", "until", "since", "toPlainDate", "toPlainTime", "toZonedDateTime", "withCalendar", "withPlainTime"],
        Builtin::TemporalPlainTime => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainTimePrototype => &["constructor", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "toString", "toJSON", "toLocaleString", "equals", "valueOf", "add", "subtract", "with", "round", "until", "since"],
        Builtin::TemporalPlainMonthDay => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainMonthDayPrototype => &["constructor", "calendarId", "monthCode", "day", "toString", "toJSON", "toLocaleString", "equals", "with", "toPlainDate", "valueOf"],
        Builtin::TemporalPlainYearMonth => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainYearMonthPrototype => &["constructor", "calendarId", "year", "month", "monthCode", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "era", "eraYear", "toString", "toJSON", "toLocaleString", "equals", "with", "add", "subtract", "until", "since", "toPlainDate", "valueOf"],
        Builtin::TemporalZonedDateTime => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalZonedDateTimePrototype => &["constructor", "epochNanoseconds", "epochMilliseconds", "timeZoneId", "calendarId", "year", "month", "monthCode", "day", "dayOfWeek", "dayOfYear", "weekOfYear", "yearOfWeek", "daysInWeek", "daysInMonth", "daysInYear", "monthsInYear", "inLeapYear", "hoursInDay", "offset", "offsetNanoseconds", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond", "toString", "toJSON", "toLocaleString", "toInstant", "toPlainDateTime", "toPlainDate", "toPlainTime", "equals", "valueOf"],
        Builtin::TemporalNow => &["instant", "plainDateISO", "plainDateTimeISO", "plainTimeISO", "timeZoneId", "zonedDateTimeISO", "Symbol.toStringTag"],
        _ => &[],
    }
}

fn own_property_names_standard(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::AbstractModuleSource
        | Builtin::AbstractModuleSourcePrototype
        | Builtin::ShadowRealm
        | Builtin::ShadowRealmPrototype
        | Builtin::BigInt
        | Builtin::BigIntPrototype
        | Builtin::Function
        | Builtin::FunctionPrototype
        | Builtin::Error
        | Builtin::Promise
        | Builtin::ThrowTypeError => own_property_names_standard_core(builtin),
        _ => own_property_names_standard_tail(builtin),
    }
}

fn own_property_names_standard_core(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::Function => &["length", "name", "prototype"],
        Builtin::AbstractModuleSource => &["length", "name", "prototype"],
        Builtin::AbstractModuleSourcePrototype => &["constructor", "Symbol.toStringTag"],
        Builtin::ShadowRealm => &["length", "name", "prototype"],
        Builtin::ShadowRealmPrototype => &[
            "constructor",
            "evaluate",
            "importValue",
            "Symbol.toStringTag",
        ],
        Builtin::BigInt => &["length", "name", "prototype", "asIntN", "asUintN"],
        Builtin::BigIntPrototype => &["constructor", "toString", "valueOf", "Symbol.toStringTag"],
        Builtin::FunctionPrototype => &[
            "length",
            "name",
            "arguments",
            "caller",
            "constructor",
            "apply",
            "bind",
            "call",
            "toString",
            "Symbol.hasInstance",
        ],
        Builtin::AsyncFunctionPrototype => &["constructor", "Symbol.toStringTag"],
        Builtin::GeneratorFunctionPrototype => &["constructor", "prototype", "Symbol.toStringTag"],
        Builtin::AsyncGeneratorFunctionPrototype => {
            &["constructor", "prototype", "Symbol.toStringTag"]
        }
        Builtin::Error => &["length", "name", "prototype", "isError"],
        Builtin::Promise => &[
            "length",
            "name",
            "prototype",
            "resolve",
            "reject",
            "all",
            "allSettled",
            "any",
            "race",
            "withResolvers",
            "try",
        ],
        Builtin::ThrowTypeError => &["length", "name"],
        _ => &[],
     }
 }

fn own_property_names_standard_tail(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::ArrayBufferPrototype => &[
            "constructor",
            "byteLength",
            "detached",
            "immutable",
            "maxByteLength",
            "resizable",
            "slice",
            "resize",
            "transferToImmutable",
            "sliceToImmutable",
            "Symbol.toStringTag",
        ],
        Builtin::SharedArrayBufferPrototype => &[
            "constructor",
            "byteLength",
            "slice",
            "grow",
            "growable",
            "maxByteLength",
            "Symbol.toStringTag",
        ],
        Builtin::ErrorPrototype => &["constructor", "name", "message", "stack", "toString"],
        Builtin::RangeErrorPrototype
        | Builtin::TypeErrorPrototype
        | Builtin::EvalErrorPrototype
        | Builtin::ReferenceErrorPrototype
        | Builtin::SyntaxErrorPrototype
        | Builtin::URIErrorPrototype => &["constructor", "name", "message"],
        Builtin::AggregateErrorPrototype => &["constructor", "name", "message"],
        Builtin::SuppressedErrorPrototype => &["constructor", "name", "message", "toString"],
        Builtin::SymbolPrototype => &[
            "constructor",
            "toString",
            "valueOf",
            "description",
            "Symbol.toStringTag",
            "Symbol.toPrimitive",
        ],
        _ => own_property_names_tail(builtin),
    }
}

fn own_property_names_tail(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::RegExp => &["length", "name", "prototype"],
        Builtin::RegExpPrototype => &[
            "constructor",
            "exec",
            "compile",
            "dotAll",
            "flags",
            "global",
            "hasIndices",
            "ignoreCase",
            "multiline",
            "source",
            "sticky",
            "unicode",
            "unicodeSets",
            "test",
            "toString",
            "Symbol.match",
            "Symbol.matchAll",
            "Symbol.replace",
            "Symbol.search",
            "Symbol.split",
        ],
        Builtin::Intl => &[
            "getCanonicalLocales",
            "supportedValuesOf",
            "DateTimeFormat",
            "NumberFormat",
            "Collator",
            "PluralRules",
            "RelativeTimeFormat",
            "ListFormat",
            "Locale",
            "DisplayNames",
            "Segmenter",
            "DurationFormat",
            "Symbol.toStringTag",
        ],
        Builtin::IntlCollator => &["length", "name", "prototype", "supportedLocalesOf"],
        Builtin::IntlCollatorPrototype => &[
            "constructor",
            "resolvedOptions",
            "compare",
            "Symbol.toStringTag",
        ],
        Builtin::IntlDateTimeFormat
        | Builtin::IntlDisplayNames
        | Builtin::IntlListFormat
        | Builtin::IntlNumberFormat
        | Builtin::IntlPluralRules
        | Builtin::IntlRelativeTimeFormat
        | Builtin::IntlSegmenter => &["length", "name", "prototype", "supportedLocalesOf"],
        Builtin::IntlDateTimeFormatPrototype => &[
            "constructor",
            "resolvedOptions",
            "formatToParts",
            "format",
            "formatRange",
            "formatRangeToParts",
            "Symbol.toStringTag",
        ],
        Builtin::IntlDisplayNamesPrototype => {
            &["constructor", "resolvedOptions", "of", "Symbol.toStringTag"]
        }
        Builtin::IntlListFormatPrototype => &[
            "constructor",
            "resolvedOptions",
            "format",
            "formatToParts",
            "Symbol.toStringTag",
        ],
        Builtin::IntlNumberFormatPrototype => &[
            "constructor",
            "resolvedOptions",
            "formatToParts",
            "format",
            "formatRange",
            "formatRangeToParts",
            "Symbol.toStringTag",
        ],
        Builtin::IntlLocale => &["length", "name", "prototype"],
        Builtin::IntlLocalePrototype => &[
            "constructor",
            "toString",
            "maximize",
            "minimize",
            "language",
            "script",
            "region",
            "baseName",
            "calendar",
            "caseFirst",
            "collation",
            "firstDayOfWeek",
            "hourCycle",
            "numeric",
            "numberingSystem",
            "getCalendars",
            "getCollations",
            "getHourCycles",
            "getNumberingSystems",
            "getTimeZones",
            "getTextInfo",
            "getWeekInfo",
            "variants",
            "Symbol.toStringTag",
        ],
        Builtin::IntlPluralRulesPrototype => &[
            "constructor",
            "resolvedOptions",
            "select",
            "selectRange",
            "Symbol.toStringTag",
        ],
        Builtin::IntlRelativeTimeFormatPrototype => &[
            "constructor",
            "resolvedOptions",
            "format",
            "formatToParts",
            "Symbol.toStringTag",
        ],
        Builtin::IntlSegmenterPrototype => &[
            "constructor",
            "resolvedOptions",
            "segment",
            "Symbol.toStringTag",
        ],
        Builtin::IntlDurationFormat => &["length", "name", "prototype", "supportedLocalesOf"],
        Builtin::IntlDurationFormatPrototype => &[
            "constructor",
            "resolvedOptions",
            "format",
            "formatToParts",
            "Symbol.toStringTag",
        ],
        Builtin::DisposableStack => &["length", "name", "prototype"],
        Builtin::DisposableStackPrototype => &[
            "constructor",
            "use",
            "adopt",
            "defer",
            "move",
            "dispose",
            "disposed",
            "Symbol.dispose",
            "Symbol.toStringTag",
        ],
        Builtin::AsyncDisposableStack => &["length", "name", "prototype"],
        Builtin::FinalizationRegistry | Builtin::FinalizationRegistryPrototype => {
            own_property_names_tail_registry(builtin)
        }
        Builtin::AsyncDisposableStackPrototype => &[
            "constructor",
            "use",
            "adopt",
            "defer",
            "move",
            "disposeAsync",
            "disposed",
            "Symbol.asyncDispose",
            "Symbol.toStringTag",
        ],
        _ => own_property_names_tail_end(builtin),
    }
}

fn own_property_names_tail_registry(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::FinalizationRegistry => &["length", "name", "prototype"],
        Builtin::FinalizationRegistryPrototype => &[
            "constructor",
            "register",
            "unregister",
            "Symbol.toStringTag",
        ],
        _ => &[],
    }
}

fn own_property_names_tail_end(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::ArrayPrototype => &["length", "Symbol.unscopables"],
        Builtin::ArrayIteratorPrototype => &["next", "constructor", "Symbol.toStringTag"],
        Builtin::Math => MATH_NAMES,
        Builtin::Reflect => REFLECT_NAMES,
        _ => &[],
    }
}
