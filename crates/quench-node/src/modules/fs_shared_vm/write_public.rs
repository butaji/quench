//! Public write policy is the configured SDK fs.js protocol; OS effects stay
//! in native continuations. Do not route raw data through the legacy byte shim.
use super::get;
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
use std::io::Write;

type Context<'a> = NativeContext<'a, NodeHost>;
const DEFAULT_WRITE_MODE: u32 = 0o666;
fn policy(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    crate::modules::fs_policy_shared_vm::call(c, name, args)
}
fn truthy(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.truthy_rooted(value)
}
fn defaults(c: &mut Context<'_>, append: bool) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    let encoding = c.string_rooted("utf8");
    let mode = c.number(DEFAULT_WRITE_MODE as f64);
    let flag = c.string_rooted(if append { "a" } else { "w" });
    for (name, value) in [("encoding", encoding), ("mode", mode), ("flag", flag)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    if !append {
        let value = c.boolean(false);
        let key = c.string_rooted("flush");
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}
fn parse_mode(c: &mut Context<'_>, options: RootId) -> Result<RootId, RootedError> {
    let mode = get(c, options, "mode")?;
    let name = c.string_rooted("mode");
    let default = c.number(DEFAULT_WRITE_MODE as f64);
    policy(c, "parseFileMode", &[mode, name, default])
}
fn is_fd(c: &mut Context<'_>, path: RootId) -> Result<bool, RootedError> {
    let value = policy(c, "isInt32", &[path])?;
    truthy(c, value)
}
fn fs_call(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let fs = crate::host::shared_vm::commonjs::fs_module(c)?;
    let function = get(c, fs, name)?;
    c.call_rooted(function, fs, args)
}

/// Native path syscalls would otherwise truncate paths at an embedded NUL.
/// Node validates this before dispatch, and `appendFile`/`writeFile` must
/// synchronously reject the same input even when their callback is async.
fn reject_nul_path(c: &mut Context<'_>, path: RootId) -> Result<(), RootedError> {
    let Some(path_text) = c.string_text(path)? else {
        return Ok(());
    };
    if !path_text.contains('\0') {
        return Ok(());
    }
    let received = path_text.replace('\0', "\\x00");
    let error = c.type_error_rooted(&format!(
        "The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '{received}'"
    ))?;
    let code = c.string_rooted("ERR_INVALID_ARG_VALUE");
    let key = c.string_rooted("code");
    c.define_data_property_rooted(error, key, code, true, true, true)?;
    Err(c.throw(error))
}

pub(crate) fn append(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let path = args.first().copied().unwrap_or(undefined);
    let data = args.get(1).copied().unwrap_or(undefined);
    let options = args.get(2).copied().unwrap_or(undefined);
    reject_nul_path(c, path)?;
    let default = defaults(c, true)?;
    let options = policy(c, "getOptions", &[options, default])?;
    parse_mode(c, options)?;
    if crate::modules::fs_policy_shared_vm::vfs_read(c, "appendFileSync", &[path, data, options])?
        .is_some()
    {
        return Ok(undefined);
    }
    // Original SDK copyObject includes inherited enumerable fields and their
    // getters. It is the sole authority; no host-side key approximation.
    let options = policy(c, "copyObject", &[options])?;
    let flag = get(c, options, "flag")?;
    if !truthy(c, flag)? || is_fd(c, path)? {
        let flag = c.string_rooted("a");
        super::set(c, options, "flag", flag)?;
    }
    fs_call(c, "writeFileSync", &[path, data, options])?;
    Ok(undefined)
}

pub(crate) fn write(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let path = args.first().copied().unwrap_or(undefined);
    let data = args.get(1).copied().unwrap_or(undefined);
    let options = args.get(2).copied().unwrap_or(undefined);
    reject_nul_path(c, path)?;
    let default = defaults(c, false)?;
    let options = policy(c, "getOptions", &[options, default])?;
    let mut flush = get(c, options, "flush")?;
    if c.rooted_value(flush)
        .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        flush = c.boolean(false);
    }
    let flush_name = c.string_rooted("options.flush");
    policy(c, "validateBoolean", &[flush, flush_name])?;
    parse_mode(c, options)?;
    if crate::modules::fs_policy_shared_vm::vfs_read(c, "writeFileSync", &[path, data, options])?
        .is_some()
    {
        return Ok(undefined);
    }
    let mut flag = get(c, options, "flag")?;
    if !truthy(c, flag)? {
        flag = c.string_rooted("w");
    }
    // Preserve the SDK short circuit: encoding is fetched again only when the
    // first comparison is false. No eager snapshotting of option getters.
    let string = c.string_text(data)?;
    let fast = if string.is_some() {
        let first = get(c, options, "encoding")?;
        if c.string_text(first)?.as_deref() == Some("utf8") {
            true
        } else {
            let second = get(c, options, "encoding")?;
            c.string_text(second)?.as_deref() == Some("utf-8")
        }
    } else {
        false
    };
    if fast {
        let target = if is_fd(c, path)? {
            let zero = c.number(0.0);
            c.bitwise_or_rooted(path, zero)?
        } else {
            crate::modules::fs_path::validate(c, path, "path")?
        };
        let flags = policy(c, "stringToFlags", &[flag])?;
        let mode = parse_mode(c, options)?;
        let binding = crate::modules::fs_directory_shared_vm::binding(c)?;
        let function = get(c, binding, "writeFileUtf8")?;
        return c.call_rooted(function, binding, &[target, data, flags, mode]);
    }
    let is_view = policy(c, "isArrayBufferView", &[data])?;
    let data = if truthy(c, is_view)? {
        data
    } else {
        let name = c.string_rooted("data");
        policy(c, "validateStringAfterArrayBufferView", &[data, name])?;
        let buffer = crate::modules::fs_policy_shared_vm::export(c, "Buffer")?;
        let from = get(c, buffer, "from")?;
        let mut encoding = get(c, options, "encoding")?;
        if !truthy(c, encoding)? {
            encoding = c.string_rooted("utf8");
        }
        c.call_rooted(from, buffer, &[data, encoding])?
    };
    let user_fd = is_fd(c, path)?;
    let fd = if user_fd {
        path
    } else {
        let mode = get(c, options, "mode")?;
        fs_call(c, "openSync", &[path, flag, mode])?
    };
    let mut offset = c.number(0.0);
    let mut length = get(c, data, "byteLength")?;
    let outcome = (|| {
        let zero = c.number(0.0);
        while c.greater_than_rooted(length, zero)? {
            let written = fs_call(c, "writeSync", &[fd, data, offset, length])?;
            offset = c.arithmetic_rooted(NativeArithmetic::Add, offset, written)?;
            length = c.subtract_rooted(length, written)?;
        }
        if truthy(c, flush)? {
            fs_call(c, "fsyncSync", &[fd])?;
        }
        Ok::<(), RootedError>(())
    })();
    if !user_fd {
        fs_call(c, "closeSync", &[fd])?;
    }
    outcome?;
    Ok(undefined)
}

/// SDK's writeFileUtf8 native continuation. All guest policy and conversion
/// has completed. The fast path does not dispatch through Buffer.from or fs
/// public methods and owns only descriptors it opened itself.
fn write_utf8(
    c: &mut Context<'_>,
    target: RootId,
    string: String,
    flags: RootId,
    mode: RootId,
) -> Result<RootId, RootedError> {
    let bytes = string.into_bytes();
    let fd = c
        .rooted_value(target)
        .and_then(|v| v.as_number())
        .map(|v| v as i32);
    if let Some(fd) = fd {
        let shared = c.host_mut().shared_state();
        let result = shared.borrow().fs.write_all_descriptor(fd, &bytes, None);
        if let Err(error) = result {
            return super::sync::throw_io_error(c, "write", None, &error);
        }
    } else {
        let path = crate::modules::fs_path::resolve(c, target)?;
        let display = crate::modules::fs_path::display(c, target)?;
        let flags = c.to_number(flags)? as i32;
        let mode = c.to_number(mode)? as u32;
        let mut file = match crate::modules::fs_ops::open_numeric(&path, flags, Some(mode)) {
            Ok(file) => file,
            Err(error) => return super::sync::throw_io_error(c, "open", Some(&display), &error),
        };
        if let Err(error) = file.write_all(&bytes) {
            return super::sync::throw_io_error(c, "write", Some(&display), &error);
        }
    }
    Ok(c.undefined())
}

pub(crate) fn write_utf8_binding(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let value = args.get(1).copied().unwrap_or(undefined);
    let flags = args.get(2).copied().unwrap_or(undefined);
    let mode = args.get(3).copied().unwrap_or(undefined);
    let string = c
        .string_text(value)?
        .ok_or_else(|| RootedError::host("internal fs.writeFileUtf8 requires string data"))?;
    write_utf8(c, target, string, flags, mode)
}
