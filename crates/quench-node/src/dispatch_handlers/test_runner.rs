pub fn test_run(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::run(state, args)
}
pub fn test_get_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::test::current_context())
}
pub fn test_run_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let emitter = args.first().cloned().unwrap_or(Value::Undefined);
    let event = args
        .get(1)
        .cloned()
        .unwrap_or(Value::String("test:pass".into()));
    crate::modules::net::emit(
        state,
        &emitter,
        &quench_runtime::execute::to_js_string(&event)?,
        vec![quench_runtime::host_api::object(vec![(
            "skip".into(),
            Value::Boolean(true),
        )])],
    )?;
    Ok(Value::Undefined)
}
pub fn test_mock_fn(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    for index in [0usize, 1usize, 2usize, 3usize] {
        if let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.get(index) {
            let times = quench_runtime::execute::get_property(&args[index], "times");
            if !matches!(times, Value::Undefined) {
                let Value::Number(value) = times else {
                    return Err(crate::modules::buffer_enc::invalid_arg_type(
                        "The \"options.times\" property must be of type number".into(),
                    ));
                };
                if !value.is_finite() || value < 1.0 || value.fract() != 0.0 {
                    return Err(crate::modules::buffer_enc::invalid_arg_value(
                        "The value of \"options.times\" is out of range".into(),
                    ));
                }
            }
        }
    }
    let implementation = args
        .get(1)
        .filter(|value| {
            matches!(
                value,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            )
        })
        .cloned()
        .or_else(|| args.first().cloned())
        .unwrap_or(Value::Undefined);
    let calls = quench_runtime::host_api::array(Vec::new());
    let original_implementation = args
        .first()
        .filter(|value| {
            matches!(
                value,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            )
        })
        .cloned()
        .unwrap_or(Value::Undefined);
    let implementation_cell = quench_runtime::value::BindingCell::new(implementation.clone());
    if let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.get(2) {
        if let Value::Number(times) = quench_runtime::execute::get_property(&args[2], "times") {
            if times.is_finite() && times >= 0.0 {
                let _ = quench_runtime::execute::set_property_in_place(
                    &calls,
                    "\0mock:times",
                    Value::Number(times),
                );
                let _ = quench_runtime::execute::set_property_in_place(
                    &calls,
                    "\0mock:original",
                    original_implementation.clone(),
                );
            }
        }
    }
    let _ = quench_runtime::execute::set_property_in_place(
        &calls,
        "\0mock:original",
        original_implementation.clone(),
    );
    let implementation_for_bind = implementation.clone();
    let metadata_target = if matches!(original_implementation, Value::Undefined) {
        implementation_for_bind.clone()
    } else {
        original_implementation.clone()
    };
    let mut wrapper = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_MOCK_CALL.cap,
            ),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            calls.clone(),
            original_implementation.clone(),
        ],
    );
    wrapper = define_mock_metadata(
        wrapper,
        "name",
        quench_runtime::execute::get_property(&metadata_target, "name"),
    )?;
    wrapper = define_mock_metadata(
        wrapper,
        "length",
        quench_runtime::execute::get_property(&metadata_target, "length"),
    )?;
    let mock = quench_runtime::host_api::object(vec![("calls".into(), calls.clone())]);
    let restore = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            original_implementation,
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
        &mock, "restore",
    ));
    let call_count = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0B),
        },
        vec![calls.clone()],
    );
    let reset_calls = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b11),
        },
        vec![calls.clone()],
    );
    let implementation_setter = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0C),
        },
        vec![Value::BindingCell(implementation_cell.clone())],
    );
    let implementation_once = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0D),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            calls.clone(),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "callCount", call_count);
    let _ = quench_runtime::execute::set_property_in_place(&mock, "resetCalls", reset_calls);
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementation",
        implementation_setter,
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementationOnce",
        implementation_once,
    );
    let wrapper = quench_runtime::execute::set_property(wrapper, "mock", mock);
    let bind = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b07),
        },
        vec![
            Value::BindingCell(implementation_cell),
            calls,
            implementation_for_bind,
        ],
    );
    Ok(quench_runtime::execute::set_property(wrapper, "bind", bind))
}
pub fn test_mock_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let call_args = args.get(3..).unwrap_or_default();
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    let initial_implementation = match implementation {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value.clone(),
    };
    if let Value::BindingCell(cell) = implementation {
        if let Value::Number(times) = quench_runtime::execute::get_property(calls, "\0mock:times") {
            if (index as f64) >= times {
                let original = quench_runtime::execute::get_property(calls, "\0mock:original");
                cell.replace(original);
            }
        }
    }
    let mut once_restore = None;
    if let Value::BindingCell(cell) = implementation {
        let once_key = format!("\0mock:once:{index}");
        let once = quench_runtime::execute::get_property(calls, &once_key);
        if !matches!(once, Value::Undefined) {
            once_restore = Some(quench_runtime::execute::get_property(
                calls,
                "\0mock:original",
            ));
            cell.replace(once);
            let _ =
                quench_runtime::execute::set_property_in_place(calls, &once_key, Value::Undefined);
        }
    }
    let implementation_value = match implementation {
        Value::BindingCell(cell) => cell.borrow().clone(),
        _ => initial_implementation,
    };
    let (result, error) = if matches!(implementation_value, Value::Undefined) {
        (Value::Undefined, Value::Undefined)
    } else {
        match quench_runtime::execute::call(&implementation_value, &this, call_args) {
            Ok(result) => (result, Value::Undefined),
            Err(VmError::Thrown(error)) => (Value::Undefined, error),
            Err(error) => return Err(error),
        }
    };
    if let (Value::BindingCell(cell), Some(original)) = (implementation, once_restore) {
        cell.replace(original);
    }
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(call_args.to_vec()),
        ),
        ("this".into(), this.clone()),
        ("result".into(), result.clone()),
        ("error".into(), error.clone()),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    if !matches!(error, Value::Undefined) {
        return Err(VmError::Thrown(error));
    }
    Ok(result)
}
pub fn test_mock_method(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let object = args.first().ok_or(VmError::NotCallable)?;
    if matches!(object, Value::Null | Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"object\" argument must be an object".into(),
        ));
    }
    let key = match args.get(1) {
        Some(Value::String(key)) => key,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"methodName\" argument must be one of type string or symbol".into(),
            ))
        }
    };
    let own_descriptor = quench_runtime::execute::get_own_property_descriptor(object, key)?;
    if !matches!(own_descriptor, Value::Undefined)
        && quench_runtime::execute::get_property(&own_descriptor, "configurable")
            == Value::Boolean(false)
    {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(format!("Cannot redefine property: {key}"))],
        )));
    }
    let options_index = if matches!(args.get(3), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        Some(3)
    } else if matches!(args.get(2), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        Some(2)
    } else {
        None
    };
    if let Some(options_index) = options_index {
        let options = &args[options_index];
        let getter = matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                options, "getter",
            ));
        let setter = matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                options, "setter",
            ));
        if getter && setter {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
        if getter || setter {
            let descriptor = quench_runtime::execute::get_own_property_descriptor(object, key)?;
            let accessor = if getter { "get" } else { "set" };
            let original = quench_runtime::execute::get_property_result(&descriptor, accessor)?;
            let mock_args = if options_index == 3 {
                vec![original, args[2].clone()]
            } else {
                vec![original]
            };
            let wrapper = test_mock_fn(state, None, &mock_args)?;
            let other = if getter { "set" } else { "get" };
            let replacement = quench_runtime::host_api::object(vec![
                (accessor.into(), wrapper.clone()),
                (
                    other.into(),
                    quench_runtime::execute::get_property(&descriptor, other),
                ),
                (
                    "enumerable".into(),
                    quench_runtime::execute::get_property(&descriptor, "enumerable"),
                ),
                (
                    "configurable".into(),
                    quench_runtime::execute::get_property(&descriptor, "configurable"),
                ),
            ]);
            let _ = quench_runtime::execute::define_property(object.clone(), key, replacement)?;
            let mock = quench_runtime::execute::get_property_result(&wrapper, "mock")?;
            let restore = quench_runtime::host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
                },
                vec![
                    object.clone(),
                    Value::String(key.clone()),
                    Value::Undefined,
                    descriptor,
                ],
            );
            let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
            crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
                &mock, "restore",
            ));
            return Ok(wrapper);
        }
    }
    let original = quench_runtime::execute::get_property_result(object, key)?;
    if !matches!(
        original,
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The argument 'methodName' must be a method".into(),
        ));
    }
    let mut mock_args = vec![original];
    if let Some(implementation) = args.get(2) {
        mock_args.push(implementation.clone());
    }
    let wrapper = test_mock_fn(state, None, &mock_args)?;
    let mock = quench_runtime::execute::get_property_result(&wrapper, "mock")?;
    let restore = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
        },
        vec![
            object.clone(),
            Value::String(key.clone()),
            original_for_restore(&mock_args),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
        &mock, "restore",
    ));
    if !quench_runtime::execute::set_property_in_place(object, key, wrapper.clone()) {
        return Err(VmError::NotCallable);
    }
    Ok(wrapper)
}
pub fn test_mock_getter(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(
            quench_runtime::execute::get_property(options, "getter"),
            Value::Boolean(false)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.getter' cannot be false".into(),
            ));
        }
        if matches!(
            quench_runtime::execute::get_property(options, "setter"),
            Value::Boolean(true)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
    }
    let mut method_args = args.to_vec();
    method_args.push(quench_runtime::host_api::object(vec![(
        "getter".into(),
        Value::Boolean(true),
    )]));
    test_mock_method(state, None, &method_args)
}
pub fn test_mock_setter(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(
            quench_runtime::execute::get_property(options, "setter"),
            Value::Boolean(false)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be false".into(),
            ));
        }
        if matches!(
            quench_runtime::execute::get_property(options, "getter"),
            Value::Boolean(true)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
    }
    let mut method_args = args.to_vec();
    method_args.push(quench_runtime::host_api::object(vec![(
        "setter".into(),
        Value::Boolean(true),
    )]));
    test_mock_method(state, None, &method_args)
}
pub fn test_mock_call_count(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let calls = args.first().ok_or(VmError::NotCallable)?;
    let length = quench_runtime::execute::get_property(calls, "length");
    Ok(length)
}
pub fn test_mock_reset_calls(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::Array(array)) = args.first() {
        let _ = quench_runtime::execute::set_array_length_in_place(&Value::Array(array.clone()), 0);
    }
    Ok(Value::Undefined)
}
pub fn test_mock_reset(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::reset_mocks();
    Ok(Value::Undefined)
}
pub fn test_mock_timers_enable(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if quench_runtime::date::mock_enabled() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are already enabled".into(),
        ));
    }
    let now = args
        .iter()
        .rev()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| {
            let configured = quench_runtime::execute::get_property(options, "now");
            if matches!(configured, Value::Undefined) {
                quench_runtime::execute::get_property(options, "timeValue")
            } else if matches!(configured, Value::Object(_) | Value::ObjectAlias(_)) {
                quench_runtime::execute::get_property(&configured, "timeValue")
            } else {
                configured
            }
        })
        .unwrap_or(Value::Number(0.0));
    let value = match now {
        Value::Undefined => 0.0,
        Value::Number(value) if value.is_finite() && value >= 0.0 => value,
        Value::Number(_) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value of \"now\" is out of range".into(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"now\" option must be a number".into(),
            ))
        }
    };
    let apis = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| quench_runtime::execute::get_property(options, "apis"));
    let date = match apis {
        Some(Value::Array(array)) => {
            quench_runtime::execute::get_property(&Value::Array(array), "0")
                == Value::String("Date".into())
        }
        _ => true,
    };
    if date {
        quench_runtime::date::set_mock_now(Some(value));
    }
    crate::modules::timers::set_mock_timer_now(Some(value.max(0.0) as u64));
    Ok(Value::Undefined)
}
pub fn test_mock_timers_tick(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let delta = match args.first() {
        Some(Value::Number(value)) if value.is_finite() => *value,
        _ => 0.0,
    };
    let now = quench_runtime::date::current_time_ms();
    if quench_runtime::date::mock_enabled() {
        quench_runtime::date::set_mock_now(Some(now + delta));
    }
    let timer_now = crate::modules::timers::mock_timer_now()
        .unwrap_or(now.max(0.0) as u64)
        .saturating_add(delta.max(0.0) as u64);
    crate::modules::timers::set_mock_timer_now(Some(timer_now));
    while crate::modules::pump::drain_one_tick(_state)? {}
    Ok(Value::Undefined)
}
pub fn test_mock_timers_set_time(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !quench_runtime::date::mock_enabled() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are not enabled".into(),
        ));
    }
    let value = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value must be a valid time".into(),
            ))
        }
    };
    quench_runtime::date::set_mock_now(Some(value));
    Ok(Value::Undefined)
}
pub fn test_mock_timers_reset(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    quench_runtime::date::set_mock_now(None);
    crate::modules::timers::set_mock_timer_now(None);
    Ok(Value::Undefined)
}
pub fn test_mock_module(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !matches!(args.first(), Some(Value::String(_))) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"specifier\" argument must be of type string".into(),
        ));
    }
    let Some(options) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    if matches!(options, Value::Null | Value::Undefined)
        || !matches!(options, Value::Object(_) | Value::ObjectAlias(_))
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be an object".into(),
        ));
    }
    for key in ["cache"] {
        let value = quench_runtime::execute::get_property(options, key);
        if !matches!(value, Value::Undefined | Value::Boolean(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{key}\" property must be of type boolean"
            )));
        }
    }
    for key in ["namedExports", "exports", "defaultExport"] {
        let value = quench_runtime::execute::get_property(options, key);
        if !matches!(
            value,
            Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{key}\" property must be an object"
            )));
        }
    }
    let exports = !matches!(
        quench_runtime::execute::get_property(options, "exports"),
        Value::Undefined
    );
    let named = !matches!(
        quench_runtime::execute::get_property(options, "namedExports"),
        Value::Undefined
    );
    let default_export = !matches!(
        quench_runtime::execute::get_property(options, "defaultExport"),
        Value::Undefined
    );
    if (exports && named) || (exports && default_export) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The options exports fields cannot be combined".into(),
        ));
    }
    crate::modules::test::register_module_mock(
        args.first()
            .and_then(|value| {
                if let Value::String(value) = value {
                    Some(value.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        options.clone(),
    );
    Ok(Value::Undefined)
}
pub fn test_context_skip(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn test_context_todo(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn test_mock_property(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let object = args.first().ok_or(VmError::NotCallable)?;
    if matches!(object, Value::Null | Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"object\" argument must be an object".into(),
        ));
    }
    let key = match args.get(1) {
        Some(Value::String(key)) => key.clone(),
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"propertyName\" argument must be one of type string or symbol".into(),
            ))
        }
    };
    let descriptor = quench_runtime::execute::get_own_property_descriptor(object, &key)?;
    if matches!(descriptor, Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The property must exist".into(),
        ));
    }
    let original = quench_runtime::execute::get_property_result(object, &key)?;
    let value = args.get(2).cloned().unwrap_or(original.clone());
    let cell = quench_runtime::value::BindingCell::new(value);
    let accesses = quench_runtime::host_api::array(Vec::new());
    let getter = bound_custom(
        0x1b16,
        vec![Value::BindingCell(cell.clone()), accesses.clone()],
    );
    let setter = bound_custom(
        0x1b17,
        vec![
            Value::BindingCell(cell.clone()),
            accesses.clone(),
            quench_runtime::execute::get_property(&descriptor, "writable"),
        ],
    );
    let replacement = quench_runtime::host_api::object(vec![
        ("get".into(), getter.clone()),
        ("set".into(), setter.clone()),
        (
            "enumerable".into(),
            quench_runtime::execute::get_property(&descriptor, "enumerable"),
        ),
        (
            "configurable".into(),
            quench_runtime::execute::get_property(&descriptor, "configurable"),
        ),
    ]);
    let mock = quench_runtime::host_api::object(vec![("accesses".into(), accesses.clone())]);
    let restore = bound_custom(
        0x1b06,
        vec![
            object.clone(),
            Value::String(key.clone()),
            Value::Undefined,
            descriptor.clone(),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "accessCount",
        bound_custom(0x1b14, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "resetAccesses",
        bound_custom(0x1b15, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementation",
        bound_custom(
            0x1b0C,
            vec![Value::BindingCell(cell.clone()), accesses.clone()],
        ),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementationOnce",
        bound_custom(0x1b18, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(&getter, "mock", mock);
    let replacement = quench_runtime::host_api::object(vec![
        ("get".into(), getter.clone()),
        ("set".into(), setter),
        (
            "enumerable".into(),
            quench_runtime::execute::get_property(&descriptor, "enumerable"),
        ),
        (
            "configurable".into(),
            quench_runtime::execute::get_property(&descriptor, "configurable"),
        ),
    ]);
    let _ = quench_runtime::execute::define_property(object.clone(), &key, replacement)?;
    let restore = bound_custom(
        0x1b06,
        vec![
            object.clone(),
            Value::String(key.clone()),
            Value::Undefined,
            descriptor.clone(),
        ],
    );
    crate::modules::test::register_mock_restore(restore);
    Ok(getter)
}
pub fn test_mock_property_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let accesses = args.get(1).ok_or(VmError::NotCallable)?;
    let index = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let key = format!("\0property:once:{index}");
    let value = match quench_runtime::execute::get_property(accesses, &key) {
        Value::Undefined => cell.load(),
        value => value,
    };
    let record = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("get".into())),
        ("value".into(), value.clone()),
    ]);
    let _ = quench_runtime::execute::set_array_element_in_place(accesses, index, record);
    Ok(value)
}
pub fn test_mock_property_once(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let accesses = args.first().ok_or(VmError::NotCallable)?;
    let implementation = args.get(1).cloned().unwrap_or(Value::Undefined);
    let current = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let on_call = match args.get(2) {
        Some(Value::Number(n)) => *n as usize,
        _ => current,
    };
    if on_call < current {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The value of \"onAccess\" is out of range. It must be >= {current}"
            ))],
        )));
    }
    let _ = quench_runtime::execute::set_property_in_place(
        accesses,
        &format!("\0property:once:{on_call}"),
        implementation,
    );
    Ok(Value::Undefined)
}
pub fn test_mock_property_set(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let accesses = args.get(1).ok_or(VmError::NotCallable)?;
    if matches!(args.get(2), Some(Value::Boolean(false))) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "Cannot assign to read only property".into(),
        ));
    }
    let value = args.last().cloned().unwrap_or(Value::Undefined);
    cell.replace(value.clone());
    let index = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("set".into())),
        ("value".into(), value),
    ]);
    let _ = quench_runtime::execute::set_array_element_in_place(accesses, index, record);
    Ok(Value::Undefined)
}
pub fn test_mock_access_count(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(quench_runtime::execute::get_property(
        args.first().ok_or(VmError::NotCallable)?,
        "length",
    ))
}
pub fn test_mock_reset_accesses(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(accesses) = args.first() {
        let _ = quench_runtime::execute::set_array_length_in_place(accesses, 0);
    }
    Ok(Value::Undefined)
}
pub fn test_mock_implementation(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    if let Some((index, Value::Array(accesses))) = args
        .iter()
        .enumerate()
        .find(|(_, value)| matches!(value, Value::Array(_)))
    {
        let value = args.last().cloned().unwrap_or(Value::Undefined);
        cell.replace(value.clone());
        let length = match quench_runtime::execute::get_property(
            &Value::Array(accesses.clone()),
            "length",
        ) {
            Value::Number(n) => n as usize,
            _ => 0,
        };
        let record = quench_runtime::host_api::object(vec![
            ("type".into(), Value::String("set".into())),
            ("value".into(), value),
        ]);
        let _ = quench_runtime::execute::set_array_element_in_place(
            &Value::Array(accesses.clone()),
            length,
            record,
        );
        let _ = index;
    } else {
        cell.replace(args.get(1).cloned().unwrap_or(Value::Undefined));
    }
    Ok(Value::Undefined)
}
pub fn test_mock_implementation_once(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let current = match quench_runtime::execute::get_property(calls, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let implementation = args.get(2).cloned().unwrap_or(Value::Undefined);
    let on_call = match args.get(3) {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value as usize,
        Some(_) => return Err(VmError::NotCallable),
        None => current,
    };
    if on_call < current {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The value of \"onCall\" is out of range. It must be >= {current}"
            ))],
        )));
    }
    let key = format!("\0mock:once:{on_call}");
    let _ = quench_runtime::execute::set_property_in_place(calls, &key, implementation);
    let _ = cell;
    Ok(Value::Undefined)
}
pub fn test_mock_restore(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    if let Some(Value::BindingCell(cell)) = args.first() {
        cell.replace(args.get(1).cloned().unwrap_or(Value::Undefined));
        return Ok(Value::Undefined);
    }
    let object = args.first().ok_or(VmError::NotCallable)?;
    let key = match args.get(1) {
        Some(Value::String(key)) => key,
        _ => return Err(VmError::NotCallable),
    };
    if let Some(descriptor) = args.get(3) {
        let _ = quench_runtime::execute::define_property(object.clone(), key, descriptor.clone())?;
        return Ok(Value::Undefined);
    }
    let original = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::execute::set_property_in_place(object, key, original) {
        return Err(VmError::NotCallable);
    }
    Ok(Value::Undefined)
}
pub fn test_mock_bind(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?.clone();
    let calls = args.get(1).ok_or(VmError::NotCallable)?.clone();
    let this = args.get(3).cloned().unwrap_or(Value::Undefined);
    Ok(quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b08),
        },
        vec![
            implementation,
            calls,
            args.get(2).cloned().unwrap_or(Value::Undefined),
            this,
        ],
    ))
}
pub fn test_mock_bound_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let this = args.get(3).ok_or(VmError::NotCallable)?;
    let call_args = args.get(4..).unwrap_or_default();
    let result = quench_runtime::execute::call(implementation, this, call_args)?;
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(call_args.to_vec()),
        ),
        ("this".into(), this.clone()),
        ("result".into(), result.clone()),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    Ok(result)
}
pub fn test_mock_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let construct_args = args.get(3..).unwrap_or_default();
    let target = match args.get(2).unwrap_or(implementation) {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value.clone(),
    };
    let (result, error) = match quench_runtime::execute::construct_value_with_new_target(
        implementation,
        &target,
        construct_args,
    ) {
        Ok(result) => (result, Value::Undefined),
        Err(VmError::Thrown(error)) => (Value::Undefined, error),
        Err(error) => return Err(error),
    };
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(construct_args.to_vec()),
        ),
        ("result".into(), result.clone()),
        ("error".into(), error.clone()),
        // A construct call preserves both the implementation target and the
        // instance returned by the constructor.  These are observable Node
        // mock facts, not wrapper metadata.
        ("target".into(), target),
        (
            "this".into(),
            if matches!(error, Value::Undefined) {
                result.clone()
            } else {
                Value::Undefined
            },
        ),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    if !matches!(error, Value::Undefined) {
        return Err(VmError::Thrown(error));
    }
    Ok(result)
}
pub fn test_skip(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::skip(state, args)
}
pub fn test_before_each(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::before_each(args)
}
pub fn test_after_each(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::after_each(args)
}
pub fn test_nested(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::nested(state, args)
}
