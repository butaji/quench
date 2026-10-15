//! EventEmitterAsyncResource composes the canonical emitter and async resource.

use super::event_constructor_shared_vm::{get, named_set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const CLASS: &str = quench_js_check::checked_js!(
    r#"(Emitter, Resource, prepare, initialize, emitNative, destroyNative, getNative, link, owner) => {
  class EventEmitterReferencingAsyncResource extends Resource {
    constructor(emitter, type, options) { super(type, options); link(this, emitter); }
    get eventEmitter() { return owner(this); }
  }
  return class EventEmitterAsyncResource extends Emitter {
    constructor(options = undefined) {
      const setup = prepare(options, new.target);
      super(setup.options);
      initialize(this, EventEmitterReferencingAsyncResource, setup);
    }
    emit(event, ...args) { return emitNative(this, event, args); }
    emitDestroy() { return destroyNative(this); }
    get asyncId() { return getNative(this, 'asyncId'); }
    get triggerAsyncId() { return getNative(this, 'triggerAsyncId'); }
    get asyncResource() { return getNative(this, 'asyncResource'); }
  };
}"#
);

pub(super) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    emitter: RootId,
) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    let api = state
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("emitter async configuration missing"))?;
    let installed = get(context, api, "asyncGetterInstalled")?;
    if context.truthy_rooted(installed)? {
        return Ok(());
    }
    let native = context.host_function_with_data(
        crate::host::shared_vm::operation("emitterAsyncConstructor"),
        api,
    )?;
    let factory = context.evaluate_script_rooted(
        "(native) => function lazyEventEmitterAsyncResource() { return native(); }",
        "node:events/async-resource-getter.js",
    )?;
    let undefined = context.undefined();
    let getter = context.call_rooted(factory, undefined, &[native])?;
    let descriptor = context.null_object_rooted()?;
    named_set(context, api, descriptor, "get", getter)?;
    let yes = context.boolean(true);
    named_set(context, api, descriptor, "enumerable", yes)?;
    named_set(context, api, descriptor, "configurable", yes)?;
    let key = context.string_rooted("EventEmitterAsyncResource");
    context.define_property_descriptor_rooted(emitter, key, descriptor)?;
    named_set(context, api, api, "asyncGetterInstalled", yes)
}

pub(crate) fn constructor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let api = context.host_function_data()?;
    let cached = get(context, api, "asyncEmitterClass")?;
    if context.truthy_rooted(cached)? {
        return Ok(cached);
    }
    let primordials = crate::host::shared_vm::commonjs::node_primordials(context)?;
    let global = context.global_root()?;
    let weak = if let Some(primordials) = primordials {
        get(context, primordials, "SafeWeakMap")?
    } else {
        get(context, global, "WeakMap")?
    };
    let prototype = get(context, weak, "prototype")?;
    let (unshift, uncurried) = if let Some(primordials) = primordials {
        (get(context, primordials, "ArrayPrototypeUnshift")?, true)
    } else {
        let array = get(context, global, "Array")?;
        let prototype = get(context, array, "prototype")?;
        (get(context, prototype, "unshift")?, false)
    };
    named_set(context, api, api, "asyncUnshift", unshift)?;
    let flag = context.boolean(uncurried);
    named_set(context, api, api, "asyncUnshiftUncurried", flag)?;
    for name in ["get", "set", "has"] {
        let method = get(context, prototype, name)?;
        named_set(context, api, api, &format!("asyncWeak{name}"), method)?;
    }
    for name in ["asyncEmitterResources", "asyncResourceEmitters"] {
        let map = context.construct_rooted(weak, weak, &[])?;
        named_set(context, api, api, name, map)?;
    }
    let async_module = super::async_hooks_shared_vm::module(context)?;
    let resource = get(context, async_module, "AsyncResource")?;
    let emitter = get(context, api, "emitterCtor")?;
    let operations = [
        "emitterAsyncPrepare",
        "emitterAsyncInit",
        "emitterAsyncEmit",
        "emitterAsyncDestroy",
        "emitterAsyncGet",
        "emitterAsyncLink",
        "emitterAsyncOwner",
    ]
    .map(|name| context.host_function_with_data(crate::host::shared_vm::operation(name), api))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let factory = context.evaluate_script_rooted(CLASS, "node:events/async-resource-class.js")?;
    let undefined = context.undefined();
    let mut arguments = vec![emitter, resource];
    arguments.extend(operations);
    let class = context.call_rooted(factory, undefined, &arguments)?;
    named_set(context, api, api, "asyncEmitterClass", class)?;
    let prototype = get(context, class, "prototype")?;
    named_set(context, api, api, "asyncEmitterHome", prototype)?;
    Ok(class)
}

fn brand_resource(
    context: &mut NativeContext<'_, NodeHost>,
    api: RootId,
    target: RootId,
    table: &str,
    field: &str,
) -> Result<RootId, RootedError> {
    let map = get(context, api, table)?;
    let has = get(context, api, "asyncWeakhas")?;
    let present = context.call_rooted(has, map, &[target])?;
    if !context.truthy_rooted(present)? {
        let error = context.type_error_rooted(&format!(
            "Cannot read private member #{field} from an object whose class did not declare it"
        ))?;
        return Err(context.throw(error));
    }
    let getter = get(context, api, "asyncWeakget")?;
    context.call_rooted(getter, map, &[target])
}

pub(crate) fn prepare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let options = args.first().copied().unwrap_or(undefined);
    let target = args.get(1).copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let (options, name) = if context.string_text(options)?.is_some() {
        (undefined, options)
    } else {
        let nullish = context
            .rooted_value(options)
            .is_some_and(|v| v.is_null() || v.is_undefined());
        let class = get(context, api, "asyncEmitterClass")?;
        if context.strict_equal_rooted(target, class)? {
            let name = if nullish {
                undefined
            } else {
                get(context, options, "name")?
            };
            if context.string_text(name)?.is_none() {
                return Err(super::util_shared_vm::argument_type_error(
                    context,
                    name,
                    "options.name",
                    "string",
                )?);
            }
        }
        let name = if nullish {
            undefined
        } else {
            get(context, options, "name")?
        };
        let name = if context.truthy_rooted(name)? {
            name
        } else {
            get(context, target, "name")?
        };
        (options, name)
    };
    let setup = context.null_object_rooted()?;
    named_set(context, api, setup, "options", options)?;
    named_set(context, api, setup, "name", name)?;
    Ok(setup)
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let constructor = args.get(1).copied().unwrap_or(undefined);
    let setup = args.get(2).copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let name = get(context, setup, "name")?;
    let options = get(context, setup, "options")?;
    let resource = context.construct_rooted(constructor, constructor, &[emitter, name, options])?;
    let map = get(context, api, "asyncEmitterResources")?;
    let setter = get(context, api, "asyncWeakset")?;
    context.call_rooted(setter, map, &[emitter, resource])?;
    Ok(undefined)
}

pub(crate) fn link(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let resource = args.first().copied().unwrap_or(undefined);
    let emitter = args.get(1).copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let map = get(context, api, "asyncResourceEmitters")?;
    let setter = get(context, api, "asyncWeakset")?;
    context.call_rooted(setter, map, &[resource, emitter])?;
    Ok(undefined)
}
pub(crate) fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let resource = args.first().copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    brand_resource(
        context,
        api,
        resource,
        "asyncResourceEmitters",
        "eventEmitter",
    )
}

pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let event = args.get(1).copied().unwrap_or(undefined);
    let values = args.get(2).copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let resource = brand_resource(
        context,
        api,
        emitter,
        "asyncEmitterResources",
        "asyncResource",
    )?;
    let home = get(context, api, "asyncEmitterHome")?;
    let parent = context.prototype_rooted(home)?;
    let super_emit = get(context, parent, "emit")?;
    let unshift = get(context, api, "asyncUnshift")?;
    let uncurried = get(context, api, "asyncUnshiftUncurried")?;
    let mut prefix = vec![super_emit, emitter, event];
    if context.truthy_rooted(uncurried)? {
        prefix.insert(0, values);
        context.call_rooted(unshift, undefined, &prefix)?;
    } else {
        context.call_rooted(unshift, values, &prefix)?;
    }
    let arguments = super::events_shared_vm::listener_records(context, values)?;
    let method = get(context, resource, "runInAsyncScope")?;
    context.call_rooted(method, resource, &arguments)
}
pub(crate) fn destroy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let resource = brand_resource(
        context,
        api,
        emitter,
        "asyncEmitterResources",
        "asyncResource",
    )?;
    let method = get(context, resource, "emitDestroy")?;
    context.call_rooted(method, resource, &[])?;
    Ok(undefined)
}
pub(crate) fn get_value(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let field = args.get(1).copied().unwrap_or(undefined);
    let api = context.host_function_data()?;
    let resource = brand_resource(
        context,
        api,
        emitter,
        "asyncEmitterResources",
        "asyncResource",
    )?;
    if context.string_text(field)?.as_deref() == Some("asyncResource") {
        return Ok(resource);
    }
    let method = context.get_property_rooted(resource, field)?;
    context.call_rooted(method, resource, &[])
}
