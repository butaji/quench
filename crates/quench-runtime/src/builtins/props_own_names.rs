pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::BigInt => &["length", "name", "prototype", "asIntN", "asUintN"],
        Builtin::Temporal => &[
            "Instant", "PlainDate", "PlainTime", "PlainDateTime", "ZonedDateTime",
            "PlainYearMonth", "PlainMonthDay", "Duration", "Now", "Symbol.toStringTag",
        ],
        Builtin::TemporalDuration => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalDurationPrototype => &[
            "constructor", "years", "months", "weeks", "days", "hours", "minutes",
            "seconds", "milliseconds", "microseconds", "nanoseconds", "sign", "blank",
            "toString", "toJSON", "valueOf",
        ],
        Builtin::TemporalPlainDate => &["length", "name", "prototype", "from"],
        Builtin::TemporalPlainDatePrototype => &[
            "constructor", "calendarId", "era", "eraYear", "year", "month", "monthCode",
            "day", "dayOfWeek", "dayOfYear", "weekOfYear", "daysInWeek", "daysInMonth",
            "daysInYear", "monthsInYear", "inLeapYear", "with", "withCalendar", "add",
            "subtract", "until", "since", "equals", "toString", "toJSON", "toLocaleString",
            "valueOf",
        ],
        Builtin::TemporalPlainTime => &["length", "name", "prototype", "from", "compare"],
        Builtin::TemporalPlainTimePrototype => &[
            "constructor", "hour", "minute", "second", "millisecond", "microsecond", "nanosecond",
            "toString", "toJSON", "valueOf",
        ],
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
        _ => &[],
    }
}
