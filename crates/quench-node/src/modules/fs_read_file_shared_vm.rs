//! General readFileSync protocol over canonical filesystem and Buffer operations.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const UNKNOWN_SIZE_READ_CHUNK: f64 = 8192.0;
#[derive(Clone, Copy)]
enum Ownership {
    Borrowed,
    Opened,
}
#[derive(Clone, Copy)]
struct Handle {
    fd: RootId,
    ownership: Ownership,
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn method(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}
fn buffer_method(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let buffer = super::fs_policy_shared_vm::export(c, "Buffer")?;
    method(c, buffer, name, args)
}
fn checked<T>(
    c: &mut Context<'_>,
    fs: RootId,
    handle: Handle,
    result: Result<T, RootedError>,
) -> Result<T, RootedError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            close(c, fs, handle)?;
            Err(error)
        }
    }
}
fn close(c: &mut Context<'_>, fs: RootId, handle: Handle) -> Result<(), RootedError> {
    if matches!(handle.ownership, Ownership::Opened) {
        method(c, fs, "closeSync", &[handle.fd])?;
    }
    Ok(())
}
fn allocate(
    c: &mut Context<'_>,
    fs: RootId,
    handle: Handle,
    size: RootId,
) -> Result<RootId, RootedError> {
    let maximum = c.number(super::fs_shared_vm::MAX_READ_FILE_BYTES as f64);
    let result = if c.greater_than_rooted(size, maximum)? {
        let message = format!("File size ({}) is greater than 2 GiB", c.to_string(size)?);
        let error = c.range_error_rooted(&message)?;
        super::node_errors_shared_vm::decorate_formatted_range_error(
            c,
            error,
            "ERR_FS_FILE_TOO_LARGE",
        )?;
        Err(c.throw(error))
    } else {
        buffer_method(c, "allocUnsafe", &[size])
    };
    checked(c, fs, handle, result)
}
fn read(
    c: &mut Context<'_>,
    fs: RootId,
    handle: Handle,
    buffer: RootId,
    offset: RootId,
    length: RootId,
) -> Result<RootId, RootedError> {
    let result = method(c, fs, "readSync", &[handle.fd, buffer, offset, length]);
    checked(c, fs, handle, result)
}
fn add(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<RootId, RootedError> {
    c.arithmetic_rooted(NativeArithmetic::Add, left, right)
}
fn read_unknown_into(
    c: &mut Context<'_>,
    fs: RootId,
    handle: Handle,
    buffer: RootId,
    name: RootId,
) -> Result<RootId, RootedError> {
    let zero = c.number(0.0);
    let mut position = zero;
    loop {
        let capacity = get(c, buffer, "byteLength")?;
        if !c.greater_than_rooted(capacity, position)? {
            break;
        }
        let capacity = get(c, buffer, "byteLength")?;
        let remaining = c.subtract_rooted(capacity, position)?;
        let count = read(c, fs, handle, buffer, position, remaining)?;
        position = add(c, position, count)?;
        if c.strict_equal_rooted(count, zero)? {
            return Ok(position);
        }
    }
    let one = c.number(1.0);
    let extra = allocate(c, fs, handle, one)?;
    let count = read(c, fs, handle, extra, zero, one)?;
    if !c.strict_equal_rooted(count, zero)? {
        close(c, fs, handle)?;
        let capacity = get(c, buffer, "byteLength")?;
        let reason = c.string_rooted("is too small to contain the entire file");
        return Err(super::fs_policy_shared_vm::invalid_value(
            c, name, capacity, reason,
        )?);
    }
    Ok(position)
}

pub(super) fn read_general(
    c: &mut Context<'_>,
    input: RootId,
    options: RootId,
    is_user_fd: bool,
    has_user_buffer: bool,
) -> Result<RootId, RootedError> {
    let fs = crate::host::shared_vm::commonjs::fs_module(c)?;
    let handle = if is_user_fd {
        Handle {
            fd: input,
            ownership: Ownership::Borrowed,
        }
    } else {
        let flag = get(c, options, "flag")?;
        let mode = c.number(0o666 as f64);
        let fd = method(c, fs, "openSync", &[input, flag, mode])?;
        Handle {
            fd,
            ownership: Ownership::Opened,
        }
    };
    let maximum = c.number(i32::MAX as f64);
    super::util_shared_vm::validate_binding_integer(c, handle.fd, "fd", 0.0, maximum)?;
    let descriptor = c.to_number(handle.fd)? as i32;
    let state = c.host_mut().shared_state().borrow().fs.clone();
    let metadata = state.with_descriptor(descriptor, |file| file.metadata());
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) => {
            close(c, fs, handle)?;
            return super::fs_shared_vm::sync::throw_io_error(c, "fstat", None, &error);
        }
    };
    let size = if metadata.is_file() {
        metadata.len() as f64
    } else {
        0.0
    };
    let size = c.number(size);
    let zero = c.number(0.0);
    let buffer = if has_user_buffer {
        let result = super::fs_policy_shared_vm::call(c, "getReadFileBuffer", &[options, size]);
        Some(checked(c, fs, handle, result)?)
    } else if !c.strict_equal_rooted(size, zero)? {
        Some(allocate(c, fs, handle, size)?)
    } else {
        None
    };
    let mut chunks = Vec::new();
    let mut position = zero;
    if has_user_buffer && c.strict_equal_rooted(size, zero)? {
        let name =
            super::fs_policy_shared_vm::call(c, "getReadFileBufferByteLengthName", &[options])?;
        position = read_unknown_into(c, fs, handle, buffer.expect("user-buffer branch"), name)?;
    } else if let Some(buffer) = buffer {
        loop {
            let remaining = c.subtract_rooted(size, position)?;
            let count = read(c, fs, handle, buffer, position, remaining)?;
            position = add(c, position, count)?;
            if c.strict_equal_rooted(count, zero)? || !c.greater_than_rooted(size, position)? {
                break;
            }
        }
    } else {
        loop {
            let length = c.number(UNKNOWN_SIZE_READ_CHUNK);
            let buffer = allocate(c, fs, handle, length)?;
            let count = read(c, fs, handle, buffer, zero, length)?;
            if !c.strict_equal_rooted(count, zero)? {
                chunks.push(method(c, buffer, "slice", &[zero, count])?);
            }
            position = add(c, position, count)?;
            if c.strict_equal_rooted(count, zero)? {
                break;
            }
        }
    }
    close(c, fs, handle)?;
    let buffer = if has_user_buffer {
        method(
            c,
            buffer.expect("user-buffer branch"),
            "subarray",
            &[zero, position],
        )?
    } else if let Some(buffer) = buffer {
        if c.greater_than_rooted(size, position)? {
            method(c, buffer, "slice", &[zero, position])?
        } else {
            buffer
        }
    } else {
        let chunks = c.array_rooted(&chunks)?;
        buffer_method(c, "concat", &[chunks, position])?
    };
    let encoding = get(c, options, "encoding")?;
    if c.truthy_rooted(encoding)? {
        let function = get(c, buffer, "toString")?;
        let encoding = get(c, options, "encoding")?;
        c.call_rooted(function, buffer, &[encoding])
    } else {
        Ok(buffer)
    }
}
