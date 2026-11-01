//! The operations of `fs`: each validates its arguments as Node's library does, then does
//! its work on the host and reports a reply or the error the work failed with. The sync,
//! callback and promise APIs (`api`) are projections of these.

use super::api::{Call, Flavor, Reply, Transfer};
use super::args::{self, Encoding, FsPath, UTF8};
use super::failure::{self, Coded, Syscall};
use crate::modules::stream_shared_vm::{get, is_nullish, is_undefined, Context};
use quench_runtime::{RootId, RootedError};
use std::ffi::CString;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The default creation modes of files and directories.
const DEFAULT_FILE_MODE: u32 = 0o666;
pub(crate) const DEFAULT_DIRECTORY_MODE: u32 = 0o777;
/// `kMaxUserId`: the largest uid and gid, `2 ** 32 - 1`.
const MAX_USER_ID: f64 = u32::MAX as f64;
/// The bounds of `access` modes (`F_OK` to `R_OK | W_OK | X_OK`) and `copyFile` modes
/// (none to every `COPYFILE_*` flag).
const MAX_ACCESS_MODE: i32 = libc::R_OK | libc::W_OK | libc::X_OK;
pub(crate) const MAX_COPY_MODE: i32 = super::UV_FS_COPYFILE_EXCL | super::UV_FS_COPYFILE_FICLONE | super::UV_FS_COPYFILE_FICLONE_FORCE;
/// `kIoMaxLength`: the largest file `readFile` reads, 2 GiB.
pub(crate) const MAX_READ_FILE_BYTES: u64 = 1 << 31;
/// The buffer `fs.read(fd, callback)` and `filehandle.read()` allocate, 16 KiB.
const DEFAULT_READ_BUFFER: usize = 16 * 1024;
/// The position meaning "the descriptor's current offset".
const CURRENT_POSITION: f64 = -1.0;
/// The most positional arguments an operation reads.
const MAX_PARAMETERS: usize = 5;
/// The random suffix `mkdtemp(3)` replaces.
const TEMPORARY_SUFFIX: &str = "XXXXXX";

/// Every operation `fs` exposes through its sync, callback and promise APIs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Access,
    Exists,
    Chmod,
    Fchmod,
    Lchmod,
    Chown,
    Fchown,
    Lchown,
    Utimes,
    Futimes,
    Lutimes,
    Stat,
    Fstat,
    Lstat,
    Statfs,
    Readdir,
    Mkdir,
    Rmdir,
    Rm,
    Mkdtemp,
    CopyFile,
    Cp,
    Symlink,
    Link,
    Rename,
    Unlink,
    Readlink,
    Realpath,
    RealpathNative,
    Truncate,
    Ftruncate,
    Open,
    Close,
    Read,
    Write,
    Readv,
    Writev,
    Fsync,
    Fdatasync,
    ReadFile,
    WriteFile,
    AppendFile,
    Opendir,
    Glob,
}

impl Op {
    pub(crate) const ALL: [Self; 44] = [
        Self::Access, Self::Exists, Self::Chmod, Self::Fchmod, Self::Lchmod, Self::Chown,
        Self::Fchown, Self::Lchown, Self::Utimes, Self::Futimes, Self::Lutimes, Self::Stat,
        Self::Fstat, Self::Lstat, Self::Statfs, Self::Readdir, Self::Mkdir, Self::Rmdir, Self::Rm,
        Self::Mkdtemp, Self::CopyFile, Self::Cp, Self::Symlink, Self::Link, Self::Rename,
        Self::Unlink, Self::Readlink, Self::Realpath, Self::RealpathNative, Self::Truncate,
        Self::Ftruncate, Self::Open, Self::Close, Self::Read, Self::Write, Self::Readv,
        Self::Writev, Self::Fsync, Self::Fdatasync, Self::ReadFile, Self::WriteFile,
        Self::AppendFile, Self::Opendir, Self::Glob,
    ];

    /// The callback API's positional parameters: how many come before the earliest the
    /// callback may take the place of, and how many there are.
    pub(crate) fn parameters(self) -> (usize, usize) {
        match self {
            Self::Exists | Self::Unlink | Self::Close | Self::Fsync | Self::Fdatasync => (1, 1),
            Self::Chmod | Self::Fchmod | Self::Lchmod | Self::Link | Self::Rename => (2, 2),
            Self::Chown | Self::Fchown | Self::Lchown | Self::Utimes | Self::Futimes | Self::Lutimes => (3, 3),
            Self::Symlink | Self::CopyFile | Self::Cp | Self::WriteFile | Self::AppendFile => (2, 3),
            Self::Readv | Self::Writev => (2, 3),
            Self::Open => (1, 3),
            Self::Read => (1, 5),
            Self::Write => (2, 5),
            _ => (1, 2),
        }
    }

    /// The operation over `given`, whose missing arguments are `undefined`.
    pub(crate) fn call(self, c: &mut Context<'_>, given: &[RootId], flavor: Flavor) -> Call {
        let mut padded = given.to_vec();
        padded.resize(given.len().max(MAX_PARAMETERS), c.undefined());
        let args = &padded[..];
        match self {
            Self::Access => access(c, args),
            Self::Exists => exists(c, args),
            Self::Chmod => chmod(c, args),
            Self::Lchmod => Err(failure::plain(c, "ERR_METHOD_NOT_IMPLEMENTED", "The lchmod() method is not implemented", false)),
            Self::Fchmod => fchmod(c, args),
            Self::Chown => chown(c, args, Some(Follow::Yes)),
            Self::Lchown => chown(c, args, Some(Follow::No)),
            Self::Fchown => chown(c, args, None),
            Self::Utimes => utimes(c, args, Some(Follow::Yes)),
            Self::Lutimes => utimes(c, args, Some(Follow::No)),
            Self::Futimes => utimes(c, args, None),
            Self::Stat => stat(c, args, Follow::Yes, flavor),
            Self::Lstat => stat(c, args, Follow::No, flavor),
            Self::Fstat => fstat(c, args),
            Self::Statfs => statfs(c, args),
            Self::Readdir => readdir(c, args),
            Self::Mkdir => mkdir(c, args),
            Self::Rmdir => rmdir(c, args),
            Self::Rm => rm(c, args),
            Self::Mkdtemp => mkdtemp(c, args),
            Self::CopyFile => copy_file(c, args),
            Self::Cp => super::copy::cp(c, args, flavor == Flavor::Sync),
            Self::Symlink => symlink(c, args),
            Self::Link => link(c, args, Op::Link),
            Self::Rename => link(c, args, Op::Rename),
            Self::Unlink => unlink(c, args),
            Self::Readlink => readlink(c, args),
            Self::Realpath => realpath(c, args, false),
            Self::RealpathNative => realpath(c, args, true),
            Self::Truncate => truncate(c, args),
            Self::Ftruncate => ftruncate(c, args),
            Self::Open => open(c, args),
            Self::Close => close(c, args),
            Self::Read => read(c, args, given.len(), flavor),
            Self::Write => write(c, args, flavor),
            Self::Readv => vector(c, args, Transfer::Readv),
            Self::Writev => vector(c, args, Transfer::Writev),
            Self::Fsync => sync(c, args, false),
            Self::Fdatasync => sync(c, args, true),
            Self::ReadFile => read_file(c, args, flavor),
            Self::WriteFile => write_file(c, args, false),
            Self::AppendFile => write_file(c, args, true),
            Self::Opendir => super::dir::opendir(c, args),
            Self::Glob => super::glob::glob(c, args),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Follow {
    Yes,
    No,
}

fn value(value: RootId) -> Result<Reply, RootedError> {
    Ok(Reply::Value(value))
}

fn nothing(c: &mut Context<'_>) -> Result<Reply, RootedError> {
    value(c.undefined())
}

/// The outcome of a host call that returns nothing.
fn completed(c: &mut Context<'_>, result: io::Result<()>, call: Syscall<'_>) -> Result<Reply, RootedError> {
    failure::check(c, result, call)?;
    nothing(c)
}

pub(crate) fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

/// A libc call's status as a result.
fn status(code: libc::c_int) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Run `operation` on the descriptor `fd` from the host's descriptor table.
pub(crate) fn with_descriptor<T>(c: &mut Context<'_>, fd: i32, operation: impl FnOnce(&mut std::fs::File) -> io::Result<T>) -> io::Result<T> {
    let fs = c.host_mut().shared_state().borrow().fs.clone();
    fs.with_descriptor(fd, operation)
}

fn raw_descriptor(c: &mut Context<'_>, fd: i32, operation: impl FnOnce(libc::c_int) -> libc::c_int) -> io::Result<()> {
    with_descriptor(c, fd, |file| status(operation(std::os::fd::AsRawFd::as_raw_fd(file))))
}

/// `getValidMode(mode, type)`: absent is `default`, else an integer within `maximum`.
pub(crate) fn valid_mode(c: &mut Context<'_>, mode: RootId, maximum: i32, default: i32) -> Result<i32, RootedError> {
    if is_nullish(c, mode) {
        return Ok(default);
    }
    let Some(number) = c.rooted_value(mode).and_then(|value| value.as_number()) else {
        let error = c.type_error_rooted("mode must be int32 or null/undefined")?;
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_TYPE")?;
        return Err(c.throw(error));
    };
    let mode = number.trunc();
    if !(0.0..=f64::from(maximum)).contains(&mode) {
        return Err(failure::plain(c, "ERR_OUT_OF_RANGE", &format!("mode is out of range: >= 0 && <= {maximum}"), true));
    }
    Ok(mode as i32)
}

fn access(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let mode = valid_mode(c, args[1], MAX_ACCESS_MODE, libc::F_OK)?;
    let result = c_path(&path.io).and_then(|target| status(unsafe { libc::access(target.as_ptr(), mode) }));
    Ok(completed(c, result, Syscall::at("access", &path.shown)))
}

/// `existsSync(path)`: whether `access(path, F_OK)` succeeds; an invalid path is `false`.
fn exists(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let exists = match crate::modules::stream_shared_vm::attempt(args::path(c, args[0], "path"))? {
        Ok(path) => c_path(&path.io).is_ok_and(|target| unsafe { libc::access(target.as_ptr(), libc::F_OK) } == 0),
        Err(_) => false,
    };
    Ok(Ok(Reply::Exists(exists)))
}

fn chmod(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let mode = args::mode(c, args[1], "mode", None)?;
    let result = std::fs::set_permissions(&path.io, std::fs::Permissions::from_mode(mode));
    Ok(completed(c, result, Syscall::at("chmod", &path.shown)))
}

fn fchmod(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let fd = args::fd(c, args[0])?;
    let mode = args::mode(c, args[1], "mode", None)?;
    let result = raw_descriptor(c, fd, |raw| unsafe { libc::fchmod(raw, mode as libc::mode_t) });
    Ok(completed(c, result, Syscall::bare("fchmod")))
}

/// The file an ownership or time change applies to: a path, or a descriptor.
enum Target {
    Path(FsPath, Follow),
    Descriptor(i32),
}

fn target(c: &mut Context<'_>, value: RootId, follow: Option<Follow>) -> Result<Target, RootedError> {
    Ok(match follow {
        Some(follow) => Target::Path(args::path(c, value, "path")?, follow),
        None => Target::Descriptor(args::fd(c, value)?),
    })
}

fn chown(c: &mut Context<'_>, args: &[RootId], follow: Option<Follow>) -> Call {
    let target = target(c, args[0], follow)?;
    let uid = args::integer(c, args[1], "uid", -1.0, MAX_USER_ID)? as i64 as libc::uid_t;
    let gid = args::integer(c, args[2], "gid", -1.0, MAX_USER_ID)? as i64 as libc::gid_t;
    Ok(match target {
        Target::Path(path, follow) => {
            let result = c_path(&path.io).and_then(|raw| {
                status(match follow {
                    Follow::Yes => unsafe { libc::chown(raw.as_ptr(), uid, gid) },
                    Follow::No => unsafe { libc::lchown(raw.as_ptr(), uid, gid) },
                })
            });
            let name = if follow == Follow::Yes { "chown" } else { "lchown" };
            completed(c, result, Syscall::at(name, &path.shown))
        }
        Target::Descriptor(fd) => {
            let result = raw_descriptor(c, fd, |raw| unsafe { libc::fchown(raw, uid, gid) });
            completed(c, result, Syscall::bare("fchown"))
        }
    })
}

/// `ToTimespec(seconds)`: whole seconds and the nanoseconds of the fraction.
fn timespec(seconds: f64) -> libc::timespec {
    const NANOSECONDS_PER_SECOND: f64 = 1e9;
    let whole = seconds.floor();
    libc::timespec { tv_sec: whole as libc::time_t, tv_nsec: ((seconds - whole) * NANOSECONDS_PER_SECOND) as libc::c_long }
}

fn utimes(c: &mut Context<'_>, args: &[RootId], follow: Option<Follow>) -> Call {
    let target = target(c, args[0], follow)?;
    let atime = args::timestamp(c, args[1], "time")?;
    let mtime = args::timestamp(c, args[2], "time")?;
    let times = [timespec(atime), timespec(mtime)];
    Ok(match target {
        Target::Path(path, follow) => {
            let flags = if follow == Follow::Yes { 0 } else { libc::AT_SYMLINK_NOFOLLOW };
            let result = c_path(&path.io)
                .and_then(|raw| status(unsafe { libc::utimensat(libc::AT_FDCWD, raw.as_ptr(), times.as_ptr(), flags) }));
            let name = if follow == Follow::Yes { "utime" } else { "lutime" };
            completed(c, result, Syscall::at(name, &path.shown))
        }
        Target::Descriptor(fd) => {
            let result = raw_descriptor(c, fd, |raw| unsafe { libc::futimens(raw, times.as_ptr()) });
            completed(c, result, Syscall::bare("futime"))
        }
    })
}

/// `options.bigint` and `options.throwIfNoEntry` of a stat call.
fn stat_options(c: &mut Context<'_>, options: RootId) -> Result<(bool, bool), RootedError> {
    if is_undefined(c, options) {
        return Ok((false, true));
    }
    let bigint = get(c, options, "bigint")?;
    let bigint = c.truthy_rooted(bigint)?;
    let throw = get(c, options, "throwIfNoEntry")?;
    let throw = !c.rooted_value(throw).is_some_and(|value| value.as_bool() == Some(false));
    Ok((bigint, throw))
}

fn stats_reply(c: &mut Context<'_>, metadata: &std::fs::Metadata, bigint: bool) -> Result<Reply, RootedError> {
    let record = super::stats::StatRecord::of(Some(metadata));
    super::stats::stats(c, &record, bigint).map(Reply::Value)
}

pub(crate) fn metadata(path: &Path, follow: bool) -> io::Result<std::fs::Metadata> {
    if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) }
}

fn missing(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR))
}

fn stat(c: &mut Context<'_>, args: &[RootId], follow: Follow, flavor: Flavor) -> Call {
    let path = args::path(c, args[0], "path")?;
    let (bigint, throw) = stat_options(c, args[1])?;
    let name = if follow == Follow::Yes { "stat" } else { "lstat" };
    Ok(match metadata(&path.io, follow == Follow::Yes) {
        Ok(metadata) => stats_reply(c, &metadata, bigint),
        Err(error) if !throw && flavor == Flavor::Sync && missing(&error) => nothing(c),
        Err(error) => Err(failure::system(c, &error, Syscall::at(name, &path.shown))),
    })
}

fn fstat(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let fd = args::fd(c, args[0])?;
    let (bigint, _) = stat_options(c, args[1])?;
    Ok(match with_descriptor(c, fd, |file| file.metadata()) {
        Ok(metadata) => stats_reply(c, &metadata, bigint),
        Err(error) => Err(failure::system(c, &error, Syscall::bare("fstat"))),
    })
}

fn statfs(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let (bigint, _) = stat_options(c, args[1])?;
    let mut buffer = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let result = c_path(&path.io).and_then(|raw| status(unsafe { libc::statfs(raw.as_ptr(), buffer.as_mut_ptr()) }));
    Ok(match result {
        Ok(()) => {
            // SAFETY: `statfs` succeeded and filled the buffer.
            let fields = unsafe { buffer.assume_init() };
            let values = [
                fields.f_type as i128,
                fields.f_bsize as i128,
                fields.f_blocks as i128,
                fields.f_bfree as i128,
                fields.f_bavail as i128,
                fields.f_files as i128,
                fields.f_ffree as i128,
            ];
            super::stats::statfs(c, values, bigint).map(Reply::Value)
        }
        Err(error) => Err(failure::system(c, &error, Syscall::at("statfs", &path.shown))),
    })
}

/// `path.join(parent, name)` for the paths `Dirent`s and errors show.
pub(crate) fn join_shown(parent: &str, name: &str) -> String {
    crate::modules::path_algorithms::posix::join_strings(&[parent.to_owned(), name.to_owned()])
}

/// One directory a recursive listing visits: where it is, how errors and dirents show it,
/// and its path relative to the root (the prefix of a recursive name).
struct Visit {
    io: PathBuf,
    shown: String,
    relative: PathBuf,
}

/// `readdir(path, { encoding, withFileTypes, recursive })`: the names (or `Dirent`s) in
/// libuv's sorted order; a recursive listing visits subdirectories breadth first.
pub(crate) fn list_directory(c: &mut Context<'_>, path: &FsPath, recursive: bool, with_types: bool, encoding: Encoding) -> Result<RootId, RootedError> {
    let mut queue = vec![Visit { io: path.io.clone(), shown: path.shown.clone(), relative: PathBuf::new() }];
    let mut results = Vec::new();
    let mut index = 0;
    while index < queue.len() {
        let entries = crate::modules::fs_directory_shared_vm::scan(&queue[index].io);
        let entries = failure::check(c, entries, Syscall::at("scandir", &queue[index].shown))?;
        for entry in entries {
            let child = queue[index].io.join(&entry.name);
            let kind = super::dir::resolve_kind(entry.kind, &child);
            let descend = recursive
                && if with_types { kind == super::UV_DIRENT_DIR } else { std::fs::metadata(&child).is_ok_and(|metadata| metadata.is_dir()) };
            let relative = queue[index].relative.join(&entry.name);
            if with_types {
                let name = encoding.project(c, entry.name.as_bytes().to_vec())?;
                let shown = queue[index].shown.clone();
                results.push(super::dir::dirent(c, name, kind, &shown)?);
            } else {
                let name = if recursive { relative.as_os_str() } else { entry.name.as_os_str() };
                results.push(encoding.project(c, name.as_bytes().to_vec())?);
            }
            if descend {
                let shown = join_shown(&queue[index].shown, &entry.name.to_string_lossy());
                queue.push(Visit { io: child, shown, relative });
            }
        }
        index += 1;
    }
    c.array_rooted(&results)
}

fn readdir(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let options = args::options(c, args[1])?;
    let path = args::path(c, args[0], "path")?;
    let recursive = get(c, options, "recursive")?;
    let recursive = if is_nullish(c, recursive) { false } else { args::boolean_option(c, options, "recursive", false)? };
    let with_types = get(c, options, "withFileTypes")?;
    let with_types = c.truthy_rooted(with_types)?;
    let encoding = args::encoding(c, options, UTF8)?;
    Ok(list_directory(c, &path, recursive, with_types, encoding).map(Reply::Value))
}

/// `mkdir(path, mode)`, subject to the process umask.
fn make_directory(path: &Path, mode: u32) -> io::Result<()> {
    c_path(path).and_then(|raw| status(unsafe { libc::mkdir(raw.as_ptr(), mode as libc::mode_t) }))
}

/// `mkdir -p`: the first directory it created, if any. A missing ancestor is made first; an
/// existing non-directory fails as `mkdir` fails on it.
pub(crate) fn make_directories(path: &Path, mode: u32) -> io::Result<Option<PathBuf>> {
    match make_directory(path, mode) {
        Ok(()) => return Ok(Some(path.to_owned())),
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
            return if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) { Ok(None) } else { Err(error) };
        }
        Err(error) if error.raw_os_error() != Some(libc::ENOENT) => return Err(error),
        Err(_) => {}
    }
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or_else(|| io::Error::from_raw_os_error(libc::ENOENT))?;
    let first = make_directories(parent, mode)?;
    match make_directory(path, mode) {
        Ok(()) => Ok(first.or_else(|| Some(path.to_owned()))),
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) => Ok(first),
        Err(error) => Err(error),
    }
}

fn mkdir(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let options = args[1];
    let mut mode = DEFAULT_DIRECTORY_MODE;
    let mut recursive = false;
    if c.rooted_value(options).is_some_and(|value| value.as_number().is_some()) || c.string_text(options)?.is_some() {
        mode = args::mode(c, options, "mode", None)?;
    } else if c.truthy_rooted(options)? {
        recursive = args::boolean_option(c, options, "recursive", false)?;
        let value = get(c, options, "mode")?;
        mode = args::mode(c, value, "options.mode", Some(DEFAULT_DIRECTORY_MODE))?;
    }
    let path = args::path(c, args[0], "path")?;
    if !recursive {
        let result = make_directory(&path.io, mode);
        return Ok(completed(c, result, Syscall::at("mkdir", &path.shown)));
    }
    Ok(match make_directories(&path.io, mode) {
        Ok(Some(first)) => {
            // The first directory made, as the path the caller gave names it.
            let below = path.io.components().count() - first.components().count();
            let shown = (0..below).fold(path.shown.clone(), |shown, _| crate::modules::path_algorithms::posix::dirname_str(&shown));
            value(c.string_rooted(&shown))
        }
        Ok(None) => nothing(c),
        Err(error) => Err(failure::system(c, &error, Syscall::at("mkdir", &path.shown))),
    })
}

fn rmdir(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let recursive = if c.is_object_rooted(args[1])? { get(c, args[1], "recursive")? } else { c.undefined() };
    let result = if c.truthy_rooted(recursive)? {
        let undefined = c.undefined();
        super::emit_deprecation(c, "In future versions of Node.js, fs.rmdir(path, { recursive: true }) will be removed. Use fs.rm(path, { recursive: true }) instead", "DEP0147", undefined)?;
        remove_tree(&path.io)
    } else {
        std::fs::remove_dir(&path.io)
    };
    Ok(completed(c, result, Syscall::at("rmdir", &path.shown)))
}

/// `rm`'s validated options.
struct RmOptions {
    force: bool,
    recursive: bool,
}

fn rm_options(c: &mut Context<'_>, options: RootId) -> Result<RmOptions, RootedError> {
    if is_undefined(c, options) {
        return Ok(RmOptions { force: false, recursive: false });
    }
    if is_nullish(c, options) || !c.is_object_rooted(options)? || c.is_array_rooted(options)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let force = args::boolean_option(c, options, "force", false)?;
    let recursive = args::boolean_option(c, options, "recursive", false)?;
    for name in ["maxRetries", "retryDelay"] {
        let value = get(c, options, name)?;
        if !is_undefined(c, value) {
            args::integer(c, value, &format!("options.{name}"), 0.0, args::MAX_UINT32)?;
        }
    }
    Ok(RmOptions { force, recursive })
}

/// Remove `path`, recursing into directories, never following a symbolic link.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

fn rm(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let options = rm_options(c, args[1])?;
    let metadata = match std::fs::symlink_metadata(&path.io) {
        Ok(metadata) => metadata,
        Err(error) if options.force && error.raw_os_error() == Some(libc::ENOENT) => return Ok(nothing(c)),
        Err(error) => return Ok(Err(failure::system(c, &error, Syscall::at("lstat", &path.shown)))),
    };
    if metadata.is_dir() && !options.recursive {
        return Ok(Err(failure::coded(c, Coded::IsDirectory, "is a directory", "rm", &path.shown)));
    }
    let result = match remove_tree(&path.io) {
        Err(error) if options.force && error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        result => result,
    };
    Ok(completed(c, result, Syscall::at("rm", &path.shown)))
}

fn mkdtemp(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let prefix = args::path(c, args[0], "prefix")?;
    let options = args::options(c, args[1])?;
    let encoding = args::encoding(c, options, UTF8)?;
    let mut template = prefix.io.into_os_string();
    template.push(TEMPORARY_SUFFIX);
    let shown = format!("{}{TEMPORARY_SUFFIX}", prefix.shown);
    let mut bytes = template.as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: `bytes` is a NUL-terminated template the call rewrites in place.
    let created = unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) };
    if created.is_null() {
        let error = io::Error::last_os_error();
        return Ok(Err(failure::system(c, &error, Syscall::at("mkdtemp", &shown))));
    }
    bytes.pop();
    // The directory keeps the prefix as given: only the suffix is new.
    let suffix = bytes.split_off(bytes.len() - TEMPORARY_SUFFIX.len());
    let mut result = prefix.shown.into_bytes();
    result.extend(suffix);
    Ok(encoding.project(c, result).map(Reply::Value))
}

/// `uv_fs_copyfile`: the source's bytes and mode in a new or truncated destination.
pub(crate) fn copy_file_contents(source: &Path, destination: &Path, mode: i32) -> io::Result<()> {
    let mut input = std::fs::File::open(source)?;
    let metadata = input.metadata()?;
    if metadata.is_dir() {
        return Err(io::Error::from_raw_os_error(libc::EISDIR));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(metadata.permissions().mode());
    if mode & super::UV_FS_COPYFILE_EXCL != 0 {
        options.create_new(true);
    }
    let mut output = options.open(destination)?;
    if mode & super::UV_FS_COPYFILE_FICLONE_FORCE != 0 {
        return Err(io::Error::from_raw_os_error(libc::ENOTSUP));
    }
    io::copy(&mut input, &mut output)?;
    output.set_permissions(metadata.permissions())
}

fn copy_file(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let source = args::path(c, args[0], "src")?;
    let destination = args::path(c, args[1], "dest")?;
    let mode = valid_mode(c, args[2], MAX_COPY_MODE, 0)?;
    let result = copy_file_contents(&source.io, &destination.io, mode);
    Ok(completed(c, result, Syscall::between("copyfile", &source.shown, &destination.shown)))
}

fn symlink(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let target = args::path_as_given(c, args[0], "target")?;
    let path = args::path(c, args[1], "path")?;
    let link_type = args[2];
    if !is_nullish(c, link_type) && !matches!(c.string_text(link_type)?.as_deref(), Some("dir" | "file" | "junction")) {
        let received = c.to_string(link_type)?;
        let message = format!("Symlink type must be one of \"dir\", \"file\", or \"junction\". Received \"{received}\"");
        return Err(failure::plain(c, "ERR_FS_INVALID_SYMLINK_TYPE", &message, false));
    }
    let result = std::os::unix::fs::symlink(&target.io, &path.io);
    Ok(completed(c, result, Syscall::between("symlink", &target.shown, &path.shown)))
}

fn link(c: &mut Context<'_>, args: &[RootId], op: Op) -> Call {
    let (from, to, name) = match op {
        Op::Link => ("existingPath", "newPath", "link"),
        _ => ("oldPath", "newPath", "rename"),
    };
    let source = args::path(c, args[0], from)?;
    let destination = args::path(c, args[1], to)?;
    let result = if op == Op::Link { std::fs::hard_link(&source.io, &destination.io) } else { std::fs::rename(&source.io, &destination.io) };
    Ok(completed(c, result, Syscall::between(name, &source.shown, &destination.shown)))
}

fn unlink(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let result = std::fs::remove_file(&path.io);
    Ok(completed(c, result, Syscall::at("unlink", &path.shown)))
}

fn readlink(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let options = args::options(c, args[1])?;
    let path = args::path(c, args[0], "oldPath")?;
    let encoding = args::encoding(c, options, UTF8)?;
    Ok(match std::fs::read_link(&path.io) {
        Ok(target) => encoding.project(c, target.as_os_str().as_bytes().to_vec()).map(Reply::Value),
        Err(error) => Err(failure::system(c, &error, Syscall::at("readlink", &path.shown))),
    })
}

/// Where `realpathSync`'s walk of the absolute `path` fails: the first prefix whose `lstat`
/// fails, or whose link `stat` cannot follow.
fn failing_prefix(path: &Path) -> Option<(&'static str, PathBuf, io::Error)> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        match std::fs::symlink_metadata(&prefix) {
            Err(error) => return Some(("lstat", prefix, error)),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if let Err(error) = std::fs::metadata(&prefix) {
                    return Some(("stat", prefix, error));
                }
            }
            Ok(_) => {}
        }
    }
    None
}

fn realpath(c: &mut Context<'_>, args: &[RootId], native: bool) -> Call {
    let path = args::path(c, args[0], "path")?;
    let options = args::options(c, args[1])?;
    let encoding = args::encoding(c, options, UTF8)?;
    Ok(match std::fs::canonicalize(&path.io) {
        Ok(resolved) => encoding.project(c, resolved.as_os_str().as_bytes().to_vec()).map(Reply::Value),
        Err(error) if !native => {
            let cwd = c.host_mut().shared_state().borrow().cwd.clone();
            let cwd = cwd.current_path().map_err(|error| RootedError::host(error.to_string()))?;
            let absolute = crate::modules::path_algorithms::posix::resolve_strings(std::slice::from_ref(&path.shown), &cwd.to_string_lossy());
            match failing_prefix(Path::new(&absolute)) {
                Some((syscall, prefix, error)) => Err(failure::system(c, &error, Syscall::at(syscall, &prefix.to_string_lossy()))),
                None => Err(failure::system(c, &error, Syscall::at("lstat", &absolute))),
            }
        }
        Err(error) => Err(failure::system(c, &error, Syscall::at("realpath", &path.shown))),
    })
}

/// `validateInteger(len, 'len')`, clamped at zero.
fn length_argument(c: &mut Context<'_>, value: RootId) -> Result<u64, RootedError> {
    if is_undefined(c, value) {
        return Ok(0);
    }
    let length = args::integer(c, value, "len", -args::MAX_SAFE_INTEGER, args::MAX_SAFE_INTEGER)?;
    Ok(length.max(0.0) as u64)
}

fn truncate(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let first = args[0];
    if c.rooted_value(first).is_some_and(|value| value.as_number().is_some()) {
        let undefined = c.undefined();
        super::emit_deprecation(c, "Using fs.truncate with a file descriptor is deprecated. Please use fs.ftruncate with a file descriptor instead.", "DEP0081", undefined)?;
        return ftruncate(c, args);
    }
    let path = args::path(c, first, "path")?;
    let length = length_argument(c, args[1])?;
    let file = std::fs::OpenOptions::new().read(true).write(true).open(&path.io);
    let file = match file {
        Ok(file) => file,
        Err(error) => return Ok(Err(failure::system(c, &error, Syscall::at("open", &path.shown)))),
    };
    Ok(completed(c, file.set_len(length), Syscall::bare("ftruncate")))
}

fn ftruncate(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let fd = args::fd(c, args[0])?;
    let length = length_argument(c, args[1])?;
    let result = with_descriptor(c, fd, |file| file.set_len(length));
    Ok(completed(c, result, Syscall::bare("ftruncate")))
}

/// `open(2)` with Node's flags (and close-on-exec, as libuv opens every file).
fn open_file(path: &Path, flags: i32, mode: u32) -> io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let path = c_path(path)?;
    // SAFETY: `path` is NUL-terminated and `mode` has the promoted C varargs type.
    let fd = unsafe { libc::open(path.as_ptr(), flags | libc::O_CLOEXEC, mode as libc::c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful open returned a new descriptor this File owns.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

/// `open(path, flags, mode)` into the host's descriptor table.
pub(crate) fn open_descriptor(c: &mut Context<'_>, path: &FsPath, flags: i32, mode: u32) -> Result<i32, RootedError> {
    let file = open_file(&path.io, flags, mode);
    let file = failure::check(c, file, Syscall::at("open", &path.shown))?;
    Ok(c.host_mut().shared_state().borrow().fs.insert_descriptor(file))
}

fn open(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let path = args::path(c, args[0], "path")?;
    let flags = args::flags(c, args[1], "r")?;
    let mode = args::mode(c, args[2], "mode", Some(DEFAULT_FILE_MODE))?;
    Ok(open_descriptor(c, &path, flags, mode).map(Reply::Descriptor))
}

pub(crate) fn close_descriptor(c: &mut Context<'_>, fd: i32) -> Result<(), RootedError> {
    let result = c.host_mut().shared_state().borrow().fs.close_descriptor(fd);
    failure::check(c, result, Syscall::bare("close"))
}

fn close(c: &mut Context<'_>, args: &[RootId]) -> Call {
    let fd = args::fd(c, args[0])?;
    Ok(close_descriptor(c, fd).and_then(|()| nothing(c)))
}

/// A validated read or write position: the descriptor's own offset, or an absolute one.
fn position(c: &mut Context<'_>, value: RootId, length: f64) -> Result<Option<u64>, RootedError> {
    if is_nullish(c, value) {
        return Ok(None);
    }
    if c.rooted_value(value).is_some_and(|value| value.as_number().is_some()) {
        let position = args::integer(c, value, "position", CURRENT_POSITION, args::MAX_SAFE_INTEGER)?;
        return Ok((position >= 0.0).then_some(position as u64));
    }
    if matches!(c.value_type_rooted(value), Some(quench_runtime::ValueType::BigInt)) {
        let position = c.to_string(value)?.parse::<i128>().unwrap_or(i128::MAX);
        let maximum = i128::from(i64::MAX) - length as i128;
        if !(-1..=maximum).contains(&position) {
            let requirement = format!(">= -1 && <= {maximum}");
            return Err(failure::out_of_range(c, "position", &requirement, value));
        }
        return Ok((position >= 0).then_some(position as u64));
    }
    Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, value, "position", "of type bigint or integer")?)
}

/// `validateOffsetLengthRead(offset, length, bufferLength)`.
fn offset_length_read(c: &mut Context<'_>, offset: f64, length: f64, capacity: usize, length_root: RootId, offset_root: RootId) -> Result<(), RootedError> {
    if offset < 0.0 {
        return Err(failure::out_of_range(c, "offset", ">= 0", offset_root));
    }
    if length < 0.0 {
        return Err(failure::out_of_range(c, "length", ">= 0", length_root));
    }
    if offset + length > capacity as f64 {
        return Err(failure::out_of_range(c, "length", &format!("<= {}", capacity as f64 - offset), length_root));
    }
    Ok(())
}

/// Read `length` bytes at `position` into `view[offset..]`: the bytes read.
pub(crate) fn read_into(c: &mut Context<'_>, fd: i32, view: RootId, offset: usize, length: usize, position: Option<u64>) -> io::Result<usize> {
    let fs = c.host_mut().shared_state().borrow().fs.clone();
    let read = c.with_buffer_bytes_mut_rooted(view, |bytes| {
        let window = &mut bytes[offset..offset + length];
        fs.with_descriptor(fd, |file| match position {
            Some(position) => file.read_at(window, position),
            None => file.read(window),
        })
    });
    read.unwrap_or(Ok(0))
}

/// The arguments of a read: `(buffer, offset, length, position)` in any of Node's layouts.
struct ReadRequest {
    buffer: RootId,
    offset: RootId,
    length: RootId,
    position: RootId,
}

/// `readSync(fd, buffer, offset, length, position)`, `readSync(fd, buffer, options)`, and the
/// callback (`read(fd, [buffer | options], ...)`) and `filehandle.read` layouts.
fn read_request(c: &mut Context<'_>, args: &[RootId], given: usize, flavor: Flavor) -> Result<ReadRequest, RootedError> {
    let undefined = c.undefined();
    let (mut buffer, mut options) = (args[1], undefined);
    let (mut offset, mut length, mut position) = (args[2], args[3], args[4]);
    let allocates = flavor != Flavor::Sync;
    if allocates && !args::is_view(c, buffer)? {
        // `read(fd)`, `read(fd, options)`, `filehandle.read(options)`.
        args::nullable_object(c, buffer, "options")?;
        options = buffer;
        buffer = if is_nullish(c, options) { undefined } else { get(c, options, "buffer")? };
        if is_undefined(c, buffer) {
            buffer = crate::modules::buffer_shared_vm::from_vec(c, vec![0; DEFAULT_READ_BUFFER])?;
        }
        (offset, length, position) = (undefined, undefined, undefined);
    } else if given <= 3 || c.is_object_rooted(offset)? || c.rooted_value(offset).is_some_and(|value| value.is_null()) {
        // `readSync(fd, buffer, options)`, `filehandle.read(buffer, options)`.
        if !is_undefined(c, offset) {
            args::nullable_object(c, offset, "options")?;
        }
        options = offset;
        (offset, length, position) = (undefined, undefined, undefined);
    }
    if !args::is_view(c, buffer)? {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, buffer, "buffer", "an instance of Buffer, TypedArray, or DataView")?);
    }
    if !is_nullish(c, options) {
        offset = get(c, options, "offset")?;
        length = get(c, options, "length")?;
        position = get(c, options, "position")?;
    }
    Ok(ReadRequest { buffer, offset, length, position })
}

fn read(c: &mut Context<'_>, args: &[RootId], given: usize, flavor: Flavor) -> Call {
    let fd = args::fd(c, args[0])?;
    let request = read_request(c, args, given, flavor)?;
    let capacity = args::byte_length(c, request.buffer)?;
    let offset = if is_undefined(c, request.offset) { 0.0 } else { args::integer(c, request.offset, "offset", 0.0, args::MAX_SAFE_INTEGER)? };
    let length = if is_undefined(c, request.length) {
        capacity as f64 - offset
    } else {
        let zero = c.number(0.0);
        let truncated = c.bitwise_or_rooted(request.length, zero)?;
        c.to_number(truncated)?
    };
    let length_root = c.number(length);
    let transfer = |count: usize| Reply::Transfer { count, data: request.buffer, kind: Transfer::Read };
    if length == 0.0 {
        return Ok(Ok(transfer(0)));
    }
    if capacity == 0 {
        return Err(crate::modules::util_shared_vm::argument_value_error(c, request.buffer, "buffer", "is empty and cannot be written")?);
    }
    let offset_root = c.number(offset);
    offset_length_read(c, offset, length, capacity, length_root, offset_root)?;
    let position = position(c, request.position, length)?;
    let read = read_into(c, fd, request.buffer, offset as usize, length as usize, position);
    Ok(failure::check(c, read, Syscall::bare("read")).map(transfer))
}

/// `validateOffsetLengthWrite(offset, length, byteLength)`.
fn offset_length_write(c: &mut Context<'_>, offset: f64, length: f64, capacity: usize) -> Result<(), RootedError> {
    if offset > capacity as f64 {
        let value = c.number(offset);
        return Err(failure::out_of_range(c, "offset", &format!("<= {capacity}"), value));
    }
    let value = c.number(length);
    if length > capacity as f64 - offset {
        return Err(failure::out_of_range(c, "length", &format!("<= {}", capacity as f64 - offset), value));
    }
    if length < 0.0 {
        return Err(failure::out_of_range(c, "length", ">= 0", value));
    }
    Ok(())
}

/// Write all of `bytes` at `position` (or the descriptor's offset): the bytes written.
pub(crate) fn write_bytes(c: &mut Context<'_>, fd: i32, bytes: &[u8], position: Option<u64>) -> io::Result<usize> {
    let fs = c.host_mut().shared_state().borrow().fs.clone();
    fs.write_descriptor(fd, bytes, position)
}

fn number_or(c: &mut Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

/// The bytes a string argument encodes to under `encoding`.
fn encode_string(c: &mut Context<'_>, text: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    let encoding = args::encoding_value(c, encoding, UTF8)?;
    let name = match encoding {
        Encoding::Text(name) => name,
        Encoding::Buffer => "utf8",
    };
    let units = c.string_code_units(text)?.unwrap_or_default();
    Ok(crate::modules::buffer_codec::encode_units(&units, name))
}

fn write(c: &mut Context<'_>, args: &[RootId], flavor: Flavor) -> Call {
    let fd = args::fd(c, args[0])?;
    let data = args[1];
    let (bytes, position) = if args::is_view(c, data)? {
        let capacity = args::byte_length(c, data)?;
        let (mut offset, mut length, mut position) = (args[2], args[3], args[4]);
        if c.is_object_rooted(offset)? || c.rooted_value(offset).is_some_and(|value| value.is_null()) {
            let options = offset;
            (offset, length, position) = if is_nullish(c, options) {
                let undefined = c.undefined();
                (undefined, undefined, undefined)
            } else {
                (get(c, options, "offset")?, get(c, options, "length")?, get(c, options, "position")?)
            };
        }
        // `writeSync` ignores an offset that is not a number; the asynchronous writes
        // validate any offset given.
        let offset = match number_or(c, offset) {
            _ if flavor != Flavor::Sync && !is_nullish(c, offset) => args::integer(c, offset, "offset", 0.0, args::MAX_SAFE_INTEGER)?,
            Some(number) if flavor == Flavor::Sync => number,
            _ => 0.0,
        };
        let length = number_or(c, length).unwrap_or(capacity as f64 - offset);
        offset_length_write(c, offset, length, capacity)?;
        let position = if number_or(c, position).is_some() { self::position(c, position, length)? } else { None };
        let bytes = c
            .with_view_bytes_rooted(data, |bytes| bytes[offset as usize..(offset + length) as usize].to_vec())
            .unwrap_or_default();
        (bytes, position)
    } else {
        if c.string_text(data)?.is_none() {
            return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, data, "buffer", "of type string or an instance of Buffer, TypedArray, or DataView")?);
        }
        let (position, encoding) = (args[2], args[3]);
        let bytes = encode_string(c, data, encoding)?;
        let position = if number_or(c, position).is_some() { self::position(c, position, bytes.len() as f64)? } else { None };
        (bytes, position)
    };
    let written = write_bytes(c, fd, &bytes, position);
    Ok(failure::check(c, written, Syscall::bare("write")).map(|count| Reply::Transfer { count, data, kind: Transfer::Write }))
}

/// `readv(fd, buffers, position)` and `writev(fd, buffers, position)`.
fn vector(c: &mut Context<'_>, args: &[RootId], kind: Transfer) -> Call {
    let fd = args::fd(c, args[0])?;
    let buffers = args[1];
    let views = if c.is_array_rooted(buffers)? { c.array_like_values_rooted(buffers)? } else { Vec::new() };
    let mut valid = c.is_array_rooted(buffers)?;
    for view in &views {
        valid &= args::is_view(c, *view)?;
    }
    if !valid {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, buffers, "buffers", "an ArrayBufferView[]")?);
    }
    let position = args[2];
    let mut position = if number_or(c, position).is_some() { self::position(c, position, 0.0)? } else { None };
    let mut total = 0;
    for view in views {
        let length = args::byte_length(c, view)?;
        let moved = if kind == Transfer::Readv {
            read_into(c, fd, view, 0, length, position)
        } else {
            let bytes = c.view_bytes_rooted(view).unwrap_or_default();
            write_bytes(c, fd, &bytes, position)
        };
        let syscall = if kind == Transfer::Readv { "read" } else { "write" };
        let moved = match failure::check(c, moved, Syscall::bare(syscall)) {
            Ok(moved) => moved,
            Err(error) => return Ok(Err(error)),
        };
        total += moved;
        position = position.map(|position| position + moved as u64);
        if moved < length {
            break;
        }
    }
    Ok(Ok(Reply::Transfer { count: total, data: buffers, kind }))
}

fn sync(c: &mut Context<'_>, args: &[RootId], data_only: bool) -> Call {
    let fd = args::fd(c, args[0])?;
    let result = with_descriptor(c, fd, |file| if data_only { file.sync_data() } else { file.sync_all() });
    Ok(completed(c, result, Syscall::bare(if data_only { "fdatasync" } else { "fsync" })))
}

/// The file `readFile` and `writeFile` work on: a path they open, or a descriptor.
pub(crate) enum FileArgument {
    Path(FsPath),
    Descriptor(i32),
}

/// A FileHandle, a descriptor number or a path (`isFd`, `kHandle`).
pub(crate) fn file_argument(c: &mut Context<'_>, value: RootId) -> Result<FileArgument, RootedError> {
    if let Some(fd) = super::file_handle::descriptor_of(c, value)? {
        return Ok(FileArgument::Descriptor(fd));
    }
    let zero = c.number(0.0);
    let integer = c.bitwise_or_rooted(value, zero)?;
    if c.rooted_value(value).is_some_and(|value| value.as_number().is_some()) && c.strict_equal_rooted(value, integer)? {
        return Ok(FileArgument::Descriptor(args::fd(c, value)?));
    }
    Ok(FileArgument::Path(args::path(c, value, "path")?))
}

/// Whether `signal` is aborted.
fn aborted(c: &mut Context<'_>, signal: RootId) -> Result<bool, RootedError> {
    let aborted = get(c, signal, "aborted")?;
    c.truthy_rooted(aborted)
}

/// Every byte from the descriptor's offset to its end, within `kIoMaxLength`.
fn read_to_end(c: &mut Context<'_>, fd: i32) -> Result<Vec<u8>, RootedError> {
    let bytes = with_descriptor(c, fd, |file| {
        let mut bytes = Vec::new();
        file.take(MAX_READ_FILE_BYTES).read_to_end(&mut bytes).map(|_| bytes)
    });
    failure::check(c, bytes, Syscall::bare("read"))
}

fn too_large(c: &mut Context<'_>, size: u64) -> RootedError {
    failure::plain(c, "ERR_FS_FILE_TOO_LARGE", &format!("File size ({size}) is greater than 2 GiB"), true)
}

pub(crate) fn read_whole(c: &mut Context<'_>, file: &FileArgument, flags: i32) -> Result<Vec<u8>, RootedError> {
    match file {
        FileArgument::Descriptor(fd) => {
            let metadata = with_descriptor(c, *fd, |file| file.metadata());
            failure::check(c, metadata, Syscall::bare("fstat"))?;
            read_to_end(c, *fd)
        }
        FileArgument::Path(path) => {
            let fd = open_descriptor(c, path, flags, DEFAULT_FILE_MODE)?;
            let size = with_descriptor(c, fd, |file| file.metadata()).map_or(0, |metadata| metadata.len());
            let bytes = if size > MAX_READ_FILE_BYTES { Err(too_large(c, size)) } else { read_to_end(c, fd) };
            close_descriptor(c, fd)?;
            bytes
        }
    }
}

fn read_file(c: &mut Context<'_>, args: &[RootId], flavor: Flavor) -> Call {
    let options = args::options(c, args[1])?;
    let flag = get(c, options, "flag")?;
    let flags = args::flags(c, flag, "r")?;
    let file = file_argument(c, args[0])?;
    let encoding = args::encoding(c, options, Encoding::Buffer)?;
    let signal = get(c, options, "signal")?;
    if flavor != Flavor::Sync && !is_undefined(c, signal) && aborted(c, signal)? {
        return Ok(Err(failure::aborted(c, signal)));
    }
    let bytes = read_whole(c, &file, flags);
    Ok(bytes
        .and_then(|bytes| match encoding {
            Encoding::Buffer if crate::modules::buffer_shared_vm::is_pooled(bytes.len()) => unsafe_buffer(c, &bytes),
            _ => encoding.project(c, bytes),
        })
        .map(|value| abortable(c, value, signal)))
}

/// `value`, cancelled when `signal` aborts before it is delivered.
fn abortable(c: &mut Context<'_>, value: RootId, signal: RootId) -> Reply {
    if is_undefined(c, signal) { Reply::Value(value) } else { Reply::Abortable { value, signal } }
}

/// `Buffer.allocUnsafe(bytes.length)` holding `bytes`, a slice of the shared pool: the buffer
/// Node's `readFile` returns for a small file.
fn unsafe_buffer(c: &mut Context<'_>, bytes: &[u8]) -> Result<RootId, RootedError> {
    let buffer = match crate::modules::buffer_shared_vm::allocate_before_library(c, bytes.len())? {
        Some(buffer) => buffer,
        None => {
            let module = crate::host::shared_vm::commonjs::buffer_module(c)?;
            let constructor = get(c, module, "Buffer")?;
            let length = c.number(bytes.len() as f64);
            crate::modules::stream_shared_vm::call_method(c, constructor, "allocUnsafe", &[length])?
        }
    };
    c.with_buffer_bytes_mut_rooted(buffer, |target| target.copy_from_slice(bytes));
    Ok(buffer)
}

/// The bytes `writeFile` writes: a view's, or a string's under the encoding.
pub(crate) fn data_bytes(c: &mut Context<'_>, data: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if args::is_view(c, data)? {
        return Ok(c.view_bytes_rooted(data).unwrap_or_default());
    }
    if c.string_text(data)?.is_none() {
        return Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, data, "data", "of type string or an instance of Buffer, TypedArray, or DataView")?);
    }
    encode_string(c, data, encoding)
}

/// Write all of `bytes` from the descriptor's offset.
fn write_all(c: &mut Context<'_>, fd: i32, bytes: &[u8]) -> Result<(), RootedError> {
    let written = with_descriptor(c, fd, |file| file.write_all(bytes));
    failure::check(c, written, Syscall::bare("write"))
}

pub(crate) fn write_whole(c: &mut Context<'_>, file: &FileArgument, bytes: &[u8], flags: i32, mode: u32, flush: bool) -> Result<(), RootedError> {
    match file {
        FileArgument::Descriptor(fd) => write_all(c, *fd, bytes),
        FileArgument::Path(path) => {
            let fd = open_descriptor(c, path, flags, mode)?;
            let written = write_all(c, fd, bytes).and_then(|()| {
                if !flush {
                    return Ok(());
                }
                let synced = with_descriptor(c, fd, |file| file.sync_all());
                failure::check(c, synced, Syscall::bare("fsync"))
            });
            close_descriptor(c, fd)?;
            written
        }
    }
}

fn write_file(c: &mut Context<'_>, args: &[RootId], append: bool) -> Call {
    let options = args::options(c, args[2])?;
    let flush = args::boolean_option(c, options, "flush", false)?;
    let file = file_argument(c, args[0])?;
    let flag = get(c, options, "flag")?;
    let default_flag = if append { "a" } else { "w" };
    let appends_by_default = append && (!c.truthy_rooted(flag)? || matches!(file, FileArgument::Descriptor(_)));
    let flag = if appends_by_default { c.undefined() } else { flag };
    let flags = args::flags(c, flag, default_flag)?;
    let mode = get(c, options, "mode")?;
    let mode = args::mode(c, mode, "mode", Some(DEFAULT_FILE_MODE))?;
    let encoding = get(c, options, "encoding")?;
    let bytes = data_bytes(c, args[1], encoding)?;
    let signal = get(c, options, "signal")?;
    if !is_undefined(c, signal) && aborted(c, signal)? {
        return Ok(Err(failure::aborted(c, signal)));
    }
    let undefined = c.undefined();
    Ok(write_whole(c, &file, &bytes, flags, mode, flush).map(|()| abortable(c, undefined, signal)))
}
