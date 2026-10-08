//! Runtime-neutral Node filesystem error data shared by both VM adapters.

use crate::modules::fs_error_codes::{code_for, strerror};
use crate::modules::fs_ops::OperationError;

pub(crate) struct FsErrorDetails {
    pub name: Option<&'static str>,
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
        name: None,
        code,
        errno,
        syscall: syscall.to_owned(),
        path: path.map(str::to_owned),
        message,
    }
}

pub(crate) fn operation_error_details(error: &OperationError) -> FsErrorDetails {
    match error {
        OperationError::Io {
            syscall,
            path,
            error,
        } => error_details(syscall, Some(path), error),
        OperationError::DirectoryRequiresRecursive { path } => FsErrorDetails {
            name: Some("SystemError"),
            code: "ERR_FS_EISDIR",
            errno: libc::EISDIR,
            syscall: "rm".to_owned(),
            path: Some(path.clone()),
            message: format!("Path is a directory: rm returned EISDIR (is a directory) {path}"),
        },
    }
}
