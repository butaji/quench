fn event_prototype() -> Value {
    EVENT_PROTOTYPE.with(|slot| {
        if let Some(value) = slot.borrow().clone() {
            return value;
        }
        let prototype = host_api::object(Vec::new());
        let descriptor = host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_TRUSTED_GET),
            ),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let prototype = execute::define_property(prototype, "isTrusted", descriptor)
            .unwrap_or_else(|_| host_api::object(Vec::new()));
        *slot.borrow_mut() = Some(prototype.clone());
        prototype
    })
}
pub fn event_trusted_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
}
pub fn events_from(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::from(state, args)
}
pub fn events_method_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_on(state, receiver, args)
}
pub fn events_method_emit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_emit(state, receiver, args)
}
pub fn events_capture_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::capture_rejections_get(state, args)
}
pub fn events_capture_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::capture_rejections_set(state, args)
}
pub fn events_default_max_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::default_max_get(state, args)
}
pub fn events_default_max_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::default_max_set(state, args)
}
pub fn events_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::new_emitter(state, args)
}
pub fn message_channel_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::event_target::new_message_channel(state)
}
pub fn event_target_rejection(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit(
        state,
        &[
            Value::String("uncaughtException".into()),
            args.first().cloned().unwrap_or(Value::Undefined),
            Value::String("uncaughtException".into()),
        ],
    )?;
    Ok(Value::Undefined)
}
pub fn event_get_property(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let valid = matches!(
        execute::get_property(receiver, "Symbol.toStringTag"),
        Value::String(ref tag) if tag == "Event" || tag == "CustomEvent"
    );
    if !valid {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    let Some(Value::String(key)) = args.first() else {
        return Ok(Value::Undefined);
    };
    Ok(execute::get_property(receiver, key))
}
pub fn events_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        crate::modules::events::initialize_emitter(state, receiver)?;
        return Ok(receiver.clone());
    }
    crate::modules::events::new_emitter(state, &[])
}
pub fn events_abort_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::abort_listener_callback(state, args)
}
pub fn events_abort_dispose(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::abort_listener_dispose(state, args)
}
pub fn events_add_abort_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::add_abort_listener(state, args)
}
pub fn abort_controller_new(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = quench_runtime::execute::set_property(signal, "aborted", Value::Boolean(false));
    let signal = quench_runtime::execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "constructor",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "Symbol.toStringTag",
        Value::String("AbortSignal".into()),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    Ok(quench_runtime::host_api::object(vec![
        ("\0quench:abort:controller".into(), Value::Boolean(true)),
        ("\0quench:abort:signal".into(), signal.clone()),
        ("signal".to_string(), signal),
        (
            "constructor".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER),
        ),
        (
            "Symbol.toStringTag".into(),
            Value::String("AbortController".into()),
        ),
        (
            "abort".to_string(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_ABORT),
        ),
    ]))
}
pub fn abort_controller_signal_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    let signal = execute::get_property(receiver, "\0quench:abort:signal");
    if !matches!(signal, Value::Object(_))
        || !matches!(
            execute::get_property(receiver, "\0quench:abort:controller"),
            Value::Boolean(true)
        )
    {
        return Err(execute::type_error("Illegal invocation"));
    }
    Ok(signal)
}
pub fn abort_signal_aborted_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(execute::get_property(receiver, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortSignal")
    {
        return Err(execute::type_error("Illegal invocation"));
    }
    Ok(execute::get_property(receiver, "aborted"))
}
pub fn abort_signal_has_instance(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(|value| {
        matches!(
            execute::get_property(value, crate::modules::event_target::ABORT_SIGNAL_BRAND),
            Value::Boolean(true)
        )
    })))
}
pub fn abort_signal_throw_if_aborted(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(signal) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(
        execute::get_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("Illegal invocation"));
    }
    if matches!(
        execute::get_property(signal, "aborted"),
        Value::Boolean(true)
    ) {
        return Err(VmError::Thrown(execute::get_property(signal, "reason")));
    }
    Ok(Value::Undefined)
}
pub fn abort_controller_abort(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(controller) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(
        execute::get_property(controller, "\0quench:abort:controller"),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("Illegal invocation"));
    }
    let original_signal =
        quench_runtime::execute::get_property(controller, "\0quench:abort:signal");
    if matches!(
        quench_runtime::execute::get_property(&original_signal, "aborted"),
        Value::Boolean(true)
    ) {
        return Ok(Value::Undefined);
    }
    let reason = args.first().cloned().unwrap_or_else(|| {
        quench_runtime::host_api::object(vec![
            ("\0domexception".into(), Value::Boolean(true)),
            ("name".into(), Value::String("AbortError".into())),
            (
                "message".into(),
                Value::String("This operation was aborted".into()),
            ),
            ("code".into(), Value::Number(20.0)),
        ])
    });
    quench_runtime::execute::set_property_in_place(
        &original_signal,
        "aborted",
        Value::Boolean(true),
    );
    quench_runtime::execute::set_property_in_place(&original_signal, "reason", reason);
    let event = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("abort".into())),
        ("isTrusted".into(), Value::Boolean(true)),
        (
            "stopImmediatePropagation".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_EVENT_STOP_IMMEDIATE),
        ),
    ]);
    let event = execute::set_prototype_of(&event, &event_prototype())?;
    crate::modules::event_target::dispatch_event(state, Some(&original_signal), &[event])?;
    propagate_abort_composites(state, &original_signal)
}
pub fn abort_event_stop_immediate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn event_new(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(value) = args.first() else {
        return Err(execute::type_error("Event type is required"));
    };
    let event_type = match value {
        Value::String(value) if value.ends_with('\0') || value.contains("ymbol") => {
            return Err(execute::type_error("Event type is invalid"));
        }
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        _ => return Err(execute::type_error("Event type is invalid")),
    };
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"options\" argument must be of type object.{}",
                    crate::modules::buffer_enc::invalid_arg_received(&options)
                )),
            ),
        ])));
    }
    let bubbles = execute::get_property_result(&options, "bubbles")
        .map(|value| execute::is_truthy(&value))
        .unwrap_or(false);
    let cancelable = execute::get_property_result(&options, "cancelable")
        .map(|value| execute::is_truthy(&value))
        .unwrap_or(false);
    let event = host_api::object(vec![
        ("type".into(), Value::String(event_type)),
        ("bubbles".into(), Value::Boolean(bubbles)),
        ("cancelable".into(), Value::Boolean(cancelable)),
        ("defaultPrevented".into(), Value::Boolean(false)),
        ("returnValue".into(), Value::Boolean(true)),
        ("\0event:cancelBubble".into(), Value::Boolean(false)),
        ("composed".into(), Value::Boolean(false)),
        ("isTrusted".into(), Value::Boolean(false)),
        ("target".into(), Value::Null),
        ("currentTarget".into(), Value::Null),
        ("srcElement".into(), Value::Null),
        ("eventPhase".into(), Value::Number(0.0)),
        ("timeStamp".into(), Value::Number(0.0)),
        ("Symbol.toStringTag".into(), Value::String("Event".into())),
        (
            "preventDefault".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_PREVENT_DEFAULT),
        ),
        (
            "stopPropagation".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_STOP_PROPAGATION),
        ),
        (
            "stopImmediatePropagation".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_STOP_IMMEDIATE),
        ),
        (
            "composedPath".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_COMPOSED_PATH),
        ),
    ]);
    let global = quench_runtime::vm::current_global_object();
    let event_prototype =
        execute::get_property(&execute::get_property(&global, "Event"), "prototype");
    let event = if matches!(event_prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&event, &event_prototype)?
    } else {
        event
    };
    let event = execute::define_property(
        event,
        "cancelBubble",
        host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_GET_CANCEL_BUBBLE),
            ),
            (
                "set".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_SET_CANCEL_BUBBLE),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    Ok(event)
}
pub fn custom_event_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let event = event_new(state, args)?;
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    let detail = if matches!(options, Value::Undefined) {
        Value::Null
    } else {
        execute::get_property(&options, "detail")
    };
    let event = execute::set_property(
        event,
        "Symbol.toStringTag",
        Value::String("CustomEvent".into()),
    );
    let event = execute::set_property(
        event,
        "constructor",
        host_api::object(vec![("name".into(), Value::String("CustomEvent".into()))]),
    );
    execute::define_property(
        event,
        "detail",
        host_api::object(vec![
            ("value".into(), detail),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(false)),
        ]),
    )
}
pub fn event_source(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
fn valid_event_receiver(receiver: Option<&Value>) -> Result<&Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    if !matches!(
        execute::get_property(receiver, "Symbol.toStringTag"),
        Value::String(ref tag) if tag == "Event" || tag == "CustomEvent"
    ) {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    Ok(receiver)
}
pub fn event_get_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    Ok(execute::get_property(receiver, "\0event:cancelBubble"))
}
pub fn event_set_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(args.first().is_some_and(execute::is_truthy)),
    );
    execute::replace_value(receiver, &updated);
    Ok(Value::Undefined)
}
pub fn define_event_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().cloned();
    let target = original
        .clone()
        .ok_or_else(|| execute::type_error("eventTarget is required"))?;
    let name = match args.get(1) {
        Some(Value::String(name)) => name.clone(),
        _ => return Err(execute::type_error("eventName is required")),
    };
    let event = match args.get(2) {
        Some(Value::String(event)) => event.clone(),
        _ => name.clone(),
    };
    let target = execute::set_property(target, "\0event:handler:event", Value::String(event));
    let target = execute::set_property(target, "\0event:handler:listener", Value::Null);
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_HANDLER_GET),
        ),
        (
            "set".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_HANDLER_SET),
        ),
        ("enumerable".into(), Value::Boolean(true)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let target = execute::define_property(target, format!("on{name}").as_str(), descriptor)?;
    if let Some(original) = original.as_ref() {
        execute::replace_value(original, &target);
    }
    let _ = state;
    Ok(target)
}
pub fn event_handler_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|value| execute::get_property(value, "\0event:handler:listener"))
        .unwrap_or(Value::Null))
}
pub fn event_handler_set(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let event = execute::get_property(receiver, "\0event:handler:event");
    let old = execute::get_property(receiver, "\0event:handler:listener");
    let listener = args.first().cloned().unwrap_or(Value::Null);
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:handler:listener",
        listener.clone(),
    );
    execute::replace_value(receiver, &updated);
    if quench_runtime::is_callable(&listener) {
        let event_name = match &event {
            Value::String(name) => name.as_str(),
            _ => "",
        };
        if !quench_runtime::is_callable(&old)
            || !crate::modules::event_target::replace_event_listener(
                state, receiver, event_name, &old, &listener,
            )
        {
            let _ = crate::modules::event_target::add_event_listener(
                state,
                Some(receiver),
                &[event, listener],
            );
        }
    } else if quench_runtime::is_callable(&old) {
        let _ = crate::modules::event_target::remove_event_listener(
            state,
            Some(receiver),
            &[event, old],
        );
    }
    Ok(Value::Undefined)
}
pub fn event_prevent_default(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    if execute::is_truthy(&execute::get_property(receiver, "\0event:passive")) {
        return Ok(Value::Undefined);
    }
    if execute::is_truthy(&execute::get_property(receiver, "cancelable")) {
        let updated =
            execute::set_property(receiver.clone(), "defaultPrevented", Value::Boolean(true));
        execute::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}
pub fn event_stop_propagation(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(true),
    );
    execute::replace_value(receiver, &updated);
    Ok(Value::Undefined)
}
pub fn event_stop_immediate(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    if let Some(identity) = receiver.object_identity() {
        state.borrow_mut().stopped_events.insert(identity);
    }
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(true),
    );
    execute::replace_value(receiver, &updated);
    Ok(Value::Undefined)
}
pub fn event_composed_path(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let active = matches!(
        execute::get_property(receiver, "eventPhase"),
        Value::Number(phase) if phase != 0.0
    );
    if !active {
        return Ok(host_api::array(Vec::new()));
    }
    match execute::get_property(receiver, "target") {
        target if !matches!(target, Value::Undefined | Value::Null) => {
            Ok(host_api::array(vec![target]))
        }
        _ => Ok(host_api::array(Vec::new())),
    }
}
pub fn abort_signal_new(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "code".into(),
            Value::String("ERR_ILLEGAL_CONSTRUCTOR".into()),
        ),
        (
            "message".into(),
            Value::String("Illegal constructor".into()),
        ),
    ])))
}
pub fn abort_signal_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = quench_runtime::execute::set_property(signal, "aborted", Value::Boolean(true));
    let signal = quench_runtime::execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    Ok(quench_runtime::execute::set_property(
        signal,
        "reason",
        args.first().cloned().unwrap_or_else(|| {
            quench_runtime::host_api::object(vec![
                ("\0domexception".into(), Value::Boolean(true)),
                ("name".into(), Value::String("AbortError".into())),
                (
                    "message".into(),
                    Value::String("This operation was aborted".into()),
                ),
                ("code".into(), Value::Number(20.0)),
            ])
        }),
    ))
}
pub fn abort_signal_timeout(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let delay = args.first().cloned().unwrap_or(Value::Number(0.0));
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = execute::set_property(signal, "aborted", Value::Boolean(false));
    let signal = execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    let signal = execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    let callback = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_TIMEOUT_FIRE);
    let timer = crate::modules::timers::set_timeout(state, &[callback, delay, signal.clone()])?;
    // Node's AbortSignal.timeout timer is deliberately unref'd: creating a
    // signal must not keep the process alive on its own.
    crate::modules::timers::method_unref(state, Some(&timer));
    Ok(signal)
}
pub fn abort_signal_timeout_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    abort_signal_timeout(state, args)
}
pub fn abort_signal_timeout_fire(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(signal) = args.first() else {
        return Ok(Value::Undefined);
    };
    let reason = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "The operation was aborted due to timeout".into(),
            )],
        ),
        "name",
        Value::String("TimeoutError".into()),
    );
    let reason = execute::set_property(reason, "code", Value::Number(23.0));
    execute::set_property_in_place(signal, "aborted", Value::Boolean(true));
    execute::set_property_in_place(signal, "reason", reason);
    let event = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("abort".into())),
        ("isTrusted".into(), Value::Boolean(true)),
    ]);
    let event = execute::set_prototype_of(&event, &event_prototype())?;
    crate::modules::event_target::dispatch_event(state, Some(signal), &[event])?;
    propagate_abort_composites(state, signal)
}
fn propagate_abort_composites(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
) -> Result<Value, VmError> {
    let Some(identity) = crate::modules::event_target::target_identity(source) else {
        return Ok(Value::Undefined);
    };
    let composites = state
        .borrow_mut()
        .abort_composites
        .remove(&identity)
        .unwrap_or_default();
    let reason = execute::get_property(source, "reason");
    for composite in composites {
        if execute::is_truthy(&execute::get_property(&composite, "aborted")) {
            continue;
        }
        execute::set_property_in_place(&composite, "aborted", Value::Boolean(true));
        execute::set_property_in_place(&composite, "reason", reason.clone());
        let event = quench_runtime::host_api::object(vec![
            ("type".into(), Value::String("abort".into())),
            ("isTrusted".into(), Value::Boolean(true)),
        ]);
        let event = execute::set_prototype_of(&event, &event_prototype())?;
        crate::modules::event_target::dispatch_event(state, Some(&composite), &[event])?;
        propagate_abort_composites(state, &composite)?;
    }
    Ok(Value::Undefined)
}
pub fn abort_signal_any(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let list = args.first().ok_or_else(|| {
        execute::type_error("The \"signals\" argument must be an instance of Array")
    })?;
    let length = match execute::get_property(list, "length") {
        Value::Number(n) if n.is_finite() && n >= 0.0 => n as usize,
        _ => {
            return Err(execute::type_error(
                "The \"signals\" argument must be an instance of Array",
            ))
        }
    };
    let composite = crate::modules::event_target::new_target(state, &[])?;
    execute::set_property_in_place(&composite, "aborted", Value::Boolean(false));
    execute::set_property_in_place(
        &composite,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    for index in 0..length {
        let source = execute::get_property(list, &index.to_string());
        if !matches!(source, Value::Object(_))
            || !matches!(
                execute::get_property(&source, crate::modules::event_target::ABORT_SIGNAL_BRAND),
                Value::Boolean(true)
            )
        {
            return Err(execute::type_error(
                "The \"signals\" argument must contain only AbortSignal instances",
            ));
        }
        if execute::is_truthy(&execute::get_property(&source, "aborted")) {
            execute::set_property_in_place(&composite, "aborted", Value::Boolean(true));
            execute::set_property_in_place(
                &composite,
                "reason",
                execute::get_property(&source, "reason"),
            );
            return Ok(composite);
        }
        if let Some(identity) = crate::modules::event_target::target_identity(&source) {
            state
                .borrow_mut()
                .abort_composites
                .entry(identity)
                .or_default()
                .push(composite.clone());
        }
    }
    Ok(composite)
}
pub fn abort_signal_any_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    abort_signal_any(state, args)
}
fn define_mock_metadata(wrapper: Value, key: &str, value: Value) -> Result<Value, VmError> {
    let descriptor = quench_runtime::host_api::object(vec![
        ("value".into(), value),
        ("writable".into(), Value::Boolean(false)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    quench_runtime::execute::define_property(wrapper, key, descriptor)
}
