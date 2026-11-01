use crate::host::NodeHost;
use crate::modules::fs_ops;
use quench_runtime::{NativeContext, RootId, RootedError};

// Node's internal fs readFile limit, independent of Buffer's larger capacity.
pub(super) const MAX_READ_FILE_BYTES: u64 = i32::MAX as u64;


use fs_ops::ReadFileFailure;

#[path = "fs_shared_vm/file_handle_read.rs"]
pub(crate) mod file_handle_read;
#[path = "fs_shared_vm/stat.rs"]
pub(crate) mod stat;
#[path = "fs_shared_vm/sync.rs"]
pub(crate) mod sync;
#[path = "fs_shared_vm/write_stream.rs"]
pub(crate) mod write_stream;
#[path = "fs_shared_vm/streams.rs"]
pub(crate) mod streams;

#[path = "fs_shared_vm/write_public.rs"]
pub(crate) mod write_public;

pub(crate) mod callback_public;
#[path = "fs_shared_vm/file_handle_write.rs"]
pub(crate) mod file_handle_write;
#[path = "fs_shared_vm/metadata_public.rs"]
pub(crate) mod metadata_public;
pub(crate) mod promise_public;



































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

pub(crate) fn fs_constants(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0builtin:fs/constants";
    const DIRENT_TYPES: &[(&str, i32)] = &[
        ("UV_DIRENT_UNKNOWN", 0),
        ("UV_DIRENT_FILE", 1),
        ("UV_DIRENT_DIR", 2),
        ("UV_DIRENT_LINK", 3),
        ("UV_DIRENT_FIFO", 4),
        ("UV_DIRENT_SOCKET", 5),
        ("UV_DIRENT_CHAR", 6),
        ("UV_DIRENT_BLOCK", 7),
    ];
    let shared = context.host_mut().shared_state();
    if let Some(constants) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(constants);
    }
    let constants = context.null_object_rooted()?;
    for &(name, value) in FS_CONSTANT_VALUES.iter().chain(DIRENT_TYPES.iter()) {
        let key = context.string_rooted(name);
        let value = context.number(value as f64);
        context.define_data_property_rooted(constants, key, value, false, true, false)?;
    }
    let retained = context.retain(constants)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(constants)
}

fn make_readdir_sync(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.host_function(crate::host::shared_vm::operation("fsReaddirPolicy"))
}

/// Existing filesystem adapters retain this indirection during module bootstrap.
/// Once initialized, every captured reader uses the same configured SDK policy.
pub(crate) fn readdir_policy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let policy = library_policy(context)?;
    let function = get(context, policy, "readdirSync")?;
    let undefined = context.undefined();
    context.call_rooted(function, undefined, args)
}

fn make_readdir_async(
    context: &mut NativeContext<'_, NodeHost>,
    readdir_sync: RootId,
) -> Result<RootId, RootedError> {
    let factory =
        crate::host::removed_javascript("node:fs/shared-async-readdir.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[readdir_sync])
}

/// `fs`'s own keys after `readFileSync`, in order, with whether each is enumerable: one lazy
/// group (`module`) that `build_module` fills when a script first observes any of them.
const MODULE_MEMBERS: [(&str, bool); 96] = [
    ("watch", true), ("readFile", true), ("statSync", true), ("statfsSync", true),
    ("statfs", true), ("exists", true), ("existsSync", true), ("access", true),
    ("accessSync", true), ("chmod", true), ("chmodSync", true), ("fchmod", true),
    ("fchmodSync", true), ("lstatSync", true), ("lstat", true), ("readdirSync", true),
    ("readdir", true), ("Dir", true), ("opendir", true), ("opendirSync", true),
    ("readlinkSync", true), ("readlink", true), ("realpathSync", true), ("realpath", true),
    ("constants", true), ("Stats", true), ("utimesSync", true), ("watchFile", true),
    ("unwatchFile", true), ("rm", true), ("mkdirSync", true), ("rmdirSync", true),
    ("mkdtempSync", true), ("mkdtempDisposableSync", true), ("copyFileSync", true),
    ("symlinkSync", true), ("linkSync", true), ("renameSync", true), ("unlinkSync", true),
    ("chownSync", true), ("lchownSync", true), ("fchownSync", true), ("truncateSync", true),
    ("ftruncateSync", true), ("rmSync", true), ("openSync", true), ("closeSync", true),
    ("fstatSync", true), ("readSync", true), ("writeSync", true), ("fsyncSync", true),
    ("fdatasyncSync", true), ("writeFileSync", true), ("appendFileSync", true),
    ("fsync", true), ("fdatasync", true), ("writev", true), ("writevSync", true),
    ("readv", true), ("readvSync", true), ("truncate", true), ("ftruncate", true),
    ("stat", true), ("mkdir", true), ("rmdir", true), ("mkdtemp", true), ("copyFile", true),
    ("cpSync", true), ("cp", true), ("symlink", true), ("link", true), ("rename", true),
    ("unlink", true), ("chown", true), ("lchown", true), ("fchown", true), ("open", true),
    ("close", true), ("read", true), ("fstat", true), ("writeFile", true),
    ("appendFile", true), ("createReadStream", true), ("ReadStream", true), ("promises", true),
    ("createWriteStream", true), ("WriteStream", true), ("lutimesSync", false),
    ("futimesSync", false), ("utimes", false), ("lutimes", false), ("futimes", false),
    ("write", false), ("Dirent", true), ("glob", true), ("globSync", true),
];

/// The `fs` module. `readFileSync` is defined directly; every other member is a lazy data
/// property of one group, built when a script first observes any of them: reading a file
/// needs none of the 50-odd guest factories and wrappers the rest of the module compiles.
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    set(context, module, "readFileSync", read_file)?;
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("fsModuleInitialize"), module,
    )?;
    context.define_lazy_data_members_rooted(module, &MODULE_MEMBERS, initialize)?;
    let global = context.global_root()?;
    let key = context.string_rooted("__nodeFs");
    if !context.define_data_property_rooted(global, key, module, false, false, true)? {
        return Err(RootedError::host("cannot install canonical Node Fs alias"));
    }
    Ok(module)
}

/// Builds the members of `fs` (the lazy group `module` defines) when a script first observes
/// one, and checks they are exactly `MODULE_MEMBERS`, ahead of any key a script added.
pub(crate) fn initialize_module(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    build_module(context, module)?;
    install_library_policy(context, module)?;
    // A script may add keys to `fs` before it first reads a lazy member; string keys it adds
    // follow the group in key order and symbol keys are listed apart, so the module's own
    // members are the leading string keys.
    let keys = context.own_keys_rooted(module)?;
    let mut names = Vec::with_capacity(keys.len());
    for key in keys {
        if !context.is_symbol_rooted(key)? {
            names.push(context.to_string(key)?);
        }
    }
    let expected = std::iter::once("readFileSync").chain(MODULE_MEMBERS.iter().map(|(name, _)| *name));
    if !names.iter().map(String::as_str).take(expected.clone().count()).eq(expected) {
        return Err(RootedError::host(format!("fs members differ from MODULE_MEMBERS: {names:?}")));
    }
    guard_permissions(context, module, None)?;
    Ok(context.undefined())
}

fn build_module(context: &mut NativeContext<'_, NodeHost>, module: RootId) -> Result<(), RootedError> {
    // The streams extend `stream.Readable` and `stream.Writable`, whose module graph is most of
    // what loading `fs` would otherwise build: each is created when a script first observes
    // one of its names. They depend on no other member.
    let initialize_read_stream = context.host_function_with_data(
        crate::host::shared_vm::operation("fsReadStreamInitialize"), module,
    )?;
    context.define_lazy_data_properties_rooted(
        module, READ_STREAM_MEMBERS, true, initialize_read_stream,
    )?;
    write_stream::install(context, module)?;
    super::fs_watch_shared_vm::install(context, module)?;
    
    let read_file = get(context, module, "readFileSync")?;
    let read_file_factory = crate::host::removed_javascript("node:fs/shared-async-read-file.js")?;
    let validate =
        context.host_function(crate::host::shared_vm::operation("fsValidateAbortSignal"))?;
    let complete =
        context.host_function(crate::host::shared_vm::operation("fsReadFileComplete"))?;
    let to_path_if_file_url =
        context.host_function(crate::host::shared_vm::operation("toPathIfFileURL"))?;
    let undefined = context.undefined();
    let read_file_async = context.call_rooted(
        read_file_factory,
        undefined,
        &[read_file, validate, complete, to_path_if_file_url],
    )?;
    set(context, module, "readFile", read_file_async)?;
    let stat_host = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let stat_factory =
        crate::host::removed_javascript("node:fs/shared-stat-sync.js")?;
    let undefined = context.undefined();
    let stat_continuation = context.call_rooted(stat_factory, undefined, &[stat_host])?;
    let stat_sync = metadata_public::wrap(context, stat_continuation, "statSync")?;
    set(context, module, "statSync", stat_sync)?;
    let statfs_host = context.host_function(crate::host::shared_vm::operation("fsStatfsSync"))?;
    let statfs_factory =
        crate::host::removed_javascript("node:fs/shared-statfs-sync.js")?;
    let statfs_continuation = context.call_rooted(statfs_factory, undefined, &[statfs_host])?;
    let statfs_sync = metadata_public::wrap(context, statfs_continuation, "statfsSync")?;
    set(context, module, "statfsSync", statfs_sync)?;
    let statfs_factory =
        crate::host::removed_javascript("node:fs/shared-async-statfs.js")?;
    let statfs = context.call_rooted(
        statfs_factory,
        undefined,
        &[statfs_continuation, to_path_if_file_url],
    )?;
    set(context, module, "statfs", statfs)?;
    let exists_factory = crate::host::removed_javascript("node:fs/shared-exists.js")?;
    let undefined = context.undefined();
    let access_host = context.host_function(crate::host::shared_vm::operation("fsAccessSync"))?;
    let chmod_host = context.host_function(crate::host::shared_vm::operation("fsChmodSync"))?;
    let fchmod_host = context.host_function(crate::host::shared_vm::operation("fsFchmodSync"))?;
    let exists_api = context.call_rooted(
        exists_factory,
        undefined,
        // existsSync falls through to the filesystem, not another VFS dispatch.
        &[stat_continuation, access_host, chmod_host, fchmod_host],
    )?;
    let exists = get(context, exists_api, "exists")?;
    let exists_sync = get(context, exists_api, "existsSync")?;
    let exists_sync = context.host_function_with_data(
        crate::host::shared_vm::operation("fsVfsExistsSync"),
        exists_sync,
    )?;
    let name_key = context.string_rooted("name");
    let name_value = context.string_rooted("existsSync");
    context.define_data_property_rooted(exists_sync, name_key, name_value, false, false, true)?;
    let access = get(context, exists_api, "access")?;
    let access_sync = get(context, exists_api, "accessSync")?;
    let access_sync = metadata_public::wrap(context, access_sync, "accessSync")?;
    let chmod = get(context, exists_api, "chmod")?;
    let chmod_sync = get(context, exists_api, "chmodSync")?;
    let chmod_sync = metadata_public::wrap(context, chmod_sync, "chmodSync")?;
    let fchmod = get(context, exists_api, "fchmod")?;
    let fchmod_sync = get(context, exists_api, "fchmodSync")?;
    let fchmod_sync = metadata_public::wrap(context, fchmod_sync, "fchmodSync")?;
    set(context, module, "exists", exists)?;
    set(context, module, "existsSync", exists_sync)?;
    set(context, module, "access", access)?;
    set(context, module, "accessSync", access_sync)?;
    set(context, module, "chmod", chmod)?;
    set(context, module, "chmodSync", chmod_sync)?;
    set(context, module, "fchmod", fchmod)?;
    set(context, module, "fchmodSync", fchmod_sync)?;
    let lstat_host = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let lstat_factory =
        crate::host::removed_javascript("node:fs/shared-lstat-sync.js")?;
    let undefined = context.undefined();
    let lstat_sync = context.call_rooted(lstat_factory, undefined, &[lstat_host])?;
    let lstat_sync = metadata_public::wrap(context, lstat_sync, "lstatSync")?;
    set(context, module, "lstatSync", lstat_sync)?;
    let lstat_factory =
        crate::host::removed_javascript("node:fs/shared-async-lstat.js")?;
    let lstat =
        context.call_rooted(lstat_factory, undefined, &[lstat_sync, to_path_if_file_url])?;
    set(context, module, "lstat", lstat)?;
    let readdir_sync = make_readdir_sync(context)?;
    set(context, module, "readdirSync", readdir_sync)?;
    let readdir = make_readdir_async(context, readdir_sync)?;
    set(context, module, "readdir", readdir)?;
    let opendir_factory =
        crate::host::removed_javascript("node:fs/shared-opendir.js")?;
    let opendir_api = context.call_rooted(opendir_factory, undefined, &[readdir_sync])?;
    let dir_constructor = get(context, opendir_api, "Dir")?;
    let opendir = get(context, opendir_api, "opendir")?;
    let opendir_sync = get(context, opendir_api, "opendirSync")?;
    let opendir_promise = get(context, opendir_api, "opendirPromise")?;
    set(context, module, "Dir", dir_constructor)?;
    set(context, module, "opendir", opendir)?;
    set(context, module, "opendirSync", opendir_sync)?;
    let readlink_host =
        context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let readlink_factory =
        crate::host::removed_javascript("node:fs/shared-readlink-sync.js")?;
    let undefined = context.undefined();
    let readlink_sync = context.call_rooted(readlink_factory, undefined, &[readlink_host])?;
    let readlink_sync = metadata_public::wrap(context, readlink_sync, "readlinkSync")?;
    set(context, module, "readlinkSync", readlink_sync)?;
    let readlink_factory =
        crate::host::removed_javascript("node:fs/shared-async-readlink.js")?;
    let readlink = context.call_rooted(readlink_factory, undefined, &[readlink_sync])?;
    set(context, module, "readlink", readlink)?;
    let realpath_host =
        context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let realpath_sync_factory =
        crate::host::removed_javascript("node:fs/shared-realpath-sync.js")?;
    let undefined = context.undefined();
    let realpath_sync = context.call_rooted(realpath_sync_factory, undefined, &[realpath_host])?;
    let realpath_sync =
        super::fs_policy_shared_vm::wrap_vfs_pair(context, realpath_sync, "realpathSync")?;
    set(context, module, "realpathSync", realpath_sync)?;
    let realpath_factory =
        crate::host::removed_javascript("node:fs/shared-realpath.js")?;
    let undefined = context.undefined();
    let realpath = context.call_rooted(realpath_factory, undefined, &[realpath_sync])?;
    set(context, module, "realpath", realpath)?;
    let constants = fs_constants(context)?;
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(module, key, constants, false, true, false)?;
    stat::install(context, module)?;
    super::fs_stat_watch_shared_vm::install(context, module)?;
    let rm = context.host_function(crate::host::shared_vm::operation("fsRm"))?;
    let factory = crate::host::removed_javascript("node:fs/rm-facade.js")?;
    let undefined = context.undefined();
    let rm = context.call_rooted(factory, undefined, &[rm])?;
    set(context, module, "rm", rm)?;
    sync::install(context, module)?;
    for name in [
        "fstatSync",
        "closeSync",
        "renameSync",
        "linkSync",
        "copyFileSync",
        "rmSync",
        "rmdirSync",
        "unlinkSync",
        "symlinkSync",
        "truncateSync",
        "mkdtempSync",
        "chownSync",
        "lchownSync",
        "utimesSync",
        "fchownSync",
        "ftruncateSync",
        "fsyncSync",
        "fdatasyncSync",
    ] {
        let original = get(context, module, name)?;
        let function = metadata_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    // SDK policy runs before data/path conversion. Replace the legacy public
    // byte shim before callbacks, promises and cp capture these functions.
    for (name, operation) in [
        ("writeFileSync", "fsWriteFilePublic"),
        ("appendFileSync", "fsAppendFilePublic"),
    ] {
        let function =
            context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        set(context, module, name, function)?;
    }
    let mkdir_sync = get(context, module, "mkdirSync")?;
    let mkdir_sync = super::fs_policy_shared_vm::wrap_vfs_pair(context, mkdir_sync, "mkdirSync")?;
    set(context, module, "mkdirSync", mkdir_sync)?;
    let fsync_sync = get(context, module, "fsyncSync")?;
    let fdatasync_sync = get(context, module, "fdatasyncSync")?;
    let async_sync_factory =
        crate::host::removed_javascript("node:fs/shared-async-sync.js")?;
    let async_sync =
        context.call_rooted(async_sync_factory, undefined, &[fsync_sync, fdatasync_sync])?;
    let fsync_async = get(context, async_sync, "fsync")?;
    let fdatasync_async = get(context, async_sync, "fdatasync")?;
    set(context, module, "fsync", fsync_async)?;
    set(context, module, "fdatasync", fdatasync_async)?;
    let write_sync = get(context, module, "writeSync")?;
    let write_factory = crate::host::removed_javascript("node:fs/shared-write.js")?;
    let write = context.call_rooted(write_factory, undefined, &[write_sync])?;
    set(context, module, "write", write)?;
    let writev_factory = crate::host::removed_javascript("node:fs/shared-writev.js")?;
    let writev_api = context.call_rooted(writev_factory, undefined, &[write_sync])?;
    let writev = get(context, writev_api, "writev")?;
    let writev_sync = get(context, writev_api, "writevSync")?;
    set(context, module, "writev", writev)?;
    set(context, module, "writevSync", writev_sync)?;
    let readv_read_sync = get(context, module, "readSync")?;
    let readv_factory = crate::host::removed_javascript("node:fs/shared-readv.js")?;
    let readv_api = context.call_rooted(readv_factory, undefined, &[readv_read_sync])?;
    let readv = get(context, readv_api, "readv")?;
    let readv_sync = get(context, readv_api, "readvSync")?;
    set(context, module, "readv", readv)?;
    set(context, module, "readvSync", readv_sync)?;
    let truncate_sync = get(context, module, "truncateSync")?;
    let truncate_factory =
        crate::host::removed_javascript("node:fs/shared-async-truncate.js")?;
    let truncate = context.call_rooted(truncate_factory, undefined, &[truncate_sync])?;
    set(context, module, "truncate", truncate)?;
    let ftruncate_sync = get(context, module, "ftruncateSync")?;
    let ftruncate_factory =
        crate::host::removed_javascript("node:fs/shared-async-ftruncate.js")?;
    let ftruncate = context.call_rooted(ftruncate_factory, undefined, &[ftruncate_sync])?;
    set(context, module, "ftruncate", ftruncate)?;
    let stat_factory =
        crate::host::removed_javascript("node:fs/shared-async-stat.js")?;
    let undefined = context.undefined();
    let stat = context.call_rooted(stat_factory, undefined, &[stat_sync, to_path_if_file_url])?;
    set(context, module, "stat", stat)?;
    let mkdir_sync = get(context, module, "mkdirSync")?;
    let mkdir_factory =
        crate::host::removed_javascript("node:fs/shared-async-mkdir.js")?;
    let undefined = context.undefined();
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, module, "mkdir", mkdir)?;
    let rmdir_sync = get(context, module, "rmdirSync")?;
    let rmdir_factory =
        crate::host::removed_javascript("node:fs/shared-async-rmdir.js")?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, module, "rmdir", rmdir)?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let mkdtemp_factory =
        crate::host::removed_javascript("node:fs/shared-async-mkdtemp.js")?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, module, "mkdtemp", mkdtemp)?;
    let copy_file_sync = get(context, module, "copyFileSync")?;
    let copy_file_factory =
        crate::host::removed_javascript("node:fs/shared-async-copyfile.js")?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, module, "copyFile", copy_file)?;
    let cp_factory = crate::host::removed_javascript("node:fs/shared-cp.js")?;
    let lstat_sync = get(context, module, "lstatSync")?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let symlink_sync_for_cp = get(context, module, "symlinkSync")?;
    let readlink_sync_for_cp = get(context, module, "readlinkSync")?;
    let realpath_sync_for_cp = get(context, module, "realpathSync")?;
    let utimes_sync_for_cp = get(context, module, "utimesSync")?;
    let cp_api = context.call_rooted(
        cp_factory,
        undefined,
        &[
            exists_sync,
            stat_sync,
            lstat_sync,
            readdir_sync,
            mkdir_sync,
            rmdir_sync,
            chmod_sync,
            unlink_sync,
            copy_file_sync,
            symlink_sync_for_cp,
            readlink_sync_for_cp,
            realpath_sync_for_cp,
            utimes_sync_for_cp,
        ],
    )?;
    let cp_sync = get(context, cp_api, "cpSync")?;
    let cp = get(context, cp_api, "cp")?;
    let cp_promise = get(context, cp_api, "cpPromise")?;
    set(context, module, "cpSync", cp_sync)?;
    set(context, module, "cp", cp)?;
    let symlink_sync = get(context, module, "symlinkSync")?;
    let symlink_factory =
        crate::host::removed_javascript("node:fs/shared-async-symlink.js")?;
    let symlink = context.call_rooted(symlink_factory, undefined, &[symlink_sync])?;
    set(context, module, "symlink", symlink)?;
    let link_sync = get(context, module, "linkSync")?;
    let link_factory =
        crate::host::removed_javascript("node:fs/shared-async-link.js")?;
    let link = context.call_rooted(link_factory, undefined, &[link_sync])?;
    set(context, module, "link", link)?;
    let rename_sync = get(context, module, "renameSync")?;
    let rename_factory =
        crate::host::removed_javascript("node:fs/shared-async-rename.js")?;
    let rename = context.call_rooted(rename_factory, undefined, &[rename_sync])?;
    set(context, module, "rename", rename)?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let unlink_factory =
        crate::host::removed_javascript("node:fs/shared-async-unlink.js")?;
    let unlink = context.call_rooted(unlink_factory, undefined, &[unlink_sync])?;
    set(context, module, "unlink", unlink)?;
    for (name, sync_name, descriptor) in [
        ("chown", "chownSync", false),
        ("lchown", "lchownSync", false),
        ("fchown", "fchownSync", true),
    ] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_factory =
            crate::host::removed_javascript("node:fs/shared-async-owner.js")?;
        let descriptor_value = context.boolean(descriptor);
        let owner =
            context.call_rooted(owner_factory, undefined, &[owner_sync, descriptor_value])?;
        set(context, module, name, owner)?;
    }
    let open_sync = get(context, module, "openSync")?;
    let close_sync = get(context, module, "closeSync")?;
    let fstat_sync = get(context, module, "fstatSync")?;
    let factory =
        crate::host::removed_javascript("node:fs/shared-open-close.js")?;
    let undefined = context.undefined();
    let read_sync = get(context, module, "readSync")?;
    let shared = context.host_mut().shared_state();
    for (name, function) in READ_FILE_CALLS.iter().zip([open_sync, read_sync, close_sync]) {
        let retained = context.retain(function)?;
        shared.borrow_mut().builtin_cache.insert(read_file_call_key(name), retained);
    }
    let open_close = context.call_rooted(
        factory,
        undefined,
        &[open_sync, close_sync, read_sync, fstat_sync],
    )?;
    let open = get(context, open_close, "open")?;
    let close = get(context, open_close, "close")?;
    let read = get(context, open_close, "read")?;
    let fstat = get(context, open_close, "fstat")?;
    set(context, module, "open", open)?;
    set(context, module, "close", close)?;
    set(context, module, "read", read)?;
    set(context, module, "fstat", fstat)?;
    let write_file_sync = get(context, module, "writeFileSync")?;
    let write_file_factory = crate::host::removed_javascript("node:fs/shared-async-write-file.js")?;
    let write_file =
        context.call_rooted(write_file_factory, undefined, &[write_file_sync, module])?;
    set(context, module, "writeFile", write_file)?;
    let append_file_sync = get(context, module, "appendFileSync")?;
    let append_file_factory = crate::host::removed_javascript("node:fs/shared-async-append-file.js")?;
    let append_file =
        context.call_rooted(append_file_factory, undefined, &[append_file_sync, module])?;
    set(context, module, "appendFile", append_file)?;
    let rm_sync = get(context, module, "rmSync")?;
    let promises = promises_module(
        context,
        constants,
        open_sync,
        close_sync,
        read_sync,
        write_sync,
        fstat_sync,
        fchmod_sync,
        fsync_sync,
        fdatasync_sync,
        write_file_sync,
        append_file_sync,
        mkdir_sync,
        rmdir_sync,
        rm_sync,
        mkdtemp_sync,
        copy_file_sync,
        readv_sync,
        writev_sync,
    )?;
    set(context, promises, "opendir", opendir_promise)?;
    for name in ["stat", "lstat"] {
        let sync = get(context, module, &format!("{name}Sync"))?;
        let promise_factory = crate::host::removed_javascript("node:fs/promises/shared-stat.js")?;
        let promise = context.call_rooted(promise_factory, undefined, &[sync])?;
        set(context, promises, name, promise)?;
    }
    let realpath_sync = get(context, module, "realpathSync")?;
    let realpath_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-realpath.js")?;
    let realpath_promise =
        context.call_rooted(realpath_promise_factory, undefined, &[realpath_sync])?;
    set(context, promises, "realpath", realpath_promise)?;
    let statfs_sync = get(context, module, "statfsSync")?;
    let statfs_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-statfs.js")?;
    let statfs_promise = context.call_rooted(statfs_promise_factory, undefined, &[statfs_sync])?;
    set(context, promises, "statfs", statfs_promise)?;
    let mkdtemp_disposable_factory = crate::host::removed_javascript("node:fs/promises/shared-mkdtemp-disposable.js")?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let rm_sync = get(context, module, "rmSync")?;
    let mkdtemp_disposable = context.call_rooted(
        mkdtemp_disposable_factory,
        undefined,
        &[mkdtemp_sync, rm_sync],
    )?;
    set(context, promises, "mkdtempDisposable", mkdtemp_disposable)?;
    let truncate_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-truncate.js")?;
    let truncate_promise =
        context.call_rooted(truncate_promise_factory, undefined, &[truncate_sync])?;
    set(context, promises, "truncate", truncate_promise)?;
    set(context, promises, "cp", cp_promise)?;
    let symlink_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-symlink.js")?;
    let symlink_promise =
        context.call_rooted(symlink_promise_factory, undefined, &[symlink_sync])?;
    set(context, promises, "symlink", symlink_promise)?;
    let link_sync = get(context, module, "linkSync")?;
    let link_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-link.js")?;
    let link_promise = context.call_rooted(link_promise_factory, undefined, &[link_sync])?;
    set(context, promises, "link", link_promise)?;
    let rename_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-rename.js")?;
    let rename_promise = context.call_rooted(rename_promise_factory, undefined, &[rename_sync])?;
    set(context, promises, "rename", rename_promise)?;
    let unlink_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-unlink.js")?;
    let unlink_promise = context.call_rooted(unlink_promise_factory, undefined, &[unlink_sync])?;
    set(context, promises, "unlink", unlink_promise)?;
    for (name, sync_name) in [("chown", "chownSync"), ("lchown", "lchownSync")] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-owner.js")?;
        let owner_promise = context.call_rooted(owner_promise_factory, undefined, &[owner_sync])?;
        set(context, promises, name, owner_promise)?;
    }
    let access_promise_factory = crate::host::removed_javascript("node:fs/shared-access-promise.js")?;
    let access_promise = context.call_rooted(access_promise_factory, undefined, &[access_sync])?;
    set(context, promises, "access", access_promise)?;
    let chmod_promise_factory = crate::host::removed_javascript("node:fs/shared-chmod-promise.js")?;
    let chmod_promise = context.call_rooted(chmod_promise_factory, undefined, &[chmod_sync])?;
    set(context, promises, "chmod", chmod_promise)?;
    let metadata_promise_factory = crate::host::removed_javascript("node:fs/promises/shared-metadata-mutations.js")?;
    for name in ["lchmod", "utimes", "lutimes"] {
        let name_root = context.string_rooted(name);
        let promise = context.call_rooted(metadata_promise_factory, undefined, &[name_root])?;
        set(context, promises, name, promise)?;
        context.release_root(name_root);
        context.release_root(promise);
    }
    promise_public::install(context, promises)?;
    set(context, module, "promises", promises)?;


    let install_missing_fs_methods = crate::host::removed_javascript("node:bootstrap/vfs-fs-methods.js")?;
    let undefined = context.undefined();
    context.call_rooted(install_missing_fs_methods, undefined, &[module])?;
    let install_time_callbacks = crate::host::removed_javascript("node:bootstrap/fs-time-callbacks.js")?;
    let process_module = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("Node process is not initialized"))?;
    context.call_rooted(install_time_callbacks, undefined, &[module, process_module])?;
    // `createReadStream` and `createWriteStream` take the same wrapper when their lazy groups
    // materialize (`initialize_read_stream`, `write_stream::initialize`).
    for name in ["lutimesSync", "futimesSync"] {
        let original = get(context, module, name)?;
        let function = metadata_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    for name in [
        "access",
        "chmod",
        "fchmod",
        "statfs",
        "exists",
        "chown",
        "lchown",
        "fchown",
        "utimes",
        "lutimes",
        "futimes",
        "fsync",
        "fdatasync",
        "ftruncate",
        "lstat",
        "rm",
    ] {
        let original = get(context, module, name)?;
        let function = callback_public::wrap(context, original, name)?;
        set(context, module, name, function)?;
    }
    let validate_encodings = crate::host::removed_javascript("node:fs/shared-encoding-validation.js")?;
    let undefined = context.undefined();
    context.call_rooted(validate_encodings, undefined, &[module])?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_vfs_fs_fallbacks");
    Ok(())
}

pub(crate) fn promises_module(
    context: &mut NativeContext<'_, NodeHost>,
    constants: RootId,
    open_sync: RootId,
    close_sync: RootId,
    read_sync: RootId,
    write_sync: RootId,
    fstat_sync: RootId,
    fchmod_sync: RootId,
    fsync_sync: RootId,
    fdatasync_sync: RootId,
    write_file_sync: RootId,
    append_file_sync: RootId,
    mkdir_sync: RootId,
    rmdir_sync: RootId,
    rm_sync: RootId,
    mkdtemp_sync: RootId,
    copy_file_sync: RootId,
    readv_sync: RootId,
    writev_sync: RootId,
) -> Result<RootId, RootedError> {
    let factory = crate::host::removed_javascript("node:fs/promises/shared.js")?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    let stat = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let lstat = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let readdir_sync = make_readdir_sync(context)?;
    let readdir = make_readdir_async(context, readdir_sync)?;
    let readlink = context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let realpath = context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let undefined = context.undefined();
    let normalize_read = context.host_function(crate::host::shared_vm::operation(
        "fsFileHandleReadParameters",
    ))?;
    let global = context.global_root()?;
    let reflect = get(context, global, "Reflect")?;
    let apply = get(context, reflect, "apply")?;
    let (handle_bridge, register_handle, close_handle) = file_handle_write::bridge(context)?;
    let promises = context.call_rooted(
        factory,
        undefined,
        &[
            read_file,
            stat,
            lstat,
            readdir,
            readlink,
            realpath,
            open_sync,
            close_sync,
            read_sync,
            write_sync,
            fstat_sync,
            fchmod_sync,
            fsync_sync,
            fdatasync_sync,
            write_file_sync,
            append_file_sync,
            readv_sync,
            writev_sync,
            normalize_read,
            apply,
            register_handle,
            close_handle,
        ],
    )?;
    file_handle_write::install(context, promises, handle_bridge)?;
    set(context, promises, "constants", constants)?;
    let mkdir_factory = crate::host::removed_javascript("node:fs/promises/shared-mkdir.js")?;
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, promises, "mkdir", mkdir)?;
    let rmdir_factory = crate::host::removed_javascript("node:fs/promises/shared-rmdir.js")?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, promises, "rmdir", rmdir)?;
    let native_watch =
        context.host_function(crate::host::shared_vm::operation("fsRecursivePromiseWatch"))?;
    let watch_factory = crate::host::removed_javascript("node:fs/promises/watch-facade.js")?;
    let watch = context.call_rooted(watch_factory, undefined, &[native_watch])?;
    set(context, promises, "watch", watch)?;
    let rm_factory = crate::host::removed_javascript("node:fs/promises/shared-rm.js")?;
    let rm = context.call_rooted(rm_factory, undefined, &[rm_sync])?;
    set(context, promises, "rm", rm)?;
    let mkdtemp_factory = crate::host::removed_javascript("node:fs/promises/shared-mkdtemp.js")?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, promises, "mkdtemp", mkdtemp)?;
    let copy_file_factory = crate::host::removed_javascript("node:fs/promises/shared-copyfile.js")?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, promises, "copyFile", copy_file)?;
    Ok(promises)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}

pub(crate) fn resolve_shared_path(
    context: &mut NativeContext<'_, NodeHost>,
    path: String,
) -> String {
    let candidate = std::path::Path::new(&path);
    if candidate.is_absolute() {
        return path;
    }
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    cwd.join(candidate).to_string_lossy().into_owned()
}

pub(super) fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if !context.set_property_rooted(object, key, value, object)? {
        return Err(RootedError::host("cannot install shared fs binding"));
    }
    Ok(())
}

pub(crate) fn validate_read_signal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let signal = args.first().copied().unwrap_or_else(|| context.undefined());
    super::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    Ok(context.undefined())
}

/// Callback invocation lives outside the read-error branch: an exception
/// thrown by the callback escapes once rather than calling it a second time.
pub(crate) fn complete_read_file(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let signal = args.get(4).copied().unwrap_or(undefined);
    let aborted = if context
        .rooted_value(signal)
        .is_some_and(|value| value.is_undefined())
    {
        false
    } else {
        let aborted = get(context, signal, "aborted")?;
        context.truthy_rooted(aborted)?
    };
    let completion = if aborted {
        let error = context.error_rooted("The operation was aborted")?;
        let name = context.string_rooted("AbortError");
        set(context, error, "name", name)?;
        let code = context.string_rooted("ABORT_ERR");
        set(context, error, "code", code)?;
        let cause = get(context, signal, "reason")?;
        let key = context.string_rooted("cause");
        context.define_data_property_rooted(error, key, cause, true, false, true)?;
        vec![error]
    } else {
        match context.call_rooted(args[0], undefined, &[args[2], args[3]]) {
            Ok(data) => vec![context.null(), data],
            Err(error) => {
                let exception = match error.exception {
                    Some(exception) => exception,
                    None => context.error_rooted(&error.error.to_string())?,
                };
                vec![exception]
            }
        }
    };
    context.call_rooted(args[1], undefined, &completion)?;
    Ok(undefined)
}

/// The members of `fs` Node's `readFileSync` calls through the module object to read a file
/// into a new Buffer; a script that replaces one of them observes its calls.
const READ_FILE_CALLS: [&str; 3] = ["openSync", "readSync", "closeSync"];

fn read_file_call_key(name: &str) -> String {
    format!("\0builtin:fs/readFileSync/{name}")
}

/// Whether `fs` still holds the members Node's `readFileSync` calls, so reading natively
/// observes nothing those calls would.
fn read_file_calls_are_builtin(context: &mut NativeContext<'_, NodeHost>) -> Result<bool, RootedError> {
    let module = crate::host::shared_vm::commonjs::fs_module(context)?;
    let shared = context.host_mut().shared_state();
    for name in READ_FILE_CALLS {
        if context.is_pending_lazy_data_property_rooted(module, name)? {
            continue;
        }
        let Some(original) = shared.borrow().builtin_cache.get(&read_file_call_key(name)).copied() else {
            return Ok(false);
        };
        let current = get(context, module, name)?;
        if !context.strict_equal_rooted(current, original)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Node's `readFileSync` of a regular file of known size without options: the bytes go into
/// `Buffer.allocUnsafe(size)` (its pooled allocation), cut to the bytes read when the file
/// shrank since its size was taken. Before `buffer.js` has loaded, the host allocates as it
/// would, without loading it (`allocate_before_library`).
fn read_into_unsafe_buffer(context: &mut NativeContext<'_, NodeHost>, bytes: Vec<u8>, size: u64) -> Result<RootId, RootedError> {
    let early = usize::try_from(size)
        .ok()
        .map(|size| crate::modules::buffer_shared_vm::allocate_before_library(context, size))
        .transpose()?
        .flatten();
    let buffer = match early {
        Some(buffer) => buffer,
        None => {
            let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
            let constructor = get(context, buffer_module, "Buffer")?;
            let allocate = get(context, constructor, "allocUnsafe")?;
            let length = context.number(size as f64);
            context.call_rooted(allocate, constructor, &[length])?
        }
    };
    let Some(storage) = context.buffer_storage_rooted(buffer)? else {
        return Err(RootedError::host("Buffer.allocUnsafe returned no buffer"));
    };
    let copied = bytes.len().min(storage.byte_length);
    if copied > 0 {
        // SAFETY: the view's pinned storage holds `byte_length` bytes and `bytes` is a separate
        // allocation.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), storage.data, copied) };
    }
    if (bytes.len() as u64) >= size {
        return Ok(buffer);
    }
    let slice = get(context, buffer, "slice")?;
    let start = context.number(0.0);
    let end = context.number(bytes.len() as f64);
    context.call_rooted(slice, buffer, &[start, end])
}

/// Whether `options` is the string `utf8` or `utf-8` and Node's option policy for it would
/// observe nothing. `getOptions(options, defaults)` then copies `defaults`, sets the encoding
/// (which `assertEncoding` accepts) and reads `signal`; `validateReadFileBufferOptions` reads
/// `buffer`. Where the plain `defaults` finds neither key along its prototype chain (a lookup
/// that runs no guest code), those reads return `undefined`, so `defaults` given the
/// encoding stands for the copy.
fn utf8_encoding_option(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    defaults: RootId,
) -> Result<bool, RootedError> {
    if !matches!(context.string_text(options)?.as_deref(), Some("utf8" | "utf-8")) {
        return Ok(false);
    }
    for name in ["signal", "buffer"] {
        let key = context.string_rooted(name);
        if context.has_property_rooted(defaults, key)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn read_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let undefined = context.undefined();
    let provided_options = args.get(1).copied().unwrap_or(undefined);
    if let Some(result) =
        super::fs_policy_shared_vm::vfs_read(context, "readFileSync", &[input, provided_options])?
    {
        return Ok(result);
    }
    let defaults = context.object_rooted()?;
    let key = context.string_rooted("flag");
    let read_flag = context.string_rooted("r");
    context.define_data_property_rooted(defaults, key, read_flag, true, true, true)?;
    // Node's `getOptions(undefined | null, defaults)` returns the defaults, which pass
    // `validateReadFileBufferOptions`: such a call reads with flag `r` into a new Buffer, and
    // needs none of Node's option policy.
    let defaulted = context
        .rooted_value(provided_options)
        .is_some_and(|value| value.is_undefined() || value.is_null());
    // An encoding name alone (`utf8_encoding_option`) also leaves the default flag `r`.
    let default_flag = defaulted || utf8_encoding_option(context, provided_options, defaults)?;
    let options = if defaulted {
        defaults
    } else if default_flag {
        let key = context.string_rooted("encoding");
        context.define_data_property_rooted(defaults, key, provided_options, true, true, true)?;
        defaults
    } else {
        let options =
            super::fs_policy_shared_vm::call(context, "getOptions", &[provided_options, defaults])?;
        super::fs_policy_shared_vm::call(context, "validateReadFileBufferOptions", &[options])?;
        options
    };
    let user_buffer = get(context, options, "buffer")?;
    let has_user_buffer = !context
        .rooted_value(user_buffer)
        .is_some_and(|value| value.is_undefined());
    let encoding = get(context, options, "encoding")?;
    let utf8 = if context.string_text(encoding)?.as_deref() == Some("utf8") {
        true
    } else {
        let encoding = get(context, options, "encoding")?;
        context.string_text(encoding)?.as_deref() == Some("utf-8")
    };
    let fast_utf8 = utf8 && !has_user_buffer;
    // Node's isInt32 intentionally performs ToInt32 before strict equality,
    // including observable valueOf/toString on non-numeric paths.
    let zero = context.number(0.0);
    let integer = context.bitwise_or_rooted(input, zero)?;
    let fd = if context.strict_equal_rooted(input, integer)? {
        Some(context.to_number(integer)? as i32)
    } else {
        None
    };
    // Without an encoding, only a primitive path (whose validation reads nothing) is read here,
    // while `fs` holds the members Node's reads call.
    let native_buffer = !fast_utf8
        && defaulted
        && fd.is_none()
        && context.string_text(input)?.is_some()
        && read_file_calls_are_builtin(context)?;
    if !fast_utf8 && !native_buffer {
        return super::fs_read_file_shared_vm::read_general(
            context,
            input,
            options,
            fd.is_some(),
            has_user_buffer,
        );
    }
    let mut filename = None;
    let bytes = if let Some(fd) = fd {
        if !default_flag {
            let flag = get(context, options, "flag")?;
            super::fs_policy_shared_vm::call(context, "stringToFlags", &[flag])?;
        }
        let fs = context.host_mut().shared_state().borrow().fs.clone();
        fs.with_descriptor(fd, |file| Ok(fs_ops::read_file(file, MAX_READ_FILE_BYTES)))
            .unwrap_or_else(|error| {
                Err(ReadFileFailure::Io {
                    syscall: "read",
                    error,
                })
            })
            .map(|bytes| {
                let size = bytes.len() as u64;
                (bytes, size)
            })
    } else {
        let original = input;
        let input = super::fs_path::validate(context, input, "path")?;
        let flag = get(context, options, "flag")?;
        let path = super::fs_path::resolve(context, input)?;
        // Node reads a file of unknown size in chunks it concatenates into a pooled Buffer;
        // only a regular file of known size reads into a Buffer of its own.
        if !fast_utf8 && !std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0) {
            return super::fs_read_file_shared_vm::read_general(context, original, options, false, false);
        }
        // Node's `stringToFlags('r')`.
        let flags = if default_flag {
            libc::O_RDONLY
        } else {
            let flags = super::fs_policy_shared_vm::call(context, "stringToFlags", &[flag])?;
            context.to_number(flags)? as i32
        };
        filename = Some(super::fs_path::display(context, input)?);
        #[cfg(unix)]
        let opened = fs_ops::open_numeric(&path, flags, None);
        #[cfg(not(unix))]
        let opened = {
            let _ = flags;
            fs_ops::open(&path, context.string_text(flag)?.as_deref(), None)
        };
        match opened {
            Ok(mut file) if native_buffer => fs_ops::read_sized_file(&mut file, MAX_READ_FILE_BYTES),
            Ok(mut file) => fs_ops::read_file(&mut file, MAX_READ_FILE_BYTES).map(|bytes| {
                let size = bytes.len() as u64;
                (bytes, size)
            }),
            Err(error) => Err(ReadFileFailure::Io {
                syscall: "open",
                error,
            }),
        }
    };
    match bytes {
        Ok((bytes, _)) if fast_utf8 => super::buffer_binding_shared_vm::decode_bytes(context, &bytes, "utf8"),
        Ok((bytes, size)) => read_into_unsafe_buffer(context, bytes, size),
        Err(ReadFileFailure::TooLarge(size)) => {
            let error =
                context.range_error_rooted(&format!("File size ({size}) is greater than 2 GiB"))?;
            super::node_errors_shared_vm::decorate_formatted_range_error(
                context,
                error,
                "ERR_FS_FILE_TOO_LARGE",
            )?;
            Err(context.throw(error))
        }
        Err(ReadFileFailure::Io { syscall, error }) => {
            let path = if syscall == "open" {
                filename.as_deref()
            } else {
                None
            };
            sync::throw_io_error(context, syscall, path, &error)
        }
    }
}

pub(crate) fn read_stream_open(
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
    let path = resolve_shared_path(context, path);
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .open_read_stream(path.clone());
    match result {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => Err(stream_io_error(context, error, "open", &path)?),
    }
}

pub(crate) fn read_stream_read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    let requested = args
        .get(1)
        .copied()
        .and_then(|arg| context.rooted_value(arg))
        .and_then(|value| value.as_number())
        .filter(|size| size.is_finite() && *size > 0.0)
        .unwrap_or(READ_STREAM_CHUNK_LIMIT as f64) as usize;
    let size = requested.min(READ_STREAM_CHUNK_LIMIT);
    let position = args
        .get(2)
        .copied()
        .and_then(|arg| context.rooted_value(arg))
        .and_then(|value| value.as_number())
        .filter(|position| position.is_finite() && *position >= 0.0 && position.fract() == 0.0)
        .and_then(|position| u64::try_from(position as u128).ok());
    let (result, path) = {
        let shared_state = context.host_mut().shared_state();
        let state = shared_state.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        let result = state.fs.read_stream_chunk(fd, size, position);
        (result, path)
    };
    match result {
        Ok(Some(bytes)) => crate::modules::buffer_shared_vm::from_vec(context, bytes),
        Ok(None) => Ok(context.null()),
        Err(error) => Err(stream_io_error(context, error, "read", &path)?),
    }
}

pub(crate) fn read_stream_close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_read_stream(fd);
    Ok(context.undefined())
}

const READ_STREAM_CHUNK_LIMIT: usize = 64 * 1024;

pub(super) fn integer_arg(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    name: &str,
) -> Result<i32, RootedError> {
    let number = argument
        .and_then(|argument| context.rooted_value(argument))
        .and_then(|value| value.as_number())
        .filter(|number| number.is_finite() && number.fract() == 0.0);
    match number.and_then(|number| i32::try_from(number as i64).ok()) {
        Some(number) => Ok(number),
        None => {
            let error = context.type_error_rooted(&format!("invalid internal fs {name}"))?;
            Err(context.throw(error))
        }
    }
}

pub(super) fn stream_io_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
    syscall: &str,
    path: &str,
) -> Result<RootedError, RootedError> {
    sync::io_error(context, syscall, (!path.is_empty()).then_some(path), &error)
}

pub(crate) const READ_STREAM_MEMBERS: &[&str] = &["createReadStream", "ReadStream"];

/// Defines `fs.createReadStream` and `fs.ReadStream` over `stream.Readable` when a script first
/// observes either (the lazy group `module` installs).
pub(crate) fn initialize_read_stream(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let read_stream = streams::read_stream_class(context)?;
    let create_read_stream = context.host_function_named_with_data(
        crate::host::shared_vm::operation("fsCreateStream"), "createReadStream", read_stream,
    )?;
    let create_read_stream = metadata_public::wrap(context, create_read_stream, "createReadStream")?;
    set(context, module, "createReadStream", create_read_stream)?;
    set(context, module, "ReadStream", read_stream)?;
    define_legacy_alias(context, module, "FileReadStream", read_stream)?;
    if permission_model_enabled(context) {
        guard_permissions(context, module, Some(&["createReadStream"]))?;
    }
    Ok(context.undefined())
}

/// Whether Node's permission model (`--permission`, `--permission-audit`) is on: only then
/// does `PERMISSION_GUARD` wrap a lazy group's members.
pub(super) fn permission_model_enabled(context: &mut NativeContext<'_, NodeHost>) -> bool {
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    state.cli_options.enabled("--permission") || state.cli_options.enabled("--permission-audit")
}

/// Node's non-enumerable legacy name for a stream constructor (`FileReadStream`,
/// `FileWriteStream`), defined with the constructor's lazy group.
fn define_legacy_alias(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
    name: &str,
    constructor: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(module, key, constructor, true, false, true)?;
    Ok(())
}

/// The members `fs` takes from Node's own `lib/fs.js` and `internal/fs/utils.js`, in the order
/// the module defines them.
const LIBRARY_POLICY_MEMBERS: [&str; 6] =
    ["Dirent", "glob", "globSync", "readdirSync", "Utf8Stream", "_toUnixTimestamp"];

/// Builds the members of `fs`'s lazy groups (`MODULE_MEMBERS`, then `LIBRARY_POLICY_MEMBERS`),
/// as Node's `fs` defines them all as it loads: a startup snapshot that preloads `fs` builds
/// them at the snapshot point (`startup_snapshot::preload`), where each process had built them
/// at its first use of a member other than `readFileSync` (8.7M instructions for an
/// `existsSync`, x86_64, 2026-10-23).
pub(crate) fn materialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let module = crate::host::shared_vm::commonjs::fs_module(context)?;
    let (first_member, _) = MODULE_MEMBERS[0];
    context.materialize_lazy_data_property_rooted(module, first_member)
}

/// Install Node's own glob, directory reader and `Dirent` when a script first observes one of
/// them: loading `lib/fs.js` builds Buffer, util and their dependencies, which reading a file
/// does not need. Node's promises module is likewise loaded only when requested; in particular
/// its readline dependencies must not eagerly capture the OS binding while merely requiring fs.
fn install_library_policy(
    context: &mut NativeContext<'_, NodeHost>, module: RootId,
) -> Result<(), RootedError> {
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("fsLibraryPolicyInitialize"), module,
    )?;
    context.define_lazy_data_properties_rooted(module, &LIBRARY_POLICY_MEMBERS, true, initialize)?;
    let promises = get(context, module, "promises")?;
    let initializer = context.host_function(crate::host::shared_vm::operation(
        "fsPromisesGlobInitialize",
    ))?;
    context.define_lazy_data_properties_rooted(promises, &["glob"], true, initializer)
}

/// The `fs` members built by a lazy group other than `MODULE_MEMBERS`' that `PERMISSION_GUARD`
/// wraps; each group guards its own when it materializes.
const GUARDED_LAZY_MEMBERS: [&str; 3] = ["readdirSync", "createReadStream", "createWriteStream"];

/// The private names under which `PERMISSION_GUARD` reads the realpath natives it wraps, and
/// the public functions they come from.
const GUARD_REALPATH_NATIVES: [(&str, &str); 2] = [
    ("__quenchInternalRealpathSyncNative", "realpathSync"),
    ("__quenchInternalRealpathNative", "realpath"),
];

/// Node's permission checks (`PERMISSION_GUARD`) over every member of `fs` (`members` `None`,
/// when `MODULE_MEMBERS` materialize) or over the `members` a later lazy group built. The guard
/// runs over a view whose prototype is the module: a pending lazy member is shadowed by
/// `undefined`, so the guard observes no group early, and a later group's run shadows
/// `fs.promises`, which the first run already guarded. Its wrappers are copied back.
fn guard_permissions(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
    members: Option<&[&str]>,
) -> Result<(), RootedError> {
    let view = context.object_rooted()?;
    context.set_prototype_rooted(view, module)?;
    let undefined = context.undefined();
    let mut shadowed = Vec::new();
    for name in GUARDED_LAZY_MEMBERS {
        if context.is_pending_lazy_data_property_rooted(module, name)? {
            shadowed.push(name);
        }
    }
    match members {
        Some(_) => shadowed.push("promises"),
        None => {
            for (internal, public) in GUARD_REALPATH_NATIVES {
                let function = get(context, module, public)?;
                let native = get(context, function, "native")?;
                let key = context.string_rooted(internal);
                context.define_data_property_rooted(view, key, native, true, false, true)?;
            }
        }
    }
    // Own data properties of the view: an assignment would look the name up on the module and
    // so observe a pending lazy member.
    for name in &shadowed {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(view, key, undefined, true, false, true)?;
    }
    let guard = crate::host::removed_javascript("node:fs/permission-guard.js")?;
    context.call_rooted(guard, undefined, &[view])?;
    let names = match members {
        Some(members) => members.iter().map(|name| (*name).to_owned()).collect(),
        None => {
            let mut names = Vec::new();
            for key in context.own_keys_rooted(view)? {
                if !context.is_symbol_rooted(key)? {
                    names.push(context.to_string(key)?);
                }
            }
            names.retain(|name| !shadowed.contains(&name.as_str()));
            names
        }
    };
    for name in names {
        if context.has_own_property_rooted(view, &name)? {
            let value = get(context, view, &name)?;
            set(context, module, &name, value)?;
        }
    }
    Ok(())
}

/// Defines the library members on the `fs` module (the lazy group `install_library_policy`
/// defines) from the directory policy.
pub(crate) fn initialize_library_policy(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let policy = library_policy(context)?;
    for name in LIBRARY_POLICY_MEMBERS {
        let value = get(context, policy, name)?;
        set(context, module, name, value)?;
    }
    guard_permissions(context, module, Some(&["readdirSync"]))?;
    Ok(context.undefined())
}

pub(crate) fn initialize_promises_glob_policy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let policy = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "fs.js",
        "internal/fs/promises.js",
    )?;
    let exports = get(context, policy, "exports")?;
    let glob = get(context, exports, "glob")?;
    let module = crate::host::shared_vm::commonjs::fs_module(context)?;
    let promises = get(context, module, "promises")?;
    set(context, promises, "glob", glob)?;
    Ok(context.undefined())
}

/// Definitions behind the SDK's lazyGlob and lazyMinimatch closures. Loading
/// these modules builds policy; traversing a directory remains a guest operation.
#[cfg(feature = "aot")]
pub(crate) fn stage_glob_policy(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    for (owner, id) in [("fs.js", "internal/fs/glob"), ("internal/fs/glob.js", "internal/deps/minimatch/index")] {
        crate::host::shared_vm::commonjs::stage_library_dependency(context, owner, id)?;
    }
    Ok(())
}

/// The private copy of Node's library members that the native readers call, whatever a script
/// later assigns to the public module. It is filled from `lib/fs.js` on first use.
fn directory_policy(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        context,
        "\0binding:fs-directory-policy",
        |context| context.null_object_rooted(),
    )
}

/// The directory policy, loading Node's `lib/fs.js` into it the first time. The native module is
/// cached before this runs, so the library and the native module observe one completed
/// filesystem object.
fn library_policy(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let policy = directory_policy(context)?;
    let reader = get(context, policy, "readdirSync")?;
    if !context.rooted_value(reader).is_some_and(|value| value.is_undefined()) {
        return Ok(policy);
    }
    let library = crate::host::shared_vm::commonjs::node_library_module(context, "fs.js", "fs.js")?;
    let dirent = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/fs/utils.js",
        "Dirent",
    )?;
    set(context, policy, "Dirent", dirent)?;
    for name in ["glob", "globSync", "readdirSync"] {
        let function = get(context, library, name)?;
        set(context, policy, name, function)?;
    }
    // Node 26's fast UTF-8 writer and its historical timestamp coercion helper.
    let utf8_stream = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "fs.js",
        "internal/streams/fast-utf8-stream.js",
    )?;
    set(context, policy, "Utf8Stream", utf8_stream)?;
    let to_unix_timestamp = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "fs.js",
        "internal/fs/utils.js",
        "toUnixTimestamp",
    )?;
    set(context, policy, "_toUnixTimestamp", to_unix_timestamp)?;
    Ok(policy)
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
