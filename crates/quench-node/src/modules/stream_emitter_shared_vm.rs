//! Public streams own EventEmitter policy; the delegate holds wrapped listeners.
use super::event_constructor_shared_vm::{get, named_set, set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn entry(c: &mut Context<'_>, data: RootId, index: usize) -> Result<RootId, RootedError> {
    let key = c.number(index as f64);
    c.get_property_rooted(data, key)
}
pub(crate) fn install(
    c: &mut Context<'_>,
    dependencies: RootId,
    constructor: RootId,
) -> Result<(), RootedError> {
    let configuration = c
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
    let existing = get(c, configuration, "streamEmitters")?;
    if !c.is_object_rooted(existing)? {
        let global = c.global_root()?;
        let weak_map = get(c, global, "WeakMap")?;
        let map = c.construct_rooted(weak_map, weak_map, &[])?;
        let prototype = get(c, weak_map, "prototype")?;
        for (field, method) in [("streamEmitterGet", "get"), ("streamEmitterSet", "set")] {
            let function = get(c, prototype, method)?;
            put(c, configuration, field, function)?;
        }
        put(c, configuration, "streamEmitters", map)?;
    }
    let prototype = get(c, constructor, "prototype")?;
    let data = c.array_rooted(&[configuration, constructor, prototype])?;
    let operation = c.host_function_with_data(
        crate::host::shared_vm::operation("streamInitializeEmitter"),
        data,
    )?;
    put(c, dependencies, "initializeEmitter", operation)
}
pub(crate) fn initialize(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let stream = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    let data = c.host_function_data()?;
    let configuration = entry(c, data, 0)?;
    let previous = get(c, stream, "_events")?;
    if c.rooted_value(previous)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        let events = c.object_rooted()?;
        named_set(c, configuration, stream, "_events", events)?;
    }
    let constructor = entry(c, data, 1)?;
    let prototype = entry(c, data, 2)?;
    let delegate = c.object_rooted()?;
    c.set_prototype_rooted(delegate, prototype)?;
    let events = c.null_object_rooted()?;
    put(c, delegate, "_events", events)?;
    let zero = c.number(0.0);
    put(c, delegate, "_eventsCount", zero)?;
    // The delegate's map starts empty. Public policy is initialized exactly once
    // on the actual stream below, including capture options and active domains.
    let shape = get(c, configuration, "shapeKey")?;
    let no = c.boolean(false);
    c.define_data_property_rooted(delegate, shape, no, true, true, true)?;
    let maximum = c.string_rooted("_maxListeners");
    let alias = c.array_rooted(&[stream, maximum, configuration])?;
    let getter = c.host_function_with_data(
        crate::host::shared_vm::operation("streamEmitterMaximumGet"),
        alias,
    )?;
    let setter = c.host_function_with_data(
        crate::host::shared_vm::operation("streamEmitterMaximumSet"),
        alias,
    )?;
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    put(c, descriptor, "set", setter)?;
    let yes = c.boolean(true);
    put(c, descriptor, "enumerable", yes)?;
    put(c, descriptor, "configurable", yes)?;
    c.define_property_descriptor_rooted(delegate, maximum, descriptor)?;
    let owner = get(c, configuration, "streamOwnerKey")?;
    c.define_data_property_rooted(delegate, owner, stream, false, false, true)?;
    named_set(c, configuration, stream, "_emitter", delegate)?;
    let map = get(c, configuration, "streamEmitters")?;
    let register = get(c, configuration, "streamEmitterSet")?;
    c.call_rooted(register, map, &[delegate, stream])?;
    c.call_rooted(constructor, stream, &[options])?;
    Ok(undefined)
}
pub(crate) fn maximum_get(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let target = entry(c, data, 0)?;
    let key = entry(c, data, 1)?;
    c.get_property_rooted(target, key)
}
pub(crate) fn maximum_set(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let target = entry(c, data, 0)?;
    let key = entry(c, data, 1)?;
    let configuration = entry(c, data, 2)?;
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    set(c, configuration, target, key, value)?;
    Ok(c.undefined())
}
