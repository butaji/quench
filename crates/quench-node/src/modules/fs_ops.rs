//! Runtime-neutral filesystem operations used by the shared Node adapters.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Default)]
pub(crate) struct MkdirOptions {
    pub mode: Option<u32>,
    pub recursive: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct RmOptions {
    pub recursive: bool,
    pub force: bool,
}

#[derive(Clone, Default)]
pub(crate) struct WriteOptions {
    pub flag: Option<String>,
    pub mode: Option<u32>,
    pub flush: bool,
}

pub(crate) enum OperationError {
    Io {
        syscall: &'static str,
        path: String,
        error: io::Error,
    },
    DirectoryRequiresRecursive {
        path: String,
    },
}

pub(crate) fn mkdir(path: &str, options: MkdirOptions) -> Result<Option<String>, OperationError> {
    let first_created = options
        .recursive
        .then(|| first_missing_path(path))
        .flatten();
    let result = if options.recursive {
        std::fs::create_dir_all(path)
    } else {
        std::fs::create_dir(path)
    };
    result.map_err(|error| io_error("mkdir", path, error))?;
    apply_mode(path, options.mode);
    Ok(first_created)
}

pub(crate) fn rm(path: &str, options: RmOptions) -> Result<(), OperationError> {
    let target = Path::new(path);
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if options.force && error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("lstat", path, error)),
    };

    let file_type = metadata.file_type();
    let result = if file_type.is_dir() {
        if options.recursive {
            std::fs::remove_dir_all(target)
        } else {
            return Err(OperationError::DirectoryRequiresRecursive {
                path: path.to_owned(),
            });
        }
    } else {
        // `symlink_metadata` keeps symlinks to directories on the file path;
        // recursive removal must never follow a symlink outside the target.
        std::fs::remove_file(target)
    };

    match result {
        Ok(()) => Ok(()),
        Err(error) if options.force && error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("rm", path, error)),
    }
}

pub(crate) fn write_file(
    path: &str,
    bytes: &[u8],
    options: WriteOptions,
) -> Result<(), OperationError> {
    let mut file =
        open_write(path, options.flag.as_deref()).map_err(|error| io_error("open", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write", path, error))?;
    if options.flush {
        file.sync_all()
            .map_err(|error| io_error("fsync", path, error))?;
    }
    apply_mode(path, options.mode);
    Ok(())
}

pub(crate) fn valid_write_flag(flag: Option<&str>) -> bool {
    matches!(
        flag.unwrap_or("w"),
        "w" | "w+" | "a" | "a+" | "wx" | "ax" | "r"
    )
}

pub(crate) enum ReadFileFailure {
    Io {
        syscall: &'static str,
        error: io::Error,
    },
    TooLarge(u64),
}

/// Shared bounded reads preserve the supplied descriptor's current file position.
pub(crate) fn read_file(
    file: &mut std::fs::File,
    maximum: u64,
) -> Result<Vec<u8>, ReadFileFailure> {
    let metadata = file.metadata().map_err(|error| ReadFileFailure::Io {
        syscall: "fstat",
        error,
    })?;
    let size = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    if size > maximum {
        return Err(ReadFileFailure::TooLarge(size));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ReadFileFailure::Io {
            syscall: "read",
            error,
        })?;
    if bytes.len() as u64 > maximum {
        return Err(ReadFileFailure::TooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

pub(crate) fn open_write(path: &str, flag: Option<&str>) -> io::Result<std::fs::File> {
    let mut open = std::fs::OpenOptions::new();
    match flag.unwrap_or("w") {
        "w" => {
            open.write(true).create(true).truncate(true);
        }
        "w+" => {
            open.read(true).write(true).create(true).truncate(true);
        }
        "a" => {
            open.append(true).create(true);
        }
        "a+" => {
            open.read(true).append(true).create(true);
        }
        "wx" => {
            open.write(true).create_new(true);
        }
        "ax" => {
            open.append(true).create_new(true);
        }
        "r" => {
            open.read(true);
        }
        _ => unreachable!("write flag is validated at the VM boundary"),
    }
    open.open(path)
}

#[cfg(not(unix))]
pub(crate) fn open(
    path: impl AsRef<Path>,
    flag: Option<&str>,
    mode: Option<u32>,
) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    match flag.unwrap_or("r") {
        "r" | "rs" => {
            options.read(true);
        }
        "r+" | "rs+" => {
            options.read(true).write(true);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
        }
        "wx" => {
            options.write(true).create_new(true);
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
        }
        "wx+" => {
            options.read(true).write(true).create_new(true);
        }
        "a" | "as" => {
            options.append(true).create(true);
        }
        "ax" => {
            options.append(true).create_new(true);
        }
        "a+" | "as+" => {
            options.read(true).append(true).create(true);
        }
        "ax+" => {
            options.read(true).append(true).create_new(true);
        }
        _ => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
    }
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    options.open(path)
}

pub(crate) fn open_numeric(
    path: impl AsRef<Path>,
    flags: i32,
    mode: Option<u32>,
) -> io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::{fd::FromRawFd, unix::ffi::OsStrExt};
        let path = std::ffi::CString::new(path.as_ref().as_os_str().as_bytes())
            .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
        // SAFETY: path is NUL-terminated and mode has the promoted C varargs type.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                flags | libc::O_CLOEXEC,
                mode.unwrap_or(0o666) as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a successful open returned a new descriptor owned by this File.
        return Ok(unsafe { std::fs::File::from_raw_fd(fd) });
    }
    #[cfg(not(unix))]
    {
        let _ = (path, flags, mode);
        Err(io::Error::from(io::ErrorKind::InvalidInput))
    }
}

fn io_error(syscall: &'static str, path: &str, error: io::Error) -> OperationError {
    OperationError::Io {
        syscall,
        path: path.to_owned(),
        error,
    }
}

fn first_missing_path(path: &str) -> Option<String> {
    let mut candidate = PathBuf::from(path);
    if candidate.exists() {
        return None;
    }
    while candidate
        .parent()
        .is_some_and(|parent| !parent.exists() && parent != Path::new(""))
    {
        candidate = candidate.parent()?.to_path_buf();
    }
    Some(candidate.to_string_lossy().into_owned())
}

pub(crate) fn apply_mode(path: &str, mode: Option<u32>) {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}
