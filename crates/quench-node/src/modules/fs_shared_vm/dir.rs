//! `fs.Dirent` and `fs.Dir` (`internal/fs/utils.js`, `internal/fs/dir.js`): directory
//! entries, and a directory handle that hands them out one at a time.

use super::api::Reply;
use super::args::{self, Encoding, UTF8};
use super::classes::{self, Class};
use super::failure::{self, Syscall};
use crate::modules::stream_shared_vm::{arg, get, is_undefined, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The `UV_DIRENT_*` type of a file type.
pub(crate) fn entry_type(kind: std::fs::FileType) -> i32 {
    use std::os::unix::fs::FileTypeExt;
    let types = [
        (kind.is_file(), super::UV_DIRENT_FILE),
        (kind.is_dir(), super::UV_DIRENT_DIR),
        (kind.is_symlink(), super::UV_DIRENT_LINK),
        (kind.is_block_device(), super::UV_DIRENT_BLOCK),
        (kind.is_char_device(), super::UV_DIRENT_CHAR),
        (kind.is_fifo(), super::UV_DIRENT_FIFO),
        (kind.is_socket(), super::UV_DIRENT_SOCKET),
    ];
    types.into_iter().find(|(matches, _)| *matches).map_or(super::UV_DIRENT_UNKNOWN, |(_, kind)| kind)
}

/// An entry's type, asking `lstat` when the directory did not say (`getDirent`).
pub(crate) fn resolve_kind(kind: i32, path: &Path) -> i32 {
    if kind != super::UV_DIRENT_UNKNOWN {
        return kind;
    }
    std::fs::symlink_metadata(path).map_or(kind, |metadata| entry_type(metadata.file_type()))
}

/// `new Dirent(name, type, parentPath)`.
pub(crate) fn dirent(c: &mut Context<'_>, name: RootId, kind: i32, parent: &str) -> Outcome {
    let dirent = classes::instance(c, Class::Dirent)?;
    let parent = c.string_rooted(parent);
    let kind = c.number(f64::from(kind));
    initialize_dirent(c, dirent, name, kind, parent)?;
    Ok(dirent)
}

fn initialize_dirent(c: &mut Context<'_>, dirent: RootId, name: RootId, kind: RootId, parent: RootId) -> Result<(), RootedError> {
    put(c, dirent, "name", name)?;
    put(c, dirent, "parentPath", parent)?;
    put(c, dirent, "path", parent)?;
    let key = classes::root(c, Class::DirentType)?;
    c.set_property_rooted(dirent, key, kind, dirent).map(drop)
}

/// `class Dirent { constructor(name, type, path) }`.
pub(crate) fn construct_dirent(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    classes::require_new(c, "Dirent")?;
    let (name, kind, parent) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    initialize_dirent(c, this, name, kind, parent)?;
    Ok(c.undefined())
}

/// `Dirent.prototype.isDirectory()` and its siblings: `this[kType] === UV_DIRENT_*`, the
/// type the function's data holds.
pub(crate) fn dirent_is(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let expected = c.host_function_data()?;
    let key = classes::root(c, Class::DirentType)?;
    let kind = c.get_property_rooted(this, key)?;
    let equal = c.strict_equal_rooted(kind, expected)?;
    Ok(c.boolean(equal))
}

pub(crate) fn dirent_class(c: &mut Context<'_>, _kind: RootId) -> Result<(RootId, RootId), RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("fsDirentConstruct"), "Dirent")?;
    let prototype = get(c, constructor, "prototype")?;
    for (name, _, kind) in super::stats::TYPE_PREDICATES {
        let key = c.string_rooted(name);
        let data = c.number(f64::from(kind));
        classes::method(c, prototype, key, "fsDirentIs", name, Some(data))?;
    }
    Ok((constructor, prototype))
}

/// The members of `Dir.prototype`: key, operation and function name.
const DIR_METHODS: [(&str, &str); 5] = [
    ("read", "fsDirRead"),
    ("readSync", "fsDirReadSync"),
    ("close", "fsDirClose"),
    ("closeSync", "fsDirCloseSync"),
    ("entries", "fsDirEntries"),
];

pub(crate) fn dir_class(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("fsDirConstruct"), "Dir")?;
    let prototype = get(c, constructor, "prototype")?;
    let getter = c.host_function_named(crate::host::shared_vm::operation("fsDirPath"), "get path")?;
    classes::accessor(c, prototype, "path", getter, None, false)?;
    for (name, operation) in DIR_METHODS {
        let key = c.string_rooted(name);
        classes::method(c, prototype, key, operation, name, None)?;
    }
    let dispose = classes::well_known_symbol(c, "dispose", "nodejs.dispose")?;
    classes::method(c, prototype, dispose, "fsDirDispose", "[nodejs.dispose]", None)?;
    let async_dispose = classes::well_known_symbol(c, "asyncDispose", "nodejs.asyncDispose")?;
    classes::method(c, prototype, async_dispose, "fsDirAsyncDispose", "[nodejs.asyncDispose]", None)?;
    let iterator = classes::well_known_symbol(c, "asyncIterator", "Symbol.asyncIterator")?;
    classes::method(c, prototype, iterator, "fsDirEntries", "entries", None)?;
    Ok((constructor, prototype))
}

/// `new Dir(handle, path, options)`: only `opendir` makes one.
pub(crate) fn construct_dir(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    classes::require_new(c, "Dir")?;
    let handle = arg(c, args, 0);
    if crate::modules::stream_shared_vm::is_nullish(c, handle) {
        let error = c.type_error_rooted("The \"handle\" argument must be specified")?;
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
        return Err(c.throw(error));
    }
    let error = c.type_error_rooted("Illegal constructor")?;
    Err(c.throw(error))
}

/// `opendir(path, { encoding, bufferSize, recursive })`: a `Dir` over the directory's
/// entries in the order the kernel reads them.
pub(crate) fn opendir(c: &mut Context<'_>, args: &[RootId]) -> super::api::Call {
    let path = args::path(c, args[0], "path")?;
    let options = args::options(c, args[1])?;
    let buffer_size = get(c, options, "bufferSize")?;
    if !is_undefined(c, buffer_size) {
        args::integer(c, buffer_size, "options.bufferSize", 1.0, args::MAX_UINT32)?;
    }
    let recursive = args::boolean_option(c, options, "recursive", false)?;
    let encoding = args::encoding(c, options, UTF8)?;
    let entries = if recursive {
        super::ops::list_directory(c, &path, true, true, encoding)
    } else {
        entries(c, &path.io, &path.shown, encoding)
    };
    let entries = match entries {
        Ok(entries) => entries,
        Err(error) => {
            let error = error.exception.map_or(error, |exception| reword_opendir(c, exception));
            return Ok(Err(error));
        }
    };
    let dir = classes::instance(c, Class::Dir)?;
    let state = c.null_object_rooted()?;
    put(c, state, "entries", entries)?;
    let zero = c.number(0.0);
    put(c, state, "index", zero)?;
    let shown = c.string_rooted(&path.shown);
    put(c, state, "path", shown)?;
    let key = classes::root(c, Class::DirState)?;
    c.set_property_rooted(dir, key, state, dir)?;
    Ok(Ok(Reply::Value(dir)))
}

/// `opendir`'s failure names no path (`uv_fs_opendir`'s request carries none).
fn reword_opendir(c: &mut Context<'_>, exception: RootId) -> RootedError {
    let rebuilt = (|| -> Result<RootedError, RootedError> {
        let code = get(c, exception, "errno")?;
        let errno = -(c.to_number(code)? as i32);
        Ok(failure::system(c, &std::io::Error::from_raw_os_error(errno), Syscall::bare("opendir")))
    })();
    match rebuilt {
        Ok(error) | Err(error) => error,
    }
}

fn entries(c: &mut Context<'_>, io: &Path, shown: &str, encoding: Encoding) -> Outcome {
    let scanned = crate::modules::fs_directory_shared_vm::scan_unsorted(io);
    let scanned = failure::check(c, scanned, Syscall::at("scandir", shown))?;
    let mut dirents = Vec::with_capacity(scanned.len());
    for entry in scanned {
        let kind = resolve_kind(entry.kind, &io.join(&entry.name));
        let name = encoding.project(c, entry.name.as_bytes().to_vec())?;
        dirents.push(dirent(c, name, kind, shown)?);
    }
    c.array_rooted(&dirents)
}

/// The state of the `Dir` `this` is, or `ERR_INVALID_THIS`.
fn state(c: &mut Context<'_>, this: RootId) -> Outcome {
    let key = classes::root(c, Class::DirState)?;
    let state = if c.is_object_rooted(this)? { c.get_property_rooted(this, key)? } else { c.undefined() };
    if is_undefined(c, state) {
        let error = c.type_error_rooted("Value of \"this\" must be of type Dir")?;
        crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
        return Err(c.throw(error));
    }
    Ok(state)
}

fn closed_error(c: &mut Context<'_>) -> RootedError {
    failure::plain(c, "ERR_DIR_CLOSED", "Directory handle was closed", false)
}

/// The open state of `this`, or `ERR_DIR_CLOSED`.
fn open_state(c: &mut Context<'_>, this: RootId) -> Outcome {
    let state = state(c, this)?;
    let entries = get(c, state, "entries")?;
    if is_undefined(c, entries) {
        return Err(closed_error(c));
    }
    Ok(state)
}

/// The next entry, or `null` at the end.
fn next_entry(c: &mut Context<'_>, state: RootId) -> Outcome {
    let entries = get(c, state, "entries")?;
    let index = get(c, state, "index")?;
    let position = c.to_number(index)?;
    let length = get(c, entries, "length")?;
    if position >= c.to_number(length)? {
        return Ok(c.null());
    }
    let entry = c.get_named_property_rooted(entries, &(position as usize).to_string())?;
    let next = c.number(position + 1.0);
    put(c, state, "index", next)?;
    Ok(entry)
}

fn close_state(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let undefined = c.undefined();
    put(c, state, "entries", undefined)
}

pub(crate) fn path(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    get(c, state, "path")
}

pub(crate) fn read_sync(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = open_state(c, this)?;
    next_entry(c, state)
}

/// `dir.read()`: a promise; `dir.read(callback)`: `callback(null, entry)` on a later tick.
pub(crate) fn read(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = open_state(c, this)?;
    if args.is_empty() || is_undefined(c, args[0]) {
        let promise = c.pending_promise_rooted()?;
        let entry = next_entry(c, state)?;
        super::api::settle_later(c, promise, Ok(entry))?;
        return Ok(promise);
    }
    let callback = args::function(c, args[0], "callback")?;
    let entry = next_entry(c, state)?;
    let null = c.null();
    crate::modules::stream_shared_vm::next_tick(c, callback, &[null, entry])?;
    Ok(c.undefined())
}

pub(crate) fn close_sync(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = open_state(c, this)?;
    close_state(c, state)?;
    Ok(c.undefined())
}

/// `dir.close()`: a promise; `dir.close(callback)`: `callback(error)` once closed.
pub(crate) fn close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    let entries = get(c, state, "entries")?;
    let outcome = if is_undefined(c, entries) {
        match closed_error(c).exception {
            Some(error) => Err(error),
            None => return Err(RootedError::host("ERR_DIR_CLOSED lost its exception")),
        }
    } else {
        close_state(c, state)?;
        Ok(c.undefined())
    };
    if args.is_empty() || is_undefined(c, args[0]) {
        let promise = c.pending_promise_rooted()?;
        super::api::settle_later(c, promise, outcome)?;
        return Ok(promise);
    }
    let callback = args::function(c, args[0], "callback")?;
    let value = match outcome {
        Ok(_) => c.null(),
        Err(error) => error,
    };
    super::streams::complete(c, callback, &[value])?;
    Ok(c.undefined())
}

pub(crate) fn dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    close_state(c, state)?;
    Ok(c.undefined())
}

pub(crate) fn async_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    close_state(c, state)?;
    let promise = c.pending_promise_rooted()?;
    let undefined = c.undefined();
    super::api::settle_later(c, promise, Ok(undefined))?;
    Ok(promise)
}

/// `dir.entries()` and `dir[Symbol.asyncIterator]()`: an async iterator that closes the
/// directory once it is exhausted.
pub(crate) fn iterate(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = state(c, this)?;
    let iterator = c.object_rooted()?;
    for (name, operation) in [("next", "fsDirIteratorNext"), ("return", "fsDirIteratorReturn")] {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, state)?;
        put(c, iterator, name, function)?;
    }
    let key = classes::well_known_symbol(c, "asyncIterator", "Symbol.asyncIterator")?;
    let identity = c.host_function_named(crate::host::shared_vm::operation("fsDirIteratorSelf"), "[Symbol.asyncIterator]")?;
    c.set_property_rooted(iterator, key, identity, iterator)?;
    Ok(iterator)
}

fn iterator_step(c: &mut Context<'_>, value: RootId, done: bool) -> Outcome {
    let result = crate::modules::stream_shared_vm::iterator_result(c, value, done)?;
    let promise = c.pending_promise_rooted()?;
    super::api::settle_later(c, promise, Ok(result))?;
    Ok(promise)
}

pub(crate) fn iterator_next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let state = c.host_function_data()?;
    let entries = get(c, state, "entries")?;
    if is_undefined(c, entries) {
        let undefined = c.undefined();
        return iterator_step(c, undefined, true);
    }
    let entry = next_entry(c, state)?;
    if c.rooted_value(entry).is_some_and(|value| value.is_null()) {
        close_state(c, state)?;
        let undefined = c.undefined();
        return iterator_step(c, undefined, true);
    }
    iterator_step(c, entry, false)
}

pub(crate) fn iterator_return(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let state = c.host_function_data()?;
    close_state(c, state)?;
    let value = arg(c, args, 0);
    iterator_step(c, value, true)
}

pub(crate) fn iterator_self(_: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    Ok(this)
}
