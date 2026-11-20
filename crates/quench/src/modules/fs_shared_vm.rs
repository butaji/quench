//! The `fs` module and `fs/promises`: every operation is one Rust implementation (`ops`)
//! presented through the sync, callback and promise APIs (`api`).

use crate::modules::stream_shared_vm::{arg, get, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};

pub(crate) mod api;
pub(crate) mod args;
pub(crate) mod classes;
pub(crate) mod copy;
pub(crate) mod dir;
pub(crate) mod failure;
pub(crate) mod file_handle;
pub(crate) mod glob;
pub(crate) mod ops;
pub(crate) mod stats;
pub(crate) mod streams;

use api::Flavor;
use ops::Op;

const UV_FS_SYMLINK_DIR: i32 = 1;
const UV_FS_SYMLINK_JUNCTION: i32 = 2;
pub(super) const UV_DIRENT_UNKNOWN: i32 = 0;
pub(super) const UV_DIRENT_FILE: i32 = 1;
pub(super) const UV_DIRENT_DIR: i32 = 2;
pub(super) const UV_DIRENT_LINK: i32 = 3;
pub(super) const UV_DIRENT_FIFO: i32 = 4;
pub(super) const UV_DIRENT_SOCKET: i32 = 5;
pub(super) const UV_DIRENT_CHAR: i32 = 6;
pub(super) const UV_DIRENT_BLOCK: i32 = 7;
/// libuv's Windows-only open flags, which are no bits elsewhere.
const UV_FS_O_FILEMAP: i32 = 0;
const UV_FS_O_TEMPORARY: i32 = 0;
const UV_FS_O_SHORT_LIVED: i32 = 0;
const UV_FS_O_SEQUENTIAL: i32 = 0;
const UV_FS_O_RANDOM: i32 = 0;
const UV_FS_COPYFILE_EXCL: i32 = 1;
const UV_FS_COPYFILE_FICLONE: i32 = 2;
const UV_FS_COPYFILE_FICLONE_FORCE: i32 = 4;
#[cfg(not(target_os = "linux"))]
const S_IRUSR: i32 = 0o400;
#[cfg(not(target_os = "linux"))]
const S_IWUSR: i32 = 0o200;

#[cfg(target_os = "linux")]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("O_RDONLY", libc::O_RDONLY),
    ("O_WRONLY", libc::O_WRONLY),
    ("O_RDWR", libc::O_RDWR),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IFMT", libc::S_IFMT as i32),
    ("S_IFREG", libc::S_IFREG as i32),
    ("S_IFDIR", libc::S_IFDIR as i32),
    ("S_IFCHR", libc::S_IFCHR as i32),
    ("S_IFBLK", libc::S_IFBLK as i32),
    ("S_IFIFO", libc::S_IFIFO as i32),
    ("S_IFLNK", libc::S_IFLNK as i32),
    ("S_IFSOCK", libc::S_IFSOCK as i32),
    ("O_CREAT", libc::O_CREAT),
    ("O_EXCL", libc::O_EXCL),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("UV_FS_O_TEMPORARY", UV_FS_O_TEMPORARY),
    ("UV_FS_O_SHORT_LIVED", UV_FS_O_SHORT_LIVED),
    ("UV_FS_O_SEQUENTIAL", UV_FS_O_SEQUENTIAL),
    ("UV_FS_O_RANDOM", UV_FS_O_RANDOM),
    ("O_NOCTTY", libc::O_NOCTTY),
    ("O_TRUNC", libc::O_TRUNC),
    ("O_APPEND", libc::O_APPEND),
    ("O_DIRECTORY", libc::O_DIRECTORY),
    ("O_NOATIME", libc::O_NOATIME),
    ("O_NOFOLLOW", libc::O_NOFOLLOW),
    ("O_SYNC", libc::O_SYNC),
    ("O_DSYNC", libc::O_DSYNC),
    ("O_DIRECT", libc::O_DIRECT),
    ("O_NONBLOCK", libc::O_NONBLOCK),
    ("S_IRWXU", libc::S_IRWXU as i32),
    ("S_IRUSR", libc::S_IRUSR as i32),
    ("S_IWUSR", libc::S_IWUSR as i32),
    ("S_IXUSR", libc::S_IXUSR as i32),
    ("S_IRWXG", libc::S_IRWXG as i32),
    ("S_IRGRP", libc::S_IRGRP as i32),
    ("S_IWGRP", libc::S_IWGRP as i32),
    ("S_IXGRP", libc::S_IXGRP as i32),
    ("S_IRWXO", libc::S_IRWXO as i32),
    ("S_IROTH", libc::S_IROTH as i32),
    ("S_IWOTH", libc::S_IWOTH as i32),
    ("S_IXOTH", libc::S_IXOTH as i32),
    ("F_OK", libc::F_OK),
    ("R_OK", libc::R_OK),
    ("W_OK", libc::W_OK),
    ("X_OK", libc::X_OK),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

#[cfg(not(target_os = "linux"))]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("O_RDONLY", libc::O_RDONLY),
    ("O_WRONLY", libc::O_WRONLY),
    ("O_RDWR", libc::O_RDWR),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IFMT", libc::S_IFMT as i32),
    ("S_IFREG", libc::S_IFREG as i32),
    ("S_IFDIR", libc::S_IFDIR as i32),
    ("S_IFCHR", libc::S_IFCHR as i32),
    ("S_IFBLK", libc::S_IFBLK as i32),
    ("S_IFIFO", libc::S_IFIFO as i32),
    ("S_IFLNK", libc::S_IFLNK as i32),
    ("S_IFSOCK", libc::S_IFSOCK as i32),
    ("O_CREAT", libc::O_CREAT),
    ("O_EXCL", libc::O_EXCL),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("UV_FS_O_TEMPORARY", UV_FS_O_TEMPORARY),
    ("UV_FS_O_SHORT_LIVED", UV_FS_O_SHORT_LIVED),
    ("UV_FS_O_SEQUENTIAL", UV_FS_O_SEQUENTIAL),
    ("UV_FS_O_RANDOM", UV_FS_O_RANDOM),
    ("O_NOCTTY", libc::O_NOCTTY),
    ("O_TRUNC", libc::O_TRUNC),
    ("O_APPEND", libc::O_APPEND),
    ("O_DIRECTORY", libc::O_DIRECTORY),
    ("O_NOFOLLOW", libc::O_NOFOLLOW),
    ("O_SYNC", libc::O_SYNC),
    ("O_DSYNC", libc::O_DSYNC),
    // Darwin's own open flag: Node exposes it and Linux does not have it.
    ("O_SYMLINK", libc::O_SYMLINK),
    ("O_NONBLOCK", libc::O_NONBLOCK),
    ("S_IRWXU", libc::S_IRWXU as i32),
    ("S_IRUSR", S_IRUSR),
    ("S_IWUSR", S_IWUSR),
    ("S_IXUSR", libc::S_IXUSR as i32),
    ("S_IRWXG", libc::S_IRWXG as i32),
    ("S_IRGRP", libc::S_IRGRP as i32),
    ("S_IWGRP", libc::S_IWGRP as i32),
    ("S_IXGRP", libc::S_IXGRP as i32),
    ("S_IRWXO", libc::S_IRWXO as i32),
    ("S_IROTH", libc::S_IROTH as i32),
    ("S_IWOTH", libc::S_IWOTH as i32),
    ("S_IXOTH", libc::S_IXOTH as i32),
    ("F_OK", libc::F_OK),
    ("R_OK", libc::R_OK),
    ("W_OK", libc::W_OK),
    ("X_OK", libc::X_OK),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

/// `fs.constants`, shared by `fs.promises.constants`.
pub(crate) fn fs_constants(c: &mut Context<'_>) -> Outcome {
    const CACHE_KEY: &str = "\0builtin:fs/constants";
    let shared = c.host_mut().shared_state();
    if let Some(constants) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(constants);
    }
    let constants = c.null_object_rooted()?;
    for &(name, value) in FS_CONSTANT_VALUES {
        let key = c.string_rooted(name);
        let value = c.number(f64::from(value));
        c.define_data_property_rooted(constants, key, value, false, true, false)?;
    }
    let retained = c.retain(constants)?;
    shared.borrow_mut().builtin_cache.insert(CACHE_KEY.to_owned(), retained);
    Ok(constants)
}

/// A member of `fs` or `fs.promises`, in Node's key order.
enum Member {
    /// A public function: name, length, operation and flavor.
    Call(&'static str, usize, Op, Flavor),
    /// A member built apart.
    Other(&'static str),
}

use Member::{Call, Other};
use Flavor::{Callback as Cb, Promise as Pr, Sync as Sy};

const MODULE_MEMBERS: &[Member] = &[
    Call("appendFile", 4, Op::AppendFile, Cb), Call("appendFileSync", 3, Op::AppendFile, Sy),
    Call("access", 3, Op::Access, Cb), Call("accessSync", 2, Op::Access, Sy),
    Call("chown", 4, Op::Chown, Cb), Call("chownSync", 3, Op::Chown, Sy),
    Call("chmod", 3, Op::Chmod, Cb), Call("chmodSync", 2, Op::Chmod, Sy),
    Call("close", 1, Op::Close, Cb), Call("closeSync", 1, Op::Close, Sy),
    Call("copyFile", 4, Op::CopyFile, Cb), Call("copyFileSync", 3, Op::CopyFile, Sy),
    Call("cp", 4, Op::Cp, Cb), Call("cpSync", 3, Op::Cp, Sy),
    Other("createReadStream"), Other("createWriteStream"),
    Call("exists", 2, Op::Exists, Cb), Call("existsSync", 1, Op::Exists, Sy),
    Call("fchown", 4, Op::Fchown, Cb), Call("fchownSync", 3, Op::Fchown, Sy),
    Call("fchmod", 3, Op::Fchmod, Cb), Call("fchmodSync", 2, Op::Fchmod, Sy),
    Call("fdatasync", 2, Op::Fdatasync, Cb), Call("fdatasyncSync", 1, Op::Fdatasync, Sy),
    Call("fstat", 1, Op::Fstat, Cb), Call("fstatSync", 1, Op::Fstat, Sy),
    Call("fsync", 2, Op::Fsync, Cb), Call("fsyncSync", 1, Op::Fsync, Sy),
    Call("ftruncate", 1, Op::Ftruncate, Cb), Call("ftruncateSync", 1, Op::Ftruncate, Sy),
    Call("futimes", 4, Op::Futimes, Cb), Call("futimesSync", 3, Op::Futimes, Sy),
    Call("glob", 3, Op::Glob, Cb), Call("globSync", 2, Op::Glob, Sy),
    Call("lchown", 4, Op::Lchown, Cb), Call("lchownSync", 3, Op::Lchown, Sy),
    Other("lchmod"), Other("lchmodSync"),
    Call("link", 3, Op::Link, Cb), Call("linkSync", 2, Op::Link, Sy),
    Call("lstat", 1, Op::Lstat, Cb), Call("lstatSync", 1, Op::Lstat, Sy),
    Call("lutimes", 4, Op::Lutimes, Cb), Call("lutimesSync", 3, Op::Lutimes, Sy),
    Call("mkdir", 3, Op::Mkdir, Cb), Call("mkdirSync", 2, Op::Mkdir, Sy),
    Call("mkdtemp", 3, Op::Mkdtemp, Cb), Call("mkdtempSync", 2, Op::Mkdtemp, Sy),
    Call("mkdtempDisposableSync", 2, Op::MkdtempDisposable, Sy),
    Call("open", 4, Op::Open, Cb), Call("openSync", 3, Op::Open, Sy),
    Call("readdir", 3, Op::Readdir, Cb), Call("readdirSync", 2, Op::Readdir, Sy),
    Call("read", 6, Op::Read, Cb), Call("readSync", 5, Op::Read, Sy),
    Call("readv", 4, Op::Readv, Cb), Call("readvSync", 3, Op::Readv, Sy),
    Call("readFile", 3, Op::ReadFile, Cb), Call("readFileSync", 2, Op::ReadFile, Sy),
    Call("readlink", 3, Op::Readlink, Cb), Call("readlinkSync", 2, Op::Readlink, Sy),
    Call("realpath", 3, Op::Realpath, Cb), Call("realpathSync", 2, Op::Realpath, Sy),
    Call("rename", 3, Op::Rename, Cb), Call("renameSync", 2, Op::Rename, Sy),
    Call("rm", 3, Op::Rm, Cb), Call("rmSync", 2, Op::Rm, Sy),
    Call("rmdir", 3, Op::Rmdir, Cb), Call("rmdirSync", 2, Op::Rmdir, Sy),
    Call("stat", 1, Op::Stat, Cb), Call("statfs", 1, Op::Statfs, Cb),
    Call("statSync", 1, Op::Stat, Sy), Call("statfsSync", 1, Op::Statfs, Sy),
    Call("symlink", 4, Op::Symlink, Cb), Call("symlinkSync", 3, Op::Symlink, Sy),
    Call("truncate", 3, Op::Truncate, Cb), Call("truncateSync", 2, Op::Truncate, Sy),
    Other("unwatchFile"),
    Call("unlink", 2, Op::Unlink, Cb), Call("unlinkSync", 1, Op::Unlink, Sy),
    Call("utimes", 4, Op::Utimes, Cb), Call("utimesSync", 3, Op::Utimes, Sy),
    Other("watch"), Other("watchFile"),
    Call("writeFile", 4, Op::WriteFile, Cb), Call("writeFileSync", 3, Op::WriteFile, Sy),
    Call("write", 6, Op::Write, Cb), Call("writeSync", 5, Op::Write, Sy),
    Call("writev", 4, Op::Writev, Cb), Call("writevSync", 3, Op::Writev, Sy),
    Other("Dirent"), Other("Stats"),
    Other("ReadStream"), Other("WriteStream"), Other("FileReadStream"), Other("FileWriteStream"),
    Other("_toUnixTimestamp"), Other("Dir"),
    Call("opendir", 3, Op::Opendir, Cb), Call("opendirSync", 2, Op::Opendir, Sy),
    Other("F_OK"), Other("R_OK"), Other("W_OK"), Other("X_OK"),
    Other("constants"), Other("promises"),
];

const PROMISES_MEMBERS: &[Member] = &[
    Call("access", 1, Op::Access, Pr), Call("copyFile", 3, Op::CopyFile, Pr),
    Call("cp", 3, Op::Cp, Pr), Other("glob"), Call("open", 3, Op::Open, Pr),
    Call("opendir", 3, Op::Opendir, Pr), Call("rename", 2, Op::Rename, Pr),
    Call("truncate", 1, Op::Truncate, Pr), Call("rm", 2, Op::Rm, Pr),
    Call("rmdir", 2, Op::Rmdir, Pr), Call("mkdir", 2, Op::Mkdir, Pr),
    Call("readdir", 2, Op::Readdir, Pr), Call("readlink", 2, Op::Readlink, Pr),
    Call("symlink", 3, Op::Symlink, Pr), Call("lstat", 1, Op::Lstat, Pr),
    Call("stat", 1, Op::Stat, Pr), Call("statfs", 1, Op::Statfs, Pr),
    Call("link", 2, Op::Link, Pr), Call("unlink", 1, Op::Unlink, Pr),
    Call("chmod", 2, Op::Chmod, Pr), Call("lchmod", 2, Op::Lchmod, Pr),
    Call("lchown", 3, Op::Lchown, Pr), Call("chown", 3, Op::Chown, Pr),
    Call("utimes", 3, Op::Utimes, Pr), Call("lutimes", 3, Op::Lutimes, Pr),
    Call("realpath", 2, Op::Realpath, Pr), Call("mkdtemp", 2, Op::Mkdtemp, Pr),
    Call("mkdtempDisposable", 2, Op::MkdtempDisposable, Pr),
    Call("writeFile", 3, Op::WriteFile, Pr), Call("appendFile", 3, Op::AppendFile, Pr),
    Call("readFile", 2, Op::ReadFile, Pr),
    Other("watch"), Other("constants"),
];

/// The names `util.promisify` gives the values a callback receives after its error.
const PROMISIFY_ARGUMENTS: [(&str, [&str; 2]); 4] = [
    ("read", ["bytesRead", "buffer"]),
    ("write", ["bytesWritten", "buffer"]),
    ("readv", ["bytesRead", "buffers"]),
    ("writev", ["bytesWritten", "buffers"]),
];

/// `fsPromises.glob(pattern, options)`'s length.
const PROMISES_GLOB_LENGTH: f64 = 2.0;

/// The access constants `fs` also exposes at its top level.
const ACCESS_CONSTANTS: [(&str, i32); 4] = [("F_OK", libc::F_OK), ("R_OK", libc::R_OK), ("W_OK", libc::W_OK), ("X_OK", libc::X_OK)];

/// The stream classes `fs` defines on first use: the cache key of each and its builder.
const STREAM_CLASSES: [(&str, fn(&mut Context<'_>) -> Outcome); 2] =
    [("\0fs:ReadStream", streams::read_stream_class), ("\0fs:WriteStream", streams::write_stream_class)];
/// The names that reach each stream class, by its index in `STREAM_CLASSES`.
const STREAM_ACCESSORS: [(&str, usize); 4] = [("ReadStream", 0), ("WriteStream", 1), ("FileReadStream", 0), ("FileWriteStream", 1)];

fn define(c: &mut Context<'_>, target: RootId, name: &str, value: RootId, writable: bool, configurable: bool) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, writable, true, configurable).map(drop)
}

fn functions(c: &mut Context<'_>, target: RootId, members: &[Member], other: &mut dyn FnMut(&mut Context<'_>, &str) -> Result<(), RootedError>) -> Result<(), RootedError> {
    for member in members {
        match *member {
            Call(name, length, op, flavor) => {
                let function = api::function(c, op, flavor, name, length)?;
                define(c, target, name, function, true, true)?;
            }
            Other(name) => other(c, name)?,
        }
    }
    Ok(())
}

/// The `fs` module.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let module = c.object_rooted()?;
    let promises = promises_module(c)?;
    let mut other = |c: &mut Context<'_>, name: &str| -> Result<(), RootedError> {
        let constant = ACCESS_CONSTANTS.iter().find(|(constant, _)| *constant == name);
        if let Some(&(_, value)) = constant {
            let value = c.number(f64::from(value));
            return define(c, module, name, value, false, false);
        }
        if let Some(&(_, index)) = STREAM_ACCESSORS.iter().find(|(stream, _)| *stream == name) {
            let data = c.number(index as f64);
            let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("fsStreamClassGet"), "get", data)?;
            let setter = c.host_function_named_with_data(crate::host::shared_vm::operation!("fsStreamClassSet"), "set", data)?;
            return classes::accessor(c, module, name, getter, Some(setter), true);
        }
        let value = match name {
            "createReadStream" | "createWriteStream" => {
                let (_, index) = STREAM_ACCESSORS[usize::from(name == "createWriteStream")];
                let index = c.number(index as f64);
                c.host_function_named_with_data(crate::host::shared_vm::operation!("fsCreateStream"), name, index)?
            }
            "lchmod" | "lchmodSync" => c.undefined(),
            "watch" => super::fs_watch_shared_vm::watch_function(c)?,
            "watchFile" | "unwatchFile" => super::fs_stat_watch_shared_vm::function(c, name)?,
            "Dirent" => classes::root(c, classes::Root::Dirent)?,
            "Stats" => classes::root(c, classes::Root::DeprecatedStats)?,
            "Dir" => classes::root(c, classes::Root::Dir)?,
            "_toUnixTimestamp" => c.host_function_named(crate::host::shared_vm::operation!("fsToUnixTimestamp"), "toUnixTimestamp")?,
            "constants" => {
                let constants = fs_constants(c)?;
                return define(c, module, name, constants, false, false);
            }
            "promises" => {
                let getter = c.host_function_named_with_data(crate::host::shared_vm::operation!("fsPromisesGet"), "get promises", promises)?;
                return classes::accessor(c, module, name, getter, None, true);
            }
            _ => return Err(RootedError::host(format!("fs member {name} has no builder"))),
        };
        define(c, module, name, value, true, true)
    };
    functions(c, module, MODULE_MEMBERS, &mut other)?;
    for (name, op) in [("realpathSync", Sy), ("realpath", Cb)] {
        let function = get(c, module, name)?;
        let native = api::function(c, Op::RealpathNative, op, "native", 2)?;
        put(c, function, "native", native)?;
    }
    let promisify_arguments = crate::modules::util_inspect_shared_vm::primordial(c, "promisifyArgs")?;
    for (name, values) in PROMISIFY_ARGUMENTS {
        let function = get(c, module, name)?;
        let names = values.map(|value| c.string_rooted(value));
        let names = c.array_rooted(&names)?;
        c.define_data_property_rooted(function, promisify_arguments, names, false, false, false)?;
    }
    let exists = get(c, module, "exists")?;
    let promisified = api::function(c, Op::Exists, Pr, "exists", 1)?;
    let custom = crate::modules::util_inspect_shared_vm::primordial(c, "promisifyCustom")?;
    c.define_data_property_rooted(exists, custom, promisified, false, false, false)?;
    Ok(module)
}

fn promises_module(c: &mut Context<'_>) -> Outcome {
    let promises = c.object_rooted()?;
    let mut other = |c: &mut Context<'_>, name: &str| -> Result<(), RootedError> {
        let value = match name {
            "watch" => super::fs_watch_shared_vm::promise_watch_function(c)?,
            "glob" => {
                let glob = c.host_function_named(crate::host::shared_vm::operation!("fsPromisesGlob"), "glob")?;
                let key = c.string_rooted("length");
                let length = c.number(PROMISES_GLOB_LENGTH);
                c.define_data_property_rooted(glob, key, length, false, false, true)?;
                glob
            }
            _ => fs_constants(c)?,
        };
        define(c, promises, name, value, true, true)
    };
    functions(c, promises, PROMISES_MEMBERS, &mut other)?;
    Ok(promises)
}

/// `get promises()`.
pub(crate) fn promises_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    c.host_function_data()
}

/// The stream class an accessor's data names, made on first use.
fn stream_class(c: &mut Context<'_>, index: usize) -> Outcome {
    let (key, build) = STREAM_CLASSES[index];
    let shared = c.host_mut().shared_state();
    if let Some(class) = shared.borrow().builtin_cache.get(key).copied() {
        return Ok(class);
    }
    let class = build(c)?;
    let retained = c.retain(class)?;
    shared.borrow_mut().builtin_cache.insert(key.to_owned(), retained);
    Ok(class)
}

fn data_index(c: &mut Context<'_>) -> Result<usize, RootedError> {
    let data = c.host_function_data()?;
    Ok(c.to_number(data)? as usize)
}

/// `get ReadStream()` and its siblings.
pub(crate) fn stream_class_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let index = data_index(c)?;
    stream_class(c, index)
}

/// `set ReadStream(value)`: the module's stream class becomes `value`.
pub(crate) fn stream_class_set(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key, _) = STREAM_CLASSES[data_index(c)?];
    let value = arg(c, args, 0);
    let retained = c.retain(value)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(key.to_owned(), retained);
    Ok(c.undefined())
}

/// `createReadStream(path, options)`: `new ReadStream(path, options)`.
pub(crate) fn create_stream(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let index = data_index(c)?;
    let class = stream_class(c, index)?;
    let (path, options) = (arg(c, args, 0), arg(c, args, 1));
    c.construct_rooted(class, class, &[path, options])
}

/// `fs._toUnixTimestamp(time, name)`.
pub(crate) fn to_unix_timestamp(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let name = arg(c, args, 1);
    let name = if crate::modules::stream_shared_vm::is_undefined(c, name) { "time".to_owned() } else { c.to_string(name)? };
    let time = arg(c, args, 0);
    let time = args::timestamp(c, time, &name)?;
    Ok(c.number(time))
}

/// `process.emitWarning(message, 'DeprecationWarning', code, deprecated)` the first time
/// `code` is deprecated, unless `--no-deprecation` silences it.
pub(crate) fn emit_deprecation(c: &mut Context<'_>, message: &str, code: &str, deprecated: RootId) -> Result<(), RootedError> {
    let shared = c.host_mut().shared_state();
    if shared.borrow().fs_classes.deprecations.iter().any(|warned| warned == code) {
        return Ok(());
    }
    shared.borrow_mut().fs_classes.deprecations.push(code.to_owned());
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    let silenced = get(c, process, "noDeprecation")?;
    if c.truthy_rooted(silenced)? {
        return Ok(());
    }
    let message = c.string_rooted(message);
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted(code);
    crate::modules::stream_shared_vm::call_method(c, process, "emitWarning", &[message, kind, code, deprecated]).map(drop)
}

#[cfg(all(test, not(target_os = "linux")))]
mod constants_tests {
    use super::FS_CONSTANT_VALUES;

    /// The names Node's own `fs.constants` exposes on Darwin (captured from Node v26.12.1,
    /// the pinned oracle). A missing name is not cosmetic: `fs.writeFileSync` and the Node
    /// library's `open` OR these values together, and an absent `O_CREAT`/`O_WRONLY` turns
    /// every create-and-write into a read-only open (ENOENT, then EBADF on the write).
    const DARWIN_CONSTANTS: &[&str] = &[
        "UV_FS_SYMLINK_DIR", "UV_FS_SYMLINK_JUNCTION", "O_RDONLY", "O_WRONLY", "O_RDWR",
        "UV_DIRENT_UNKNOWN", "UV_DIRENT_FILE", "UV_DIRENT_DIR", "UV_DIRENT_LINK",
        "UV_DIRENT_FIFO", "UV_DIRENT_SOCKET", "UV_DIRENT_CHAR", "UV_DIRENT_BLOCK", "S_IFMT",
        "S_IFREG", "S_IFDIR", "S_IFCHR", "S_IFBLK", "S_IFIFO", "S_IFLNK", "S_IFSOCK", "O_CREAT",
        "O_EXCL", "UV_FS_O_FILEMAP", "UV_FS_O_TEMPORARY", "UV_FS_O_SHORT_LIVED",
        "UV_FS_O_SEQUENTIAL", "UV_FS_O_RANDOM", "O_NOCTTY", "O_TRUNC", "O_APPEND",
        "O_DIRECTORY", "O_NOFOLLOW", "O_SYNC", "O_DSYNC", "O_SYMLINK", "O_NONBLOCK", "S_IRWXU",
        "S_IRUSR", "S_IWUSR", "S_IXUSR", "S_IRWXG", "S_IRGRP", "S_IWGRP", "S_IXGRP", "S_IRWXO",
        "S_IROTH", "S_IWOTH", "S_IXOTH", "F_OK", "R_OK", "W_OK", "X_OK", "UV_FS_COPYFILE_EXCL",
        "COPYFILE_EXCL", "UV_FS_COPYFILE_FICLONE", "COPYFILE_FICLONE",
        "UV_FS_COPYFILE_FICLONE_FORCE", "COPYFILE_FICLONE_FORCE",
    ];

    #[test]
    fn exposes_every_darwin_fs_constant() {
        let names = FS_CONSTANT_VALUES.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        for expected in DARWIN_CONSTANTS {
            assert!(names.contains(expected), "fs.constants does not expose {expected}");
        }
        assert_eq!(names, DARWIN_CONSTANTS, "fs.constants order diverges from Node's");
    }

    #[test]
    fn the_create_and_write_flag_set_is_usable() {
        let value = |name: &str| {
            FS_CONSTANT_VALUES
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|(_, value)| *value)
                .unwrap_or_else(|| panic!("fs.constants does not expose {name}"))
        };
        let flags = value("O_WRONLY") | value("O_CREAT") | value("O_TRUNC");
        assert_ne!(value("O_WRONLY"), 0, "O_WRONLY must ask for a writable descriptor");
        assert_ne!(flags & value("O_CREAT"), 0, "the write flag set must create");
        assert_ne!(flags & value("O_TRUNC"), 0, "the write flag set must truncate");
    }
}
