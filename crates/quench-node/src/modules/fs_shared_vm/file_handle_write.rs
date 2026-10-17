//! Promise write entry points recognize only handles issued by this provider.
//! The private registry retains the descriptor continuation, never public .fd
//! or a replaceable .writeFile method supplied by the caller.
use super::{get, set};
use crate::host::NodeHost;
use crate::modules::fs_policy_shared_vm as policy;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn undefined(c: &Context<'_>, v: RootId) -> bool {
    c.rooted_value(v).is_some_and(|v| v.is_undefined())
}
fn nullish(c: &Context<'_>, v: RootId) -> bool {
    c.rooted_value(v)
        .is_some_and(|v| v.is_null() || v.is_undefined())
}
fn call(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let f = get(c, object, name)?;
    c.call_rooted(f, object, args)
}
fn function(
    c: &mut Context<'_>,
    data: RootId,
    operation: &str,
    name: &str,
    length: usize,
) -> Result<RootId, RootedError> {
    let f = c.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
    for (key, value) in [
        ("name", c.string_rooted(name)),
        ("length", c.number(length as f64)),
    ] {
        let key = c.string_rooted(key);
        c.define_data_property_rooted(f, key, value, false, false, true)?;
    }
    Ok(f)
}
pub(super) fn bridge(c: &mut Context<'_>) -> Result<(RootId, RootId, RootId), RootedError> {
    let data = c.null_object_rooted()?;
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let weak = get(c, global, "WeakMap")?;
        let map = c.construct_rooted(weak, weak, &[])?;
        let proto = get(c, weak, "prototype")?;
        let setter = get(c, proto, "set")?;
        let promise = get(c, global, "Promise")?;
        let resolve = get(c, promise, "resolve")?;
        let reject = get(c, promise, "reject")?;
        let proto = get(c, promise, "prototype")?;
        let then = get(c, proto, "then")?;
        let immediate = get(c, global, "setImmediate")?;
        let uint8 = get(c, global, "Uint8Array")?;
        set(c, data, "immediate", immediate)?;
        set(c, data, "Uint8Array", uint8)?;
        for (name, v) in [
            ("map", map),
            ("set", setter),
            ("promise", promise),
            ("resolve", resolve),
            ("reject", reject),
            ("then", then),
        ] {
            set(c, data, name, v)?;
        }
        Ok(())
    })?;
    let register = function(c, data, "fsFileHandleRegister", "", 1)?;
    let close = function(c, data, "fsFileHandleClosed", "", 1)?;
    Ok((data, register, close))
}
pub(super) fn install(
    c: &mut Context<'_>,
    promises: RootId,
    data: RootId,
) -> Result<(), RootedError> {
    for name in ["writeFile", "appendFile"] {
        let fallback = get(c, promises, name)?;
        set(c, data, name, fallback)?;
        let name_root = c.string_rooted(name);
        let wrapper_data = c.array_rooted(&[data, name_root])?;
        let f = function(c, wrapper_data, "fsPromiseWriteOwned", name, 3)?;
        set(c, promises, name, f)?;
    }
    Ok(())
}

fn state(c: &mut Context<'_>, data: RootId, value: RootId) -> Result<RootId, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(c.undefined());
    }
    let map = get(c, data, "map")?;
    c.weak_map_get_rooted(map, value)
}
pub(crate) fn register(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let handle = args[0];
    let fd = args[1];
    let entry = c.null_object_rooted()?;
    let continuation = get(c, handle, "writeFile")?;
    set(c, entry, "continuation", continuation)?;
    set(c, entry, "fd", fd)?;
    let original_close = get(c, handle, "close")?;
    set(c, entry, "close", original_close)?;
    let refs = c.number(0.0);
    set(c, entry, "refs", refs)?;
    let close_data = c.array_rooted(&[data, handle])?;
    let close = function(c, close_data, "fsFileHandleCloseOwned", "close", 0)?;
    set(c, handle, "close", close)?;
    let closed = c.boolean(false);
    set(c, entry, "closed", closed)?;
    let map = get(c, data, "map")?;
    let setter = get(c, data, "set")?;
    c.call_rooted(setter, map, &[handle, entry])?;
    for name in ["writeFile", "appendFile"] {
        let f = function(c, data, "fsFileHandleWriteOwned", name, 2)?;
        set(c, handle, name, f)?;
    }
    Ok(handle)
}
pub(crate) fn closed(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let entry = state(c, data, args[0])?;
    let closed = c.boolean(true);
    set(c, entry, "closed", closed)?;
    Ok(c.undefined())
}
fn defaults(c: &mut Context<'_>, append: bool) -> Result<RootId, RootedError> {
    let obj = c.object_rooted()?;
    for (name, v) in [
        ("encoding", c.string_rooted("utf8")),
        ("mode", c.number(0o666 as f64)),
        ("flag", c.string_rooted(if append { "a" } else { "w" })),
    ] {
        set(c, obj, name, v)?;
    }
    if !append {
        let flush = c.boolean(false);
        set(c, obj, "flush", flush)?;
    }
    Ok(obj)
}
fn parse_mode(c: &mut Context<'_>, options: RootId) -> Result<(), RootedError> {
    let mode = get(c, options, "mode")?;
    let name = c.string_rooted("mode");
    let default = c.number(0o666 as f64);
    policy::call(c, "parseFileMode", &[mode, name, default])?;
    Ok(())
}
fn abort(c: &mut Context<'_>, signal: RootId) -> Result<(), RootedError> {
    if !nullish(c, signal) {
        let aborted = get(c, signal, "aborted")?;
        if c.truthy_rooted(aborted)? {
            let ctor = policy::export(c, "AbortError")?;
            let options = c.object_rooted()?;
            let reason = get(c, signal, "reason")?;
            set(c, options, "cause", reason)?;
            let u = c.undefined();
            let error = c.construct_rooted(ctor, ctor, &[u, options])?;
            return Err(c.throw(error));
        }
    }
    Ok(())
}
fn options(c: &mut Context<'_>, input: RootId, append: bool) -> Result<RootId, RootedError> {
    let default = defaults(c, append)?;
    let o = policy::call(c, "getOptions", &[input, default])?;
    if !append {
        let mut flush = get(c, o, "flush")?;
        if nullish(c, flush) {
            flush = c.boolean(false);
        }
        let name = c.string_rooted("options.flush");
        policy::call(c, "validateBoolean", &[flush, name])?;
    }
    parse_mode(c, o)?;
    Ok(o)
}
fn dispatch(
    c: &mut Context<'_>,
    name: &str,
    path: RootId,
    data: RootId,
    options: RootId,
) -> Result<Option<RootId>, RootedError> {
    if let Some(h) = policy::vfs_handlers(c)? {
        let signal = get(c, options, "signal")?;
        abort(c, signal)?;
        return policy::vfs_call(c, h, name, &[path, data, options]);
    }
    Ok(None)
}
fn write(
    c: &mut Context<'_>,
    registry: RootId,
    entry: RootId,
    path: RootId,
    mut data: RootId,
    input: RootId,
    append: bool,
) -> Result<RootId, RootedError> {
    let mut o = options(c, input, append)?;
    if append {
        if let Some(result) = dispatch(c, "appendFile", path, data, o)? {
            return Ok(result);
        }
        o = policy::call(c, "copyObject", &[o])?;
        let flag = get(c, o, "flag")?;
        if !c.truthy_rooted(flag)? {
            let flag = c.string_rooted("a");
            set(c, o, "flag", flag)?;
        }
        o = options(c, o, false)?;
    }
    if let Some(result) = dispatch(c, "writeFile", path, data, o)? {
        return Ok(result);
    }
    let flag = get(c, o, "flag")?;
    c.truthy_rooted(flag)?;
    let view = policy::call(c, "isArrayBufferView", &[data])?;
    // Existing descriptor continuation owns iterable consumption and asynchronous
    // progress. Strings/views are normalized here before entering that effect path.
    let iterable = if c.truthy_rooted(view)? {
        false
    } else {
        let is_iterable = policy::export(c, "isIterable")?;
        let u = c.undefined();
        let result = c.call_rooted(is_iterable, u, &[data])?;
        c.truthy_rooted(result)? && c.string_text(data)?.is_none()
    };
    if !c.truthy_rooted(view)? && !iterable {
        let name = c.string_rooted("data");
        policy::call(c, "validateStringAfterArrayBufferView", &[data, name])?;
        let mut encoding = get(c, o, "encoding")?;
        if !c.truthy_rooted(encoding)? {
            encoding = c.string_rooted("utf8");
        }
        let buffer = policy::export(c, "Buffer")?;
        data = call(c, buffer, "from", &[data, encoding])?;
    }
    let signal = get(c, o, "signal")?;
    policy::call(c, "validateAbortSignal", &[signal])?;
    let signal = get(c, o, "signal")?;
    let encoding = get(c, o, "encoding")?;
    let result = if iterable {
        let snapshot = c.null_object_rooted()?;
        set(c, snapshot, "signal", signal)?;
        set(c, snapshot, "encoding", encoding)?;
        let continuation = get(c, entry, "continuation")?;
        c.call_rooted(continuation, path, &[data, snapshot])?
    } else {
        abort(c, signal)?;
        let array = get(c, registry, "Uint8Array")?;
        let buffer = get(c, data, "buffer")?;
        let offset = get(c, data, "byteOffset")?;
        let length = get(c, data, "byteLength")?;
        let bytes = c.construct_rooted(array, array, &[buffer, offset, length])?;
        let length = get(c, bytes, "byteLength")?;
        let operation = c.null_object_rooted()?;
        for (name, value) in [
            ("registry", registry),
            ("entry", entry),
            ("bytes", bytes),
            ("signal", signal),
            ("length", length),
        ] {
            set(c, operation, name, value)?;
        }
        let executor = c.host_function_with_data(
            crate::host::shared_vm::operation("fsFileHandleWriteSchedule"),
            operation,
        )?;
        let promise = get(c, registry, "promise")?;
        c.construct_rooted(promise, promise, &[executor])?
    };
    let refs = get(c, entry, "refs")?;
    let one = c.number(1.0);
    let refs = c.arithmetic_rooted(NativeArithmetic::Add, refs, one)?;
    set(c, entry, "refs", refs)?;
    let promise = get(c, registry, "promise")?;
    let resolve = get(c, registry, "resolve")?;
    let result = c.call_rooted(resolve, promise, &[result])?;
    let success_flag = c.boolean(false);
    let success_data = c.array_rooted(&[registry, entry, path, success_flag])?;
    let failure_flag = c.boolean(true);
    let failure_data = c.array_rooted(&[registry, entry, path, failure_flag])?;
    let success = c.host_function_with_data(
        crate::host::shared_vm::operation("fsFileHandleWriteComplete"),
        success_data,
    )?;
    let failure = c.host_function_with_data(
        crate::host::shared_vm::operation("fsFileHandleWriteComplete"),
        failure_data,
    )?;
    let then = get(c, registry, "then")?;
    c.call_rooted(then, result, &[success, failure])
}
fn settle(
    c: &mut Context<'_>,
    data: RootId,
    result: Result<RootId, RootedError>,
) -> Result<RootId, RootedError> {
    let promise = get(c, data, "promise")?;
    let value = match result {
        Ok(v) => {
            let resolve = get(c, data, "resolve")?;
            c.call_rooted(resolve, promise, &[v])?
        }
        Err(error) => {
            let Some(exception) = error.exception else {
                return Err(error);
            };
            let reject = get(c, data, "reject")?;
            return c.call_rooted(reject, promise, &[exception]);
        }
    };
    let discard = c.host_function(crate::host::shared_vm::operation("fsPromiseWriteVoid"))?;
    let then = get(c, data, "then")?;
    c.call_rooted(then, value, &[discard])
}
pub(crate) fn void(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}
pub(crate) fn public(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let wrapper = c.host_function_data()?;
    let data = get(c, wrapper, "0")?;
    let name = get(c, wrapper, "1")?;
    let name = c.to_string(name)?;
    let u = c.undefined();
    let path = args.first().copied().unwrap_or(u);
    let entry = state(c, data, path)?;
    if undefined(c, entry) {
        let fallback = get(c, data, &name)?;
        return c.call_rooted(fallback, this, args);
    }
    let result = write(
        c,
        data,
        entry,
        path,
        args.get(1).copied().unwrap_or(u),
        args.get(2).copied().unwrap_or(u),
        name == "appendFile",
    );
    settle(c, data, result)
}
pub(crate) fn method(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let u = c.undefined();
    let result = (|| {
        let entry = state(c, data, this)?;
        if undefined(c, entry) {
            let condition = c.boolean(false);
            let message = c.string_rooted("handle must be an instance of FileHandle");
            policy::call(c, "assert", &[condition, message])?;
            unreachable!("configured assert rejects an invalid receiver");
        }
        let closed = get(c, entry, "closed")?;
        if c.truthy_rooted(closed)? {
            let error = c.error_rooted("file closed")?;
            let code = c.string_rooted("EBADF");
            set(c, error, "code", code)?;
            let syscall = c.string_rooted("writeFile");
            set(c, error, "syscall", syscall)?;
            return Err(c.throw(error));
        }
        write(
            c,
            data,
            entry,
            this,
            args.first().copied().unwrap_or(u),
            args.get(1).copied().unwrap_or(u),
            false,
        )
    })();
    settle(c, data, result)
}

/// A pending owned write keeps close behind its last descriptor operation.
pub(crate) fn close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let close_data = c.host_function_data()?;
    let registry = get(c, close_data, "0")?;
    let this = get(c, close_data, "1")?;
    let entry = state(c, registry, this)?;
    if undefined(c, entry) {
        return Err(RootedError::host("FileHandle close receiver missing"));
    }
    let existing = get(c, entry, "closePromise")?;
    if !undefined(c, existing) {
        return Ok(existing);
    }
    let refs = get(c, entry, "refs")?;
    let zero = c.number(0.0);
    let result = if c.strict_equal_rooted(refs, zero)? {
        let close = get(c, entry, "close")?;
        c.call_rooted(close, this, &[])?
    } else {
        let executor = c.host_function_with_data(
            crate::host::shared_vm::operation("fsFileHandleCloseWait"),
            entry,
        )?;
        let promise = get(c, registry, "promise")?;
        c.construct_rooted(promise, promise, &[executor])?
    };
    set(c, entry, "closePromise", result)?;
    Ok(result)
}
pub(crate) fn close_wait(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let entry = c.host_function_data()?;
    set(c, entry, "closeResolve", args[0])?;
    Ok(c.undefined())
}
pub(crate) fn complete(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let entry = get(c, data, "1")?;
    let handle = get(c, data, "2")?;
    let refs = get(c, entry, "refs")?;
    let one = c.number(1.0);
    let refs = c.subtract_rooted(refs, one)?;
    set(c, entry, "refs", refs)?;
    let zero = c.number(0.0);
    if c.strict_equal_rooted(refs, zero)? {
        let resolve = get(c, entry, "closeResolve")?;
        if !undefined(c, resolve) {
            let u = c.undefined();
            set(c, entry, "closeResolve", u)?;
            let close = get(c, entry, "close")?;
            let promise = c.call_rooted(close, handle, &[])?;
            c.call_rooted(resolve, u, &[promise])?;
        }
    }
    let failed = get(c, data, "3")?;
    if c.truthy_rooted(failed)? {
        let e = args.first().copied().unwrap_or_else(|| c.undefined());
        return Err(c.throw(e));
    }
    Ok(c.undefined())
}

/// Schedule the finite native descriptor effect after the promise API's entry
/// validation. The issued descriptor remains owned by the FileHandle.
pub(crate) fn schedule(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let op = c.host_function_data()?;
    set(c, op, "resolve", args[0])?;
    set(c, op, "reject", args[1])?;
    let callback = c.host_function_with_data(
        crate::host::shared_vm::operation("fsFileHandleWriteEffect"),
        op,
    )?;
    let registry = get(c, op, "registry")?;
    let immediate = get(c, registry, "immediate")?;
    let u = c.undefined();
    c.call_rooted(immediate, u, &[callback])?;
    Ok(u)
}
pub(crate) fn effect(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let op = c.host_function_data()?;
    let result = (|| {
        let entry = get(c, op, "entry")?;
        let fd = get(c, entry, "fd")?;
        let bytes = get(c, op, "bytes")?;
        let signal = get(c, op, "signal")?;
        let mut remaining = get(c, op, "length")?;
        let mut offset = c.number(0.0);
        let zero = c.number(0.0);
        let limit = policy::export(c, "kWriteFileMaxChunkSize")?;
        while c.greater_than_rooted(remaining, zero)? {
            abort(c, signal)?;
            let closed = get(c, entry, "closed")?;
            if c.truthy_rooted(closed)? {
                let error = c.error_rooted("file closed")?;
                let code = c.string_rooted("EBADF");
                set(c, error, "code", code)?;
                let syscall = c.string_rooted("write");
                set(c, error, "syscall", syscall)?;
                return Err(c.throw(error));
            }

            let length = if c.greater_than_rooted(remaining, limit)? {
                limit
            } else {
                remaining
            };
            let position = c.null();
            let written = crate::modules::fs_write_shared_vm::write_sync(
                c,
                zero,
                &[fd, bytes, offset, length, position],
            )?;
            remaining = c.subtract_rooted(remaining, written)?;
            offset = c.arithmetic_rooted(NativeArithmetic::Add, offset, written)?;
        }
        Ok(c.undefined())
    })();
    let u = c.undefined();
    match result {
        Ok(value) => {
            let resolve = get(c, op, "resolve")?;
            c.call_rooted(resolve, u, &[value])?;
        }
        Err(error) => {
            let Some(e) = error.exception else {
                return Err(error);
            };
            let reject = get(c, op, "reject")?;
            c.call_rooted(reject, u, &[e])?;
        }
    }
    Ok(u)
}
