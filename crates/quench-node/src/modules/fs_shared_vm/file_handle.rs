//! `FileHandle` (`internal/fs/promises.js`): an `EventEmitter` owning one descriptor, whose
//! methods are the promise API over it.

use super::api::{self, Flavor};
use super::classes::{self, Root};
use super::ops::Op;
use crate::modules::stream_shared_vm::{arg, call_method, get, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// The descriptor of a closed handle.
const CLOSED: f64 = -1.0;

/// A member of `FileHandle.prototype`, in Node's order.
enum Member {
    /// A promise method: name, length, operation and the system call a closed handle's
    /// error names (`fsCall`'s `fn.name`).
    Promise(&'static str, usize, Op, &'static str),
    /// Another method: name and operation.
    Method(&'static str, &'static str),
    /// `get fd()`.
    Fd,
}

const PROTOTYPE: [Member; 20] = [
    Member::Method("getAsyncId", "fsHandleAsyncId"),
    Member::Fd,
    Member::Promise("appendFile", 2, Op::AppendFile, "writeFileHandle"),
    Member::Promise("chmod", 1, Op::Fchmod, "fchmod"),
    Member::Promise("chown", 2, Op::Fchown, "fchown"),
    Member::Promise("datasync", 0, Op::Fdatasync, "fdatasync"),
    Member::Promise("sync", 0, Op::Fsync, "fsync"),
    Member::Promise("read", 4, Op::Read, "read"),
    Member::Promise("readv", 2, Op::Readv, "readv"),
    Member::Promise("readFile", 1, Op::ReadFile, "readFileHandle"),
    Member::Method("readLines", "fsHandleReadLines"),
    Member::Promise("stat", 1, Op::Fstat, "fstat"),
    Member::Promise("truncate", 0, Op::Ftruncate, "ftruncate"),
    Member::Promise("utimes", 2, Op::Futimes, "futimes"),
    Member::Promise("write", 4, Op::Write, "write"),
    Member::Promise("writev", 2, Op::Writev, "writev"),
    Member::Promise("writeFile", 2, Op::WriteFile, "writeFileHandle"),
    Member::Method("createReadStream", "fsHandleCreateReadStream"),
    Member::Method("createWriteStream", "fsHandleCreateWriteStream"),
    Member::Method("[nodejs.asyncDispose]", "fsHandleDispose"),
];

pub(crate) fn define_prototype(c: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    for member in PROTOTYPE {
        match member {
            Member::Promise(name, length, op, _) => {
                let function = api::function(c, op, Flavor::Handle, name, length)?;
                let key = c.string_rooted(name);
                c.define_data_property_rooted(prototype, key, function, true, false, true)?;
            }
            Member::Method(name, operation) => {
                let key = if name.starts_with('[') {
                    classes::well_known_symbol(c, "asyncDispose", "nodejs.asyncDispose")?
                } else {
                    c.string_rooted(name)
                };
                classes::method(c, prototype, key, operation, name, None)?;
            }
            Member::Fd => {
                let getter = c.host_function_named(crate::host::shared_vm::operation!("fsHandleFd"), "get fd")?;
                classes::accessor(c, prototype, "fd", getter, None, false)?;
            }
        }
    }
    Ok(())
}

/// `new FileHandle(fd)`.
pub(crate) fn create(c: &mut Context<'_>, fd: i32) -> Outcome {
    let handle = classes::instance(c, Root::FileHandle)?;
    let global = c.global_root()?;
    let emitter = get(c, global, "__nodeEventEmitter")?;
    c.call_rooted(emitter, handle, &[])?;
    // `close = () => ...`: a class field, bound to its handle.
    let close = c.host_function_named_with_data(crate::host::shared_vm::operation!("fsHandleClose"), "close", handle)?;
    put(c, handle, "close", close)?;
    let key = classes::root(c, Root::HandleFd)?;
    let fd = c.number(f64::from(fd));
    c.set_property_rooted(handle, key, fd, handle)?;
    Ok(handle)
}

/// `FileHandle` is not constructible by scripts: Node's takes an internal handle.
pub(crate) fn construct(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    classes::require_new(c, "FileHandle")?;
    let error = c.type_error_rooted("Illegal constructor")?;
    Err(c.throw(error))
}

/// The descriptor of `value` when it is a `FileHandle`.
pub(crate) fn descriptor_of(c: &mut Context<'_>, value: RootId) -> Result<Option<i32>, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(None);
    }
    let key = classes::root(c, Root::HandleFd)?;
    let fd = c.get_property_rooted(value, key)?;
    Ok(c.rooted_value(fd).and_then(|value| value.as_number()).map(|fd| fd as i32))
}

fn handle_fd(c: &mut Context<'_>, this: RootId) -> Result<i32, RootedError> {
    match descriptor_of(c, this)? {
        Some(fd) => Ok(fd),
        None => {
            let error = c.type_error_rooted("Value of \"this\" must be of type FileHandle")?;
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn fd(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let fd = handle_fd(c, this)?;
    Ok(c.number(f64::from(fd)))
}

pub(crate) fn async_id(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    fd(c, this, &[])
}

/// A promise method of a handle: the operation over `[fd, ...args]`. A closed handle
/// rejects with `EBADF: file closed`.
pub(crate) fn call(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (op, _) = api::decode(c)?;
    let fd = handle_fd(c, this)?;
    if f64::from(fd) == CLOSED {
        let syscall = PROTOTYPE
            .iter()
            .find_map(|member| match member {
                Member::Promise(_, _, candidate, syscall) if *candidate == op => Some(*syscall),
                _ => None,
            })
            .unwrap_or_default();
        let error = c.error_rooted("file closed")?;
        let code = c.string_rooted("EBADF");
        put(c, error, "code", code)?;
        let syscall = c.string_rooted(syscall);
        put(c, error, "syscall", syscall)?;
        let promise = c.pending_promise_rooted()?;
        api::settle_later(c, promise, Err(error))?;
        return Ok(promise);
    }
    let mut forwarded = vec![c.number(f64::from(fd))];
    forwarded.extend_from_slice(args);
    api::run(c, op, Flavor::Handle, &forwarded)
}

/// `filehandle.close()`, bound to the handle its data holds.
pub(crate) fn close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let handle = c.host_function_data()?;
    close_handle(c, handle)
}

/// `filehandle[Symbol.asyncDispose]()`: `this.close()`.
pub(crate) fn dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    call_method(c, this, "close", &[])
}

/// Close the handle's descriptor once; a closing or closed handle's close resolves with
/// the same outcome.
fn close_handle(c: &mut Context<'_>, this: RootId) -> Outcome {
    let fd = handle_fd(c, this)?;
    let closing = classes::root(c, Root::HandleClosing)?;
    let pending = c.get_property_rooted(this, closing)?;
    if c.is_promise_rooted(pending)? {
        return Ok(pending);
    }
    let promise = c.pending_promise_rooted()?;
    if f64::from(fd) == CLOSED {
        let undefined = c.undefined();
        api::settle_later(c, promise, Ok(undefined))?;
        return Ok(promise);
    }
    c.set_property_rooted(this, closing, promise, this)?;
    let key = classes::root(c, Root::HandleFd)?;
    let closed = c.number(CLOSED);
    c.set_property_rooted(this, key, closed, this)?;
    let outcome = crate::modules::stream_shared_vm::attempt(super::ops::close_descriptor(c, fd))?;
    let undefined = c.undefined();
    let closer = crate::modules::stream_shared_vm::closure(c, "fsHandleClosed", "", &[this, promise])?;
    let rejected = c.boolean(outcome.is_err());
    let value = outcome.err().unwrap_or(undefined);
    super::streams::complete(c, closer, &[rejected, value])?;
    Ok(promise)
}

/// The close `close` queued: settle, forget the pending close, and emit `'close'`.
pub(crate) fn closed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [handle, promise] = crate::modules::stream_shared_vm::captures::<2>(c)?;
    let closing = classes::root(c, Root::HandleClosing)?;
    let undefined = c.undefined();
    c.set_property_rooted(handle, closing, undefined, handle)?;
    let (rejected, value) = (arg(c, args, 0), arg(c, args, 1));
    if c.truthy_rooted(rejected)? {
        c.reject_promise_rooted(promise, value)?;
    } else {
        c.resolve_promise_rooted(promise, undefined)?;
        crate::modules::stream_shared_vm::emit(c, handle, "close", &[])?;
    }
    Ok(undefined)
}

/// The stream `options` names this handle as its descriptor (`{ ...options, fd: this }`).
fn stream_options(c: &mut Context<'_>, this: RootId, options: RootId) -> Outcome {
    let copy = c.object_rooted()?;
    if c.is_object_rooted(options)? {
        for key in c.enumerable_own_keys_rooted(options, false, true)? {
            let value = c.get_property_rooted(options, key)?;
            c.set_property_rooted(copy, key, value, copy)?;
        }
    }
    put(c, copy, "fd", this)?;
    Ok(copy)
}

fn create_stream(c: &mut Context<'_>, this: RootId, args: &[RootId], name: &str) -> Outcome {
    handle_fd(c, this)?;
    let options = arg(c, args, 0);
    let options = stream_options(c, this, options)?;
    let fs = crate::host::shared_vm::commonjs::fs_module(c)?;
    let undefined = c.undefined();
    call_method(c, fs, name, &[undefined, options])
}

pub(crate) fn create_read_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    create_stream(c, this, args, "createReadStream")
}

pub(crate) fn create_write_stream(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    create_stream(c, this, args, "createWriteStream")
}

/// `filehandle.readLines(options)`: `readline.createInterface` over a read stream.
pub(crate) fn read_lines(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let input = create_read_stream(c, this, args)?;
    let options = c.object_rooted()?;
    put(c, options, "input", input)?;
    let infinity = c.number(f64::INFINITY);
    put(c, options, "crlfDelay", infinity)?;
    let readline = crate::host::shared_vm::commonjs::readline_module(c)?;
    call_method(c, readline, "createInterface", &[options])
}

/// The operations a stream over a handle uses: the host's, except that closing closes the
/// handle (`FileHandleOperations`).
pub(crate) fn stream_operations(c: &mut Context<'_>, handle: RootId) -> Outcome {
    let fs = crate::host::shared_vm::commonjs::fs_module(c)?;
    let operations = c.object_rooted()?;
    for name in ["open", "read", "write", "fsync"] {
        let operation = get(c, fs, name)?;
        put(c, operations, name, operation)?;
    }
    let close = crate::modules::stream_shared_vm::closure(c, "fsHandleStreamClose", "close", &[handle])?;
    put(c, operations, "close", close)?;
    Ok(operations)
}

/// A handle stream's `close(fd, callback)`: `handle.close()`, then `callback(error)`.
pub(crate) fn stream_close(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [handle] = crate::modules::stream_shared_vm::captures::<1>(c)?;
    let callback = arg(c, args, 1);
    let closing = close_handle(c, handle)?;
    let done = crate::modules::stream_shared_vm::closure(c, "fsHandleStreamClosed", "", &[callback])?;
    let failed = crate::modules::stream_shared_vm::closure(c, "fsHandleStreamClosed", "", &[callback])?;
    call_method(c, closing, "then", &[done, failed])?;
    Ok(c.undefined())
}

pub(crate) fn stream_closed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [callback] = crate::modules::stream_shared_vm::captures::<1>(c)?;
    let error = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])
}
