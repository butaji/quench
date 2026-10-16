//! Objects, arrays and property access.

use super::abi::{node_api, PropertyDescriptor};
use super::values::value_type;
use super::{api, c_text, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{RootId, ValueType};
use std::ffi::c_char;

/// `napi_key_collection_mode`, `napi_key_filter` bits and `napi_key_conversion`.
const KEY_INCLUDE_PROTOTYPES: i32 = 0;
const KEY_OWN_ONLY: i32 = 1;
const KEY_WRITABLE: i32 = 1;
const KEY_ENUMERABLE: i32 = 1 << 1;
const KEY_CONFIGURABLE: i32 = 1 << 2;
const KEY_SKIP_STRINGS: i32 = 1 << 3;
const KEY_SKIP_SYMBOLS: i32 = 1 << 4;
const KEY_KEEP_NUMBERS: i32 = 0;
const KEY_NUMBERS_TO_STRINGS: i32 = 1;

pub(super) fn object(c: &mut Context<'_>, value: RootId) -> Result<RootId, Status> {
    match value_type(c, value)? {
        ValueType::Object | ValueType::Function => Ok(value),
        _ => Err(Status::ObjectExpected),
    }
}

/// A startup intrinsic `holder.name`, immune to guest replacement.
pub(super) fn intrinsic(env: &Env, c: &mut Context<'_>, path: &[&str]) -> Result<RootId, Status> {
    let mut value = c.global_root().napi(env, c)?;
    for name in path {
        value = c
            .startup_data_property_rooted(value, name)
            .ok_or(Status::GenericFailure)?;
    }
    Ok(value)
}

fn named_key(c: &mut Context<'_>, name: *const c_char) -> Result<RootId, Status> {
    if name.is_null() {
        return Err(Status::InvalidArg);
    }
    Ok(c.string_rooted(&c_text(name, super::AUTO_LENGTH)?))
}

fn get(
    env: &Env,
    c: &mut Context<'_>,
    target: NapiValue,
    key: RootId,
    result: *mut NapiValue,
) -> Result<(), Status> {
    let target = env.value(target)?;
    let target = to_object(env, c, target)?;
    let value = c.get_property_rooted(target, key).napi(env, c)?;
    out(result, env.handle(value))
}

fn set(
    env: &Env,
    c: &mut Context<'_>,
    target: NapiValue,
    key: RootId,
    value: NapiValue,
) -> Result<(), Status> {
    let target = env.value(target)?;
    let target = to_object(env, c, target)?;
    let value = env.value(value)?;
    c.set_property_rooted(target, key, value, target)
        .napi(env, c)
        .map(drop)
}

fn has(
    env: &Env,
    c: &mut Context<'_>,
    target: NapiValue,
    key: RootId,
    result: *mut bool,
) -> Result<(), Status> {
    let target = env.value(target)?;
    let target = to_object(env, c, target)?;
    out(result, c.has_property_rooted(target, key).napi(env, c)?)
}

fn delete(
    env: &Env,
    c: &mut Context<'_>,
    target: NapiValue,
    key: RootId,
    result: *mut bool,
) -> Result<(), Status> {
    let target = env.value(target)?;
    let target = to_object(env, c, target)?;
    let delete = intrinsic(env, c, &["Reflect", "deleteProperty"])?;
    let undefined = c.undefined();
    let deleted = c
        .call_rooted(delete, undefined, &[target, key])
        .napi(env, c)?;
    if !result.is_null() {
        out(result, c.truthy_rooted(deleted).napi(env, c)?)?;
    }
    Ok(())
}

/// V8 `ToObject` on the receiver, as Node's property functions apply it.
fn to_object(env: &Env, c: &mut Context<'_>, value: RootId) -> Result<RootId, Status> {
    match value_type(c, value)? {
        ValueType::Object | ValueType::Function => Ok(value),
        ValueType::Undefined | ValueType::Null => Err(Status::ObjectExpected),
        _ => {
            let constructor = intrinsic(env, c, &["Object"])?;
            c.call_rooted(constructor, constructor, &[value])
                .napi(env, c)
        }
    }
}

fn descriptor_flag(
    env: &Env,
    c: &mut Context<'_>,
    descriptor: RootId,
    name: &str,
) -> Result<bool, Status> {
    let key = c.string_rooted(name);
    let value = c.get_property_rooted(descriptor, key).napi(env, c)?;
    c.truthy_rooted(value).napi(env, c)
}

/// Keys of `target` (and its prototypes for `KEY_INCLUDE_PROTOTYPES`) passing `filter`,
/// each reported once, in the order the chain reports them.
fn collect_keys(
    env: &Env,
    c: &mut Context<'_>,
    target: RootId,
    mode: i32,
    filter: i32,
    conversion: i32,
) -> Result<RootId, Status> {
    let mut seen: Vec<RootId> = Vec::new();
    let mut keys = Vec::new();
    let mut holder = target;
    loop {
        for key in c.own_keys_rooted(holder).napi(env, c)? {
            let symbol = c.is_symbol_rooted(key).napi(env, c)?;
            let mut duplicate = false;
            for &previous in &seen {
                if c.strict_equal_rooted(previous, key).napi(env, c)? {
                    duplicate = true;
                    break;
                }
            }
            if duplicate {
                continue;
            }
            seen.push(key);
            if (symbol && filter & KEY_SKIP_SYMBOLS != 0)
                || (!symbol && filter & KEY_SKIP_STRINGS != 0)
            {
                continue;
            }
            let descriptor = c.own_property_descriptor_rooted(holder, key).napi(env, c)?;
            if value_type(c, descriptor)? == ValueType::Undefined {
                continue;
            }
            // V8's ONLY_WRITABLE excludes read-only data properties; accessors pass.
            let accessor_key = c.string_rooted("get");
            let accessor = c
                .has_property_rooted(descriptor, accessor_key)
                .napi(env, c)?;
            let required = [
                (KEY_WRITABLE, "writable"),
                (KEY_ENUMERABLE, "enumerable"),
                (KEY_CONFIGURABLE, "configurable"),
            ];
            let mut accepted = true;
            for (bit, name) in required {
                if filter & bit != 0
                    && !(bit == KEY_WRITABLE && accessor)
                    && !descriptor_flag(env, c, descriptor, name)?
                {
                    accepted = false;
                    break;
                }
            }
            if !accepted {
                continue;
            }
            let key = match (
                conversion == KEY_NUMBERS_TO_STRINGS,
                c.string_text(key).ok().flatten(),
            ) {
                (false, Some(text)) => match text.parse::<u32>() {
                    Ok(index) if index != u32::MAX && index.to_string() == text => {
                        c.number(f64::from(index))
                    }
                    _ => key,
                },
                _ => key,
            };
            keys.push(key);
        }
        if mode != KEY_INCLUDE_PROTOTYPES {
            break;
        }
        holder = c.prototype_rooted(holder).napi(env, c)?;
        if value_type(c, holder)? == ValueType::Null {
            break;
        }
    }
    c.array_rooted(&keys).napi(env, c)
}

pub(super) fn define_properties(
    env: &Env,
    c: &mut Context<'_>,
    target: RootId,
    descriptors: &[PropertyDescriptor],
) -> Result<(), Status> {
    for descriptor in descriptors {
        super::functions::define_property(env, c, target, descriptor)?;
    }
    Ok(())
}

fn integrity(env: *mut Env, value: NapiValue, operation: &str) -> Status {
    js_api(env, |env, c| {
        let value = object(c, env.value(value)?)?;
        let function = intrinsic(env, c, &["Object", operation])?;
        let undefined = c.undefined();
        c.call_rooted(function, undefined, &[value])
            .napi(env, c)
            .map(drop)
    })
}

node_api! {
    fn napi_create_object(env: *mut Env, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let object = c.object_rooted().napi(env, c)?;
            out(result, env.handle(object))
        })
    }

    fn napi_create_array(env: *mut Env, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let array = c.array_rooted(&[]).napi(env, c)?;
            out(result, env.handle(array))
        })
    }

    fn napi_create_array_with_length(env: *mut Env, length: usize, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let array = c.array_rooted(&[]).napi(env, c)?;
            // V8 Array::New takes an int; negative lengths create an empty array.
            let length = (length as isize as i32).max(0);
            let key = c.string_rooted("length");
            let length = c.number(f64::from(length));
            c.set_property_rooted(array, key, length, array).napi(env, c)?;
            out(result, env.handle(array))
        })
    }

    fn napi_is_array(env: *mut Env, value: NapiValue, result: *mut bool) -> Status {
        api(env, |env, c| {
            let value = env.value(value)?;
            out(result, c.is_array_rooted(value).napi(env, c)?)
        })
    }

    fn napi_get_array_length(env: *mut Env, value: NapiValue, result: *mut u32) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            if !c.is_array_rooted(value).napi(env, c)? {
                return Err(Status::ArrayExpected);
            }
            let key = c.string_rooted("length");
            let length = c.get_property_rooted(value, key).napi(env, c)?;
            out(result, c.to_number(length).napi(env, c)? as u32)
        })
    }

    fn napi_get_prototype(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let value = to_object(env, c, value)?;
            let prototype = c.prototype_rooted(value).napi(env, c)?;
            out(result, env.handle(prototype))
        })
    }

    fn napi_get_property_names(env: *mut Env, value: NapiValue, result: *mut NapiValue) -> Status {
        napi_get_all_property_names(env, value, KEY_INCLUDE_PROTOTYPES, KEY_ENUMERABLE | KEY_SKIP_SYMBOLS, KEY_NUMBERS_TO_STRINGS, result)
    }

    fn napi_get_all_property_names(env: *mut Env, value: NapiValue, mode: i32, filter: i32, conversion: i32, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            if !matches!(mode, KEY_INCLUDE_PROTOTYPES | KEY_OWN_ONLY) || !matches!(conversion, KEY_KEEP_NUMBERS | KEY_NUMBERS_TO_STRINGS) {
                return Err(Status::InvalidArg);
            }
            let value = env.value(value)?;
            let value = to_object(env, c, value)?;
            let keys = collect_keys(env, c, value, mode, filter, conversion)?;
            out(result, env.handle(keys))
        })
    }

    fn napi_set_property(env: *mut Env, target: NapiValue, key: NapiValue, value: NapiValue) -> Status {
        js_api(env, |env, c| { let key = env.value(key)?; set(env, c, target, key, value) })
    }

    fn napi_get_property(env: *mut Env, target: NapiValue, key: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| { let key = env.value(key)?; get(env, c, target, key, result) })
    }

    fn napi_has_property(env: *mut Env, target: NapiValue, key: NapiValue, result: *mut bool) -> Status {
        js_api(env, |env, c| { let key = env.value(key)?; has(env, c, target, key, result) })
    }

    fn napi_delete_property(env: *mut Env, target: NapiValue, key: NapiValue, result: *mut bool) -> Status {
        js_api(env, |env, c| { let key = env.value(key)?; delete(env, c, target, key, result) })
    }

    fn napi_has_own_property(env: *mut Env, target: NapiValue, key: NapiValue, result: *mut bool) -> Status {
        js_api(env, |env, c| {
            let target = env.value(target)?;
            let target = to_object(env, c, target)?;
            let key = env.value(key)?;
            if !matches!(value_type(c, key)?, ValueType::String | ValueType::Symbol) {
                return Err(Status::NameExpected);
            }
            let descriptor = c.own_property_descriptor_rooted(target, key).napi(env, c)?;
            out(result, value_type(c, descriptor)? != ValueType::Undefined)
        })
    }

    fn napi_set_named_property(env: *mut Env, target: NapiValue, name: *const c_char, value: NapiValue) -> Status {
        js_api(env, |env, c| { let key = named_key(c, name)?; set(env, c, target, key, value) })
    }

    fn napi_get_named_property(env: *mut Env, target: NapiValue, name: *const c_char, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| { let key = named_key(c, name)?; get(env, c, target, key, result) })
    }

    fn napi_has_named_property(env: *mut Env, target: NapiValue, name: *const c_char, result: *mut bool) -> Status {
        js_api(env, |env, c| { let key = named_key(c, name)?; has(env, c, target, key, result) })
    }

    fn napi_set_element(env: *mut Env, target: NapiValue, index: u32, value: NapiValue) -> Status {
        js_api(env, |env, c| { let key = c.number(f64::from(index)); set(env, c, target, key, value) })
    }

    fn napi_get_element(env: *mut Env, target: NapiValue, index: u32, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| { let key = c.number(f64::from(index)); get(env, c, target, key, result) })
    }

    fn napi_has_element(env: *mut Env, target: NapiValue, index: u32, result: *mut bool) -> Status {
        js_api(env, |env, c| { let key = c.number(f64::from(index)); has(env, c, target, key, result) })
    }

    fn napi_delete_element(env: *mut Env, target: NapiValue, index: u32, result: *mut bool) -> Status {
        js_api(env, |env, c| { let key = c.number(f64::from(index)); delete(env, c, target, key, result) })
    }

    fn napi_define_properties(env: *mut Env, target: NapiValue, count: usize, properties: *const PropertyDescriptor) -> Status {
        js_api(env, |env, c| {
            if count > 0 && properties.is_null() {
                return Err(Status::InvalidArg);
            }
            let target = object(c, env.value(target)?)?;
            // SAFETY: the addon supplies `count` descriptors.
            let descriptors = if count == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(properties, count) } };
            define_properties(env, c, target, descriptors)
        })
    }

    fn napi_instanceof(env: *mut Env, value: NapiValue, constructor: NapiValue, result: *mut bool) -> Status {
        js_api(env, |env, c| {
            let value = env.value(value)?;
            let constructor = object(c, env.value(constructor)?)?;
            if value_type(c, constructor)? != ValueType::Function {
                let kind = super::errors::Kind::TypeError;
                super::errors::throw_coded(env, c, kind, Some("ERR_NAPI_CONS_FUNCTION"), "Constructor must be a function");
                return Err(Status::FunctionExpected);
            }
            out(result, c.instance_of_rooted(value, constructor).napi(env, c)?)
        })
    }

    fn napi_object_freeze(env: *mut Env, value: NapiValue) -> Status {
        integrity(env, value, "freeze")
    }

    fn napi_object_seal(env: *mut Env, value: NapiValue) -> Status {
        integrity(env, value, "seal")
    }
}
