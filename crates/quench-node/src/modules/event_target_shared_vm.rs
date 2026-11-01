//! Event dispatch identity and its active/completed lifecycle.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn install(
    c: &mut Context<'_>,
    data: RootId,
    event: RootId,
    target: RootId,
) -> Result<(), RootedError> {
    for (field, description) in [
        ("eventTargetKey", "kTarget"),
        ("eventDispatchingKey", "kIsBeingDispatched"),
    ] {
        let key = c.private_symbol_rooted(Some(description));
        c.assign_named_property_rooted(data, field, key)?;
    }
    let prototype = get(c, event, "prototype")?;
    for (name, operation) in [
        ("target", "eventTargetGet"),
        ("currentTarget", "eventCurrentTargetGet"),
    ] {
        let getter = c.host_function_named_with_data(
            crate::host::shared_vm::operation(operation),
            &format!("get {name}"),
            data,
        )?;
        let key = c.string_rooted(name);
        let undefined = c.undefined();
        c.define_property_fields_rooted(
            prototype,
            key,
            DescriptorFields {
                get: Some(getter),
                set: Some(undefined),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        )?;
    }
    let prototype = get(c, target, "prototype")?;
    let remove = get(c, prototype, "removeEventListener")?;
    c.assign_named_property_rooted(data, "eventRemoveListener", remove)?;
    let dispatch = c.host_function_named_with_data(
        crate::host::shared_vm::operation("eventTargetDispatch"),
        "dispatchEvent",
        data,
    )?;
    c.define_named_data_property_rooted(prototype, "dispatchEvent", dispatch, true, false, true)?;
    Ok(())
}

pub(crate) fn with_dispatch(
    c: &mut Context<'_>,
    data: RootId,
    event: RootId,
    target: RootId,
    run: impl FnOnce(&mut Context<'_>) -> Result<RootId, RootedError>,
) -> Result<RootId, RootedError> {
    let key = get(c, data, "eventTargetKey")?;
    c.assign_property_rooted(event, key, target)?;
    let key = get(c, data, "eventDispatchingKey")?;
    let active = c.boolean(true);
    c.assign_property_rooted(event, key, active)?;
    let result = run(c);
    let completed = complete_delivery(c, data, event);
    // Cleanup must not replace a listener's original abrupt completion.
    match result {
        Err(error) => Err(error),
        Ok(value) => completed.map(|()| value),
    }
}

fn invalid_this(c: &mut Context<'_>, name: &str) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted(&format!("Value of \"this\" must be of type {name}"))?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
    Ok(c.throw(error))
}

pub(crate) fn complete_delivery(
    c: &mut Context<'_>,
    data: RootId,
    event: RootId,
) -> Result<(), RootedError> {
    let key = get(c, data, "eventDispatchingKey")?;
    let active = c.boolean(false);
    c.assign_property_rooted(event, key, active)
}

pub(crate) fn initialize(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let event = args[0];
    let data = c.host_function_data()?;
    let key = get(c, data, "eventTargetKey")?;
    let null = c.null();
    c.define_data_property_rooted(event, key, null, true, false, true)?;
    let key = get(c, data, "eventDispatchingKey")?;
    let no = c.boolean(false);
    c.define_data_property_rooted(event, key, no, true, false, true)?;
    Ok(c.undefined())
}

fn target(c: &mut Context<'_>, data: RootId, receiver: RootId) -> Result<RootId, RootedError> {
    let key = get(c, data, "eventTargetKey")?;
    if !c.is_object_rooted(receiver)? || c.own_property_fields_rooted(receiver, key)?.is_none() {
        return Err(invalid_this(c, "Event")?);
    }
    c.get_property_rooted(receiver, key)
}

pub(crate) fn get_target(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    target(c, data, receiver)
}

pub(crate) fn get_current_target(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let target = target(c, data, receiver)?;
    let key = get(c, data, "eventDispatchingKey")?;
    let active = c.get_property_rooted(receiver, key)?;
    if c.truthy_rooted(active)? {
        Ok(target)
    } else {
        Ok(c.null())
    }
}

pub(crate) fn dispatch(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let event = args.first().copied().unwrap_or_else(|| c.undefined());
    let records_key = get(c, data, "recordsKey")?;
    if !c.is_object_rooted(receiver)? {
        return Err(invalid_this(c, "EventTarget")?);
    }
    let records = c.get_property_rooted(receiver, records_key)?;
    if !c.is_object_rooted(records)? {
        return Err(invalid_this(c, "EventTarget")?);
    }
    if args.is_empty() {
        let error = c.type_error_rooted("The \"event\" argument must be specified")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
        return Err(c.throw(error));
    }
    let constructor = get(c, data, "eventCtor")?;
    if !c.instance_of_rooted(event, constructor)? {
        return Err(super::util_shared_vm::argument_type_error(
            c, event, "event", "Event",
        )?);
    }
    let kind = get(c, event, "type")?;
    let active_key = get(c, data, "eventDispatchingKey")?;
    let active = c.get_property_rooted(event, active_key)?;
    if c.truthy_rooted(active)? {
        let kind = c.string_text(kind)?.unwrap_or_default();
        let error = c.error_rooted(&format!("The event \"{kind}\" is already being dispatched"))?;
        super::node_errors_shared_vm::decorate_error(c, error, "ERR_EVENT_RECURSION")?;
        return Err(c.throw(error));
    }
    let entries = c.get_property_rooted(records, kind)?;
    let snapshot = if c
        .rooted_value(entries)
        .is_some_and(|value| value.is_undefined())
    {
        Vec::new()
    } else {
        super::events_shared_vm::listener_records(c, entries)?
    };
    with_dispatch(c, data, event, receiver, |c| {
        for entry in snapshot {
            let current = c.get_property_rooted(records, kind)?;
            let mut present = false;
            for candidate in super::events_shared_vm::listener_records(c, current)? {
                if c.strict_equal_rooted(candidate, entry)? {
                    present = true;
                    break;
                }
            }
            if !present {
                continue;
            }
            let listener = get(c, entry, "listener")?;
            let once = get(c, entry, "once")?;
            if c.truthy_rooted(once)? {
                let remove = get(c, data, "eventRemoveListener")?;
                c.call_rooted(remove, receiver, &[kind, listener])?;
            }
            let delivery = c.call_rooted(listener, receiver, &[event]);
            complete_delivery(c, data, event)?;
            match delivery {
                Ok(value) => {
                    super::events_abort_shared_vm::capture_abort_listener_rejection(c, value)?
                }
                Err(error) => {
                    let Some(reason) = error.exception else {
                        return Err(error);
                    };
                    super::events_abort_shared_vm::queue_listener_error(c, receiver, &[reason])?;
                }
            }
            let stopped = get(c, event, "_quenchImmediatePropagationStopped")?;
            if c.truthy_rooted(stopped)? {
                break;
            }
        }
        let prevented = get(c, event, "defaultPrevented")?;
        let prevented = c.truthy_rooted(prevented)?;
        Ok(c.boolean(!prevented))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_dispatch_identity_matches_the_node_lifecycle() {
        std::thread::Builder::new()
            .stack_size(quench_runtime::WORKER_STACK_SIZE)
            .spawn(|| {
                let source = include_str!(
                    "../../../../tests/node-compat/stage-3048/event-target-state-facts.js"
                );
                for mode in [
                    quench_runtime::ExecutionMode::Interpreted,
                    quench_runtime::ExecutionMode::Compiled,
                ] {
                    let host = crate::NodeHost::new(vec!["quench-node".into()]);
                    let mut runtime = quench_runtime::Runtime::new(host);
                    runtime.set_execution_mode(mode);
                    let program =
                        quench_runtime::Engine::specialize(source, "event-dispatch-lifecycle.js")
                            .unwrap();
                    runtime.execute(&program).unwrap();
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
