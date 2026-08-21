//! `fs` module — real filesystem operations with Node's coded
//! errors, `Stats`/`Dirent` values, and async variants whose
//! callbacks run on the host event loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct FsState;

impl Default for FsState {
    fn default() -> Self {
        Self
    }
}

impl FsState {
    pub fn new() -> Self {
        Self
    }
}
thread_local! { static FDS: RefCell<HashMap<i32, File>> = RefCell::new(HashMap::new()); }
static NEXT_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(100);
pub(crate) fn fd_open(path: &str, flag: Option<&str>) -> Result<i32, VmError> {
    let flag = flag.unwrap_or("r");
    let mut o = std::fs::OpenOptions::new();
    let f = match flag {
        "r" => o.read(true).open(path),
        "r+" => o.read(true).write(true).open(path),
        "w" => o.write(true).create(true).truncate(true).open(path),
        "w+" => o
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path),
        "a" => o.create(true).append(true).open(path),
        "a+" => o.read(true).create(true).append(true).open(path),
        // Exclusive-create variants (Node's `wx`/`wx+`/`ax`/`ax+`).
        "wx" | "xw" => o.write(true).create_new(true).open(path),
        "wx+" | "xw+" => o.read(true).write(true).create_new(true).open(path),
        "ax" | "xa" => o.create_new(true).append(true).open(path),
        "ax+" | "xa+" => o.read(true).create_new(true).append(true).open(path),
        _ => {
            return Err(crate::modules::fs_error::fs_error(
                "open",
                Some(path),
                &std::io::Error::from(std::io::ErrorKind::InvalidInput),
            ));
        }
    }
    .map_err(|e| crate::modules::fs_error::fs_error("open", Some(path), &e))?;
    let fd = NEXT_FD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    FDS.with(|t| {
        t.borrow_mut().insert(fd, f);
    });
    Ok(fd)
}
pub(crate) fn fd_stat(fd: i32) -> Result<Value, VmError> {
    if let Some(stream) = std_stream_fd(fd) {
        return std::fs::metadata(stream)
            .map(|m| super::fs_stats::stats(&m))
            .map_err(|e| crate::modules::fs_error::fs_error("fstat", None, &e));
    }
    FDS.with(|t| {
        t.borrow()
            .get(&fd)
            .ok_or_else(|| {
                crate::modules::buffer_enc::invalid_arg_value("Invalid file descriptor".into())
            })
            .and_then(|f| {
                f.metadata()
                    .map(|m| super::fs_stats::stats(&m))
                    .map_err(|e| crate::modules::fs_error::fs_error("fstat", None, &e))
            })
    })
}
pub(crate) fn fd_close(fd: i32) -> Result<Value, VmError> {
    if std_stream_fd(fd).is_some() {
        return Ok(Value::Undefined);
    }
    FDS.with(|t| {
        if t.borrow_mut().remove(&fd).is_some() {
            Ok(Value::Undefined)
        } else {
            Err(crate::modules::buffer_enc::invalid_arg_value(
                "Invalid file descriptor".into(),
            ))
        }
    })
}

pub(crate) fn fd_truncate(fd: i32, len: u64) -> Result<Value, VmError> {
    FDS.with(|t| {
        t.borrow()
            .get(&fd)
            .ok_or_else(|| {
                crate::modules::buffer_enc::invalid_arg_value("Invalid file descriptor".into())
            })
            .and_then(|f| {
                f.set_len(len)
                    .map(|_| Value::Undefined)
                    .map_err(|e| crate::modules::fs_error::fs_error("ftruncate", None, &e))
            })
    })
}
pub(crate) fn fd_write(
    fd: i32,
    data: &[u8],
    offset: usize,
    length: usize,
    position: Option<u64>,
) -> Result<usize, VmError> {
    use std::io::{Seek, SeekFrom, Write};
    FDS.with(|t| {
        let mut table = t.borrow_mut();
        let file = table.get_mut(&fd).ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("Invalid file descriptor".into())
        })?;
        if let Some(pos) = position {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| crate::modules::fs_error::fs_error("write", None, &e))?;
        }
        file.write(&data[offset..offset.saturating_add(length).min(data.len())])
            .map_err(|e| crate::modules::fs_error::fs_error("write", None, &e))
    })
}

pub(crate) fn fd_read(
    fd: i32,
    out: &mut [u8],
    offset: usize,
    length: usize,
    position: Option<u64>,
) -> Result<usize, VmError> {
    use std::io::{Read, Seek, SeekFrom};
    FDS.with(|t| {
        let mut table = t.borrow_mut();
        let file = table.get_mut(&fd).ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("Invalid file descriptor".into())
        })?;
        if let Some(pos) = position {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| crate::modules::fs_error::fs_error("read", None, &e))?;
        }
        let end = offset.saturating_add(length).min(out.len());
        let start = offset.min(out.len());
        file.read(&mut out[start..end])
            .map_err(|e| crate::modules::fs_error::fs_error("read", None, &e))
    })
}
#[cfg(unix)]
fn with_raw_fd<T>(
    fd: i32,
    f: impl FnOnce(std::os::unix::io::RawFd) -> Result<T, VmError>,
) -> Result<T, VmError> {
    use std::os::unix::io::AsRawFd;
    FDS.with(|t| {
        t.borrow()
            .get(&fd)
            .map(|file| f(file.as_raw_fd()))
            .unwrap_or_else(|| {
                Err(crate::modules::buffer_enc::invalid_arg_value(
                    "Invalid file descriptor".into(),
                ))
            })
    })
}

#[cfg(unix)]
pub(crate) fn fd_chmod(fd: i32, mode: u32) -> Result<Value, VmError> {
    with_raw_fd(fd, |raw| {
        let rc = unsafe { libc::fchmod(raw, mode as libc::mode_t) };
        if rc == -1 {
            return Err(crate::modules::fs_error::fs_error(
                "fchmod",
                None,
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(Value::Undefined)
    })
}

#[cfg(unix)]
pub(crate) fn fd_chown(fd: i32, uid: u32, gid: u32) -> Result<Value, VmError> {
    with_raw_fd(fd, |raw| {
        let rc = unsafe { libc::fchown(raw, uid as libc::uid_t, gid as libc::gid_t) };
        if rc == -1 {
            return Err(crate::modules::fs_error::fs_error(
                "fchown",
                None,
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(Value::Undefined)
    })
}

#[cfg(unix)]
pub(crate) fn fd_utimes(fd: i32, atime: f64, mtime: f64) -> Result<Value, VmError> {
    with_raw_fd(fd, |raw| {
        let times = [
            libc::timeval {
                tv_sec: atime.trunc() as libc::time_t,
                tv_usec: (atime.fract() * 1_000_000.0) as libc::suseconds_t,
            },
            libc::timeval {
                tv_sec: mtime.trunc() as libc::time_t,
                tv_usec: (mtime.fract() * 1_000_000.0) as libc::suseconds_t,
            },
        ];
        let rc = unsafe { libc::futimes(raw, times.as_ptr()) };
        if rc == -1 {
            return Err(crate::modules::fs_error::fs_error(
                "futimes",
                None,
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(Value::Undefined)
    })
}

#[cfg(not(unix))]
pub(crate) fn fd_chmod(_: i32, _: u32) -> Result<Value, VmError> {
    Err(VmError::NotCallable)
}
#[cfg(not(unix))]
pub(crate) fn fd_chown(_: i32, _: u32, _: u32) -> Result<Value, VmError> {
    Err(VmError::NotCallable)
}
#[cfg(not(unix))]
pub(crate) fn fd_utimes(_: i32, _: f64, _: f64) -> Result<Value, VmError> {
    Err(VmError::NotCallable)
}

/// The `/dev/fd/N` path for standard stream descriptors 0/1/2, which are
/// always-open real fds in Node (stdin/stdout/stderr).
fn std_stream_fd(fd: i32) -> Option<&'static str> {
    match fd {
        0 => Some("/dev/stdin"),
        1 => Some("/dev/stdout"),
        2 => Some("/dev/stderr"),
        _ => None,
    }
}

/// Parsed `options` argument shared by the sync and async families.
#[derive(Default)]
pub(crate) struct FsOptions {
    pub encoding: Option<String>,
    pub flag: Option<String>,
    pub mode: Option<u32>,
    pub recursive: bool,
    pub force: bool,
    pub with_file_types: bool,
    pub throw_if_no_entry: bool,
    pub signal_aborted: bool,
}

/// `path` argument: string only (Buffer/URL paths unsupported).
pub(crate) fn path_arg(value: Option<&Value>) -> Result<String, VmError> {
    crate::modules::path::validate_string(value.unwrap_or(&Value::Undefined), "path")
}

/// Parse the trailing `options` argument (string encoding or object).
pub(crate) fn parse_options(value: Option<&Value>) -> Result<FsOptions, VmError> {
    let mut options = FsOptions::default();
    match value {
        None | Some(Value::Undefined) | Some(Value::Null) => {}
        Some(Value::String(encoding)) => set_encoding(&mut options, encoding)?,
        Some(object @ Value::Object(_)) => parse_option_object(&mut options, object)?,
        Some(other) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options\" argument must be of type string or an instance of Object.{}",
                crate::modules::util::invalid_arg_received(other)
            )));
        }
    }
    Ok(options)
}

fn set_encoding(options: &mut FsOptions, encoding: &str) -> Result<(), VmError> {
    match crate::modules::buffer_enc::canonical_encoding(encoding) {
        Some(canonical) => {
            options.encoding = Some(canonical.to_string());
            Ok(())
        }
        None => Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'options' is invalid. Received {encoding:?}"
        ))),
    }
}

fn parse_option_object(options: &mut FsOptions, object: &Value) -> Result<(), VmError> {
    let get = |key: &str| quench_runtime::vm::get_property(object, key);
    if let Value::String(encoding) = get("encoding") {
        set_encoding(options, &encoding)?;
    }
    if let Value::String(flag) = get("flag") {
        options.flag = Some(flag);
    }
    if let Value::Number(mode) = get("mode") {
        options.mode = Some(mode as u32);
    }
    options.recursive = truthy(&get("recursive"));
    options.force = truthy(&get("force"));
    options.with_file_types = truthy(&get("withFileTypes"));
    options.throw_if_no_entry = truthy(&get("throwIfNoEntry"));
    if let signal @ Value::Object(_) = get("signal") {
        options.signal_aborted = truthy(&quench_runtime::vm::get_property(&signal, "aborted"));
    }
    Ok(())
}

pub(crate) fn truthy(value: &Value) -> bool {
    !matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(false) | Value::Number(0.0)
    ) && !matches!(value, Value::String(s) if s.is_empty())
}

/// Node's `fs.exists` callback takes a single boolean, no error.
pub(crate) fn require_callback(value: Option<&Value>) -> Result<Value, VmError> {
    match value {
        Some(cb) if quench_runtime::is_callable(cb) => Ok(cb.clone()),
        Some(other) => Err(callback_type_error(other)),
        None => Err(callback_type_error(&Value::Undefined)),
    }
}

fn callback_type_error(value: &Value) -> VmError {
    crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"callback\" argument must be of type function.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

/// Queue an async fs callback on the event loop's immediate queue.
pub(crate) fn defer(state: &Rc<RefCell<HostState>>, cb: &Value, args: Vec<Value>) {
    state.borrow().event_loop.queue_immediate(cb.clone(), args);
}

/// Split `args` into `(leading, callback)` for the async family: the
/// callback is always the last argument.
pub(crate) fn async_args(args: &[Value]) -> Result<(&[Value], Value), VmError> {
    let (callback, leading) = match args.split_last() {
        Some((cb, rest)) => (Some(cb), rest),
        None => (None, &[][..]),
    };
    Ok((leading, require_callback(callback)?))
}

/// The error half of an async callback result.
pub(crate) fn err_value(result: &Result<Value, VmError>) -> Value {
    match result {
        Ok(_) => Value::Null,
        Err(VmError::Thrown(value)) => value.clone(),
        Err(_) => host_api::object(vec![(
            "message".to_string(),
            Value::String("I/O error".to_string()),
        )]),
    }
}
#[path = "fs_build.rs"]
mod fs_build;

pub fn build() -> Value {
    fs_build::build()
}
#[cfg(target_os = "macos")]
mod flags {
    pub const O_CREAT: f64 = 0x200 as f64;
    pub const O_EXCL: f64 = 0x800 as f64;
    pub const O_TRUNC: f64 = 0x400 as f64;
    pub const O_DIRECTORY: f64 = 0x100000 as f64;
    pub const O_NOFOLLOW: f64 = 0x100 as f64;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod flags {
    pub const O_CREAT: f64 = 0x40 as f64;
    pub const O_EXCL: f64 = 0x80 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0x10000 as f64;
    pub const O_NOFOLLOW: f64 = 0x20000 as f64;
}

#[cfg(not(unix))]
mod flags {
    pub const O_CREAT: f64 = 0x100 as f64;
    pub const O_EXCL: f64 = 0x400 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0.0;
    pub const O_NOFOLLOW: f64 = 0.0;
}

const CONSTANT_ENTRIES: &[(&str, f64)] = &[
    ("F_OK", 0.0),
    ("R_OK", 4.0),
    ("W_OK", 2.0),
    ("X_OK", 1.0),
    ("COPYFILE_EXCL", 1.0),
    ("COPYFILE_FICLONE", 2.0),
    ("COPYFILE_FICLONE_FORCE", 4.0),
    ("O_RDONLY", 0.0),
    ("O_WRONLY", 1.0),
    ("O_RDWR", 2.0),
    ("O_CREAT", flags::O_CREAT),
    ("O_EXCL", flags::O_EXCL),
    ("O_TRUNC", flags::O_TRUNC),
    ("O_APPEND", 8.0),
    ("O_DIRECTORY", flags::O_DIRECTORY),
    ("O_NOFOLLOW", flags::O_NOFOLLOW),
    ("S_IFMT", 0o170000 as f64),
    ("S_IFREG", 0o100000 as f64),
    ("S_IFDIR", 0o40000 as f64),
    ("S_IFCHR", 0o20000 as f64),
    ("S_IFBLK", 0o60000 as f64),
    ("S_IFIFO", 0o10000 as f64),
    ("S_IFLNK", 0o120000 as f64),
    ("S_IFSOCK", 0o140000 as f64),
    ("S_IRWXU", 0o700 as f64),
    ("S_IRUSR", 0o400 as f64),
    ("S_IWUSR", 0o200 as f64),
    ("S_IXUSR", 0o100 as f64),
    ("S_IRWXG", 0o70 as f64),
    ("S_IRWXO", 0o7 as f64),
];

/// Dispatch table reused by the async and promises families.
pub(crate) type Op =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;

pub(crate) fn sync_op(name: &str) -> Option<Op> {
    use super::fs_sync as sync;
    Some(match name {
        "readFile" => sync::read_file_sync,
        "writeFile" => sync::write_file_sync,
        "appendFile" => sync::append_file_sync,
        "stat" => sync::stat_sync,
        "lstat" => sync::lstat_sync,
        "statfs" => sync::statfs_sync,
        "readdir" => sync::readdir_sync,
        "mkdir" => sync::mkdir_sync,
        "unlink" => sync::unlink_sync,
        "rmdir" => sync::rmdir_sync,
        "rm" => sync::rm_sync,
        "rename" => sync::rename_sync,
        "copyFile" => sync::copy_file_sync,
        "access" => sync::access_sync,
        "mkdtemp" => sync::mkdtemp_sync,
        "readlink" => sync::readlink_sync,
        "chmod" => sync::chmod_sync,
        "truncate" => sync::truncate_sync,
        "chown" => sync::chown_sync,
        "utimes" => sync::utimes_sync,
        "realpath" => sync::realpath_sync,
        "link" => sync::link_sync,
        "symlink" => sync::symlink_sync,
        "lchown" => sync::lchown_sync,
        "lutimes" => sync::lutimes_sync,
        "lchmod" => sync::lchmod_sync,
        _ => return None,
    })
}
