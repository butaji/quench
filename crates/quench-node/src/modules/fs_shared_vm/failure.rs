//! The errors fs operations report: libuv's `uvException` for a failed system call, and
//! Node's coded errors for the conditions its library detects.

use crate::modules::stream_shared_vm::{put, Context};
use quench_runtime::{RootId, RootedError};

/// The system call an error names, with the paths it involved.
#[derive(Clone, Copy)]
pub(crate) struct Syscall<'a> {
    pub(crate) name: &'static str,
    pub(crate) path: Option<&'a str>,
    pub(crate) dest: Option<&'a str>,
}

impl<'a> Syscall<'a> {
    pub(crate) const fn bare(name: &'static str) -> Self {
        Self { name, path: None, dest: None }
    }

    pub(crate) const fn at(name: &'static str, path: &'a str) -> Self {
        Self { name, path: Some(path), dest: None }
    }

    pub(crate) const fn between(name: &'static str, path: &'a str, dest: &'a str) -> Self {
        Self { name, path: Some(path), dest: Some(dest) }
    }
}

/// `uvException`: `CODE: description, syscall 'path' -> 'dest'`, with `errno`, `code`,
/// `syscall`, `path` and `dest` in that order.
pub(crate) fn system(c: &mut Context<'_>, error: &std::io::Error, call: Syscall<'_>) -> RootedError {
    match system_error(c, error, call) {
        Ok(error) | Err(error) => error,
    }
}

fn system_error(c: &mut Context<'_>, error: &std::io::Error, call: Syscall<'_>) -> Result<RootedError, RootedError> {
    let (code, errno) = crate::modules::fs_error_codes::code_for(error);
    let mut message = format!("{code}: {}, {}", crate::modules::fs_error_codes::strerror(code), call.name);
    if let Some(path) = call.path {
        message.push_str(&format!(" '{path}'"));
    }
    if let Some(dest) = call.dest {
        message.push_str(&format!(" -> '{dest}'"));
    }
    let exception = c.error_rooted(&message)?;
    let errno = c.number(f64::from(errno));
    put(c, exception, "errno", errno)?;
    let code = c.string_rooted(code);
    put(c, exception, "code", code)?;
    let syscall = c.string_rooted(call.name);
    put(c, exception, "syscall", syscall)?;
    for (name, value) in [("path", call.path), ("dest", call.dest)] {
        if let Some(value) = value {
            let value = c.string_rooted(value);
            put(c, exception, name, value)?;
        }
    }
    Ok(c.throw(exception))
}

/// `result`, or the system error it failed with.
pub(crate) fn check<T>(c: &mut Context<'_>, result: std::io::Result<T>, call: Syscall<'_>) -> Result<T, RootedError> {
    result.map_err(|error| system(c, &error, call))
}

/// The errors of Node's `SystemError` subclasses fs raises: their code, message prefix and
/// the libuv error each reports.
#[derive(Clone, Copy)]
pub(crate) enum Coded {
    /// `ERR_FS_EISDIR`.
    IsDirectory,
    /// `ERR_FS_CP_EINVAL`.
    CopyInvalid,
    /// `ERR_FS_CP_DIR_TO_NON_DIR`.
    CopyDirectoryToNonDirectory,
    /// `ERR_FS_CP_NON_DIR_TO_DIR`.
    CopyNonDirectoryToDirectory,
    /// `ERR_FS_CP_EEXIST`.
    CopyExists,
    /// `ERR_FS_CP_SOCKET`.
    CopySocket,
    /// `ERR_FS_CP_FIFO_PIPE`.
    CopyFifo,
    /// `ERR_FS_CP_UNKNOWN`.
    CopyUnknown,
}

impl Coded {
    fn describe(self) -> (&'static str, &'static str, &'static str, i32) {
        match self {
            Self::IsDirectory => ("ERR_FS_EISDIR", "Path is a directory", "EISDIR", libc::EISDIR),
            Self::CopyInvalid => ("ERR_FS_CP_EINVAL", "Invalid src or dest", "EINVAL", libc::EINVAL),
            Self::CopyDirectoryToNonDirectory => ("ERR_FS_CP_DIR_TO_NON_DIR", "Cannot overwrite non-directory with directory", "EISDIR", libc::EISDIR),
            Self::CopyNonDirectoryToDirectory => ("ERR_FS_CP_NON_DIR_TO_DIR", "Cannot overwrite directory with non-directory", "ENOTDIR", libc::ENOTDIR),
            Self::CopyExists => ("ERR_FS_CP_EEXIST", "Target already exists", "EEXIST", libc::EEXIST),
            Self::CopySocket => ("ERR_FS_CP_SOCKET", "Cannot copy a socket file", "EINVAL", libc::EINVAL),
            Self::CopyFifo => ("ERR_FS_CP_FIFO_PIPE", "Cannot copy a FIFO pipe", "EINVAL", libc::EINVAL),
            Self::CopyUnknown => ("ERR_FS_CP_UNKNOWN", "Cannot copy an unknown file type", "EINVAL", libc::EINVAL),
        }
    }
}

/// `new ERR_FS_*({ message, path, syscall, errno, code })`.
pub(crate) fn coded(c: &mut Context<'_>, kind: Coded, message: &str, syscall: &str, path: &str) -> RootedError {
    let built = (|| -> Result<RootedError, RootedError> {
        let (code, prefix, uv_code, errno) = kind.describe();
        let info = c.object_rooted()?;
        for (name, value) in [("message", message), ("path", path), ("syscall", syscall)] {
            let value = c.string_rooted(value);
            put(c, info, name, value)?;
        }
        let errno = c.number(f64::from(errno));
        put(c, info, "errno", errno)?;
        let uv_code = c.string_rooted(uv_code);
        put(c, info, "code", uv_code)?;
        crate::modules::util_system_error_shared_vm::coded_system_error(c, info, code, prefix)
    })();
    match built {
        Ok(error) | Err(error) => error,
    }
}

/// An `Error` (or a `RangeError`) carrying a Node error code.
pub(crate) fn plain(c: &mut Context<'_>, code: &'static str, message: &str, range: bool) -> RootedError {
    let built = (|| -> Result<RootedError, RootedError> {
        let error = if range { c.range_error_rooted(message)? } else { c.error_rooted(message)? };
        if range {
            crate::modules::node_errors_shared_vm::decorate_range_error(c, error, code)?;
        } else {
            crate::modules::node_errors_shared_vm::decorate_error(c, error, code)?;
        }
        Ok(c.throw(error))
    })();
    match built {
        Ok(error) | Err(error) => error,
    }
}

/// `ERR_OUT_OF_RANGE`: `The value of "name" is out of range. It must be requirement.`
pub(crate) fn out_of_range(c: &mut Context<'_>, name: &str, requirement: &str, value: RootId) -> RootedError {
    let built = (|| -> Result<RootedError, RootedError> {
        let received = crate::modules::util_shared_vm::range_error_received(c, value)?;
        let message = format!("The value of \"{name}\" is out of range. It must be {requirement}. Received {received}");
        Ok(plain(c, "ERR_OUT_OF_RANGE", &message, true))
    })();
    match built {
        Ok(error) | Err(error) => error,
    }
}

/// `new AbortError(undefined, { cause: signal.reason })`.
pub(crate) fn aborted(c: &mut Context<'_>, signal: RootId) -> RootedError {
    let built = (|| -> Result<RootedError, RootedError> {
        let error = c.error_rooted("The operation was aborted")?;
        let name = c.string_rooted("AbortError");
        put(c, error, "name", name)?;
        let code = c.string_rooted("ABORT_ERR");
        put(c, error, "code", code)?;
        let cause = c.get_named_property_rooted(signal, "reason")?;
        let key = c.string_rooted("cause");
        c.define_data_property_rooted(error, key, cause, true, false, true)?;
        Ok(c.throw(error))
    })();
    match built {
        Ok(error) | Err(error) => error,
    }
}
