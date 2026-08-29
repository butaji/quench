//! `os` module — pure-Rust system info via `sysinfo`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn platform() -> String {
    if cfg!(target_os = "macos") {
        "darwin".into()
    } else if cfg!(target_os = "linux") {
        "linux".into()
    } else if cfg!(target_os = "windows") {
        "win32".into()
    } else {
        "unknown".into()
    }
}

pub fn arch() -> String {
    if cfg!(target_arch = "x86_64") {
        "x64".into()
    } else if cfg!(target_arch = "aarch64") {
        "arm64".into()
    } else {
        "unknown".into()
    }
}

pub fn type_str() -> String {
    if cfg!(target_os = "macos") {
        "Darwin".into()
    } else if cfg!(target_os = "linux") {
        "Linux".into()
    } else if cfg!(target_os = "windows") {
        "Windows_NT".into()
    } else {
        "Unknown".into()
    }
}

pub fn release() -> String {
    sysinfo_release().unwrap_or_else(|| "unknown".into())
}

pub fn eol() -> String {
    if cfg!(target_os = "windows") {
        "\r\n".into()
    } else {
        "\n".into()
    }
}

pub fn endianness(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        if cfg!(target_endian = "little") {
            "LE"
        } else {
            "BE"
        }
        .into(),
    ))
}

pub fn version(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(format!("{} {}", type_str(), release())))
}

pub fn machine(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(arch()))
}

pub fn user_info(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    let homedir = env_value("HOME").unwrap_or_else(|| "/".into());
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let encoding = args.first().and_then(|options| match options {
        Value::Object(_) => match quench_runtime::execute::get_property(options, "encoding") {
            Value::String(value) => Some(value),
            _ => None,
        },
        _ => None,
    });
    let fields = vec![
        ("username", username),
        ("homedir", homedir),
        ("shell", shell),
    ];
    let mut out = Vec::new();
    let buffer_from = if encoding.as_deref() == Some("buffer") {
        let global = quench_runtime::vm::current_global_object();
        let buffer = quench_runtime::execute::get_property(&global, "Buffer");
        match quench_runtime::execute::get_property(&buffer, "from") {
            value if quench_runtime::is_callable(&value) => Some(value),
            _ => None,
        }
    } else {
        None
    };
    for (name, value) in fields {
        out.push((
            name.into(),
            if let Some(from) = &buffer_from {
                quench_runtime::execute::call(from, &Value::Undefined, &[Value::String(value)])?
            } else {
                Value::String(value)
            },
        ));
    }
    out.extend([
        ("uid".into(), Value::Number(0.0)),
        ("gid".into(), Value::Number(0.0)),
    ]);
    Ok(crate::host::namespace_object_from_pairs(out))
}

pub fn cpus(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let cpus = sysinfo_cpus();
    let mut out = Vec::new();
    for cpu in cpus {
        out.push(host_api::object(vec![
            ("model".to_string(), Value::String(cpu.brand)),
            ("speed".to_string(), Value::Number(cpu.frequency as f64)),
            (
                "times".to_string(),
                host_api::object(vec![
                    ("user".to_string(), Value::Number(cpu.user_jiffies as f64)),
                    ("nice".to_string(), Value::Number(cpu.nice_jiffies as f64)),
                    ("sys".to_string(), Value::Number(cpu.system_jiffies as f64)),
                    ("idle".to_string(), Value::Number(cpu.idle_jiffies as f64)),
                    ("irq".to_string(), Value::Number(0.0)),
                ]),
            ),
        ]));
    }
    if out.is_empty() {
        out.push(host_api::object(vec![
            ("model".into(), Value::String("unknown".into())),
            ("speed".into(), Value::Number(0.0)),
            (
                "times".into(),
                host_api::object(vec![
                    ("user".into(), Value::Number(0.0)),
                    ("nice".into(), Value::Number(0.0)),
                    ("sys".into(), Value::Number(0.0)),
                    ("idle".into(), Value::Number(0.0)),
                    ("irq".into(), Value::Number(0.0)),
                ]),
            ),
        ]));
    }
    Ok(host_api::array(out))
}

struct CpuInfo {
    brand: String,
    frequency: u64,
    user_jiffies: u64,
    nice_jiffies: u64,
    system_jiffies: u64,
    idle_jiffies: u64,
}

fn sysinfo_cpus() -> Vec<CpuInfo> {
    let mut s = sysinfo::System::new();
    s.refresh_cpu_all();
    s.cpus()
        .iter()
        .map(|cpu| CpuInfo {
            brand: cpu.brand().to_string(),
            frequency: cpu.frequency(),
            user_jiffies: cpu.cpu_usage() as u64,
            nice_jiffies: 0,
            system_jiffies: 0,
            idle_jiffies: 0,
        })
        .collect()
}

pub fn totalmem(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(sysinfo_total() as f64))
}

pub fn freemem(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(sysinfo_avail() as f64))
}

pub fn loadavg(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let (a, b, c) = sysinfo_loadavg();
    Ok(host_api::array(vec![
        Value::Number(a),
        Value::Number(b),
        Value::Number(c),
    ]))
}

pub fn uptime(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(sysinfo_uptime() as f64))
}

pub fn hostname(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let name = sysinfo_hostname().unwrap_or_else(|| "quench-node".into());
    Ok(Value::String(name))
}

pub fn homedir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir =
        env_value("HOME").unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    Ok(Value::String(dir))
}

pub fn tmpdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = env_value("TMPDIR")
        .filter(|value| !value.is_empty())
        .or_else(|| env_value("TMP").filter(|value| !value.is_empty()))
        .or_else(|| env_value("TEMP").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                "C:\\Windows\\Temp".into()
            } else {
                "/tmp".into()
            }
        });
    let dir = if dir.len() > 1 {
        dir.trim_end_matches('/').to_string()
    } else {
        dir
    };
    Ok(Value::String(dir))
}

fn env_value(name: &str) -> Option<String> {
    let global = quench_runtime::vm::current_global_object();
    let process = quench_runtime::execute::get_property(&global, "process");
    let env = quench_runtime::execute::get_property(&process, "env");
    match quench_runtime::execute::get_property(&env, name) {
        Value::String(value) => Some(value),
        _ => None,
    }
}

pub fn network_interfaces(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let ifaces = read_ifaddrs();
    let mut out = Vec::new();
    for (name, addrs) in ifaces {
        let mut arr = Vec::new();
        for addr in addrs {
            let loopback = addr.starts_with("127.");
            let netmask = if loopback { "255.0.0.0" } else { "0.0.0.0" };
            let prefix = if loopback { 8 } else { 0 };
            arr.push(host_api::object(vec![
                ("address".to_string(), Value::String(addr.clone())),
                ("netmask".to_string(), Value::String(netmask.into())),
                ("family".to_string(), Value::String("IPv4".to_string())),
                (
                    "mac".to_string(),
                    Value::String(if loopback {
                        "00:00:00:00:00:00".into()
                    } else {
                        "00:00:00:00:00:00".into()
                    }),
                ),
                ("internal".to_string(), Value::Boolean(loopback)),
                (
                    "cidr".to_string(),
                    Value::String(format!("{addr}/{prefix}")),
                ),
            ]));
        }
        out.push((name, host_api::array(arr)));
    }
    Ok(host_api::object(out))
}

#[cfg(unix)]
fn read_ifaddrs() -> Vec<(String, Vec<String>)> {
    use std::ffi::CStr;
    use std::net::Ipv4Addr;
    extern "C" {
        fn getifaddrs(ifap: *mut *mut libc::ifaddrs) -> i32;
        fn freeifaddrs(ifa: *mut libc::ifaddrs);
    }
    let mut raw: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { getifaddrs(&mut raw) } != 0 {
        return Vec::new();
    }
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut p = raw;
    while !p.is_null() {
        unsafe {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == 2 {
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                let sin = ifa.ifa_addr as *const libc::sockaddr_in;
                let octets = (*sin).sin_addr.s_addr.to_ne_bytes();
                let ip = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string();
                out.push((name, vec![ip]));
            }
            p = ifa.ifa_next;
        }
    }
    unsafe {
        freeifaddrs(raw);
    }
    out
}

#[cfg(not(unix))]
fn read_ifaddrs() -> Vec<(String, Vec<String>)> {
    Vec::new()
}

pub fn build() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    os_static_props(&mut out);
    os_capability_props(&mut out);
    out
}

fn os_static_props(out: &mut Vec<(String, Value)>) {
    // Node exposes type/platform/arch/release/hostname as functions (see
    // os_capability_props); only the constant `EOL` is a property.
    out.push(("EOL".to_string(), Value::String(eol())));
    out.push((
        "devNull".to_string(),
        Value::String(
            if cfg!(target_os = "windows") {
                "\\\\.\\nul"
            } else {
                "/dev/null"
            }
            .into(),
        ),
    ));
    out.push((
        "constants".to_string(),
        frozen_object(vec![
            (
                "priority".to_string(),
                frozen_object(vec![
                    ("PRIORITY_LOW".to_string(), Value::Number(19.0)),
                    ("PRIORITY_BELOW_NORMAL".to_string(), Value::Number(10.0)),
                    ("PRIORITY_NORMAL".to_string(), Value::Number(0.0)),
                    ("PRIORITY_ABOVE_NORMAL".to_string(), Value::Number(-7.0)),
                    ("PRIORITY_HIGH".to_string(), Value::Number(-14.0)),
                    ("PRIORITY_HIGHEST".to_string(), Value::Number(-20.0)),
                ]),
            ),
            (
                "errno".to_string(),
                frozen_object(vec![
                    ("ENOENT".to_string(), Value::Number(2.0)),
                    ("EACCES".to_string(), Value::Number(13.0)),
                    ("EEXIST".to_string(), Value::Number(17.0)),
                ]),
            ),
            (
                "signals".to_string(),
                frozen_object(vec![
                    ("SIGHUP".into(), Value::Number(1.0)),
                    ("SIGINT".into(), Value::Number(2.0)),
                    ("SIGABRT".into(), Value::Number(6.0)),
                    ("SIGKILL".into(), Value::Number(9.0)),
                    ("SIGTERM".into(), Value::Number(15.0)),
                ]),
            ),
        ]),
    ));
}

fn frozen_object(properties: Vec<(String, Value)>) -> Value {
    let value = host_api::object(properties);
    let global = quench_runtime::vm::current_global_object();
    let freeze = quench_runtime::execute::get_property(
        &&quench_runtime::execute::get_property(&global, "Object"),
        "freeze",
    );
    quench_runtime::execute::call(&freeze, &Value::Undefined, &[value.clone()]).unwrap_or(value)
}

fn os_capability_props(out: &mut Vec<(String, Value)>) {
    out.push((
        "availableParallelism".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_AVAILABLE_PARALLELISM),
    ));
    out.push((
        "platform".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_PLATFORM),
    ));
    out.push((
        "arch".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_ARCH),
    ));
    out.push((
        "hostname".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_HOSTNAME),
    ));
    out.push((
        "type".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_TYPE),
    ));
    out.push((
        "release".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_RELEASE),
    ));
    out.push((
        "uptime".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_UPTIME),
    ));
    out.push((
        "homedir".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_HOMEDIR),
    ));
    out.push((
        "tmpdir".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_TMPDIR),
    ));
    out.push((
        "totalmem".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_TOTALMEM),
    ));
    out.push((
        "freemem".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_FREEMEM),
    ));
    out.push((
        "cpus".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_CPUS),
    ));
    out.push((
        "loadavg".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_LOADAVG),
    ));
    out.push((
        "networkInterfaces".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_NETWORKINTERFACES),
    ));
    out.push((
        "endianness".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_ENDIANNESS),
    ));
    out.push((
        "version".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_VERSION),
    ));
    out.push((
        "machine".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_MACHINE),
    ));
    out.push((
        "userInfo".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_USERINFO),
    ));
    out.push((
        "getPriority".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_GET_PRIORITY),
    ));
    out.push((
        "setPriority".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_SET_PRIORITY),
    ));
    for (_, value) in out.iter_mut() {
        if !quench_runtime::is_callable(value) {
            continue;
        }
        let function = value.clone();
        let function = quench_runtime::execute::set_property(function, "toString", value.clone());
        *value = quench_runtime::execute::set_property(function, "valueOf", value.clone());
    }
}

pub fn available_parallelism(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f64,
    ))
}

// ---- sysinfo-backed helpers ----
//
// `sysinfo::System` is the canonical cross-platform source for
// `os` data. We construct one per call so the host stays
// stateless; the kernel call is cheap.

fn sysinfo_total() -> u64 {
    sysinfo::System::new_all().total_memory()
}

fn sysinfo_avail() -> u64 {
    sysinfo::System::new_all().available_memory()
}

fn sysinfo_loadavg() -> (f64, f64, f64) {
    let load = sysinfo::System::load_average();
    (load.one, load.five, load.fifteen)
}

fn sysinfo_uptime() -> u64 {
    sysinfo::System::uptime()
}

fn sysinfo_hostname() -> Option<String> {
    sysinfo::System::host_name()
}

fn sysinfo_release() -> Option<String> {
    sysinfo::System::kernel_version()
}
