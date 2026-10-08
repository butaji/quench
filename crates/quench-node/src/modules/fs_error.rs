//! Coded `fs` errors — real `Error` instances with Node's `code`,
//! `errno`, `syscall`, and `path` properties.

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

/// One Node-style error record shared by the legacy and shared VM adapters.
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

pub(crate) fn operation_error_details(
    error: &crate::modules::fs::ops::OperationError,
) -> FsErrorDetails {
    match error {
        crate::modules::fs::ops::OperationError::Io {
            syscall,
            path,
            error,
        } => error_details(syscall, Some(path), error),
        crate::modules::fs::ops::OperationError::DirectoryRequiresRecursive { path } => {
            FsErrorDetails {
                name: Some("SystemError"),
                code: "ERR_FS_EISDIR",
                errno: libc::EISDIR,
                syscall: "rm".to_owned(),
                path: Some(path.clone()),
                message: format!("Path is a directory: rm returned EISDIR (is a directory) {path}"),
            }
        }
    }
}

/// `fs` system errors are plain `Error` instances with extra props.
pub fn fs_error(syscall: &str, path: Option<&str>, error: &std::io::Error) -> VmError {
    from_details(error_details(syscall, path, error))
}

pub(crate) fn operation_error(error: &crate::modules::fs::ops::OperationError) -> VmError {
    from_details(operation_error_details(error))
}

fn from_details(details: FsErrorDetails) -> VmError {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::Error),
        &Value::Undefined,
        &[Value::String(details.message)],
    )
    .unwrap_or_else(|_| host_api::object(vec![]));
    if let Some(name) = details.name {
        let _ = quench_runtime::execute::set_property_in_place(
            &error,
            "name",
            Value::String(name.to_string()),
        );
    }
    for (name, value) in [
        ("code", Value::String(details.code.to_string())),
        ("errno", Value::Number(details.errno as f64)),
        ("syscall", Value::String(details.syscall.to_string())),
    ] {
        let _ = quench_runtime::execute::set_property_in_place(&error, name, value);
    }
    if let Some(p) = details.path {
        let _ = quench_runtime::execute::set_property_in_place(&error, "path", Value::String(p));
    }
    if details.syscall == "access" {
        let stack = quench_runtime::execute::get_property(&error, "stack");
        let stack = match stack {
            Value::String(stack) => format!("{stack}\n    at async Object.access"),
            _ => "Error\n    at async Object.access".to_string(),
        };
        let _ =
            quench_runtime::execute::set_property_in_place(&error, "stack", Value::String(stack));
    }
    VmError::Thrown(error)
}

/// Copy operations expose both the source (`path`) and destination (`dest`)
/// while retaining the ordinary coded fs error fields.
pub fn copyfile_error(source: &str, destination: &str, error: &std::io::Error) -> VmError {
    with_dest_error("copyfile", source, destination, error)
}

pub fn with_dest_error(
    syscall: &str,
    source: &str,
    destination: &str,
    error: &std::io::Error,
) -> VmError {
    let result = fs_error(syscall, Some(source), error);
    if let VmError::Thrown(value) = &result {
        let (code, _) = code_for(error);
        let message = format!(
            "{code}: {}, {syscall} '{source}' -> '{destination}'",
            strerror(code),
        );
        let _ = quench_runtime::execute::set_property_in_place(
            value,
            "message",
            Value::String(message),
        );
        let _ = quench_runtime::execute::set_property_in_place(
            value,
            "dest",
            Value::String(destination.to_string()),
        );
    }
    result
}

/// Node/libuv-style code and (negative) errno for an I/O error.
pub(crate) fn code_for(error: &std::io::Error) -> (&'static str, i32) {
    if let Some(raw) = error.raw_os_error() {
        return (code_name(raw), -raw);
    }
    use std::io::ErrorKind::*;
    match error.kind() {
        NotFound => ("ENOENT", -2),
        PermissionDenied => ("EACCES", -13),
        AlreadyExists => ("EEXIST", -17),
        _ => ("EIO", -5),
    }
}

#[cfg(unix)]
fn code_name(raw: i32) -> &'static str {
    match raw {
        1 => "EPERM",
        2 => "ENOENT",
        9 => "EBADF",
        13 => "EACCES",
        16 => "EBUSY",
        17 => "EEXIST",
        18 => "EXDEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        27 => "EFBIG",
        28 => "ENOSPC",
        30 => "EROFS",
        31 => "EMLINK",
        62 => "ELOOP",
        63 => "ENAMETOOLONG",
        66 => "ENOTEMPTY",
        78 => "ENOSYS",
        _ => "EIO",
    }
}

#[cfg(not(unix))]
fn code_name(raw: i32) -> &'static str {
    match raw {
        2 => "ENOENT",
        5 => "EIO",
        13 => "EACCES",
        17 => "EEXIST",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        _ => "EIO",
    }
}

pub(crate) fn strerror(code: &str) -> &'static str {
    match code {
        "EPERM" => "operation not permitted",
        "ENOENT" => "no such file or directory",
        "EBADF" => "bad file descriptor",
        "EACCES" => "permission denied",
        "EBUSY" => "resource busy or locked",
        "EEXIST" => "file already exists",
        "EXDEV" => "cross-device link not permitted",
        "ENOTDIR" => "not a directory",
        "EISDIR" => "illegal operation on a directory",
        "EINVAL" => "invalid argument",
        "EFBIG" => "file too large",
        "ENOSPC" => "no space left on device",
        "EROFS" => "read-only file system",
        "EMLINK" => "too many links",
        "ELOOP" => "too many levels of symbolic links",
        "ENAMETOOLONG" => "name too long",
        "ENOTEMPTY" => "directory not empty",
        "ENOSYS" => "function not implemented",
        _ => "I/O error",
    }
}

/// `AbortError` for an operation cancelled by an `AbortSignal`.
pub fn abort_error() -> Value {
    host_api::object(vec![
        ("name".to_string(), Value::String("AbortError".to_string())),
        (
            "message".to_string(),
            Value::String("The operation was aborted".to_string()),
        ),
        ("code".to_string(), Value::String("ABORT_ERR".to_string())),
    ])
}

/// Node rejects buffers larger than its 32-bit I/O ceiling before allocating
/// the result. Keep this as one coded error constructor for sync, callback,
/// and promise read paths.
pub fn file_too_large(size: u64) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::RangeError,
        &[Value::String(format!(
            "File size ({size}) is greater than 2 GiB"
        ))],
    );
    VmError::Thrown(quench_runtime::execute::set_property(
        error,
        "code",
        Value::String("ERR_FS_FILE_TOO_LARGE".into()),
    ))
}
