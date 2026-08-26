fn assert_module() -> Value {
    if let Some(module) = NODE_ASSERT_MODULE.with(|stored| stored.borrow().clone()) {
        return module;
    }
    let module = crate::modules::assert::build_value();
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
    let hrtime = quench_runtime::execute::set_property(
        capability_function(HostCapabilityKind::Custom(CapabilityName::ProcessHrtime)),
        "bigint",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::ProcessHrtimeBigint,
        )),
    );
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
            "getBuiltinModule".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessGetBuiltinModule,
            )),
        ),
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
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessRemoveListener,
            )),
        ),
        (
            "removeAllListeners".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessRemoveAllListeners,
            )),
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
        ("hrtime".into(), hrtime),
        (
            "getActiveResourcesInfo".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ProcessActiveResourcesInfo,
            )),
        ),
        ("features".into(), crate::modules::process::features()),
        (
            "permission".into(),
            quench_runtime::host_api::object(vec![(
                "has".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::ProcessPermissionHas,
                )),
            )]),
        ),
    ]);
    NODE_PROCESS_MODULE.with(|current| current.replace(Some(module.clone())));
    module
}

fn process_on(arguments: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::String(event)), Some(listener)) = (arguments.first(), arguments.get(1)) {
        NODE_PROCESS_WARNING_LISTENERS.with(|listeners| {
            listeners
                .borrow_mut()
                .push((event.clone(), listener.clone(), false))
        });
    }
    Ok(NODE_PROCESS_MODULE
        .with(|module| module.borrow().clone())
        .unwrap_or(Value::Undefined))
}

fn drain_pending_process_warnings() -> Result<(), VmError> {
    let pending = NODE_PENDING_PROCESS_WARNINGS
        .with(|warnings| warnings.borrow_mut().drain(..).collect::<Vec<_>>());
    for warning in pending {
        process_emit(&[Value::String("warning".into()), warning])?;
    }
    Ok(())
}

fn process_emit(arguments: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::String(event)), Some(value)) = (arguments.first(), arguments.get(1)) {
        let listeners = NODE_PROCESS_WARNING_LISTENERS.with(|listeners| {
            listeners
                .borrow()
                .iter()
                .filter(|(name, _, _)| name == event)
                .map(|(_, listener, _)| listener.clone())
                .collect::<Vec<_>>()
        });
        for listener in listeners {
            quench_runtime::execute::call(
                &listener,
                &Value::Undefined,
                std::slice::from_ref(value),
            )?;
        }
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
}

fn process_remove_listener(arguments: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::String(event)), Some(target)) = (arguments.first(), arguments.get(1)) {
        NODE_PROCESS_WARNING_LISTENERS.with(|listeners| {
            if let Some(index) = listeners
                .borrow()
                .iter()
                .rposition(|(name, listener, _)| name == event && listener == target)
            {
                listeners.borrow_mut().remove(index);
            }
        });
    }
    Ok(NODE_PROCESS_MODULE
        .with(|module| module.borrow().clone())
        .unwrap_or(Value::Undefined))
}

fn process_remove_all_listeners(arguments: &[Value]) -> Result<Value, VmError> {
    let event = match arguments.first() {
        Some(Value::String(name)) => Some(name.clone()),
        _ => None,
    };
    NODE_PROCESS_WARNING_LISTENERS.with(|listeners| {
        if let Some(event) = event {
            listeners.borrow_mut().retain(|(name, _, _)| name != &event);
        } else {
            listeners.borrow_mut().clear();
        }
    });
    Ok(NODE_PROCESS_MODULE
        .with(|module| module.borrow().clone())
        .unwrap_or(Value::Undefined))
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

fn determine_specific_type(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    Ok(Value::String(specific_type_of(&value).into()))
}

fn err_type_truncate(value: &str) -> String {
    if value.chars().count() > 28 {
        let mut prefix: String = value.chars().take(25).collect();
        prefix.push_str("...");
        prefix
    } else {
        value.to_owned()
    }
}

fn err_type_quoted(value: &str) -> String {
    let truncated = err_type_truncate(value);
    if truncated.contains('\'') {
        format!("\"{truncated}\"")
    } else {
        format!("'{truncated}'")
    }
}

fn typed_array_name(value: &Value) -> Option<&'static str> {
    match value {
        Value::Int8Array(_) => Some("Int8Array"),
        Value::Int16Array(_) => Some("Int16Array"),
        Value::Int32Array(_) => Some("Int32Array"),
        Value::Float32Array(_) => Some("Float32Array"),
        Value::Float64Array(_) => Some("Float64Array"),
        Value::BigInt64Array(_) => Some("BigInt64Array"),
        Value::BigUint64Array(_) => Some("BigUint64Array"),
        Value::Uint8Array(_) => Some("Uint8Array"),
        Value::Uint8ClampedArray(_) => Some("Uint8ClampedArray"),
        Value::Uint16Array(_) => Some("Uint16Array"),
        Value::Uint32Array(_) => Some("Uint32Array"),
        _ => None,
    }
}

fn specific_type_of(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Boolean(value) => format!("type boolean ({value})"),
        Value::Number(value) => {
            let rendered = if value.is_nan() {
                "NaN".to_owned()
            } else if value.is_infinite() && *value > 0.0 {
                "Infinity".to_owned()
            } else if value.is_infinite() {
                "-Infinity".to_owned()
            } else {
                value.to_string()
            };
            format!("type number ({rendered})")
        }
        Value::BigInt(value) => format!("type bigint ({value}n)"),
        Value::String(value) => {
            if let Some(name) = value.strip_prefix("Symbol.") {
                let name = name.split('\0').next().unwrap_or("Symbol");
                return format!("type symbol (Symbol({name}))");
            }
            format!("type string ({})", err_type_quoted(value))
        }
        Value::Array(_) => "an instance of Array".into(),
        Value::Map(map) if map.is_weak() => "an instance of WeakMap".into(),
        Value::Map(_) => "an instance of Map".into(),
        Value::Set(set) if set.is_weak() => "an instance of WeakSet".into(),
        Value::Set(_) => "an instance of Set".into(),
        Value::Promise(_) => "an instance of Promise".into(),
        Value::Function(_) | Value::BoundFunction(_) | Value::HostCapability(_) => {
            let name = quench_runtime::execute::get_property_result(value, "name")
                .ok()
                .and_then(|value| match value {
                    Value::String(name) => Some(name),
                    _ => None,
                })
                .unwrap_or_default();
            format!("function {name}")
        }
        _ => {
            if let Some(name) = typed_array_name(value) {
                return format!("an instance of {name}");
            }
            if matches!(
                quench_runtime::execute::get_property_result(value, "timeValue"),
                Ok(Value::Number(_))
            ) {
                return "an instance of Date".into();
            }
            if matches!(
                quench_runtime::execute::get_property_result(value, "constructor"),
                Ok(Value::Undefined)
            ) {
                return "[Object: null prototype] {}".into();
            }
            "an instance of Object".into()
        }
    }
}

fn process_get_builtin_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(id)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"id\" argument must be of type string",
        )));
    };
    if id.starts_with("Symbol.") {
        // The engine represents symbols as a Symbol.-prefixed string; a symbol
        // is not an accepted id.
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"id\" argument must be of type string",
        )));
    }
    if !is_process_builtin(id) {
        return Ok(Value::Undefined);
    }
    require_module(&[Value::String(id.clone())])
}

fn is_process_builtin(id: &str) -> bool {
    let bare = id.strip_prefix("node:").unwrap_or(id);
    matches!(
        bare,
        "assert"
            | "buffer"
            | "child_process"
            | "crypto"
            | "events"
            | "fs"
            | "fs/promises"
            | "http"
            | "https"
            | "module"
            | "net"
            | "os"
            | "path"
            | "process"
            | "stream"
            | "string_decoder"
            | "timers"
            | "timers/promises"
            | "tls"
            | "url"
            | "util"
            | "v8"
            | "worker_threads"
    )
}
