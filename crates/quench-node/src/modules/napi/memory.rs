//! Buffers, ArrayBuffers, typed arrays, DataViews and externals.
//!
//! Native code addresses guest bytes directly: the runtime pins a buffer's backing store
//! when its address is first handed out, so the pointer stays valid while the buffer lives.
//! External buffers are copied into guest-owned storage, and their finalizer runs once the
//! JavaScript object holding the copy is collected.

use super::abi::{node_api, Finalize};
use super::errors::throw_range_error;
use super::lifecycle::{watch, Finalizer};
use super::objects::intrinsic;
use super::{api, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{BufferStorage, BuiltinBrand, RootId};
use std::ffi::c_void;

/// `napi_typedarray_type`, in header order, with each kind's brand and intrinsic name.
const TYPED_ARRAYS: [(BuiltinBrand, &str); 12] = [
    (BuiltinBrand::Int8Array, "Int8Array"),
    (BuiltinBrand::Uint8Array, "Uint8Array"),
    (BuiltinBrand::Uint8ClampedArray, "Uint8ClampedArray"),
    (BuiltinBrand::Int16Array, "Int16Array"),
    (BuiltinBrand::Uint16Array, "Uint16Array"),
    (BuiltinBrand::Int32Array, "Int32Array"),
    (BuiltinBrand::Uint32Array, "Uint32Array"),
    (BuiltinBrand::Float32Array, "Float32Array"),
    (BuiltinBrand::Float64Array, "Float64Array"),
    (BuiltinBrand::BigInt64Array, "BigInt64Array"),
    (BuiltinBrand::BigUint64Array, "BigUint64Array"),
    (BuiltinBrand::Float16Array, "Float16Array"),
];

fn storage(env: &Env, c: &mut Context<'_>, value: RootId) -> Result<Option<BufferStorage>, Status> {
    c.buffer_storage_rooted(value).napi(env, c)
}

/// The storage of `value` when it has one of `kinds`; `napi_invalid_arg` otherwise.
fn storage_of(
    env: &Env,
    c: &mut Context<'_>,
    value: NapiValue,
    accept: impl Fn(BuiltinBrand) -> bool,
) -> Result<BufferStorage, Status> {
    let value = env.value(value)?;
    storage(env, c, value)?
        .filter(|storage| accept(storage.kind))
        .ok_or(Status::InvalidArg)
}

fn is_typed_array(kind: BuiltinBrand) -> bool {
    TYPED_ARRAYS.iter().any(|(brand, _)| *brand == kind)
}

fn is_view(kind: BuiltinBrand) -> bool {
    kind == BuiltinBrand::DataView || is_typed_array(kind)
}

fn is_array_buffer(kind: BuiltinBrand) -> bool {
    kind == BuiltinBrand::ArrayBuffer
}

/// A new Buffer of `length` zeroed bytes and the address of its first byte.
fn new_buffer(env: &Env, c: &mut Context<'_>, length: usize) -> Result<(RootId, *mut u8), Status> {
    let constructor = crate::modules::buffer_shared_vm::fast_buffer(c).napi(env, c)?;
    let length = c.number(length as f64);
    let buffer = c
        .construct_rooted(constructor, constructor, &[length])
        .napi(env, c)?;
    let data = storage(env, c, buffer)?.map_or(std::ptr::null_mut(), |storage| storage.data);
    Ok((buffer, data))
}

fn new_array_buffer(
    env: &Env,
    c: &mut Context<'_>,
    length: usize,
) -> Result<(RootId, *mut u8), Status> {
    let constructor = intrinsic(env, c, &["ArrayBuffer"])?;
    let length = c.number(length as f64);
    let buffer = c
        .construct_rooted(constructor, constructor, &[length])
        .napi(env, c)?;
    let data = storage(env, c, buffer)?.map_or(std::ptr::null_mut(), |storage| storage.data);
    Ok((buffer, data))
}

/// Copy `length` native bytes to `target` (a new guest allocation).
fn copy_in(source: *const c_void, target: *mut u8, length: usize) -> Result<(), Status> {
    if length == 0 {
        return Ok(());
    }
    if source.is_null() || target.is_null() {
        return Err(Status::InvalidArg);
    }
    // SAFETY: the addon supplies `length` readable bytes; the target is a fresh allocation.
    unsafe { std::ptr::copy_nonoverlapping(source.cast::<u8>(), target, length) };
    Ok(())
}

/// Adopt external native bytes: copy them, and finalize the native allocation once the
/// guest object is collected, as Node finalizes an external backing store.
fn adopt(
    env: &Env,
    c: &mut Context<'_>,
    value: RootId,
    finalize: Finalize,
    data: *mut c_void,
    hint: *mut c_void,
) -> Result<(), Status> {
    if finalize.is_some() {
        watch(
            env,
            c,
            value,
            Finalizer {
                callback: finalize,
                data,
                hint,
            },
        )?;
    }
    Ok(())
}

fn write_optional<T>(target: *mut T, value: T) -> Result<(), Status> {
    if target.is_null() {
        Ok(())
    } else {
        out(target, value)
    }
}

fn has_brand(
    env: *mut Env,
    value: NapiValue,
    result: *mut bool,
    accept: fn(BuiltinBrand) -> bool,
) -> Status {
    api(env, |env, c| {
        let value = env.value(value)?;
        let kind = storage(env, c, value)?.map(|storage| storage.kind);
        out(result, kind.is_some_and(accept))
    })
}

node_api! {
    fn napi_create_buffer(env: *mut Env, length: usize, data: *mut *mut c_void, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let (buffer, bytes) = new_buffer(env, c, length)?;
            write_optional(data, bytes.cast())?;
            out(result, env.handle(buffer))
        })
    }

    fn napi_create_buffer_copy(env: *mut Env, length: usize, source: *const c_void, data: *mut *mut c_void, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let (buffer, bytes) = new_buffer(env, c, length)?;
            copy_in(source, bytes, length)?;
            write_optional(data, bytes.cast())?;
            out(result, env.handle(buffer))
        })
    }

    fn napi_create_external_buffer(env: *mut Env, length: usize, source: *mut c_void, finalize: Finalize, hint: *mut c_void, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let (buffer, bytes) = new_buffer(env, c, length)?;
            copy_in(source, bytes, length)?;
            adopt(env, c, buffer, finalize, source, hint)?;
            out(result, env.handle(buffer))
        })
    }

    fn node_api_create_buffer_from_arraybuffer(env: *mut Env, array_buffer: NapiValue, offset: usize, length: usize, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let source = storage_of(env, c, array_buffer, is_array_buffer)?;
            if offset.checked_add(length).is_none_or(|end| end > source.byte_length) {
                // Node returns the status of the throw itself here: napi_ok.
                throw_range_error(env, c, "ERR_OUT_OF_RANGE", "The byte offset + length is out of range");
                return Ok(());
            }
            let constructor = crate::modules::buffer_shared_vm::fast_buffer(c).napi(env, c)?;
            let (offset, length) = (c.number(offset as f64), c.number(length as f64));
            let buffer = c.construct_rooted(constructor, constructor, &[source.buffer, offset, length]).napi(env, c)?;
            out(result, env.handle(buffer))
        })
    }

    fn napi_get_buffer_info(env: *mut Env, value: NapiValue, data: *mut *mut c_void, length: *mut usize) -> Status {
        api(env, |env, c| {
            let view = storage_of(env, c, value, is_view)?;
            write_optional(data, view.data.cast())?;
            write_optional(length, view.byte_length)
        })
    }

    fn napi_is_buffer(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        has_brand(env, value, result, is_view)
    }

    fn napi_create_arraybuffer(env: *mut Env, length: usize, data: *mut *mut c_void, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let (buffer, bytes) = new_array_buffer(env, c, length)?;
            write_optional(data, bytes.cast())?;
            out(result, env.handle(buffer))
        })
    }

    fn napi_create_external_arraybuffer(env: *mut Env, source: *mut c_void, length: usize, finalize: Finalize, hint: *mut c_void, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let (buffer, bytes) = new_array_buffer(env, c, length)?;
            copy_in(source, bytes, length)?;
            adopt(env, c, buffer, finalize, source, hint)?;
            out(result, env.handle(buffer))
        })
    }

    fn napi_get_arraybuffer_info(env: *mut Env, value: NapiValue, data: *mut *mut c_void, length: *mut usize) -> Status {
        api(env, |env, c| {
            let buffer = storage_of(env, c, value, is_array_buffer)?;
            write_optional(data, buffer.data.cast())?;
            write_optional(length, buffer.byte_length)
        })
    }

    fn napi_is_arraybuffer(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        has_brand(env, value, result, is_array_buffer)
    }

    fn napi_is_typedarray(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        has_brand(env, value, result, is_typed_array)
    }

    fn napi_is_dataview(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        has_brand(env, value, result, |kind| kind == BuiltinBrand::DataView)
    }

    fn napi_create_typedarray(env: *mut Env, kind: i32, length: usize, array_buffer: NapiValue, offset: usize, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let buffer = storage_of(env, c, array_buffer, is_array_buffer)?;
            let (_, name) = *usize::try_from(kind).ok().and_then(|kind| TYPED_ARRAYS.get(kind)).ok_or(Status::InvalidArg)?;
            let constructor = intrinsic(env, c, &[name])?;
            let width_key = c.string_rooted("BYTES_PER_ELEMENT");
            let width = c.get_property_rooted(constructor, width_key).napi(env, c)?;
            let width = c.to_number(width).napi(env, c)? as usize;
            if width > 1 && offset % width != 0 {
                let message = format!("start offset of {name} should be a multiple of {width}");
                return Err(throw_range_error(env, c, "ERR_NAPI_INVALID_TYPEDARRAY_ALIGNMENT", &message));
            }
            let end = length.checked_mul(width).and_then(|bytes| bytes.checked_add(offset));
            if end.is_none_or(|end| end > buffer.byte_length) {
                return Err(throw_range_error(env, c, "ERR_NAPI_INVALID_TYPEDARRAY_LENGTH", "Invalid typed array length"));
            }
            let (offset, length) = (c.number(offset as f64), c.number(length as f64));
            let array = c.construct_rooted(constructor, constructor, &[buffer.buffer, offset, length]).napi(env, c)?;
            out(result, env.handle(array))
        })
    }

    fn napi_get_typedarray_info(env: *mut Env, value: NapiValue, kind: *mut i32, length: *mut usize, data: *mut *mut c_void, array_buffer: *mut NapiValue, offset: *mut usize) -> Status {
        api(env, |env, c| {
            let view = storage_of(env, c, value, is_typed_array)?;
            let index = TYPED_ARRAYS.iter().position(|(brand, _)| *brand == view.kind).ok_or(Status::InvalidArg)?;
            write_optional(kind, index as i32)?;
            write_optional(length, view.length)?;
            write_optional(data, view.data.cast())?;
            write_optional(array_buffer, env.handle(view.buffer))?;
            write_optional(offset, view.byte_offset)
        })
    }

    fn napi_create_dataview(env: *mut Env, length: usize, array_buffer: NapiValue, offset: usize, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let accept = |kind| matches!(kind, BuiltinBrand::ArrayBuffer | BuiltinBrand::SharedArrayBuffer);
            let buffer = storage_of(env, c, array_buffer, accept)?;
            if length.checked_add(offset).is_none_or(|end| end > buffer.byte_length) {
                let message = "byte_offset + byte_length should be less than or equal to the size in bytes of the array passed in";
                return Err(throw_range_error(env, c, "ERR_NAPI_INVALID_DATAVIEW_ARGS", message));
            }
            let constructor = intrinsic(env, c, &["DataView"])?;
            let (offset, length) = (c.number(offset as f64), c.number(length as f64));
            let view = c.construct_rooted(constructor, constructor, &[buffer.buffer, offset, length]).napi(env, c)?;
            out(result, env.handle(view))
        })
    }

    fn napi_get_dataview_info(env: *mut Env, value: NapiValue, length: *mut usize, data: *mut *mut c_void, array_buffer: *mut NapiValue, offset: *mut usize) -> Status {
        api(env, |env, c| {
            let view = storage_of(env, c, value, |kind| kind == BuiltinBrand::DataView)?;
            write_optional(length, view.byte_length)?;
            write_optional(data, view.data.cast())?;
            write_optional(array_buffer, env.handle(view.buffer))?;
            write_optional(offset, view.byte_offset)
        })
    }

    fn napi_create_external(env: *mut Env, data: *mut c_void, finalize: Finalize, hint: *mut c_void, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let external = c.external_rooted(data as usize).napi(env, c)?;
            adopt(env, c, external, finalize, data, hint)?;
            out(result, env.handle(external))
        })
    }

    fn napi_get_value_external(env: *mut Env, value: NapiValue, result: *mut *mut c_void) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let address = c.external_address_rooted(value).ok_or(Status::InvalidArg)?;
            out(result, address as *mut c_void)
        })
    }
}
