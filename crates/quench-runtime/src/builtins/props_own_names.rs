const TEMPORAL_DURATION_PROTOTYPE_NAMES: &[&str] = &[
    "constructor",
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
        | Builtin::TemporalDuration
        | Builtin::TemporalDurationPrototype
        | Builtin::TemporalPlainDate
        | Builtin::TemporalPlainDatePrototype => own_property_names_temporal(builtin),
        _ => own_property_names_standard(builtin),
    }
}

fn own_property_names_temporal(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::Temporal => &["Duration", "PlainDate", "Symbol.toStringTag"],
        Builtin::TemporalDuration => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalDurationPrototype => TEMPORAL_DURATION_PROTOTYPE_NAMES,
        Builtin::TemporalPlainDate => &["length", "name", "prototype", "from"],
        Builtin::TemporalPlainDatePrototype => TEMPORAL_PLAIN_DATE_PROTOTYPE_NAMES,
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
        | Builtin::Error
        | Builtin::ThrowTypeError => own_property_names_standard_core(builtin),
        _ => own_property_names_standard_tail(builtin),
    }
}

fn own_property_names_standard_core(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
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
        Builtin::AsyncFunctionPrototype => &["constructor", "Symbol.toStringTag"],
        Builtin::GeneratorFunctionPrototype => &["constructor", "prototype", "Symbol.toStringTag"],
        Builtin::AsyncGeneratorFunctionPrototype => {
            &["constructor", "prototype", "Symbol.toStringTag"]
        }
        Builtin::Error => &["length", "name", "prototype", "isError"],
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
        _ => own_property_names_tail(builtin),
    }
}

fn own_property_names_tail(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::IntlDisplayNamesPrototype => &["Symbol.toStringTag"],
        Builtin::IntlListFormatPrototype => &["Symbol.toStringTag"],
        Builtin::IntlNumberFormatPrototype => &["Symbol.toStringTag"],
        Builtin::IntlLocalePrototype => &["Symbol.toStringTag"],
        Builtin::IntlPluralRulesPrototype => &["Symbol.toStringTag"],
        Builtin::IntlRelativeTimeFormatPrototype => &["Symbol.toStringTag"],
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
        Builtin::ArrayIteratorPrototype => &["next", "constructor", "Symbol.toStringTag"],
        Builtin::Math => MATH_NAMES,
        Builtin::Reflect => REFLECT_NAMES,
        _ => &[],
    }
}
