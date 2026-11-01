//! `fs.cp` and `fs.cpSync` (`internal/fs/cp/*`): a recursive copy whose checks the sync
//! API reports as Node's binding does and the asynchronous APIs as its library does.

use super::api::{Call, Reply};
use super::args::{self, FsPath};
use super::failure::{self, Coded, Syscall};
use crate::modules::stream_shared_vm::{get, is_undefined, Context};
use quench_runtime::{PromiseSnapshot, RootId, RootedError};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// `cp`'s validated options (`validateCpOptions`).
#[derive(Clone, Copy)]
struct Options {
    dereference: bool,
    error_on_exist: bool,
    force: bool,
    preserve_timestamps: bool,
    recursive: bool,
    verbatim_symlinks: bool,
    mode: i32,
    filter: Option<RootId>,
    /// Whether errors take the binding's form (`cpSync`) or the library's (`cp`).
    sync: bool,
}

fn options(c: &mut Context<'_>, value: RootId, sync: bool) -> Result<Options, RootedError> {
    let mut options = Options {
        dereference: false,
        error_on_exist: false,
        force: true,
        preserve_timestamps: false,
        recursive: false,
        verbatim_symlinks: false,
        mode: 0,
        filter: None,
        sync,
    };
    if is_undefined(c, value) {
        return Ok(options);
    }
    if !c.is_object_rooted(value)? || c.is_array_rooted(value)? {
        return Err(crate::modules::util_shared_vm::argument_type_error(c, value, "options", "Object")?);
    }
    for (name, field) in [
        ("dereference", &mut options.dereference),
        ("errorOnExist", &mut options.error_on_exist),
        ("force", &mut options.force),
        ("preserveTimestamps", &mut options.preserve_timestamps),
        ("recursive", &mut options.recursive),
        ("verbatimSymlinks", &mut options.verbatim_symlinks),
    ] {
        *field = args::boolean_option(c, value, name, *field)?;
    }
    let mode = get(c, value, "mode")?;
    options.mode = super::ops::valid_mode(c, mode, super::ops::MAX_COPY_MODE, 0)?;
    if options.dereference && options.verbatim_symlinks {
        let error = c.type_error_rooted("The 'dereference' and 'verbatimSymlinks' options are mutually exclusive")?;
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INCOMPATIBLE_OPTION_PAIR")?;
        return Err(c.throw(error));
    }
    let filter = get(c, value, "filter")?;
    if !is_undefined(c, filter) {
        options.filter = Some(args::function(c, filter, "options.filter")?);
    }
    Ok(options)
}

pub(crate) fn cp(c: &mut Context<'_>, call_args: &[RootId], sync: bool) -> Call {
    let options = options(c, call_args[2], sync)?;
    let source = args::path(c, call_args[0], "src")?;
    let destination = args::path(c, call_args[1], "dest")?;
    let copy = Copy { options };
    Ok(copy.entry(c, &source, &destination).and_then(|()| Ok(Reply::Value(c.undefined()))))
}

/// The conditions a copy refuses, each reported in the binding's or the library's form.
#[derive(Clone, Copy)]
enum Refusal {
    Same,
    DirectoryToNonDirectory,
    NonDirectoryToDirectory,
    Subdirectory,
    Directory,
    Socket,
    Fifo,
    Unknown,
}

struct Copy {
    options: Options,
}

/// `path` with a trailing separator, as the binding prints a directory.
fn directory_text(path: &str) -> String {
    if path.ends_with('/') { path.to_owned() } else { format!("{path}/") }
}

impl Copy {
    fn refuse(&self, c: &mut Context<'_>, refusal: Refusal, source: &str, destination: &str) -> RootedError {
        if self.options.sync {
            let (code, message) = match refusal {
                Refusal::Same => ("ERR_FS_CP_EINVAL", format!("src and dest cannot be the same {destination}")),
                Refusal::DirectoryToNonDirectory => ("ERR_FS_CP_DIR_TO_NON_DIR", format!("Cannot overwrite non-directory {destination} with directory {source}")),
                Refusal::NonDirectoryToDirectory => ("ERR_FS_CP_NON_DIR_TO_DIR", format!("Cannot overwrite directory {destination} with non-directory {source}")),
                Refusal::Subdirectory => ("ERR_FS_CP_EINVAL", format!("Cannot copy {} to a subdirectory of self {destination}", directory_text(source))),
                Refusal::Directory => ("ERR_FS_EISDIR", format!("Recursive option not enabled, cannot copy a directory: {}", directory_text(source))),
                Refusal::Socket => ("ERR_FS_CP_SOCKET", format!("Cannot copy a socket file: {destination}")),
                Refusal::Fifo => ("ERR_FS_CP_FIFO_PIPE", format!("Cannot copy a FIFO pipe: {destination}")),
                Refusal::Unknown => ("ERR_FS_CP_UNKNOWN", format!("Cannot copy an unknown file type: {destination}")),
            };
            return failure::plain(c, code, &message, false);
        }
        let (kind, message, path) = match refusal {
            Refusal::Same => (Coded::CopyInvalid, "src and dest cannot be the same".to_owned(), destination),
            Refusal::DirectoryToNonDirectory => (Coded::CopyDirectoryToNonDirectory, format!("cannot overwrite non-directory {destination} with directory {source}"), destination),
            Refusal::NonDirectoryToDirectory => (Coded::CopyNonDirectoryToDirectory, format!("cannot overwrite directory {destination} with non-directory {source}"), destination),
            Refusal::Subdirectory => (Coded::CopyInvalid, format!("cannot copy {source} to a subdirectory of self {destination}"), destination),
            Refusal::Directory => (Coded::IsDirectory, format!("{source} is a directory (not copied)"), source),
            Refusal::Socket => (Coded::CopySocket, format!("cannot copy a socket file: {destination}"), destination),
            Refusal::Fifo => (Coded::CopyFifo, format!("cannot copy a FIFO pipe: {destination}"), destination),
            Refusal::Unknown => (Coded::CopyUnknown, format!("cannot copy an unknown file type: {destination}"), destination),
        };
        failure::coded(c, kind, &message, "cp", path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        super::ops::metadata(path, self.options.dereference)
    }

    /// `opts.filter(src, dest)`, whose settled value a promise stands for.
    fn admits(&self, c: &mut Context<'_>, source: &str, destination: &str) -> Result<bool, RootedError> {
        let Some(filter) = self.options.filter else {
            return Ok(true);
        };
        let source = c.string_rooted(source);
        let destination = c.string_rooted(destination);
        let undefined = c.undefined();
        let verdict = c.call_rooted(filter, undefined, &[source, destination])?;
        if c.is_promise_rooted(verdict)? {
            if let Some(PromiseSnapshot::Fulfilled(value)) = c.promise_snapshot_rooted(verdict)? {
                return c.truthy_rooted(value);
            }
            if self.options.sync {
                let error = c.type_error_rooted("Expected a boolean to be returned from the \"filter\" function but got an instance of Promise.")?;
                crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_RETURN_VALUE")?;
                return Err(c.throw(error));
            }
        }
        c.truthy_rooted(verdict)
    }

    fn entry(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath) -> Result<(), RootedError> {
        if !self.admits(c, &source.shown, &destination.shown)? {
            return Ok(());
        }
        self.check(c, source, destination)?;
        if let Some(parent) = destination.io.parent() {
            if !parent.exists() {
                let made = super::ops::make_directories(parent, super::ops::DEFAULT_DIRECTORY_MODE);
                failure::check(c, made, Syscall::at("mkdir", &parent.to_string_lossy()))?;
            }
        }
        self.copy(c, source, destination)
    }

    /// `cpSyncCheckPaths` / `checkPaths`.
    fn check(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath) -> Result<(), RootedError> {
        let syscall = if self.options.dereference { "stat" } else { "lstat" };
        let source_metadata = self.metadata(&source.io);
        let source_metadata = failure::check(c, source_metadata, Syscall::at(syscall, &source.shown))?;
        let source_directory = source_metadata.is_dir();
        if let Ok(destination_metadata) = self.metadata(&destination.io) {
            if source_metadata.dev() == destination_metadata.dev() && source_metadata.ino() == destination_metadata.ino() {
                return Err(self.refuse(c, Refusal::Same, &source.shown, &destination.shown));
            }
            match (source_directory, destination_metadata.is_dir()) {
                (true, false) => return Err(self.refuse(c, Refusal::DirectoryToNonDirectory, &source.shown, &destination.shown)),
                (false, true) => return Err(self.refuse(c, Refusal::NonDirectoryToDirectory, &source.shown, &destination.shown)),
                _ => {}
            }
        }
        if source_directory && is_subdirectory(&source.io, &destination.io) {
            return Err(self.refuse(c, Refusal::Subdirectory, &source.shown, &destination.shown));
        }
        if source_directory && !self.options.recursive {
            return Err(self.refuse(c, Refusal::Directory, &source.shown, &destination.shown));
        }
        Ok(())
    }

    /// `getStats`: dispatch on the source's type.
    fn copy(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath) -> Result<(), RootedError> {
        use std::os::unix::fs::FileTypeExt;
        let syscall = if self.options.dereference { "stat" } else { "lstat" };
        let metadata = self.metadata(&source.io);
        let metadata = failure::check(c, metadata, Syscall::at(syscall, &source.shown))?;
        let kind = metadata.file_type();
        if kind.is_dir() {
            if !self.options.recursive {
                return Err(self.refuse(c, Refusal::Directory, &source.shown, &destination.shown));
            }
            return self.directory(c, source, destination, &metadata);
        }
        if kind.is_file() || kind.is_char_device() || kind.is_block_device() {
            return self.file(c, source, destination, &metadata);
        }
        if kind.is_symlink() {
            return self.link(c, source, destination);
        }
        let refusal = if kind.is_socket() {
            Refusal::Socket
        } else if kind.is_fifo() {
            Refusal::Fifo
        } else {
            Refusal::Unknown
        };
        Err(self.refuse(c, refusal, &source.shown, &destination.shown))
    }

    fn file(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath, metadata: &std::fs::Metadata) -> Result<(), RootedError> {
        if std::fs::symlink_metadata(&destination.io).is_ok() {
            if !self.options.force {
                if self.options.error_on_exist {
                    let message = format!("{} already exists", destination.shown);
                    return Err(failure::coded(c, Coded::CopyExists, &message, "cp", &destination.shown));
                }
                return Ok(());
            }
            let removed = std::fs::remove_file(&destination.io);
            failure::check(c, removed, Syscall::at("unlink", &destination.shown))?;
        }
        let copied = super::ops::copy_file_contents(&source.io, &destination.io, self.options.mode);
        failure::check(c, copied, Syscall::between("copyfile", &source.shown, &destination.shown))?;
        if self.options.preserve_timestamps {
            let times = [(metadata.atime(), metadata.atime_nsec()), (metadata.mtime(), metadata.mtime_nsec())]
                .map(|(seconds, nanoseconds)| libc::timespec { tv_sec: seconds as libc::time_t, tv_nsec: nanoseconds as libc::c_long });
            let set = super::ops::c_path(&destination.io).and_then(|raw| {
                // SAFETY: `raw` is NUL-terminated and `times` holds two timespecs.
                let code = unsafe { libc::utimensat(libc::AT_FDCWD, raw.as_ptr(), times.as_ptr(), 0) };
                if code == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
            });
            failure::check(c, set, Syscall::at("utime", &destination.shown))?;
        }
        Ok(())
    }

    fn directory(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath, metadata: &std::fs::Metadata) -> Result<(), RootedError> {
        let created = std::fs::symlink_metadata(&destination.io).is_err();
        if created {
            let made = std::fs::create_dir(&destination.io);
            failure::check(c, made, Syscall::at("mkdir", &destination.shown))?;
        }
        // Only the top-level source is dereferenced; entries are copied as they are.
        let nested = Copy { options: Options { dereference: false, ..self.options } };
        let entries = crate::modules::fs_directory_shared_vm::scan_unsorted(&source.io);
        let entries = failure::check(c, entries, Syscall::bare("opendir"))?;
        for entry in entries {
            let name = entry.name.to_string_lossy();
            let item = |parent: &FsPath| FsPath { io: parent.io.join(&entry.name), shown: super::ops::join_shown(&parent.shown, &name) };
            let (source_item, destination_item) = (item(source), item(destination));
            if !nested.admits(c, &source_item.shown, &destination_item.shown)? {
                continue;
            }
            nested.check(c, &source_item, &destination_item)?;
            nested.copy(c, &source_item, &destination_item)?;
        }
        if created {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::set_permissions(&destination.io, std::fs::Permissions::from_mode(metadata.mode()));
            failure::check(c, mode, Syscall::at("chmod", &destination.shown))?;
        }
        Ok(())
    }

    fn link(&self, c: &mut Context<'_>, source: &FsPath, destination: &FsPath) -> Result<(), RootedError> {
        let target = std::fs::read_link(&source.io);
        let mut target = failure::check(c, target, Syscall::at("readlink", &source.shown))?;
        let cwd = c.host_mut().shared_state().borrow().cwd.clone();
        let cwd = cwd.current_path().map_err(|error| RootedError::host(error.to_string()))?;
        let cwd = cwd.to_string_lossy();
        if !self.options.verbatim_symlinks && target.is_relative() {
            target = resolve(&source.shown, &target, &cwd);
        }
        if std::fs::symlink_metadata(&destination.io).is_ok() {
            if let Ok(existing) = std::fs::read_link(&destination.io) {
                let existing = if existing.is_relative() { resolve(&destination.shown, &existing, &cwd) } else { existing };
                if is_subdirectory(&target, &existing) {
                    let (target, existing) = (target.to_string_lossy().into_owned(), existing.to_string_lossy().into_owned());
                    return Err(self.refuse(c, Refusal::Subdirectory, &target, &existing));
                }
            }
            let removed = std::fs::remove_file(&destination.io);
            failure::check(c, removed, Syscall::at("unlink", &destination.shown))?;
        }
        let linked = std::os::unix::fs::symlink(&target, &destination.io);
        let shown = target.to_string_lossy().into_owned();
        failure::check(c, linked, Syscall::between("symlink", &shown, &destination.shown))
    }
}

/// `path.resolve(path.dirname(link), target)`: a link's target as an absolute path.
fn resolve(link: &str, target: &Path, cwd: &str) -> PathBuf {
    let directory = crate::modules::path_algorithms::posix::dirname_str(link);
    PathBuf::from(crate::modules::path_algorithms::posix::resolve_strings(&[directory, target.to_string_lossy().into_owned()], cwd))
}

/// `isSrcSubdir(src, dest)`: whether `destination` lies inside `source`.
fn is_subdirectory(source: &Path, destination: &Path) -> bool {
    let normalize = |path: &Path| PathBuf::from(crate::modules::path_algorithms::posix::resolve_strings(&[path.to_string_lossy().into_owned()], "/"));
    let (source, destination) = (normalize(source), normalize(destination));
    destination != source && destination.starts_with(&source)
}
