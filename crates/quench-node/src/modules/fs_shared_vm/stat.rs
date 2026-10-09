use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt,
    os::unix::fs::PermissionsExt,
};

#[cfg(not(unix))]
const S_IF_DIRECTORY: f64 = 0o040000 as f64;
#[cfg(not(unix))]
const S_IF_REGULAR: f64 = 0o100000 as f64;

const STAT_API: &str = r#"(readMetadata) => {
  function makeStats(metadata) {
    const stats = {
      dev: metadata.dev,
      ino: metadata.ino,
      mode: metadata.mode,
      nlink: metadata.nlink,
      uid: metadata.uid,
      gid: metadata.gid,
      rdev: metadata.rdev,
      size: metadata.size,
      blksize: metadata.blksize,
      blocks: metadata.blocks,
      atimeMs: metadata.atimeMs,
      mtimeMs: metadata.mtimeMs,
      ctimeMs: metadata.ctimeMs,
      birthtimeMs: metadata.birthtimeMs,
      atime: new Date(metadata.atimeMs),
      mtime: new Date(metadata.mtimeMs),
      ctime: new Date(metadata.ctimeMs),
      birthtime: new Date(metadata.birthtimeMs),
    };
    stats.isDirectory = () => metadata.isDirectory;
    return stats;
  }

  return function stat(path, callback) {
    if (typeof callback !== 'function') {
      throw new TypeError('The "cb" argument must be of type function');
    }
    setImmediate(() => {
      let result;
      try {
        result = makeStats(readMetadata(path));
      } catch (error) {
        callback(error);
        return;
      }
      callback(null, result);
    });
  };
}"#;

const DECORATE_STATS: &str = quench_js_check::checked_js!(r#"(stats) => {
  for (const name of ["atime", "mtime", "ctime", "birthtime"]) {
    stats[name] = new Date(stats[`${name}Ms`]);
  }
  for (const name of ["isDirectory", "isFile", "isSymbolicLink", "isBlockDevice", "isCharacterDevice", "isFIFO", "isSocket"]) {
    const result = stats[name];
    Object.defineProperty(stats, name, { value: () => result, configurable: true });
  }
  return stats;
}"#);

const STATS_CONSTRUCTOR: &str = quench_js_check::checked_js!(r#"(emitWarning) => {
  function Stats(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atime, mtime, ctime, birthtime) {
    emitWarning('fs.Stats constructor is deprecated.', { type: 'DeprecationWarning', code: 'DEP0180' });
    for (const [name, value] of Object.entries({ dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks })) this[name] = value;
    for (const [name, value] of Object.entries({ atime, mtime, ctime, birthtime })) {
      this[`${name}Ms`] = value;
      this[name] = new Date(value);
    }
    for (const name of ['isDirectory', 'isFile', 'isSymbolicLink', 'isBlockDevice', 'isCharacterDevice', 'isFIFO', 'isSocket']) this[name] = () => false;
  }
  return Stats;
}"#);

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let host_operation =
        context.host_function(crate::host::shared_vm::operation("fsStatMetadata"))?;
    let factory = context.evaluate_script_rooted(STAT_API, "node:fs/shared-stat.js")?;
    let undefined = context.undefined();
    let stat = context.call_rooted(factory, undefined, &[host_operation])?;
    set(context, module, "stat", stat)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let emit_warning = get(context, process, "emitWarning")?;
    let constructor_factory =
        context.evaluate_script_rooted(STATS_CONSTRUCTOR, "node:fs/shared-stats-constructor.js")?;
    let stats = context.call_rooted(constructor_factory, undefined, &[emit_warning])?;
    set(context, module, "Stats", stats)
}

pub(crate) fn metadata(
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
    match std::fs::metadata(&path) {
        Ok(metadata) => stats(context, &metadata),
        Err(error) => Err(stat_error(context, error, &path)?),
    }
}

pub(crate) fn stat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    sync_metadata(context, args, false)
}

pub(crate) fn statfs_sync(
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
                        ("type", stats.f_type as f64),
                        ("bsize", stats.f_bsize as f64),
                        ("frsize", stats.f_frsize as f64),
                        ("blocks", stats.f_blocks as f64),
                        ("bfree", stats.f_bfree as f64),
                        ("bavail", stats.f_bavail as f64),
                        ("files", stats.f_files as f64),
                        ("ffree", stats.f_ffree as f64),
                    ])
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            Err(_) => Err(io::Error::from_raw_os_error(libc::EINVAL)),
        }
    };
    #[cfg(not(target_os = "linux"))]
    let result: Result<[(&str, f64); 8], io::Error> =
        Err(io::Error::from_raw_os_error(libc::ENOSYS));
    match result {
        Ok(values) => {
            let object = context.object_rooted()?;
            for (name, value) in values {
                set_number(context, object, name, value)?;
            }
            Ok(object)
        }
        Err(error) => Err(path_error(context, error, &path, "statfs")?),
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

fn truncate_length(
    context: &NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> u64 {
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
        Err(error) => Err(path_error(context, error, &path, "access")?),
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
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = super::resolve_shared_path(context, path);
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
            if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
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
            &path,
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
                if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
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
        .map(|value| if value == -1.0 { u32::MAX } else { value as u32 })
        .unwrap_or(u32::MAX)
}

pub(crate) fn lstat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    sync_metadata(context, args, true)
}

fn sync_metadata(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    follow_links: bool,
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = super::resolve_shared_path(context, path);
    let result = if follow_links {
        std::fs::symlink_metadata(&path)
    } else {
        std::fs::metadata(&path)
    };
    match result {
        Ok(metadata) => stats(context, &metadata),
        Err(error) => Err(stat_error(context, error, &path)?),
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
    let path = super::resolve_shared_path(context, path);
    match std::fs::canonicalize(&path) {
        Ok(canonical) => Ok(context.string_rooted(&canonical.to_string_lossy())),
        Err(error) => Err(stat_error(context, error, &path)?),
    }
}

pub(crate) fn read_dir_sync(
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
    let entries = match std::fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => return Err(stat_error(context, error, &path)?),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Err(stat_error(context, error, &path)?),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => return Err(stat_error(context, error, &path)?),
        };
        let dirent = context.object_rooted()?;
        set_string(context, dirent, "name", &entry.file_name().to_string_lossy())?;
        set_bool(context, dirent, "isFile", metadata.is_file())?;
        set_bool(context, dirent, "isDirectory", metadata.is_dir())?;
        set_bool(context, dirent, "isSymbolicLink", metadata.file_type().is_symlink())?;
        for name in ["isBlockDevice", "isCharacterDevice", "isFIFO", "isSocket"] {
            set_bool(context, dirent, name, false)?;
        }
        names.push(dirent);
    }
    context.array_rooted(&names)
}

pub(crate) fn fstat_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let descriptor_metadata = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let descriptor = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| (descriptor.file.metadata(), descriptor.path.clone()));
        descriptor
    };
    let (metadata, path) = match descriptor_metadata {
        Some((metadata, path)) => (metadata, path),
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
            (metadata, String::new())
        }
    };
    match metadata {
        Ok(metadata) => stats(context, &metadata),
        Err(error) => Err(super::stream_io_error(context, error, "fstat", &path)?),
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
        Err(error) => Err(stat_error(context, error, &path)?),
    }
}

fn stats(
    context: &mut NativeContext<'_, NodeHost>,
    metadata: &std::fs::Metadata,
) -> Result<RootId, RootedError> {
    let snapshot = StatSnapshot::from(metadata);
    let result = context.object_rooted()?;
    for (name, value) in [
        ("dev", snapshot.dev),
        ("ino", snapshot.ino),
        ("mode", snapshot.mode),
        ("nlink", snapshot.nlink),
        ("uid", snapshot.uid),
        ("gid", snapshot.gid),
        ("rdev", snapshot.rdev),
        ("size", snapshot.size),
        ("blksize", snapshot.blksize),
        ("blocks", snapshot.blocks),
        ("atimeMs", snapshot.atime_ms),
        ("mtimeMs", snapshot.mtime_ms),
        ("ctimeMs", snapshot.ctime_ms),
        ("birthtimeMs", snapshot.birthtime_ms),
    ] {
        set_number(context, result, name, value)?;
    }
    set_bool(context, result, "isDirectory", snapshot.is_directory)?;
    set_bool(context, result, "isFile", metadata.is_file())?;
    set_bool(context, result, "isSymbolicLink", metadata.file_type().is_symlink())?;
    for name in ["isBlockDevice", "isCharacterDevice", "isFIFO", "isSocket"] {
        set_bool(context, result, name, false)?;
    }
    let decorator = context.evaluate_script_rooted(DECORATE_STATS, "node:fs/shared-stat-methods.js")?;
    let undefined = context.undefined();
    context.call_rooted(decorator, undefined, &[result])
}

struct StatSnapshot {
    mode: f64,
    dev: f64,
    ino: f64,
    nlink: f64,
    uid: f64,
    gid: f64,
    rdev: f64,
    size: f64,
    blksize: f64,
    blocks: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
    is_directory: bool,
}

#[cfg(unix)]
impl From<&std::fs::Metadata> for StatSnapshot {
    fn from(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        let created_ms = metadata.created().ok().map(system_time_ms).unwrap_or(0.0);
        Self {
            mode: metadata.mode() as f64,
            dev: metadata.dev() as f64,
            ino: metadata.ino() as f64,
            nlink: metadata.nlink() as f64,
            uid: metadata.uid() as f64,
            gid: metadata.gid() as f64,
            rdev: metadata.rdev() as f64,
            size: metadata.len() as f64,
            blksize: metadata.blksize() as f64,
            blocks: metadata.blocks() as f64,
            atime_ms: unix_time_ms(metadata.atime(), metadata.atime_nsec()),
            mtime_ms: unix_time_ms(metadata.mtime(), metadata.mtime_nsec()),
            ctime_ms: unix_time_ms(metadata.ctime(), metadata.ctime_nsec()),
            birthtime_ms: created_ms,
            is_directory: metadata.is_dir(),
        }
    }
}

#[cfg(not(unix))]
impl From<&std::fs::Metadata> for StatSnapshot {
    fn from(metadata: &std::fs::Metadata) -> Self {
        let time = |value: Option<SystemTime>| value.map(system_time_ms).unwrap_or(0.0);
        let created_ms = time(metadata.created().ok());
        Self {
            mode: if metadata.is_dir() {
                S_IF_DIRECTORY
            } else {
                S_IF_REGULAR
            },
            dev: 0.0,
            ino: 0.0,
            nlink: 1.0,
            uid: 0.0,
            gid: 0.0,
            rdev: 0.0,
            size: metadata.len() as f64,
            blksize: 0.0,
            blocks: 0.0,
            atime_ms: time(metadata.accessed().ok()),
            mtime_ms: time(metadata.modified().ok()),
            ctime_ms: created_ms,
            birthtime_ms: created_ms,
            is_directory: metadata.is_dir(),
        }
    }
}

#[cfg(unix)]
fn unix_time_ms(seconds: i64, nanoseconds: i64) -> f64 {
    seconds as f64 * 1_000.0 + nanoseconds as f64 / 1_000_000.0
}

fn system_time_ms(time: SystemTime) -> f64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            duration.as_secs() as f64 * 1_000.0 + duration.subsec_nanos() as f64 / 1_000_000.0
        }
        Err(error) => {
            let duration = error.duration();
            -(duration.as_secs() as f64 * 1_000.0 + duration.subsec_nanos() as f64 / 1_000_000.0)
        }
    }
}

fn stat_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: io::Error,
    path: &str,
) -> Result<RootedError, RootedError> {
    let code = error_code(&error);
    let description = match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::NotADirectory => "not a directory",
        io::ErrorKind::IsADirectory => "illegal operation on a directory",
        _ if error.raw_os_error() == Some(libc::EBADF) => "bad file descriptor",
        _ => "input/output error",
    };
    let exception = context.error_rooted(&format!("{code}: {description}, stat '{path}'"))?;
    set_string(context, exception, "code", code)?;
    set_string(context, exception, "syscall", "stat")?;
    set_string(context, exception, "path", path)?;
    if let Some(errno) = error.raw_os_error() {
        set_number(context, exception, "errno", -f64::from(errno))?;
    }
    Ok(context.throw(exception))
}

fn path_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: io::Error,
    path: &str,
    syscall: &str,
) -> Result<RootedError, RootedError> {
    let code = error_code(&error);
    let description = match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::NotADirectory => "not a directory",
        _ => "input/output error",
    };
    let exception = context.error_rooted(&format!("{code}: {description}, {syscall} '{path}'"))?;
    set_string(context, exception, "code", code)?;
    set_string(context, exception, "syscall", syscall)?;
    set_string(context, exception, "path", path)?;
    if let Some(errno) = error.raw_os_error() {
        set_number(context, exception, "errno", -f64::from(errno))?;
    }
    Ok(context.throw(exception))
}

fn error_code(error: &io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(libc::EACCES) => "EACCES",
        Some(libc::EBADF) => "EBADF",
        Some(libc::EEXIST) => "EEXIST",
        Some(libc::EISDIR) => "EISDIR",
        Some(libc::EINVAL) => "EINVAL",
        Some(libc::ENOTDIR) => "ENOTDIR",
        Some(libc::ENOENT) => "ENOENT",
        _ => match error.kind() {
            io::ErrorKind::NotFound => "ENOENT",
            io::ErrorKind::PermissionDenied => "EACCES",
            io::ErrorKind::NotADirectory => "ENOTDIR",
            _ => "EIO",
        },
    }
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

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
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

fn set_bool(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = context.boolean(value);
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
