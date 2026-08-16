fn os_module() -> Value {
    let mut module = quench_runtime::host_api::object(vec![
        (
            "platform".into(),
            os_string_function(CapabilityName::OsPlatform),
        ),
        ("arch".into(), os_string_function(CapabilityName::OsArch)),
        (
            "tmpdir".into(),
            os_string_function(CapabilityName::OsTmpdir),
        ),
        (
            "homedir".into(),
            os_string_function(CapabilityName::OsHomedir),
        ),
        ("EOL".into(), Value::String("\n".into())),
        (
            "devNull".into(),
            Value::String(if cfg!(windows) { "NUL" } else { "/dev/null" }.into()),
        ),
        (
            "cpus".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsCpus)),
        ),
        (
            "freemem".into(),
            os_numeric_function(CapabilityName::OsFreemem),
        ),
        (
            "totalmem".into(),
            os_numeric_function(CapabilityName::OsTotalmem),
        ),
        ("type".into(), os_string_function(CapabilityName::OsType)),
        (
            "release".into(),
            os_string_function(CapabilityName::OsRelease),
        ),
        (
            "endianness".into(),
            os_string_function(CapabilityName::OsEndianness),
        ),
        (
            "loadavg".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsLoadavg)),
        ),
        (
            "networkInterfaces".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::OsNetworkInterfaces,
            )),
        ),
        (
            "userInfo".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsUserInfo)),
        ),
        (
            "uptime".into(),
            os_numeric_function(CapabilityName::OsUptime),
        ),
        (
            "getPriority".into(),
            os_numeric_function(CapabilityName::OsGetPriority),
        ),
        (
            "setPriority".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::OsSetPriority)),
        ),
        (
            "availableParallelism".into(),
            os_numeric_function(CapabilityName::OsAvailableParallelism),
        ),
        (
            "hostname".into(),
            os_string_function(CapabilityName::OsHostname),
        ),
        (
            "version".into(),
            os_string_function(CapabilityName::OsVersion),
        ),
        (
            "machine".into(),
            os_string_function(CapabilityName::OsMachine),
        ),
        (
            "constants".into(),
            quench_runtime::host_api::object(vec![
                (
                    "priority".into(),
                    quench_runtime::host_api::object(vec![
                        ("PRIORITY_LOW".into(), Value::Number(19.0)),
                        ("PRIORITY_NORMAL".into(), Value::Number(0.0)),
                        ("PRIORITY_HIGHEST".into(), Value::Number(-20.0)),
                    ]),
                ),
                (
                    "errno".into(),
                    quench_runtime::host_api::object(vec![("ENOENT".into(), Value::Number(2.0))]),
                ),
            ]),
        ),
    ]);
    let env = NODE_PROCESS_ENV
        .with(|current| current.borrow().clone())
        .unwrap_or_else(|| quench_runtime::host_api::object(vec![]));
    module = quench_runtime::execute::set_property(module, "\0env", env);
    module
}

fn os_numeric_function(kind: u16) -> Value {
    let function = capability_function(HostCapabilityKind::Custom(kind));
    quench_runtime::execute::set_property(function.clone(), "valueOf", function)
}

fn os_get_priority(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if !matches!(value, Value::Number(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "pid must be a number",
            )));
        }
    }
    Ok(Value::Number(NODE_PRIORITY.with(Cell::get) as f64))
}

fn os_set_priority(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments
        .first()
        .is_some_and(|value| !matches!(value, Value::Number(_)))
        || arguments
            .get(1)
            .is_some_and(|value| !matches!(value, Value::Number(_)))
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "pid and priority must be numbers",
        )));
    }
    if let Some(Value::Number(value)) = arguments.get(1) {
        NODE_PRIORITY.with(|priority| priority.set(*value as i32));
    }
    Ok(Value::Undefined)
}

fn os_string_function(kind: u16) -> Value {
    let function = capability_function(HostCapabilityKind::Custom(kind));
    quench_runtime::execute::set_property(function.clone(), "toString", function)
}

fn os_platform() -> Result<Value, VmError> {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        value => value,
    };
    Ok(Value::String(platform.into()))
}

fn os_arch() -> Result<Value, VmError> {
    Ok(Value::String(std::env::consts::ARCH.into()))
}

fn os_tmpdir(receiver: Option<&Value>) -> Result<Value, VmError> {
    let env = receiver
        .and_then(|receiver| quench_runtime::execute::get_property_result(receiver, "\0env").ok())
        .unwrap_or(Value::Undefined);
    for key in ["TMPDIR", "TMP", "TEMP"] {
        if let Ok(Value::String(value)) = quench_runtime::execute::get_property_result(&env, key) {
            if !value.is_empty() {
                let value = if value.len() > 1 && value.ends_with('/') {
                    &value[..value.len() - 1]
                } else {
                    &value
                };
                return Ok(Value::String(value.to_owned().into()));
            }
        }
    }
    Ok(Value::String(
        std::env::temp_dir().to_string_lossy().into_owned().into(),
    ))
}

fn os_homedir() -> Result<Value, VmError> {
    if let Some(binding) = NODE_OS_BINDING.with(|stored| stored.borrow().clone()) {
        let context = quench_runtime::host_api::object(vec![]);
        if let Ok(get_home) =
            quench_runtime::execute::get_property_result(&binding, "getHomeDirectory")
        {
            let _ = quench_runtime::execute::call(
                &get_home,
                &Value::Undefined,
                std::slice::from_ref(&context),
            );
            if matches!(
                quench_runtime::execute::get_property_result(&context, "syscall"),
                Ok(Value::String(_))
            ) {
                NODE_OS_HOME_ERROR.with(|stored| stored.replace(Some(context)));
            }
        }
    }
    if let Some(context) = NODE_OS_HOME_ERROR.with(|stored| stored.borrow_mut().take()) {
        let syscall = quench_runtime::execute::get_property_result(&context, "syscall")
            .unwrap_or(Value::Undefined);
        let code = quench_runtime::execute::get_property_result(&context, "code")
            .unwrap_or(Value::Undefined);
        let message = quench_runtime::execute::get_property_result(&context, "message")
            .unwrap_or(Value::Undefined);
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![(
            "message".into(),
            Value::String(
                format!(
                    "A system error occurred: {} returned {} ({})",
                    safe_value_string(&syscall),
                    safe_value_string(&code),
                    safe_value_string(&message)
                )
                .into(),
            ),
        )])));
    }
    Ok(Value::String(
        std::env::var("HOME").unwrap_or_else(|_| "/".into()),
    ))
}

fn module_api() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "builtinModules".into(),
            quench_runtime::host_api::array(vec![Value::String("fs".into())]),
        ),
        (
            "isBuiltin".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::ModuleIsBuiltin)),
        ),
        (
            "createRequire".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleCreateRequire,
            )),
        ),
        (
            "findSourceMap".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleFindSourceMap,
            )),
        ),
        (
            "syncBuiltinESMExports".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::ModuleSyncBuiltinExports,
            )),
        ),
    ])
}

fn module_is_builtin(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(matches!(
        name.as_str(),
        "assert"
            | "buffer"
            | "crypto"
            | "events"
            | "fs"
            | "http"
            | "module"
            | "net"
            | "os"
            | "path"
            | "stream"
            | "url"
            | "util"
    )))
}

fn os_extra(kind: HostCapabilityKind) -> Result<Value, VmError> {
    match kind {
        HostCapabilityKind::Custom(CapabilityName::OsCpus) => {
            Ok(quench_runtime::host_api::array(vec![]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsFreemem)
        | HostCapabilityKind::Custom(CapabilityName::OsTotalmem) => Ok(Value::Number(1.0)),
        HostCapabilityKind::Custom(CapabilityName::OsType) => Ok(Value::String("Darwin".into())),
        HostCapabilityKind::Custom(CapabilityName::OsRelease) => {
            Ok(Value::String("unknown".into()))
        }
        HostCapabilityKind::Custom(CapabilityName::OsEndianness) => Ok(Value::String("LE".into())),
        HostCapabilityKind::Custom(CapabilityName::OsLoadavg) => {
            Ok(quench_runtime::host_api::array(vec![
                Value::Number(0.0),
                Value::Number(0.0),
                Value::Number(0.0),
            ]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsNetworkInterfaces) => {
            Ok(quench_runtime::host_api::object(vec![(
                "lo".into(),
                quench_runtime::host_api::array(vec![quench_runtime::host_api::object(vec![
                    ("address".into(), Value::String("127.0.0.1".into())),
                    ("netmask".into(), Value::String("255.0.0.0".into())),
                    ("family".into(), Value::String("IPv4".into())),
                    ("mac".into(), Value::String("00:00:00:00:00:00".into())),
                    ("internal".into(), Value::Boolean(true)),
                    ("cidr".into(), Value::String("127.0.0.1/8".into())),
                ])]),
            )]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => {
            Ok(quench_runtime::host_api::object(vec![
                (
                    "username".into(),
                    Value::String(
                        std::env::var("USER")
                            .unwrap_or_else(|_| "user".into())
                            .into(),
                    ),
                ),
                ("uid".into(), Value::Number(0.0)),
                ("gid".into(), Value::Number(0.0)),
                ("shell".into(), Value::String("/bin/sh".into())),
                (
                    "homedir".into(),
                    Value::String(std::env::var("HOME").unwrap_or_else(|_| "/".into()).into()),
                ),
            ]))
        }
        _ => Err(VmError::NotCallable),
    }
}

fn safe_value_string(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) if value.starts_with("Symbol.") => {
            let name = value
                .split('\0')
                .next()
                .unwrap_or("Symbol")
                .strip_prefix("Symbol.")
                .unwrap_or("");
            format!("Symbol({name})")
        }
        Value::String(value) => value.clone(),
        Value::BigInt(value) => format!("{value}n"),
        Value::Array(_) => "[Array]".into(),
        Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
        Value::Function(_) | Value::BoundFunction(_) => "[Function]".into(),
        _ => "[Value]".into(),
    }
}
