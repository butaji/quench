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
                    quench_runtime::host_api::object(vec![
                        ("EPERM".into(), Value::Number(1.0)),
                        ("ENOENT".into(), Value::Number(2.0)),
                        ("EINTR".into(), Value::Number(4.0)),
                        ("EIO".into(), Value::Number(5.0)),
                        ("EACCES".into(), Value::Number(13.0)),
                        ("EEXIST".into(), Value::Number(17.0)),
                        ("ENOTDIR".into(), Value::Number(20.0)),
                        ("EISDIR".into(), Value::Number(21.0)),
                        ("EINVAL".into(), Value::Number(22.0)),
                        ("ENOSPC".into(), Value::Number(28.0)),
                        ("EPIPE".into(), Value::Number(32.0)),
                        ("ERANGE".into(), Value::Number(34.0)),
                    ]),
                ),
            ],
        )),
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
            quench_runtime::host_api::array(
                [
                    "assert", "assert/strict", "buffer", "child_process", "cluster", "console",
                    "constants", "crypto", "dgram", "dns", "domain", "events", "fs",
                    "fs/promises", "http", "http2", "https", "module", "net", "os", "path",
                    "perf_hooks", "process", "punycode", "querystring", "readline", "stream",
                    "string_decoder", "timers", "timers/promises", "tls", "trace_events", "tty",
                    "url", "util", "v8", "vm", "worker_threads", "zlib", "test",
                ]
                .iter()
                .map(|name| Value::String((*name).into()))
                .collect(),
            ),
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
            let mut system = sysinfo::System::new();
            system.refresh_cpu_all();
            let cpus = system.cpus().iter().map(|cpu| {
                quench_runtime::host_api::object(vec![
                    ("model".into(), Value::String(cpu.brand().into())),
                    ("speed".into(), Value::Number(cpu.frequency() as f64)),
                    ("times".into(), quench_runtime::host_api::object(vec![
                        ("user".into(), Value::Number(cpu.cpu_usage() as f64)),
                        ("nice".into(), Value::Number(0.0)),
                        ("sys".into(), Value::Number(0.0)),
                        ("idle".into(), Value::Number(0.0)),
                        ("irq".into(), Value::Number(0.0)),
                    ])),
                ])
            }).collect();
            Ok(quench_runtime::host_api::array(cpus))
        }
        HostCapabilityKind::Custom(CapabilityName::OsFreemem) =>
            Ok(Value::Number(sysinfo::System::new_all().available_memory() as f64)),
        HostCapabilityKind::Custom(CapabilityName::OsTotalmem) =>
            Ok(Value::Number(sysinfo::System::new_all().total_memory() as f64)),
        HostCapabilityKind::Custom(CapabilityName::OsType) => Ok(Value::String(
            if cfg!(target_os = "macos") { "Darwin" } else if cfg!(target_os = "linux") { "Linux" } else { "Unknown" }.into())),
        HostCapabilityKind::Custom(CapabilityName::OsRelease) =>
            Ok(Value::String(sysinfo::System::kernel_version().unwrap_or_else(|| "unknown".into()).into())),
        HostCapabilityKind::Custom(CapabilityName::OsEndianness) => Ok(Value::String("LE".into())),
        HostCapabilityKind::Custom(CapabilityName::OsLoadavg) => {
            let l = sysinfo::System::load_average();
            Ok(quench_runtime::host_api::array(vec![Value::Number(l.one), Value::Number(l.five), Value::Number(l.fifteen)]))
        }
        HostCapabilityKind::Custom(CapabilityName::OsUptime) =>
            Ok(Value::Number(sysinfo::System::uptime() as f64)),
        HostCapabilityKind::Custom(CapabilityName::OsNetworkInterfaces) => os_network_interfaces(),
        HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => {
            let uid = unsafe { libc::getuid() };
            let gid = unsafe { libc::getgid() };
            Ok(quench_runtime::host_api::object(vec![
                ("username".into(), Value::String(std::env::var("USER").unwrap_or_else(|_| "unknown".into()).into())),
                ("uid".into(), Value::Number(uid as f64)), ("gid".into(), Value::Number(gid as f64)),
                ("shell".into(), Value::String(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()).into())),
                ("homedir".into(), Value::String(std::env::var("HOME").unwrap_or_else(|_| "/".into()).into())),
            ]))
        }
        _ => Err(VmError::NotCallable),
    }
}

fn os_network_interfaces() -> Result<Value, VmError> {
    use std::ffi::CStr;
    let mut raw = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut raw) } != 0 { return Ok(quench_runtime::host_api::object(vec![])); }
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut p = raw;
    while !p.is_null() {
        unsafe {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET {
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                let ip = std::net::Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)).to_string();
                let internal = ip == "127.0.0.1";
                let entry = quench_runtime::host_api::object(vec![
                    ("address".into(), Value::String(ip.clone().into())), ("netmask".into(), Value::String(if internal {"255.0.0.0"} else {"0.0.0.0"}.into())),
                    ("family".into(), Value::String("IPv4".into())), ("mac".into(), Value::String("".into())),
                    ("internal".into(), Value::Boolean(internal)), ("cidr".into(), Value::String(format!("{ip}/{}", if internal {8} else {0}).into())),
                ]);
                if let Some((_, Value::Array(items))) = out.iter_mut().find(|(n, _)| n == &name) { items.push(entry); }
                else { out.push((name, quench_runtime::host_api::array(vec![entry]))); }
            }
            p = ifa.ifa_next;
        }
    }
    unsafe { libc::freeifaddrs(raw); }
    Ok(quench_runtime::host_api::object(out))
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
