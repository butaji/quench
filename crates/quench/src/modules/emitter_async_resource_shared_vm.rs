//! `events.EventEmitterAsyncResource`: an EventEmitter whose emits run in the scope of its own
//! `AsyncResource`, an `EventEmitterReferencingAsyncResource` that names it back.
use super::event_target_shared_vm::{api, argument, attach_slot, get, put, receiver_slot, require_new};
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The event group's members naming the private slots: an emitter's resource, a resource's
/// emitter.
pub(super) const RESOURCE_SLOT: &str = "asyncEmitterResourceSlot";
pub(super) const EMITTER_SLOT: &str = "asyncResourceEmitterSlot";
const CLASS: &str = "EventEmitterAsyncResource";
const REFERENCING_CLASS: &str = "EventEmitterReferencingAsyncResource";

/// `(name, getter operation)` of the class's accessors.
const ACCESSORS: [(&str, &str); 3] = [
    ("asyncId", "emitterAsyncId"),
    ("triggerAsyncId", "emitterAsyncTriggerId"),
    ("asyncResource", "emitterAsyncResource"),
];

/// `EventEmitter.EventEmitterAsyncResource`, a getter that builds the class on first use.
pub(super) fn install(c: &mut Context<'_>, emitter: RootId) -> Result<(), RootedError> {
    let getter = c.host_function_named(crate::host::shared_vm::operation!("emitterAsyncConstructor"), "get EventEmitterAsyncResource")?;
    let key = c.string_rooted(CLASS);
    let undefined = c.undefined();
    c.define_property_fields_rooted(
        emitter,
        key,
        DescriptorFields { get: Some(getter), set: Some(undefined), enumerable: Some(true), configurable: Some(true), ..Default::default() },
    )?;
    Ok(())
}

fn define_accessor(c: &mut Context<'_>, holder: RootId, name: &str, operation: &str) -> Result<(), RootedError> {
    let getter = c.host_function_named(crate::host::shared_vm::operation(operation), &format!("get {name}"))?;
    let key = c.string_rooted(name);
    let undefined = c.undefined();
    c.define_property_fields_rooted(
        holder,
        key,
        DescriptorFields { get: Some(getter), set: Some(undefined), enumerable: Some(false), configurable: Some(true), ..Default::default() },
    )?;
    Ok(())
}

fn define_method(c: &mut Context<'_>, holder: RootId, name: &str, operation: &str) -> Result<(), RootedError> {
    let method = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
    c.define_named_data_property_rooted(holder, name, method, true, false, true)?;
    Ok(())
}

/// A subclass constructor of `base` with a read-only `prototype`.
fn subclass(c: &mut Context<'_>, name: &str, operation: &str, base: RootId) -> Result<(RootId, RootId), RootedError> {
    let class = c.host_constructor_named(crate::host::shared_vm::operation(operation), name)?;
    c.set_prototype_rooted(class, base)?;
    let prototype = get(c, class, "prototype")?;
    let base_prototype = get(c, base, "prototype")?;
    c.set_prototype_rooted(prototype, base_prototype)?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    Ok((class, prototype))
}

/// The getter: the class, built once.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = api(c)?;
    let cached = get(c, data, "asyncEmitterClass")?;
    if c.is_callable_rooted(cached)? {
        return Ok(cached);
    }
    let hooks = super::async_hooks_shared_vm::module(c)?;
    let resource = get(c, hooks, "AsyncResource")?;
    let (referencing, prototype) = subclass(c, REFERENCING_CLASS, "emitterAsyncReferencingConstructor", resource)?;
    define_accessor(c, prototype, "eventEmitter", "emitterAsyncEventEmitter")?;
    put(c, data, "asyncReferencingClass", referencing)?;
    let emitter = get(c, data, "emitter")?;
    let (class, prototype) = subclass(c, CLASS, "emitterAsyncResourceConstructor", emitter)?;
    define_method(c, prototype, "emit", "emitterAsyncEmit")?;
    define_method(c, prototype, "emitDestroy", "emitterAsyncDestroy")?;
    for (name, operation) in ACCESSORS {
        define_accessor(c, prototype, name, operation)?;
    }
    put(c, data, "asyncEmitterClass", class)?;
    Ok(class)
}

/// `new EventEmitterAsyncResource(options)`: a string names the resource; otherwise
/// `options.name` does (required of the class itself), else the subclass's name.
pub(crate) fn initialize(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, CLASS)?;
    let options = argument(c, args, 0);
    let (options, name) = if c.string_text(options)?.is_some() {
        (c.undefined(), options)
    } else {
        let name = if super::event_target_shared_vm::is_nullish(c, options) { c.undefined() } else { get(c, options, "name")? };
        let data = api(c)?;
        let class = get(c, data, "asyncEmitterClass")?;
        let target = c.new_target_rooted().unwrap_or(class);
        if c.strict_equal_rooted(target, class)? && c.string_text(name)?.is_none() {
            return Err(super::util_shared_vm::argument_type_error(c, name, "options.name", "string")?);
        }
        let name = if c.truthy_rooted(name)? { name } else { get(c, target, "name")? };
        (options, name)
    };
    let data = api(c)?;
    let emitter = get(c, data, "emitter")?;
    c.call_rooted(emitter, receiver, &[options])?;
    let referencing = get(c, data, "asyncReferencingClass")?;
    let resource = c.construct_rooted(referencing, referencing, &[receiver, name, options])?;
    attach_slot(c, receiver, RESOURCE_SLOT, resource)?;
    Ok(c.undefined())
}

/// `new EventEmitterReferencingAsyncResource(emitter, type, options)`.
pub(crate) fn initialize_referencing(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let emitter = argument(c, args, 0);
    super::async_hooks_module_shared_vm::resource_constructor(c, receiver, args.get(1..).unwrap_or_default())?;
    attach_slot(c, receiver, EMITTER_SLOT, emitter)?;
    Ok(c.undefined())
}

fn resource(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    receiver_slot(c, receiver, RESOURCE_SLOT, CLASS)
}

/// `emit(event, ...args)`: `EventEmitter.prototype.emit` in the resource's scope.
pub(crate) fn emit(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let resource = resource(c, receiver)?;
    let data = api(c)?;
    let emitter = get(c, data, "emitter")?;
    let prototype = get(c, emitter, "prototype")?;
    let super_emit = get(c, prototype, "emit")?;
    let mut call = vec![super_emit, receiver];
    call.extend_from_slice(args);
    super::async_hooks_module_shared_vm::run_in_async_scope(c, resource, &call)
}

pub(crate) fn destroy(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = resource(c, receiver)?;
    super::async_hooks_module_shared_vm::resource_destroy(c, resource, &[])?;
    Ok(c.undefined())
}

pub(crate) fn async_id(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = resource(c, receiver)?;
    super::async_hooks_module_shared_vm::resource_async_id(c, resource, &[])
}

pub(crate) fn trigger_async_id(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = resource(c, receiver)?;
    super::async_hooks_module_shared_vm::resource_trigger_async_id(c, resource, &[])
}

pub(crate) fn async_resource(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    resource(c, receiver)
}

pub(crate) fn event_emitter(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    receiver_slot(c, receiver, EMITTER_SLOT, REFERENCING_CLASS)
}
