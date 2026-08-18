//! `os` module — pure Rust operating-system info.

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
    platform()
}

pub fn release() -> String {
    "quench-0.1.0".into()
}

pub fn eol() -> String {
    if cfg!(target_os = "windows") {
        "\r\n".into()
    } else {
        "\n".into()
    }
}

pub fn freemem() -> f64 {
    sys_memory().map(|(t, a)| t.saturating_sub(a)).unwrap_or(0) as f64
}
pub fn totalmem() -> f64 {
    sys_memory().map(|(t, _)| t).unwrap_or(0) as f64
}

fn sys_memory() -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        return read_meminfo();
    }
    #[cfg(target_os = "macos")]
    {
        return read_meminfo_macos();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_meminfo() -> Option<(u64, u64)> {
    let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total: Option<u64> = None;
    let mut avail: Option<u64> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_kb(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail = parse_kb(rest);
        }
    }
    Some((total? * 1024, avail? * 1024))
}

#[cfg(target_os = "linux")]
fn parse_kb(s: &str) -> Option<u64> {
    s.trim().split_whitespace().next()?.parse().ok()
}

#[cfg(target_os = "macos")]
fn read_meminfo_macos() -> Option<(u64, u64)> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    extern "C" {
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    let total = sysctl_u64("hw.memsize")?;
    let page = sysctl_u64("hw.pagesize")?;
    // VM stats via host_statistics64: keep it simple, return 0 for free.
    let _ = (total, page);
    let mut vmstat: MaybeUninit<[i32; 6]> = MaybeUninit::uninit();
    let mut size = std::mem::size_of_val(&vmstat);
    let res = unsafe {
        sysctlbyname(
            CStr::from_bytes_with_nul(b"vm.stat\0").ok()?.as_ptr(),
            vmstat.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    let free = if res == 0 {
        let v = unsafe { vmstat.assume_init() };
        let pages_free = v[1] as u64;
        let pages_inactive = v[3] as u64;
        let page = sysctl_u64("hw.pagesize").unwrap_or(4096);
        (pages_free + pages_inactive) * page
    } else {
        0
    };
    Some((total, free))
}

#[cfg(target_os = "macos")]
fn sysctl_time_t(name: &str) -> Option<i64> {
    use std::ffi::CString;
    extern "C" {
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    let cname = CString::new(name).ok()?;
    let mut buf = [0u8; 16];
    let mut size = buf.len();
    let res = unsafe {
        sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if res != 0 || size < 8 {
        return None;
    }
    let secs = i64::from_ne_bytes(buf[0..8].try_into().ok()?);
    Some(secs)
}

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    use std::ffi::CString;
    extern "C" {
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    let cname = CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let res = unsafe {
        sysctlbyname(
            cname.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if res == 0 {
        Some(value)
    } else {
        None
    }
}

pub fn hostname(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String("quench-node".into()))
}

pub fn tmpdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    Ok(Value::String(dir))
}

pub fn homedir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/".into());
    Ok(Value::String(dir))
}

pub fn uptime(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let secs = read_uptime_secs();
    Ok(Value::Number(secs as f64))
}

fn read_uptime_secs() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = std::fs::read_to_string("/proc/uptime") {
            if let Some(first) = raw.split_whitespace().next() {
                if let Ok(secs) = first.parse::<f64>() {
                    return secs as u64;
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(boottime) = sysctl_time_t("kern.boottime") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            return now.saturating_sub(boottime).max(0) as u64;
        }
    }
    let start = std::time::Instant::now();
    let _ = start;
    0
}

pub fn cpus(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let n = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    Ok(host_api::array(vec![Value::Number(n)]))
}

pub fn loadavg(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    // Best-effort: on Linux, read /proc/loadavg. On macOS, parse
    // `sysctl kern.loadavg` (returns 3 doubles packed in a 12-byte
    // buffer).
    let load = read_loadavg().unwrap_or([0.0, 0.0, 0.0]);
    Ok(host_api::array(vec![
        Value::Number(load[0]),
        Value::Number(load[1]),
        Value::Number(load[2]),
    ]))
}

#[cfg(target_os = "linux")]
fn read_loadavg() -> Option<[f64; 3]> {
    let raw = std::fs::read_to_string("/proc/loadavg").ok()?;
    let mut parts = raw.split_whitespace();
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    Some([a, b, c])
}

#[cfg(target_os = "macos")]
fn read_loadavg() -> Option<[f64; 3]> {
    use std::ffi::CString;
    extern "C" {
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    // On macOS, `vm.loadavg` returns a `struct loadavg`:
    //   uint32_t ldavg[3];
    //   int      ldfscale;
    // The scale converts each ldavg[i] to a double via ldavg[i] / ldfscale.
    let cname = CString::new("vm.loadavg").ok()?;
    let mut buf = [0u8; 32];
    let mut size = buf.len();
    let res = unsafe {
        sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if res != 0 || size < 20 {
        return None;
    }
    let ldavg = [
        u32::from_ne_bytes(buf[0..4].try_into().ok()?) as f64,
        u32::from_ne_bytes(buf[4..8].try_into().ok()?) as f64,
        u32::from_ne_bytes(buf[8..12].try_into().ok()?) as f64,
    ];
    // On Darwin 25 the scale lives at offset 16 (after 4 bytes of
    // padding following the 3 u32s).
    let ldfscale = i32::from_ne_bytes(buf[16..20].try_into().ok()?) as f64;
    if ldfscale == 0.0 {
        return None;
    }
    Some([
        ldavg[0] / ldfscale,
        ldavg[1] / ldfscale,
        ldavg[2] / ldfscale,
    ])
}

pub fn network_interfaces(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    // Best-effort: enumerate `lo`, common interfaces if any.
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

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
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
    out.push(("platform".to_string(), Value::String(platform())));
    out.push(("arch".to_string(), Value::String(arch())));
    out.push(("hostname".to_string(), Value::String("quench-node".into())));
    out.push(("type".to_string(), Value::String(type_str())));
    out.push(("release".to_string(), Value::String(release())));
    out.push(("EOL".to_string(), Value::String(eol())));
    out.push((
        "homedir".to_string(),
        Value::String(std::env::var("HOME").unwrap_or_else(|_| "/".into())),
    ));
    out.push((
        "tmpdir".to_string(),
        Value::String(std::env::temp_dir().to_string_lossy().into_owned()),
    ));
}

fn os_capability_props(out: &mut Vec<(String, Value)>) {
    out.push((
        "uptime".to_string(),
        crate::host::capability(crate::registry::SPEC_OS_UPTIME),
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
}
