#[path = "stat_types.rs"]
pub(crate) mod types;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::VecDeque;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{ffi::CString, os::unix::ffi::OsStrExt, os::unix::fs::PermissionsExt};

#[cfg(not(unix))]
const S_IF_DIRECTORY: f64 = 0o040000 as f64;
#[cfg(not(unix))]
const S_IF_REGULAR: f64 = 0o100000 as f64;

/// Kernel integer fields and timestamps are authoritative for every stat view.
#[derive(Clone, Default)]
struct StatMetadata {
    fields: [i128; types::BASE_FIELDS.len()],
    times: [i128; types::TIME_FIELDS.len()],
}
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const MILLISECONDS_PER_SECOND: f64 = (NANOSECONDS_PER_SECOND / NANOSECONDS_PER_MILLISECOND) as f64;
impl StatMetadata {
    fn from_metadata(metadata: Option<&std::fs::Metadata>) -> Self {
        let Some(metadata) = metadata else {
            return Self::default();
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let birth = metadata.created().ok().map(exact_time_ns).unwrap_or(0);
            Self {
                fields: [
                    i128::from(metadata.dev()),
                    i128::from(metadata.mode()),
                    i128::from(metadata.nlink()),
                    i128::from(metadata.uid()),
                    i128::from(metadata.gid()),
                    i128::from(metadata.rdev()),
                    i128::from(metadata.blksize()),
                    i128::from(metadata.ino()),
                    i128::from(metadata.len()),
                    i128::from(metadata.blocks()),
                ],
                times: [
                    i128::from(metadata.atime()) * NANOSECONDS_PER_SECOND
                        + i128::from(metadata.atime_nsec()),
                    i128::from(metadata.mtime()) * NANOSECONDS_PER_SECOND
                        + i128::from(metadata.mtime_nsec()),
                    i128::from(metadata.ctime()) * NANOSECONDS_PER_SECOND
                        + i128::from(metadata.ctime_nsec()),
                    birth,
                ],
            }
        }
        #[cfg(not(unix))]
        {
            let mut result = Self::default();
            result.fields[1] = if metadata.is_dir() {
                S_IF_DIRECTORY as i128
            } else {
                S_IF_REGULAR as i128
            };
            result.fields[2] = 1;
            result.fields[8] = i128::from(metadata.len());
            result.times = [
                metadata.accessed().ok(),
                metadata.modified().ok(),
                metadata.created().ok(),
                metadata.created().ok(),
            ]
            .map(|time| time.map(exact_time_ns).unwrap_or(0));
            result
        }
    }
}
fn exact_time_ns(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

pub(crate) fn metadata_stats(
    context: &mut NativeContext<'_, NodeHost>,
    metadata: Option<&std::fs::Metadata>,
    bigint: bool,
) -> Result<RootId, RootedError> {
    let snapshot = StatMetadata::from_metadata(metadata);
    let result = context.object_rooted()?;
    for (name, value) in types::BASE_FIELDS.into_iter().zip(snapshot.fields) {
        let value = if bigint {
            context.bigint_rooted(value)
        } else {
            context.number(value as f64)
        };
        set(context, result, name, value)?;
    }
    for (name, time) in types::TIME_FIELDS.into_iter().zip(snapshot.times) {
        let milliseconds = if bigint {
            context.bigint_rooted(time / NANOSECONDS_PER_MILLISECOND)
        } else {
            context.number(stat_time_ms(time))
        };
        set(context, result, &format!("{name}Ms"), milliseconds)?;
        if bigint {
            let nanoseconds = context.bigint_rooted(time);
            set(context, result, &format!("{name}Ns"), nanoseconds)?;
        } else {
            let nanos = context.number(time.rem_euclid(NANOSECONDS_PER_SECOND) as f64);
            types::set_partial_time(context, result, name, nanos)?;
        }
    }
    types::decorate_stats(context, result, bigint)
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    types::install(context, module)?;
    let utimes = context.host_function(crate::host::shared_vm::operation("fsUtimesSync"))?;
    set(context, module, "utimesSync", utimes)?;
    let lutimes = context.host_function(crate::host::shared_vm::operation("fsLutimesSync"))?;
    set(context, module, "lutimesSync", lutimes)?;
    let futimes = context.host_function(crate::host::shared_vm::operation("fsFutimesSync"))?;
    set(context, module, "futimesSync", futimes)
}

pub(crate) fn utimes_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_times(context, args, false)
}

pub(crate) fn lutimes_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_times(context, args, true)
}

pub(crate) fn futimes_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let seconds = |index: usize| {
        args.get(index)
            .copied()
            .and_then(|value| context.rooted_value(value))
            .and_then(|value| value.as_number())
            .unwrap_or(0.0)
    };
    #[cfg(unix)]
    let result = {
        let times = [to_timespec(seconds(1)), to_timespec(seconds(2))];
        use std::os::fd::AsRawFd;
        let shared = context.host_mut().shared_state();
        let host = shared.borrow();
        let descriptors = host.fs.descriptors();
        let result = match descriptors.get(&fd) {
            Some(descriptor) => {
                // SAFETY: the descriptor remains borrowed for the duration of
                // the syscall, and `times` contains two valid timespec values.
                if unsafe { libc::futimens(descriptor.file.as_raw_fd(), times.as_ptr()) } == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            None => Err(io::Error::from_raw_os_error(libc::EBADF)),
        };
        result
    };
    #[cfg(not(unix))]
    let result = Err(io::Error::from_raw_os_error(libc::EBADF));
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, "", "futime")?),
    }
}

fn set_times(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    nofollow: bool,
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = super::super::fs_path::validate(context, path, "path")?;
    let path = super::super::fs_path::resolve(context, path)?;
    let path_text = path.to_string_lossy().into_owned();
    let seconds = |index: usize| {
        args.get(index)
            .copied()
            .and_then(|value| context.rooted_value(value))
            .and_then(|value| value.as_number())
            .unwrap_or(0.0)
    };
    let atime = seconds(1);
    let mtime = seconds(2);
    #[cfg(unix)]
    let result = {
        let path = CString::new(path_text.as_bytes())
            .map_err(|_| RootedError::host("utimes path contains an interior null byte"))?;
        let times = [to_timespec(atime), to_timespec(mtime)];
        // SAFETY: path and times point to valid C values for the duration of the call.
        let flags = if nofollow {
            libc::AT_SYMLINK_NOFOLLOW
        } else {
            0
        };
        let result =
            unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), flags) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    };
    #[cfg(not(unix))]
    let result = std::fs::metadata(&path).map(|_| ());
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, &path_text, "utime")?),
    }
}

#[cfg(unix)]
fn to_timespec(seconds: f64) -> libc::timespec {
    let whole = seconds.floor();
    libc::timespec {
        tv_sec: whole as libc::time_t,
        tv_nsec: ((seconds - whole) * 1_000_000_000.0) as libc::c_long,
    }
}

pub(crate) fn stat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    sync_metadata(context, args, MetadataOperation::Stat)
}

// libuv statfs counters in the order consumed by Node getStatFsFromBinding.
#[cfg(not(target_os = "linux"))]
const STATFS_FIELD_COUNT: usize = 8;

pub(crate) fn statfs_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let value = super::super::fs_path::validate(context, value, "path")?;
    let bigint = bigint_option(context, args.get(1).copied())?;
    let path = super::super::fs_path::resolve(context, value)?;
    #[cfg(target_os = "linux")]
    let result = {
        let c_path = CString::new(std::path::Path::new(&path).as_os_str().as_bytes());
        match c_path {
            Ok(c_path) => {
                let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
                let status = unsafe { libc::statfs(c_path.as_ptr(), stats.as_mut_ptr()) };
                if status == 0 {
                    let stats = unsafe { stats.assume_init() };
                    Ok([
                        stats.f_type as i128,
                        stats.f_bsize as i128,
                        stats.f_frsize as i128,
                        stats.f_blocks as i128,
                        stats.f_bfree as i128,
                        stats.f_bavail as i128,
                        stats.f_files as i128,
                        stats.f_ffree as i128,
                    ])
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            Err(_) => Err(io::Error::from_raw_os_error(libc::EINVAL)),
        }
    };
    #[cfg(not(target_os = "linux"))]
    let result: Result<[i128; STATFS_FIELD_COUNT], io::Error> =
        Err(io::Error::from_raw_os_error(libc::ENOSYS));
    match result {
        Ok(values) => {
            let values = values
                .into_iter()
                .map(|value| {
                    if bigint {
                        context.bigint_rooted(value)
                    } else {
                        context.number(value as f64)
                    }
                })
                .collect::<Vec<_>>();
            types::statfs_from_values(context, &values)
        }
        Err(error) => {
            let path = super::super::fs_path::display(context, value)?;
            Err(path_error(context, error, &path, "statfs")?)
        }
    }
}

pub(crate) fn truncate_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = super::resolve_shared_path(context, path);
    let length = truncate_length(context, args.get(1).copied());
    let result = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_len(length));
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, &path, "open")?),
    }
}

pub(crate) fn ftruncate_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let length = truncate_length(context, args.get(1).copied());
    let result = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let result = state
            .fs
            .descriptors()
            .get(&fd)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EBADF))
            .and_then(|descriptor| descriptor.file.set_len(length));
        result
    };
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, "", "ftruncate")?),
    }
}

fn truncate_length(context: &NativeContext<'_, NodeHost>, value: Option<RootId>) -> u64 {
    value
        .and_then(|value| context.rooted_value(value))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= u64::MAX as f64)
        .map(|value| value as u64)
        .unwrap_or(0)
}

pub(crate) fn access_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = super::super::fs_path::validate(context, path, "path")?;
    let path = super::super::fs_path::resolve(context, path)?;
    let path_text = path.to_string_lossy().into_owned();
    let mode = args
        .get(1)
        .copied()
        .and_then(|mode| context.rooted_value(mode))
        .and_then(|mode| mode.as_number())
        .unwrap_or(0.0) as i32;
    #[cfg(unix)]
    let result = match CString::new(std::path::Path::new(&path).as_os_str().as_bytes()) {
        Ok(path_string) => {
            let status = unsafe { libc::access(path_string.as_ptr(), mode) };
            if status == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        Err(_) => Err(io::Error::from_raw_os_error(libc::EINVAL)),
    };
    #[cfg(not(unix))]
    let result = std::fs::metadata(&path).map(|_| ());
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, &path_text, "access")?),
    }
}

pub(crate) fn chmod_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = super::resolve_shared_path(context, path);
    let mode = args
        .get(1)
        .copied()
        .and_then(|mode| context.rooted_value(mode))
        .and_then(|mode| mode.as_number())
        .unwrap_or(0.0) as u32;
    #[cfg(unix)]
    let result = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
    #[cfg(not(unix))]
    let result = std::fs::metadata(&path).map(|_| ());
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, &path, "chmod")?),
    }
}

pub(crate) fn fchmod_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let mode = args
        .get(1)
        .copied()
        .and_then(|mode| context.rooted_value(mode))
        .and_then(|mode| mode.as_number())
        .unwrap_or(0.0) as u32;
    let result = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let result = state
            .fs
            .descriptors()
            .get(&fd)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EBADF))
            .and_then(|descriptor| {
                #[cfg(unix)]
                {
                    descriptor
                        .file
                        .set_permissions(std::fs::Permissions::from_mode(mode))
                }
                #[cfg(not(unix))]
                {
                    descriptor.file.metadata().map(|_| ())
                }
            });
        result
    };
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, "", "fchmod")?),
    }
}

pub(crate) fn chown_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    change_owner(context, args, false)
}

pub(crate) fn lchown_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    change_owner(context, args, true)
}

fn change_owner(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    no_follow: bool,
) -> Result<RootId, RootedError> {
    let path = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = super::super::fs_path::validate(context, path, "path")?;
    let path = super::super::fs_path::resolve(context, path)?;
    let path_text = path.to_string_lossy().into_owned();
    let uid = owner_id(context, args.get(1).copied());
    let gid = owner_id(context, args.get(2).copied());
    #[cfg(unix)]
    let result = match CString::new(std::path::Path::new(&path).as_os_str().as_bytes()) {
        Ok(path_string) => {
            let status = unsafe {
                if no_follow {
                    libc::lchown(path_string.as_ptr(), uid, gid)
                } else {
                    libc::chown(path_string.as_ptr(), uid, gid)
                }
            };
            if status == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        Err(_) => Err(io::Error::from_raw_os_error(libc::EINVAL)),
    };
    #[cfg(not(unix))]
    let result = std::fs::metadata(&path).map(|_| ());
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(
            context,
            error,
            &path_text,
            if no_follow { "lchown" } else { "chown" },
        )?),
    }
}

pub(crate) fn fchown_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let uid = owner_id(context, args.get(1).copied());
    let gid = owner_id(context, args.get(2).copied());
    #[cfg(unix)]
    let result = {
        use std::os::fd::AsRawFd;
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let descriptor = state.fs.descriptors();
        match descriptor.get(&fd) {
            Some(descriptor) => {
                let status = unsafe { libc::fchown(descriptor.file.as_raw_fd(), uid, gid) };
                if status == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            None => Err(io::Error::from_raw_os_error(libc::EBADF)),
        }
    };
    #[cfg(not(unix))]
    let result = Err(io::Error::from_raw_os_error(libc::EBADF));
    match result {
        Ok(()) => Ok(context.undefined()),
        Err(error) => Err(path_error(context, error, "", "fchown")?),
    }
}

fn owner_id(context: &NativeContext<'_, NodeHost>, value: Option<RootId>) -> libc::uid_t {
    value
        .and_then(|value| context.rooted_value(value))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= -1.0 && *value <= u32::MAX as f64)
        .map(|value| {
            if value == -1.0 {
                u32::MAX
            } else {
                value as u32
            }
        })
        .unwrap_or(u32::MAX)
}

pub(crate) fn lstat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    sync_metadata(context, args, MetadataOperation::Lstat)
}

#[derive(Clone, Copy)]
enum MetadataOperation {
    Stat,
    Lstat,
}
impl MetadataOperation {
    fn syscall(self) -> &'static str {
        match self {
            Self::Stat => "stat",
            Self::Lstat => "lstat",
        }
    }
    fn missing(self, error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::NotFound
            || (matches!(self, Self::Stat) && error.kind() == io::ErrorKind::NotADirectory)
    }
}

fn sync_metadata(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    operation: MetadataOperation,
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let value = super::super::fs_path::validate(context, value, "path")?;
    let options = args.get(1).copied();
    let bigint = bigint_option(context, options)?;
    let throw_if_missing = match options {
        Some(options)
            if !context
                .rooted_value(options)
                .is_some_and(|value| value.is_undefined()) =>
        {
            let key = context.string_rooted("throwIfNoEntry");
            let value = context.get_property_rooted(options, key)?;
            !context
                .rooted_value(value)
                .is_some_and(|value| value.as_bool() == Some(false))
        }
        _ => true,
    };
    let path = super::super::fs_path::resolve(context, value)?;
    let result = match operation {
        MetadataOperation::Stat => std::fs::metadata(&path),
        MetadataOperation::Lstat => std::fs::symlink_metadata(&path),
    };
    match result {
        Ok(metadata) => metadata_stats(context, Some(&metadata), bigint),
        Err(error) if !throw_if_missing && operation.missing(&error) => Ok(context.undefined()),
        Err(error) => {
            let path = super::super::fs_path::display(context, value)?;
            Err(path_error(context, error, &path, operation.syscall())?)
        }
    }
}

pub(crate) fn realpath_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let native = args
        .get(1)
        .copied()
        .and_then(|value| context.rooted_value(value))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let path = super::resolve_shared_path(context, path);
    match std::fs::canonicalize(&path) {
        Ok(canonical) => Ok(context.string_rooted(&canonical.to_string_lossy())),
        Err(error) => {
            // Some platforms report EIO for a symlink cycle where Node reports
            // ELOOP. Detect the cycle before translating the host error.
            let error = if symlink_loop(Path::new(&path)) {
                io::Error::from_raw_os_error(libc::ELOOP)
            } else {
                error
            };
            // The regular realpath implementation reports its failed path
            // lookup as lstat; the `.native` entry point exposes libuv's
            // realpath syscall name.
            Err(path_error(
                context,
                error,
                &path,
                if native { "realpath" } else { "lstat" },
            )?)
        }
    }
}

fn symlink_loop(path: &Path) -> bool {
    let mut pending = path
        .components()
        .map(|part| PathBuf::from(part.as_os_str()))
        .collect::<VecDeque<_>>();
    let mut current = PathBuf::new();
    let mut followed = 0;

    while let Some(component) = pending.pop_front() {
        match component.components().next() {
            Some(Component::Prefix(prefix)) => current.push(prefix.as_os_str()),
            Some(Component::RootDir) => current.push(component.as_os_str()),
            Some(Component::CurDir) | None => {}
            Some(Component::ParentDir) => {
                current.pop();
            }
            Some(Component::Normal(name)) => {
                current.push(name);
                let Ok(metadata) = std::fs::symlink_metadata(&current) else {
                    return false;
                };
                if !metadata.file_type().is_symlink() {
                    continue;
                }
                followed += 1;
                if followed > 40 {
                    return true;
                }
                let Ok(target) = std::fs::read_link(&current) else {
                    return false;
                };
                let mut replacement = if target.is_absolute() {
                    target
                } else {
                    current
                        .parent()
                        .unwrap_or_else(|| Path::new("/"))
                        .join(target)
                };
                replacement.extend(pending.into_iter());
                pending = replacement
                    .components()
                    .map(|part| PathBuf::from(part.as_os_str()))
                    .collect();
                current = PathBuf::new();
            }
        }
    }
    false
}

pub(crate) fn directory_entry_type(kind: std::fs::FileType) -> i32 {
    if kind.is_file() {
        return super::UV_DIRENT_FILE;
    }
    if kind.is_dir() {
        return super::UV_DIRENT_DIR;
    }
    if kind.is_symlink() {
        return super::UV_DIRENT_LINK;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if kind.is_block_device() {
            return super::UV_DIRENT_BLOCK;
        }
        if kind.is_char_device() {
            return super::UV_DIRENT_CHAR;
        }
        if kind.is_fifo() {
            return super::UV_DIRENT_FIFO;
        }
        if kind.is_socket() {
            return super::UV_DIRENT_SOCKET;
        }
    }
    super::UV_DIRENT_UNKNOWN
}

pub(crate) fn fstat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let bigint = bigint_option(context, args.get(1).copied())?;
    let descriptor_metadata = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let metadata = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.file.metadata());
        metadata
    };
    let metadata = match descriptor_metadata {
        Some(metadata) => metadata,
        None => {
            #[cfg(unix)]
            let metadata = {
                use std::os::fd::BorrowedFd;
                let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
                borrowed
                    .try_clone_to_owned()
                    .map(std::fs::File::from)
                    .and_then(|file| file.metadata())
            };
            #[cfg(not(unix))]
            let metadata = Err(io::Error::from_raw_os_error(libc::EBADF));
            metadata
        }
    };
    match metadata {
        Ok(metadata) => metadata_stats(context, Some(&metadata), bigint),
        // fstat has no path argument, so Node omits the quoted path suffix and
        // the `path` property from system errors for this syscall.
        Err(error) => super::sync::throw_io_error(context, "fstat", None, &error),
    }
}

pub(crate) fn read_link_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = super::resolve_shared_path(context, path);
    match std::fs::read_link(&path) {
        Ok(target) => Ok(context.string_rooted(&target.to_string_lossy())),
        Err(error) => Err(path_error(context, error, &path, "readlink")?),
    }
}

fn bigint_option(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<bool, RootedError> {
    let Some(options) = options else {
        return Ok(false);
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(false);
    }
    let key = context.string_rooted("bigint");
    let value = context.get_property_rooted(options, key)?;
    Ok(context
        .rooted_value(value)
        .and_then(|value| value.as_bool())
        == Some(true))
}

fn stat_time_ms(time: i128) -> f64 {
    time.div_euclid(NANOSECONDS_PER_SECOND) as f64 * MILLISECONDS_PER_SECOND
        + time.rem_euclid(NANOSECONDS_PER_SECOND) as f64 / NANOSECONDS_PER_MILLISECOND as f64
}

fn path_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: io::Error,
    path: &str,
    syscall: &str,
) -> Result<RootedError, RootedError> {
    let (code, _) = crate::modules::fs_error_codes::code_for(&error);
    let description = crate::modules::fs_error_codes::strerror(code);
    let message = if path.is_empty() {
        format!("{code}: {description}, {syscall}")
    } else {
        format!("{code}: {description}, {syscall} '{path}'")
    };
    let exception = context.error_rooted(&message)?;
    if let Some(errno) = error.raw_os_error() {
        set_number(context, exception, "errno", -f64::from(errno))?;
    }
    set_string(context, exception, "code", code)?;
    set_string(context, exception, "syscall", syscall)?;
    if !path.is_empty() {
        set_string(context, exception, "path", path)?;
    }
    Ok(context.throw(exception))
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if !context.set_property_rooted(object, key, value, object)? {
        return Err(RootedError::host("cannot set shared fs stat property"));
    }
    Ok(())
}

fn set_number(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), RootedError> {
    let value = context.number(value);
    set(context, object, name, value)
}

fn set_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}
