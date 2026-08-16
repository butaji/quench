fn assert_module() -> Value {
    if let Some(module) = NODE_ASSERT_MODULE.with(|stored| stored.borrow().clone()) {
        return module;
    }
    let mut module = capability_function(HostCapabilityKind::Custom(CapabilityName::Assert));
    for (name, id) in [
        ("strictEqual", CapabilityName::AssertStrictEqual),
        ("deepStrictEqual", CapabilityName::AssertDeepStrictEqual),
        ("deepEqual", CapabilityName::AssertDeepStrictEqual),
        ("ok", CapabilityName::AssertOk),
        ("throws", CapabilityName::AssertThrows),
        ("doesNotThrow", CapabilityName::AssertDoesNotThrow),
        ("ifError", CapabilityName::AssertIfError),
        ("notStrictEqual", CapabilityName::AssertNotStrictEqual),
        ("equal", CapabilityName::AssertEqual),
        ("notEqual", CapabilityName::AssertNotEqual),
        ("match", CapabilityName::AssertMatchValue),
        (
            "notDeepStrictEqual",
            CapabilityName::AssertNotDeepStrictEqual,
        ),
        ("AssertionError", CapabilityName::AssertError),
    ] {
        module = quench_runtime::execute::set_property(
            module,
            name,
            capability_function(HostCapabilityKind::Custom(id)),
        );
    }
    NODE_ASSERT_MODULE.with(|stored| stored.replace(Some(module.clone())));
    module
}

fn process_module() -> Value {
    if let Some(module) = NODE_PROCESS_MODULE.with(|current| current.borrow().clone()) {
        return module;
    }
    let env = quench_runtime::host_api::object(
        std::env::vars()
            .map(|(key, value)| (key, Value::String(value.into())))
            .collect(),
    );
    NODE_PROCESS_ENV.with(|current| *current.borrow_mut() = Some(env.clone()));
    let module = quench_runtime::host_api::object(vec![
        ("env".into(), env),
        (
            "argv".into(),
            quench_runtime::host_api::array(std::env::args().map(Value::String).collect()),
        ),
        (
            "execPath".into(),
            Value::String(std::env::args().next().unwrap_or_default()),
        ),
        ("argv0".into(), Value::String("node".into())),
        (
            "title".into(),
            Value::String(
                NODE_PROCESS_TITLE
                    .with(|title| title.borrow().clone())
                    .into(),
            ),
        ),
        ("Symbol.toStringTag".into(), Value::String("process".into())),
        ("pid".into(), Value::Number(std::process::id() as f64)),
        (
            "platform".into(),
            Value::String(
                match std::env::consts::OS {
                    "macos" => "darwin",
                    value => value,
                }
                .into(),
            ),
        ),
        ("arch".into(), Value::String(std::env::consts::ARCH.into())),
        (
            "cwd".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::Cwd)),
        ),
        (
            "nextTick".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessNextTick)),
        ),
        (
            "umask".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessUmask)),
        ),
        (
            "on".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessOn)),
        ),
        (
            "once".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessOn)),
        ),
        (
            "removeListener".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessOn)),
        ),
        (
            "emit".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessEmit)),
        ),
        (
            "binding".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
        ),
        (
            "cpuUsage".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessCpuUsage)),
        ),
        (
            "hrtime".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessHrtime)),
        ),
        (
            "getActiveResourcesInfo".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessActiveResourcesInfo,
            )),
        ),
    ]);
    NODE_PROCESS_MODULE.with(|current| current.replace(Some(module.clone())));
    module
}

fn process_on(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(listener) = arguments.get(1) {
        NODE_PROCESS_WARNING_LISTENERS.with(|listeners| listeners.borrow_mut().push(listener.clone()));
    }
    Ok(NODE_PROCESS_MODULE
        .with(|module| module.borrow().clone())
        .unwrap_or(Value::Undefined))
}

fn process_emit(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.get(1) {
        let listeners = NODE_PROCESS_WARNING_LISTENERS.with(|listeners| listeners.borrow().clone());
        for listener in listeners {
            quench_runtime::execute::call(&listener, &Value::Undefined, std::slice::from_ref(value))?;
        }
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
}

fn process_cpu_usage(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if !matches!(value, Value::Object(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "options must be an object",
            )));
        }
        if let Ok(Value::Number(user)) = quench_runtime::execute::get_property_result(value, "user")
        {
            if user < 0.0 {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_VALUE",
                    "user must be non-negative",
                )));
            }
        }
    }
    Ok(quench_runtime::host_api::object(vec![
        ("user".into(), Value::Number(0.0)),
        ("system".into(), Value::Number(0.0)),
    ]))
}

fn process_hrtime(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        let values = array_values(value)
            .map_err(|_| VmError::Thrown(fs_error("ERR_OUT_OF_RANGE", "time must be an array")))?;
        if values.len() != 2 {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "time must have two elements",
            )));
        }
    }
    Ok(quench_runtime::host_api::array(vec![
        Value::Number(0.0),
        Value::Number(0.0),
    ]))
}

fn process_active_resources_info() -> Result<Value, VmError> {
    let (timeouts, immediates) = NODE_TIMER_COUNTS.with(Cell::get);
    let mut resources = Vec::new();
    resources.extend((0..timeouts).map(|_| Value::String("Timeout".into())));
    resources.extend((0..immediates).map(|_| Value::String("Immediate".into())));
    Ok(quench_runtime::host_api::array(resources))
}
