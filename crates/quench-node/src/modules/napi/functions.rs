//! Native functions, classes, calls and property definition.

use super::abi::{
    node_api, Callback, PropertyDescriptor, CONFIGURABLE, ENUMERABLE, STATIC, WRITABLE,
};
use super::values::value_type;
use super::{api, c_text, enter, js_api, out, Context, Env, NapiValue, Status, Thrown};
use quench_runtime::{RootId, RootedError, ValueType};
use std::ffi::{c_char, c_void};

/// What a native function was created with; leaked, since the function may outlive any
/// owner Quench could observe.
struct CallbackRecord {
    env: *mut Env,
    callback: unsafe extern "C" fn(*mut Env, *mut c_void) -> NapiValue,
    data: *mut c_void,
    /// A class prototype method's V8 signature: the class whose instances it accepts.
    signature: *const CallbackRecord,
    /// Whether this is a class constructor, whose constructed receivers are its instances.
    class: bool,
}

/// `napi_callback_info` for one invocation.
struct CallbackInfo {
    this: RootId,
    args: Vec<RootId>,
    new_target: Option<RootId>,
    data: *mut c_void,
}

/// The class record whose constructor created `receiver`, if any.
fn instance_class(
    c: &mut Context<'_>,
    env: &Env,
    receiver: RootId,
) -> Result<Option<usize>, RootedError> {
    let class = c.weak_map_get_rooted(env.template_instances, receiver)?;
    Ok(c.external_address_rooted(class))
}

/// The shared trampoline every native function and accessor dispatches through.
pub(crate) fn invoke(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let address = c
        .external_address_rooted(data)
        .ok_or_else(|| RootedError::host("native function record is missing"))?;
    // SAFETY: records are leaked at creation and only ever read.
    let record = unsafe { &*(address as *const CallbackRecord) };
    // SAFETY: environments are leaked at creation.
    let env = unsafe { &*record.env };
    let new_target = c.new_target_rooted();
    // V8 checks a signature on calls only; a construct call makes its own receiver.
    if !record.signature.is_null()
        && new_target.is_none()
        && instance_class(c, env, receiver)? != Some(record.signature as usize)
    {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    if record.class && new_target.is_some() {
        let class = c.external_rooted(address)?;
        c.call_rooted(env.weak_map_set, env.template_instances, &[receiver, class])?;
    }
    let info = CallbackInfo {
        this: receiver,
        args: args.to_vec(),
        new_target,
        data: record.data,
    };
    let callback = record.callback;
    // SAFETY: the addon's callback receives its environment and this invocation's info.
    enter(c, record.env, |env| unsafe {
        callback(env, (&info as *const CallbackInfo).cast_mut().cast())
    })
}

/// Create a native function. Like V8 API functions, every one is constructible; only the
/// name and a class method's signature differ.
fn native_function(
    env: &Env,
    c: &mut Context<'_>,
    name: &str,
    callback: Callback,
    data: *mut c_void,
    signature: *const CallbackRecord,
    class: bool,
) -> Result<(RootId, *const CallbackRecord), Status> {
    let callback = callback.ok_or(Status::InvalidArg)?;
    let env_pointer = (env as *const Env).cast_mut();
    let record = Box::into_raw(Box::new(CallbackRecord {
        env: env_pointer,
        callback,
        data,
        signature,
        class,
    }));
    let external = c.external_rooted(record as usize).napi(env, c)?;
    let operation = crate::host::shared_vm::operation("napiCallback");
    let function = c
        .host_constructor_named_with_data(operation, name, external)
        .napi(env, c)?;
    Ok((function, record))
}

pub(super) fn function(
    env: &Env,
    c: &mut Context<'_>,
    name: &str,
    callback: Callback,
    data: *mut c_void,
) -> Result<RootId, Status> {
    native_function(env, c, name, callback, data, std::ptr::null(), false)
        .map(|(function, _)| function)
}

fn property_key(
    env: &Env,
    c: &mut Context<'_>,
    descriptor: &PropertyDescriptor,
) -> Result<RootId, Status> {
    if !descriptor.utf8name.is_null() {
        return Ok(c.string_rooted(&c_text(descriptor.utf8name, super::AUTO_LENGTH)?));
    }
    let name = env
        .value(descriptor.name)
        .map_err(|_| Status::NameExpected)?;
    match value_type(c, name)? {
        ValueType::String | ValueType::Symbol => Ok(name),
        _ => Err(Status::NameExpected),
    }
}

/// Define one `napi_property_descriptor` on `target` as Node's `napi_define_properties`
/// does, or as a class template member when `class` names the class (methods then carry
/// its signature and their property name).
pub(super) fn define_property(
    env: &Env,
    c: &mut Context<'_>,
    target: RootId,
    property: &PropertyDescriptor,
) -> Result<(), Status> {
    define_member(env, c, target, property, None)
}

fn define_member(
    env: &Env,
    c: &mut Context<'_>,
    target: RootId,
    property: &PropertyDescriptor,
    class: Option<*const CallbackRecord>,
) -> Result<(), Status> {
    let key = property_key(env, c, property)?;
    let descriptor = c.null_object_rooted().napi(env, c)?;
    let put = |c: &mut Context<'_>, field: &str, value: RootId| {
        let field = c.string_rooted(field);
        c.define_data_property_rooted(descriptor, field, value, true, true, true)
            .napi(env, c)
            .map(drop)
    };
    let flag = |c: &mut Context<'_>, bit: i32| c.boolean(property.attributes & bit != 0);
    if property.getter.is_some() || property.setter.is_some() {
        for (field, callback) in [("get", property.getter), ("set", property.setter)] {
            if callback.is_some() {
                let accessor = function(env, c, "", callback, property.data)?;
                put(c, field, accessor)?;
            }
        }
    } else {
        let value = if property.method.is_some() {
            let (name, signature) = match class {
                Some(class) => (c.string_text(key).ok().flatten().unwrap_or_default(), class),
                None => (String::new(), std::ptr::null()),
            };
            native_function(
                env,
                c,
                &name,
                property.method,
                property.data,
                signature,
                false,
            )?
            .0
        } else {
            env.value(property.value)?
        };
        put(c, "value", value)?;
        let writable = flag(c, WRITABLE);
        put(c, "writable", writable)?;
    }
    let enumerable = flag(c, ENUMERABLE);
    put(c, "enumerable", enumerable)?;
    let configurable = flag(c, CONFIGURABLE);
    put(c, "configurable", configurable)?;
    c.define_property_descriptor_rooted(target, key, descriptor)
        .napi(env, c)
        .map(drop)
}

fn info<'a>(info: *mut c_void) -> Result<&'a CallbackInfo, Status> {
    // SAFETY: a callback info pointer is valid for the duration of its callback.
    unsafe { info.cast::<CallbackInfo>().as_ref() }.ok_or(Status::InvalidArg)
}

fn arguments(env: &Env, argc: usize, argv: *const NapiValue) -> Result<Vec<RootId>, Status> {
    if argc > 0 && argv.is_null() {
        return Err(Status::InvalidArg);
    }
    (0..argc)
        // SAFETY: the addon supplies `argc` handles.
        .map(|index| env.value(unsafe { *argv.add(index) }))
        .collect()
}

pub(super) fn callable(c: &mut Context<'_>, value: RootId) -> Result<RootId, Status> {
    match value_type(c, value)? {
        ValueType::Function => Ok(value),
        _ => Err(Status::FunctionExpected),
    }
}

pub(super) fn call(
    env: &Env,
    c: &mut Context<'_>,
    receiver: NapiValue,
    function: NapiValue,
    argc: usize,
    argv: *const NapiValue,
    result: *mut NapiValue,
) -> Result<(), Status> {
    let receiver = env.value(receiver)?;
    let function = callable(c, env.value(function)?)?;
    let args = arguments(env, argc, argv)?;
    let value = c.call_rooted(function, receiver, &args).napi(env, c)?;
    if !result.is_null() {
        out(result, env.handle(value))?;
    }
    Ok(())
}

node_api! {
    fn napi_create_function(env: *mut Env, name: *const c_char, length: usize, callback: Callback, data: *mut c_void, result: *mut NapiValue) -> Status {
        api(env, |env, c| {
            let name = c_text(name, if name.is_null() { 0 } else { length })?;
            let function = function(env, c, &name, callback, data)?;
            out(result, env.handle(function))
        })
    }

    fn napi_get_cb_info(env: *mut Env, cbinfo: *mut c_void, argc: *mut usize, argv: *mut NapiValue, this: *mut NapiValue, data: *mut *mut c_void) -> Status {
        api(env, |env, c| {
            let info = info(cbinfo)?;
            if !argv.is_null() {
                // SAFETY: with a non-null argv, argc names its capacity.
                let capacity = unsafe { argc.as_ref() }.copied().ok_or(Status::InvalidArg)?;
                for index in 0..capacity {
                    let value = info.args.get(index).copied().unwrap_or_else(|| c.undefined());
                    // SAFETY: the addon supplies `capacity` writable slots.
                    unsafe { *argv.add(index) = env.handle(value) };
                }
            }
            if !argc.is_null() {
                out(argc, info.args.len())?;
            }
            if !this.is_null() {
                out(this, env.handle(info.this))?;
            }
            if !data.is_null() {
                out(data, info.data)?;
            }
            Ok(())
        })
    }

    fn napi_get_new_target(env: *mut Env, cbinfo: *mut c_void, result: *mut NapiValue) -> Status {
        api(env, |env, _| {
            let target = info(cbinfo)?.new_target;
            out(result, target.map_or(super::NULL_VALUE, |target| env.handle(target)))
        })
    }

    fn napi_call_function(env: *mut Env, receiver: NapiValue, function: NapiValue, argc: usize, argv: *const NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| call(env, c, receiver, function, argc, argv, result))
    }

    fn napi_new_instance(env: *mut Env, constructor: NapiValue, argc: usize, argv: *const NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let constructor = callable(c, env.value(constructor)?)?;
            let args = arguments(env, argc, argv)?;
            let instance = c.construct_rooted(constructor, constructor, &args).napi(env, c)?;
            out(result, env.handle(instance))
        })
    }

    fn napi_define_class(env: *mut Env, name: *const c_char, length: usize, constructor: Callback, data: *mut c_void, count: usize, properties: *const PropertyDescriptor, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            if count > 0 && properties.is_null() {
                return Err(Status::InvalidArg);
            }
            let name = c_text(name, length)?;
            let (class, record) = native_function(env, c, &name, constructor, data, std::ptr::null(), true)?;
            let key = c.string_rooted("prototype");
            let prototype = c.get_property_rooted(class, key).napi(env, c)?;
            // SAFETY: the addon supplies `count` descriptors.
            let properties = if count == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(properties, count) } };
            let (statics, members): (Vec<_>, Vec<_>) = properties.iter().partition(|property| property.attributes & STATIC != 0);
            // A V8 template instantiates its prototype members before `constructor`.
            let constructor_key = c.string_rooted("constructor");
            let delete = super::objects::intrinsic(env, c, &["Reflect", "deleteProperty"])?;
            c.call_rooted(delete, delete, &[prototype, constructor_key]).napi(env, c)?;
            for property in members {
                define_member(env, c, prototype, property, Some(record))?;
            }
            c.define_data_property_rooted(prototype, constructor_key, class, true, false, true).napi(env, c)?;
            for property in statics {
                define_property(env, c, class, property)?;
            }
            out(result, env.handle(class))
        })
    }

    fn napi_run_script(env: *mut Env, script: NapiValue, result: *mut NapiValue) -> Status {
        js_api(env, |env, c| {
            let script = env.value(script)?;
            let source = c.string_text(script).ok().flatten().ok_or(Status::StringExpected)?;
            let value = c.evaluate_script_rooted(&source, "evalmachine.<anonymous>").napi(env, c)?;
            out(result, env.handle(value))
        })
    }
}
