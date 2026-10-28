pub(crate) const EXIT_CODES: &[(&str, i32)] = &[
    ("kNoFailure", 0),
    ("kGenericUserError", 1),
    ("kInternalJSParseError", 3),
    ("kInternalJSEvaluationFailure", 4),
    ("kV8FatalError", 5),
    ("kInvalidFatalExceptionMonkeyPatching", 6),
    ("kExceptionInFatalExceptionHandler", 7),
    ("kInvalidCommandLineArgument", 9),
    ("kBootstrapFailure", 10),
    ("kInvalidCommandLineArgument2", 12),
    ("kUnsettledTopLevelAwait", 13),
    ("kStartupSnapshotFailure", 14),
    ("kProcessTimeout", 124),
    ("kAbort", 134),
];
pub(crate) const COMPILE_CACHE_STATUS: &[&str] = &[
    "FAILED",
    "ENABLED",
    "ALREADY_ENABLED",
    "DISABLED",
];
