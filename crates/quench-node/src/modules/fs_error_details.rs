//! Runtime-neutral Node filesystem error data shared by both VM adapters.

use crate::modules::fs_error_codes::{code_for, strerror};

pub(crate) struct FsErrorDetails {
    pub code: &'static str,
    pub errno: i32,
    pub syscall: String,
    pub path: Option<String>,
    pub message: String,
}

pub(crate) fn error_details(
    syscall: &str,
    path: Option<&str>,
    error: &std::io::Error,
) -> FsErrorDetails {
    let (code, errno) = code_for(error);
    let message = match path {
        Some(path) => format!("{code}: {}, {syscall} '{path}'", strerror(code)),
        None => format!("{code}: {}, {syscall}", strerror(code)),
    };
    FsErrorDetails {
        code,
        errno,
        syscall: syscall.to_owned(),
        path: path.map(str::to_owned),
        message,
    }
}
