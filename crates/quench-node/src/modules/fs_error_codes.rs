/// Node/libuv-style code and negative errno for an I/O error.
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
