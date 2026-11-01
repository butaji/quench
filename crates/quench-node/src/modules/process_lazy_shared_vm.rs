//! `process`'s members Node defines late in bootstrap (lib/internal/bootstrap/node.js,
//! lib/internal/process/per_thread.js and lib/internal/process/finalization.js), defined
//! together on the first read of any of them (`process_surface::LAZY_MEMBERS`).
use super::event_constructor_shared_vm::get;
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `process.debugPort` without an inspector: Node's default inspector port.
const DEFAULT_DEBUG_PORT: f64 = 9229.0;

/// The plain `process` methods: property and operation.
const METHODS: &[(&str, &str)] = &[
    ("getActiveResourcesInfo", "processGetActiveResourcesInfo"),
    ("_getActiveRequests", "processGetActiveRequests"),
    ("availableMemory", "availableMemory"),
    ("constrainedMemory", "constrainedMemory"),
    ("setSourceMapsEnabled", "processSetSourceMapsEnabled"),
    ("abort", "processAbort"),
    ("getBuiltinModule", "processGetBuiltinModule"),
    ("ref", "processRef"),
    ("unref", "processUnref"),
];

/// `process.allowedNodeEnvironmentFlags`'s class prototype methods, each with its operation.
const FLAG_SET_MEMBERS: &[(&str, &str)] = &[
    ("add", "processFlagsAdd"),
    ("delete", "processFlagsDelete"),
    ("clear", "processFlagsClear"),
    ("has", "processFlagsHas"),
    ("entries", "processFlagsEntries"),
    ("forEach", "processFlagsForEach"),
];
const FLAG_SET_CLASS: &str = "NodeEnvironmentFlagsSet";
const FLAG_SET_CACHE: &str = "\0native:allowed-node-environment-flags";
const ALLOWED_FLAGS: &str = "allowedNodeEnvironmentFlags";

/// The events `process.finalization` runs its callbacks on, with their listener names.
const FINALIZATION_EVENTS: [(&str, &str); 2] = [("exit", "onExit"), ("beforeExit", "onBeforeExit")];
const FINALIZATION_FUNCTIONS: [(&str, &str, &str); 3] = [
    ("register", "processFinalizationRegister", "process.finalization.register"),
    ("registerBeforeExit", "processFinalizationRegisterBeforeExit", "process.finalization.registerBeforeExit"),
    ("unregister", "processFinalizationUnregister", "process.finalization.unregister"),
];
/// A finalization state's member holding an event's listener.
const LISTENER_SUFFIX: &str = "Listener";

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn put(c: &mut Context<'_>, target: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, value, true, true, true)?;
    Ok(())
}

fn accessor(c: &mut Context<'_>, target: RootId, key: RootId, get: RootId, set: Option<RootId>, enumerable: bool) -> Result<(), RootedError> {
    let fields = DescriptorFields {
        get: Some(get),
        set: Some(set.unwrap_or_else(|| c.undefined())),
        enumerable: Some(enumerable),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields).map(drop)
}

/// A startup intrinsic: `globalThis[constructor]`, or a member of its prototype.
fn intrinsic(c: &mut Context<'_>, constructor: &str, member: Option<&str>) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let constructor = get(c, global, constructor)?;
        match member {
            Some(member) => {
                let prototype = get(c, constructor, "prototype")?;
                get(c, prototype, member)
            }
            None => Ok(constructor),
        }
    })
}

/// Defines the lazy group.
pub(crate) fn initialize(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let process = process(c)?;
    for &(name, operation_name) in METHODS {
        let function = c.host_function_named(operation(operation_name), name)?;
        put(c, process, name, function)?;
    }
    #[cfg(unix)]
    super::process_metrics_shared_vm::define_methods(c, process)?;
    let key = c.string_rooted("sourceMapsEnabled");
    let getter = c.host_function_named(operation("processSourceMapsEnabled"), "get")?;
    accessor(c, process, key, getter, None, true)?;
    let port = c.number(DEFAULT_DEBUG_PORT);
    put(c, process, "debugPort", port)?;
    let key = c.string_rooted(ALLOWED_FLAGS);
    let getter = c.host_function_named(operation("processAllowedFlagsGet"), "get")?;
    let setter = c.host_function_named(operation("processAllowedFlagsSet"), "set")?;
    accessor(c, process, key, getter, Some(setter), true)?;
    let cell = c.array_rooted(&[])?;
    let key = c.string_rooted("finalization");
    let getter = c.host_function_named_with_data(operation("processFinalizationGet"), "get", cell)?;
    let setter = c.host_function_named_with_data(operation("processFinalizationSet"), "set", cell)?;
    accessor(c, process, key, getter, Some(setter), true)?;
    super::process_permission_shared_vm::define(c, process)?;
    Ok(c.undefined())
}

/// `process.getActiveResourcesInfo()`: the active requests (none are tracked), handles,
/// then timers.
pub(crate) fn active_resources_info(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let handles = super::process_shared_vm::get_active_network_resources(c, this, &[])?;
    let timers = super::timers_shared_vm::active_resources(c, this, &[])?;
    let mut resources = c.array_like_values_rooted(handles)?;
    resources.extend(c.array_like_values_rooted(timers)?);
    c.array_rooted(&resources)
}

/// `process._getActiveRequests()`.
pub(crate) fn active_requests(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    c.array_rooted(&[])
}

/// `process.sourceMapsEnabled`.
pub(crate) fn source_maps_enabled(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let enabled = super::source_map_shared_vm::support(c).enabled;
    Ok(c.boolean(enabled))
}

/// `process.setSourceMapsEnabled(enabled)`: the legacy switch, which also maps
/// `node_modules` and generated code.
pub(crate) fn set_source_maps_enabled(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let enabled = argument(c, args, 0);
    let Some(enabled) = c.rooted_value(enabled).and_then(|value| value.as_bool()) else {
        return Err(super::util_shared_vm::argument_type_error(c, enabled, "enabled", "boolean")?);
    };
    super::source_map_shared_vm::set_support(c, super::source_map_shared_vm::Support {
        enabled,
        node_modules: enabled,
        generated_code: enabled,
    });
    Ok(c.undefined())
}

/// `process.abort()`: ends the process with `SIGABRT` once what it wrote is out.
pub(crate) fn abort(_: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::abort()
}

/// `process.getBuiltinModule(id)`: the built-in `id` names, as Node's
/// `BuiltinModule.normalizeRequirableId` admits it, or `undefined`.
pub(crate) fn get_builtin_module(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 0);
    if c.string_text(id)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, id, "id", "string")?);
    }
    let builtin = super::module_shared_vm::is_builtin(c, this, &[id])?;
    if !c.truthy_rooted(builtin)? {
        return Ok(c.undefined());
    }
    let global = c.global_root()?;
    let require = get(c, global, crate::host::shared_vm::commonjs::BOOTSTRAP_REQUIRE)?;
    let undefined = c.undefined();
    c.call_rooted(require, undefined, &[id])
}

/// `process.ref(maybeRefable)` and `process.unref(maybeRefable)`: calls the value's
/// `Symbol.for(protocol)` method, or its `fallback` method.
fn call_ref_protocol(c: &mut Context<'_>, args: &[RootId], protocol: &str, fallback: &str) -> Result<RootId, RootedError> {
    let target = argument(c, args, 0);
    if c.is_object_rooted(target)? {
        let symbol_for = intrinsic(c, "Symbol", None)?;
        let symbol_for = get(c, symbol_for, "for")?;
        let description = c.string_rooted(protocol);
        let undefined = c.undefined();
        let symbol = c.call_rooted(symbol_for, undefined, &[description])?;
        let mut method = c.get_property_rooted(target, symbol)?;
        if !c.truthy_rooted(method)? {
            method = get(c, target, fallback)?;
        }
        if c.is_callable_rooted(method)? {
            c.call_rooted(method, target, &[])?;
        }
    }
    Ok(c.undefined())
}

pub(crate) fn ref_object(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    call_ref_protocol(c, args, "nodejs.ref", "ref")
}

pub(crate) fn unref_object(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    call_ref_protocol(c, args, "nodejs.unref", "unref")
}

/// `allowedNodeEnvironmentFlags`'s getter: the flag set, after which the property is a
/// plain data property holding it.
pub(crate) fn allowed_flags_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let flags = crate::host::shared_vm::commonjs::cached_native_binding(c, FLAG_SET_CACHE, flag_set)?;
    let process = process(c)?;
    let key = c.string_rooted(ALLOWED_FLAGS);
    c.define_data_property_rooted(process, key, flags, true, true, true)?;
    Ok(flags)
}

/// `allowedNodeEnvironmentFlags`'s setter: the receiver's property becomes `value`.
pub(crate) fn allowed_flags_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let key = c.string_rooted(ALLOWED_FLAGS);
    c.define_data_property_rooted(this, key, value, true, true, true)?;
    Ok(c.undefined())
}

/// The frozen `NodeEnvironmentFlagsSet`: an empty `Set` whose read-only overrides answer
/// for Node's flags (`process_surface::NODE_FLAGS`), held by an inner `Set`.
fn flag_set(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let set = intrinsic(c, "Set", None)?;
    let set_prototype = get(c, set, "prototype")?;
    let flags = super::process_surface::NODE_FLAGS.iter().map(|flag| c.string_rooted(flag)).collect::<Vec<_>>();
    let flags = c.array_rooted(&flags)?;
    let inner = c.construct_rooted(set, set, &[flags])?;
    let class = c.host_constructor_named(operation("processConstructor"), FLAG_SET_CLASS)?;
    c.set_prototype_rooted(class, set)?;
    let prototype = get(c, class, "prototype")?;
    c.set_prototype_rooted(prototype, set_prototype)?;
    for &(name, operation_name) in FLAG_SET_MEMBERS {
        let function = c.host_function_named_with_data(operation(operation_name), name, inner)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    let size = c.host_function_named(operation("processFlagsSize"), "get size")?;
    let key = c.string_rooted("size");
    accessor(c, prototype, key, size, None, false)?;
    let values = c.host_function_named_with_data(operation("processFlagsValues"), "values", inner)?;
    let symbol = intrinsic(c, "Symbol", None)?;
    let iterator = get(c, symbol, "iterator")?;
    for key in ["values", "keys"].map(|name| c.string_rooted(name)).into_iter().chain([iterator]) {
        c.define_data_property_rooted(prototype, key, values, true, false, true)?;
    }
    let instance = c.construct_rooted(set, class, &[])?;
    let freeze = intrinsic(c, "Object", None)?;
    let freeze = get(c, freeze, "freeze")?;
    let object = intrinsic(c, "Object", None)?;
    for value in [class, prototype, instance] {
        c.call_rooted(freeze, object, &[value])?;
    }
    Ok(instance)
}

pub(crate) fn flags_add(_: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(this)
}

pub(crate) fn flags_delete(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.boolean(false))
}

pub(crate) fn flags_clear(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// Whether `flag` names one of Node's flags, with or without its leading dashes, with
/// underscores for dashes, and, with dashes, with a value after `=`.
fn allows_flag(flag: &str) -> bool {
    let flag = flag.replace('_', "-");
    let flags = super::process_surface::NODE_FLAGS;
    if flag.starts_with('-') {
        let name = flag.split_once('=').map_or(flag.as_str(), |(name, _)| name);
        flags.contains(&name)
    } else {
        flags.iter().any(|known| known.strip_prefix("--").or_else(|| known.strip_prefix('-')) == Some(flag.as_str()))
    }
}

pub(crate) fn flags_has(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let flag = argument(c, args, 0);
    let allowed = c.string_text(flag)?.is_some_and(|flag| allows_flag(&flag));
    Ok(c.boolean(allowed))
}

fn inner_iterator(c: &mut Context<'_>, member: &str) -> Result<RootId, RootedError> {
    let inner = c.host_function_data()?;
    let method = intrinsic(c, "Set", Some(member))?;
    c.call_rooted(method, inner, &[])
}

pub(crate) fn flags_entries(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    inner_iterator(c, "entries")
}

pub(crate) fn flags_values(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    inner_iterator(c, "values")
}

/// `forEach(callback, thisArg)`: `callback(flag, flag, set)` for each flag.
pub(crate) fn flags_for_each(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = argument(c, args, 0);
    let receiver = argument(c, args, 1);
    for flag in super::process_surface::NODE_FLAGS {
        let flag = c.string_rooted(flag);
        c.call_rooted(callback, receiver, &[flag, flag, this])?;
    }
    Ok(c.undefined())
}

pub(crate) fn flags_size(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.number(super::process_surface::NODE_FLAGS.len() as f64))
}

/// `process.finalization`'s getter: what was assigned, else the finalization API made on
/// first read.
pub(crate) fn finalization_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let cell = c.host_function_data()?;
    let value = get(c, cell, "0")?;
    if !c.rooted_value(value).is_some_and(|value| value.is_undefined()) {
        return Ok(value);
    }
    let state = c.null_object_rooted()?;
    for (event, listener) in FINALIZATION_EVENTS {
        let entries = c.array_rooted(&[])?;
        put(c, state, event, entries)?;
        let name = c.string_rooted(event);
        let data = c.array_rooted(&[state, name])?;
        let function = c.host_function_named_with_data(operation("processFinalizationRun"), listener, data)?;
        put(c, state, &format!("{event}{LISTENER_SUFFIX}"), function)?;
    }
    let api = c.object_rooted()?;
    for (name, operation_name, _) in FINALIZATION_FUNCTIONS {
        let function = c.host_function_named_with_data(operation(operation_name), name, state)?;
        put(c, api, name, function)?;
    }
    put(c, cell, "0", api)?;
    Ok(api)
}

pub(crate) fn finalization_set(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let cell = c.host_function_data()?;
    let value = argument(c, args, 0);
    put(c, cell, "0", value)?;
    Ok(c.undefined())
}

fn listener(c: &mut Context<'_>, state: RootId, event: &str) -> Result<RootId, RootedError> {
    get(c, state, &format!("{event}{LISTENER_SUFFIX}"))
}

/// `register(obj, fn)` for `event`: `fn(obj, event)` runs on `event` while `obj` lives.
fn register(c: &mut Context<'_>, args: &[RootId], index: usize, event: &str) -> Result<RootId, RootedError> {
    super::process_warning_shared_vm::emit_experimental(c, FINALIZATION_FUNCTIONS[index].2)?;
    let target = argument(c, args, 0);
    super::perf_hooks::validate_object(c, target, "obj")?;
    let callback = argument(c, args, 1);
    let state = c.host_function_data()?;
    let entries = get(c, state, event)?;
    let mut values = c.array_like_values_rooted(entries)?;
    if values.is_empty() {
        let process = process(c)?;
        let on = get(c, process, "on")?;
        let name = c.string_rooted(event);
        let listener = listener(c, state, event)?;
        c.call_rooted(on, process, &[name, listener])?;
    }
    let weak_ref = intrinsic(c, "WeakRef", None)?;
    let reference = c.construct_rooted(weak_ref, weak_ref, &[target])?;
    let entry = c.array_rooted(&[reference, callback])?;
    values.push(entry);
    let entries = c.array_rooted(&values)?;
    put(c, state, event, entries)?;
    Ok(c.undefined())
}

pub(crate) fn finalization_register(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    register(c, args, 0, FINALIZATION_EVENTS[0].0)
}

pub(crate) fn finalization_register_before_exit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    register(c, args, 1, FINALIZATION_EVENTS[1].0)
}

/// An entry's target while it lives, else `undefined`.
fn entry_target(c: &mut Context<'_>, entry: RootId) -> Result<RootId, RootedError> {
    let reference = get(c, entry, "0")?;
    let deref = intrinsic(c, "WeakRef", Some("deref"))?;
    c.call_rooted(deref, reference, &[])
}

/// `unregister(obj)`: drops every registration of `obj`, and an event's listener with its
/// last registration.
pub(crate) fn finalization_unregister(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::process_warning_shared_vm::emit_experimental(c, FINALIZATION_FUNCTIONS[2].2)?;
    let target = argument(c, args, 0);
    let state = c.host_function_data()?;
    for (event, _) in FINALIZATION_EVENTS {
        let entries = get(c, state, event)?;
        let values = c.array_like_values_rooted(entries)?;
        if values.is_empty() {
            continue;
        }
        let mut kept = Vec::with_capacity(values.len());
        for entry in values {
            let registered = entry_target(c, entry)?;
            if !c.strict_equal_rooted(registered, target)? {
                kept.push(entry);
            }
        }
        if kept.is_empty() {
            let process = process(c)?;
            let remove = get(c, process, "removeListener")?;
            let name = c.string_rooted(event);
            let listener = listener(c, state, event)?;
            c.call_rooted(remove, process, &[name, listener])?;
        }
        let kept = c.array_rooted(&kept)?;
        put(c, state, event, kept)?;
    }
    Ok(c.undefined())
}

/// An event's listener: each live registration's `fn(obj, event)`.
pub(crate) fn finalization_run(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let state = get(c, data, "0")?;
    let event = get(c, data, "1")?;
    let event_name = c.to_string(event)?;
    let entries = get(c, state, &event_name)?;
    let undefined = c.undefined();
    for entry in c.array_like_values_rooted(entries)? {
        let target = entry_target(c, entry)?;
        if c.rooted_value(target).is_some_and(|value| value.is_undefined()) {
            continue;
        }
        let callback = get(c, entry, "1")?;
        c.call_rooted(callback, undefined, &[target, event])?;
    }
    Ok(c.undefined())
}
