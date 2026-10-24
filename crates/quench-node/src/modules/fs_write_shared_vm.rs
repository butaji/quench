//! Public writeSync policy and one native descriptor write.
use super::fs_policy_shared_vm as policy;
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const STDERR_FD: i32 = 2;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}
fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
/// Node's native GetOffset accepts only primitive safe integers. Other values,
/// including BigInts and fractions, select the descriptor's current position.
fn position(c: &Context<'_>, value: RootId) -> Option<u64> {
    c.rooted_value(value)
        .and_then(|value| value.as_number())
        .filter(|number| {
            number.is_finite()
                && number.fract() == 0.0
                && *number >= 0.0
                && *number <= super::util_shared_vm::MAX_SAFE_INTEGER
        })
        .map(|number| number as u64)
}
fn fd(c: &mut Context<'_>, value: RootId) -> Result<i32, RootedError> {
    let maximum = c.number(i32::MAX as f64);
    Ok(super::util_shared_vm::validate_binding_integer(c, value, "fd", 0.0, maximum)? as i32)
}
fn write(
    c: &mut Context<'_>,
    fd: i32,
    bytes: &[u8],
    position: Option<u64>,
) -> Result<RootId, RootedError> {
    // Node's worker stderr stream can be backed by SyncWriteStream, which
    // writes directly through fs.writeSync(2) instead of process.stderr.write.
    // Keep those native fd-2 writes on the worker event channel as well.
    if fd == STDERR_FD && position.is_none() && super::worker_threads_shared_vm::is_child(c) {
        let output = c.host_mut().shared_state().borrow().output.clone();
        if let Some(output) = output {
            let text = String::from_utf8_lossy(bytes);
            output(text.as_ref());
            return Ok(c.number(bytes.len() as f64));
        }
    }
    let fs = c.host_mut().shared_state().borrow().fs.clone();
    match fs.write_descriptor(fd, bytes, position) {
        Ok(count) => Ok(c.number(count as f64)),
        Err(error) => {
            let context = c.object_rooted()?;
            let (_, errno) = super::fs_error_codes::code_for(&error);
            let key = c.string_rooted("errno");
            let errno = c.number(f64::from(errno));
            c.define_data_property_rooted(context, key, errno, true, true, true)?;
            let key = c.string_rooted("syscall");
            let syscall = c.string_rooted("write");
            c.define_data_property_rooted(context, key, syscall, true, true, true)?;
            policy::call(c, "handleErrorFromBinding", &[context])
        }
    }
}
pub(crate) fn write_sync(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let descriptor = args.first().copied().unwrap_or(missing);
    let buffer = args.get(1).copied().unwrap_or(missing);
    let mut offset = args.get(2).copied().unwrap_or(missing);
    let mut length = args.get(3).copied().unwrap_or(missing);
    let mut at = args.get(4).copied().unwrap_or(missing);
    let zero = c.number(0.0);
    if c.has_builtin_brand_rooted(buffer, BuiltinBrand::ArrayBufferView)? {
        if c.rooted_value(offset).is_some_and(|value| value.is_null())
            || c.is_object_rooted(offset)?
        {
            let options = offset;
            let empty = nullish(c, options);
            offset = if empty {
                missing
            } else {
                get(c, options, "offset")?
            };
            if undefined(c, offset) {
                offset = zero;
            }
            length = if empty {
                missing
            } else {
                get(c, options, "length")?
            };
            if undefined(c, length) {
                let capacity = get(c, buffer, "byteLength")?;
                length = c.subtract_rooted(capacity, offset)?;
            }
            at = if empty {
                missing
            } else {
                get(c, options, "position")?
            };
            if undefined(c, at) {
                at = c.null();
            }
        }
        if undefined(c, at) {
            at = c.null();
        }
        if nullish(c, offset) {
            offset = zero;
        } else {
            let name = c.string_rooted("offset");
            policy::call(c, "validateInteger", &[offset, name, zero])?;
        }
        if c.rooted_value(length)
            .and_then(|value| value.as_number())
            .is_none()
        {
            let capacity = get(c, buffer, "byteLength")?;
            length = c.subtract_rooted(capacity, offset)?;
        }
        let capacity = get(c, buffer, "byteLength")?;
        policy::call(c, "validateOffsetLengthWrite", &[offset, length, capacity])?;
        if let Some(result) =
            policy::vfs_read(c, "writeSync", &[descriptor, buffer, offset, length, at])?
        {
            return Ok(result);
        }
        let descriptor = fd(c, descriptor)?;
        let offset = c.to_number(offset)? as usize;
        let length = c.to_number(length)? as usize;
        let bytes = c.view_bytes_rooted(buffer).unwrap_or_default();
        // Reflective metadata is not authority for the backing store's bounds.
        let Some(bytes) = bytes.get(offset..offset.saturating_add(length)) else {
            let error = c.range_error_rooted("Offset is outside the bounds of the buffer")?;
            return Err(c.throw(error));
        };
        let position = position(c, at);
        write(c, descriptor, bytes, position)
    } else {
        let name = c.string_rooted("buffer");
        policy::call(c, "validateStringAfterArrayBufferView", &[buffer, name])?;
        policy::call(c, "validateEncoding", &[buffer, length])?;
        if undefined(c, offset) {
            offset = c.null();
        }
        if let Some(handlers) = policy::vfs_handlers(c)? {
            let constructor = policy::export(c, "Buffer")?;
            let from = get(c, constructor, "from")?;
            let bytes = c.call_rooted(from, constructor, &[buffer, length])?;
            let byte_length = get(c, bytes, "length")?;
            if let Some(result) = policy::vfs_call(
                c,
                handlers,
                "writeSync",
                &[descriptor, bytes, zero, byte_length, offset],
            )? {
                return Ok(result);
            }
        }
        let descriptor = fd(c, descriptor)?;
        let encoding = c
            .string_text(length)?
            .and_then(|encoding| super::buffer_codec::canonical_encoding(&encoding));
        let encoding = encoding
            .map(|encoding| c.string_rooted(encoding))
            .unwrap_or(missing);
        let bytes = super::buffer_shared_vm::encode_bytes(c, buffer, Some(encoding))?;
        let position = position(c, offset);
        write(c, descriptor, &bytes, position)
    }
}
