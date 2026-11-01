#[repr(i32)]
pub(crate) enum ExitCode {
    NoFailure = 0,
    GenericUserError = 1,
    InternalJSParseError = 3,
    InternalJSEvaluationFailure = 4,
    V8FatalError = 5,
    InvalidFatalExceptionMonkeyPatching = 6,
    ExceptionInFatalExceptionHandler = 7,
    InvalidCommandLineArgument = 9,
    BootstrapFailure = 10,
    InvalidCommandLineArgument2 = 12,
    UnsettledTopLevelAwait = 13,
    StartupSnapshotFailure = 14,
    ProcessTimeout = 124,
    Abort = 134,
}
pub(crate) const EXIT_CODES: &[(&str, i32)] = &[
    ("kNoFailure", ExitCode::NoFailure as i32),
    ("kGenericUserError", ExitCode::GenericUserError as i32),
    ("kInternalJSParseError", ExitCode::InternalJSParseError as i32),
    ("kInternalJSEvaluationFailure", ExitCode::InternalJSEvaluationFailure as i32),
    ("kV8FatalError", ExitCode::V8FatalError as i32),
    ("kInvalidFatalExceptionMonkeyPatching", ExitCode::InvalidFatalExceptionMonkeyPatching as i32),
    ("kExceptionInFatalExceptionHandler", ExitCode::ExceptionInFatalExceptionHandler as i32),
    ("kInvalidCommandLineArgument", ExitCode::InvalidCommandLineArgument as i32),
    ("kBootstrapFailure", ExitCode::BootstrapFailure as i32),
    ("kInvalidCommandLineArgument2", ExitCode::InvalidCommandLineArgument2 as i32),
    ("kUnsettledTopLevelAwait", ExitCode::UnsettledTopLevelAwait as i32),
    ("kStartupSnapshotFailure", ExitCode::StartupSnapshotFailure as i32),
    ("kProcessTimeout", ExitCode::ProcessTimeout as i32),
    ("kAbort", ExitCode::Abort as i32),
];
pub(crate) const COMPILE_CACHE_STATUS: &[&str] = &[
    "FAILED",
    "ENABLED",
    "ALREADY_ENABLED",
    "DISABLED",
];
pub(crate) const CACHED_CODE_TYPES: &[(&str, u32)] = &[
    ("kCommonJS", 0),
    ("kESM", 1),
    ("kStrippedTypeScript", 2),
];
