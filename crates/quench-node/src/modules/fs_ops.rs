//! Runtime-neutral filesystem operations used by the shared Node adapters.

use std::io::{self, Write};
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
