//! Primitive values, strings and conversions.

use super::abi::node_api;
use super::{api, c_text, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{RootId, ValueType};
use std::ffi::c_char;

/// `napi_valuetype`, in header order.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) enum NapiType {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Symbol,
    Object,
    Function,
    External,
    Bigint,
}

pub(super) fn value_type(c: &mut Context<'_>, value: RootId) -> Result<ValueType, Status> {
    c.value_type_rooted(value).ok_or(Status::InvalidArg)
}

/// Read `value` as a Number without coercion.
pub(super) fn number(c: &mut Context<'_>, value: RootId) -> Result<f64, Status> {
    if value_type(c, value)? != ValueType::Number {
        return Err(Status::NumberExpected);
    }
    c.to_number(value).map_err(|_| Status::NumberExpected)
}

fn create(
    env: *mut Env,
    result: *mut NapiValue,
    make: impl FnOnce(&mut Context<'_>) -> RootId,
) -> Status {
    api(env, |env, c| {
        let value = make(c);
        out(result, env.handle(value))
    })
}

/// Copy text into an addon buffer of `capacity` units, NUL-terminated, without splitting a
/// UTF-8 sequence (V8 `WriteUtf8`); with a null buffer report the full length instead.
fn write_units<T: Copy + Default>(
    units: &[T],
    boundary: impl Fn(usize) -> bool,
    buffer: *mut T,
    capacity: usize,
    result: *mut usize,
) -> Result<(), Status> {
    if buffer.is_null() {
        return out(result, units.len());
    }
    let mut written = units.len().min(capacity.saturating_sub(1));
    while written > 0 && !boundary(written) {
        written -= 1;
    }
    if capacity > 0 {
        // SAFETY: the addon supplies `capacity` writable units.
        let target = unsafe { std::slice::from_raw_parts_mut(buffer, capacity) };
        target[..written].copy_from_slice(&units[..written]);
        target[written] = T::default();
    }
    if !result.is_null() {
        out(result, written)?;
    }
    Ok(())
}

fn string_units(c: &mut Context<'_>, value: RootId) -> Result<Vec<u16>, Status> {
    c.string_code_units(value)
        .ok()
        .flatten()
        .ok_or(Status::StringExpected)
}

node_api! {
    fn napi_get_undefined(env: *mut Env, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.undefined())
    }

    fn napi_get_null(env: *mut Env, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.null())
    }

    fn napi_get_global(env: *mut Env, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let global = c.global_root().napi(env, c)?;
            out(result, env.handle(global))
        })
    }

    fn napi_get_boolean(env: *mut Env, value: bool, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.boolean(value))
    }

    fn napi_create_int32(env: *mut Env, value: i32, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.number(f64::from(value)))
    }

    fn napi_create_uint32(env: *mut Env, value: u32, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.number(f64::from(value)))
    }

    fn napi_create_int64(env: *mut Env, value: i64, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.number(value as f64))
    }

    fn napi_create_double(env: *mut Env, value: f64, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.number(value))
    }

    fn napi_create_bigint_int64(env: *mut Env, value: i64, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.bigint_rooted(i128::from(value)))
    }

    fn napi_create_bigint_uint64(env: *mut Env, value: u64, result: *mut NapiValue) -> Status {
        create(env, result, |c| c.bigint_rooted(i128::from(value)))
    }

    fn napi_create_string_utf8(env: *mut Env, text: *const c_char, length: usize, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let text = c_text(text, length)?;
            out(result, env.handle(c.string_rooted(&text)))
        })
    }

    fn napi_create_string_latin1(env: *mut Env, text: *const c_char, length: usize, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let bytes = super::c_bytes(text, length)?;
            let units: Vec<u16> = bytes.iter().map(|&byte| u16::from(byte)).collect();
            out(result, env.handle(c.string_units_rooted(&units)))
        })
    }

    fn napi_create_string_utf16(env: *mut Env, text: *const u16, length: usize, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let units = utf16_units(text, length)?;
            out(result, env.handle(c.string_units_rooted(units)))
        })
    }

    fn node_api_create_property_key_utf8(env: *mut Env, text: *const c_char, length: usize, result: *mut NapiValue) -> Status {
        napi_create_string_utf8(env, text, length, result)
    }

    fn node_api_create_property_key_latin1(env: *mut Env, text: *const c_char, length: usize, result: *mut NapiValue) -> Status {
        napi_create_string_latin1(env, text, length, result)
    }

    fn node_api_create_property_key_utf16(env: *mut Env, text: *const u16, length: usize, result: *mut NapiValue) -> Status {
        napi_create_string_utf16(env, text, length, result)
    }

    fn napi_create_symbol(env: *mut Env, description: NapiValue, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let description = if description.is_null() {
                None
            } else {
                let value = env.value(description)?;
                Some(c.string_text(value).ok().flatten().ok_or(Status::StringExpected)?)
            };
            out(result, env.handle(c.symbol_rooted(description.as_deref())))
        })
    }

    fn node_api_symbol_for(env: *mut Env, text: *const c_char, length: usize, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let key = c.string_rooted(&c_text(text, length)?);
            let global = c.global_root().napi(env, c)?;
            let symbol = c.startup_data_property_rooted(global, "Symbol").ok_or(Status::GenericFailure)?;
            let symbol_for = c.startup_data_property_rooted(symbol, "for").ok_or(Status::GenericFailure)?;
            let value = c.call_rooted(symbol_for, symbol, &[key]).napi(env, c)?;
            out(result, env.handle(value))
        })
    }

    fn napi_typeof(env: *mut Env, value: NapiValue, result: *mut NapiType) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let kind = match value_type(c, value)? {
                ValueType::Undefined => NapiType::Undefined,
                ValueType::Null => NapiType::Null,
                ValueType::Boolean => NapiType::Boolean,
                ValueType::Number => NapiType::Number,
                ValueType::String => NapiType::String,
                ValueType::Symbol => NapiType::Symbol,
                ValueType::BigInt => NapiType::Bigint,
                ValueType::Function => NapiType::Function,
                ValueType::Object if c.external_address_rooted(value).is_some() => NapiType::External,
                ValueType::Object => NapiType::Object,
            };
            out(result, kind)
        })
    }

    fn napi_get_value_bool(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            if value_type(c, value)? != ValueType::Boolean {
                return Err(Status::BooleanExpected);
            }
            out(result, c.truthy_rooted(value).map_err(|_| Status::BooleanExpected)?)
        })
    }

    fn napi_get_value_double(env: *mut Env, value: NapiValue, result: *mut f64) -> Status {
        api(env, |env, c| out(result, number(c, env.value(value)?)?))
    }

    fn napi_get_value_int32(env: *mut Env, value: NapiValue, result: *mut i32) -> Status {
        api(env, |env, c| out(result, to_int32(number(c, env.value(value)?)?)))
    }

    fn napi_get_value_uint32(env: *mut Env, value: NapiValue, result: *mut u32) -> Status {
        api(env, |env, c| out(result, to_int32(number(c, env.value(value)?)?) as u32))
    }

    fn napi_get_value_int64(env: *mut Env, value: NapiValue, result: *mut i64) -> Status {
        // Node maps non-finite values to zero; finite values saturate like V8 IntegerValue.
        api(env, |env, c| {
            let value = number(c, env.value(value)?)?;
            out(result, if value.is_finite() { value as i64 } else { 0 })
        })
    }

    fn napi_get_value_string_utf8(env: *mut Env, value: NapiValue, buffer: *mut c_char, capacity: usize, result: *mut usize) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let units = string_units(c, value)?;
            let text = String::from_utf16_lossy(&units);
            let bytes = text.as_bytes();
            let bytes: &[c_char] = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len()) };
            write_units(bytes, |end| text.is_char_boundary(end), buffer, capacity, result)
        })
    }

    fn napi_get_value_string_latin1(env: *mut Env, value: NapiValue, buffer: *mut c_char, capacity: usize, result: *mut usize) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let bytes: Vec<c_char> = string_units(c, value)?.iter().map(|&unit| unit as u8 as c_char).collect();
            write_units(&bytes, |_| true, buffer, capacity, result)
        })
    }

    fn napi_get_value_string_utf16(env: *mut Env, value: NapiValue, buffer: *mut u16, capacity: usize, result: *mut usize) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            let units = string_units(c, value)?;
            write_units(&units, |_| true, buffer, capacity, result)
        })
    }

    fn napi_coerce_to_bool(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let truthy = c.truthy_rooted(value).napi(env, c)?;
            out(result, env.handle(c.boolean(truthy)))
        })
    }

    fn napi_coerce_to_number(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let number = c.to_number(value).napi(env, c)?;
            out(result, env.handle(c.number(number)))
        })
    }

    fn napi_coerce_to_string(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let units = c.to_string_code_units(value).napi(env, c)?;
            out(result, env.handle(c.string_units_rooted(&units)))
        })
    }

    fn napi_coerce_to_object(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let global = c.global_root().napi(env, c)?;
            let object = c.startup_data_property_rooted(global, "Object").ok_or(Status::GenericFailure)?;
            if matches!(value_type(c, value)?, ValueType::Undefined | ValueType::Null) {
                let error = c.type_error_rooted("Cannot convert undefined or null to object").napi(env, c)?;
                return Err(env.throw_value(c, error));
            }
            let boxed = c.call_rooted(object, object, &[value]).napi(env, c)?;
            out(result, env.handle(boxed))
        })
    }

    fn napi_strict_equals(env: *mut Env, left: NapiValue, right: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let (left, right) = (env.value(left)?, env.value(right)?);
            out(result, c.strict_equal_rooted(left, right).napi(env, c)?)
        })
    }
}

/// ECMAScript ToInt32 of a Number; non-finite values become zero.
fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    let wrapped = value.trunc().rem_euclid(TWO_32);
    wrapped as u32 as i32
}

fn utf16_units<'a>(text: *const u16, length: usize) -> Result<&'a [u16], Status> {
    if text.is_null() {
        return if length == 0 {
            Ok(&[])
        } else {
            Err(Status::InvalidArg)
        };
    }
    let length = if length == super::AUTO_LENGTH {
        // SAFETY: NAPI_AUTO_LENGTH promises a NUL-terminated string.
        (0..)
            .take_while(|&index| unsafe { *text.add(index) } != 0)
            .count()
    } else if length > i32::MAX as usize {
        return Err(Status::InvalidArg);
    } else {
        length
    };
    // SAFETY: the caller supplies `length` readable units.
    Ok(unsafe { std::slice::from_raw_parts(text, length) })
}
