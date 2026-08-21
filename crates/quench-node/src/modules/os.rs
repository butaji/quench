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
    let dir = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    Ok(Value::String(dir))
}

pub fn tmpdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    Ok(Value::String(dir))
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
            arr.push(host_api::object(vec![
                ("address".to_string(), Value::String(addr)),
                ("family".to_string(), Value::String("IPv4".to_string())),
                ("internal".to_string(), Value::Boolean(false)),
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
}

fn os_capability_props(out: &mut Vec<(String, Value)>) {
    os_identity_caps(out);
    os_path_caps(out);
    os_resource_caps(out);
}

fn os_identity_caps(out: &mut Vec<(String, Value)>) {
    use crate::registry::*;
    out.push(("platform".into(), crate::host::capability(SPEC_OS_PLATFORM)));
    out.push(("arch".into(), crate::host::capability(SPEC_OS_ARCH)));
    out.push(("hostname".into(), crate::host::capability(SPEC_OS_HOSTNAME)));
    out.push(("type".into(), crate::host::capability(SPEC_OS_TYPE)));
    out.push(("release".into(), crate::host::capability(SPEC_OS_RELEASE)));
}

fn os_path_caps(out: &mut Vec<(String, Value)>) {
    use crate::registry::*;
    out.push(("homedir".into(), crate::host::capability(SPEC_OS_HOMEDIR)));
    out.push(("tmpdir".into(), crate::host::capability(SPEC_OS_TMPDIR)));
}

fn os_resource_caps(out: &mut Vec<(String, Value)>) {
    use crate::registry::*;
    out.push(("uptime".into(), crate::host::capability(SPEC_OS_UPTIME)));
    out.push(("totalmem".into(), crate::host::capability(SPEC_OS_TOTALMEM)));
    out.push(("freemem".into(), crate::host::capability(SPEC_OS_FREEMEM)));
    out.push(("cpus".into(), crate::host::capability(SPEC_OS_CPUS)));
    out.push(("loadavg".into(), crate::host::capability(SPEC_OS_LOADAVG)));
    out.push(("networkInterfaces".into(), crate::host::capability(SPEC_OS_NETWORKINTERFACES)));
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
