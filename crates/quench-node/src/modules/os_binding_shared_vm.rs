//! Native OS facts consumed by the unmodified configured Node OS module.
//!
//! Every operation mirrors the source libuv reads for Node on the host
//! platform, so the projected values keep Node's identities and units.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::ffi::CStr;
type Context<'a> = NativeContext<'a, NodeHost>;

/// Node's `os.constants.signals` inventory in declaration order. Linux shares
/// the process-delivery table; Darwin numbers the POSIX names differently and
/// adds `SIGINFO`, so it keeps its own inventory.
#[cfg(target_os = "linux")]
const SIGNAL_CONSTANTS: &[(&str, libc::c_int)] = super::process_signals_state::SIGNALS;
#[cfg(target_os = "macos")]
const SIGNAL_CONSTANTS: &[(&str, libc::c_int)] = &[
    ("SIGHUP", libc::SIGHUP),
    ("SIGINT", libc::SIGINT),
    ("SIGQUIT", libc::SIGQUIT),
    ("SIGILL", libc::SIGILL),
    ("SIGTRAP", libc::SIGTRAP),
    ("SIGABRT", libc::SIGABRT),
    ("SIGIOT", libc::SIGIOT),
    ("SIGBUS", libc::SIGBUS),
    ("SIGFPE", libc::SIGFPE),
    ("SIGKILL", libc::SIGKILL),
    ("SIGUSR1", libc::SIGUSR1),
    ("SIGSEGV", libc::SIGSEGV),
    ("SIGUSR2", libc::SIGUSR2),
    ("SIGPIPE", libc::SIGPIPE),
    ("SIGALRM", libc::SIGALRM),
    ("SIGTERM", libc::SIGTERM),
    ("SIGCHLD", libc::SIGCHLD),
    ("SIGCONT", libc::SIGCONT),
    ("SIGSTOP", libc::SIGSTOP),
    ("SIGTSTP", libc::SIGTSTP),
    ("SIGTTIN", libc::SIGTTIN),
    ("SIGTTOU", libc::SIGTTOU),
    ("SIGURG", libc::SIGURG),
    ("SIGXCPU", libc::SIGXCPU),
    ("SIGXFSZ", libc::SIGXFSZ),
    ("SIGVTALRM", libc::SIGVTALRM),
    ("SIGPROF", libc::SIGPROF),
    ("SIGWINCH", libc::SIGWINCH),
    ("SIGIO", libc::SIGIO),
    ("SIGINFO", libc::SIGINFO),
    ("SIGSYS", libc::SIGSYS),
];

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn text(c: &mut Context<'_>, object: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let value = c.string_rooted(value);
    put(c, object, name, value)
}
fn number(c: &mut Context<'_>, object: RootId, name: &str, value: f64) -> Result<(), RootedError> {
    let value = c.number(value);
    put(c, object, name, value)
}
fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}
fn binding(c: &mut Context<'_>, functions: &[(&str, &str)]) -> Result<RootId, RootedError> {
    let result = c.object_rooted()?;
    for &(name, operation) in functions {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), "")?;
        put(c, result, name, function)?;
    }
    Ok(result)
}
pub(crate) fn os_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:os", build_os_binding)
}
pub(crate) fn credentials_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:credentials", build_credentials_binding)
}
pub(crate) fn constants(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:os.constants", build_constants)
}
fn build_os_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let result = binding(
        c,
        &[
            ("getAvailableParallelism", "osAvailableParallelism"),
            ("getCPUs", "osBindingCpus"),
            ("getFreeMem", "osFreemem"),
            ("getHomeDirectory", "osBindingHome"),
            ("getHostname", "osBindingHostname"),
            ("getInterfaceAddresses", "osBindingInterfaces"),
            ("getLoadAvg", "osBindingLoadavg"),
            ("getPriority", "osBindingGetPriority"),
            ("getOSInformation", "osBindingInformation"),
            ("getTotalMem", "osTotalmem"),
            ("getUserInfo", "osBindingUserInfo"),
            ("getUptime", "osBindingUptime"),
            ("setPriority", "osBindingSetPriority"),
        ],
    )?;
    let endian = c.boolean(cfg!(target_endian = "big"));
    put(c, result, "isBigEndian", endian)?;
    Ok(result)
}
fn build_credentials_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let binding=binding(c, &[("getTempDir", "osBindingTempdir")])?;
    let supported=c.boolean(cfg!(unix));
    put(c,binding,"implementsPosixCredentials",supported)?;
    Ok(binding)
}
fn system_error(
    c: &mut Context<'_>,
    ctx: RootId,
    errno: i32,
    syscall: &str,
) -> Result<(), RootedError> {
    let code = super::util_system_error_shared_vm::system_error_name(-f64::from(errno));
    number(c, ctx, "errno", -f64::from(errno))?;
    text(c, ctx, "code", &code)?;
    // libc's message is copied before any other host operation can overwrite it.
    let message = unsafe { CStr::from_ptr(libc::strerror(errno)) }
        .to_string_lossy()
        .to_lowercase();
    text(c, ctx, "message", &message)?;
    text(c, ctx, "syscall", syscall)
}
macro_rules! operation {
    ($name:ident, $c:ident, $args:ident, $body:block) => {
        pub(crate) fn $name($c: &mut Context<'_>, _: RootId, $args: &[RootId]) -> Result<RootId, RootedError> $body
    };
}
operation!(available_parallelism, c, _args, {
    Ok(c.number(super::os_facts::available_parallelism() as f64))
});
operation!(cpus, c, _args, {
    let mut result = Vec::new();
    for cpu in super::os_facts::cpus() {
        result.push(c.string_rooted(&cpu.model));
        for value in [
            cpu.speed,
            cpu.times.user,
            cpu.times.nice,
            cpu.times.sys,
            cpu.times.idle,
            cpu.times.irq,
        ] {
            result.push(c.number(value as f64));
        }
    }
    c.array_rooted(&result)
});
operation!(information, c, args, {
    let mut info = unsafe { std::mem::zeroed::<libc::utsname>() };
    if unsafe { libc::uname(&mut info) } != 0 {
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        let ctx = argument(c, args, args.len().saturating_sub(1));
        system_error(c, ctx, errno, "uv_os_uname")?;
        return Ok(c.undefined());
    }
    let result = [&info.sysname, &info.version, &info.release, &info.machine].map(|value| {
        let value = unsafe { CStr::from_ptr(value.as_ptr()) }.to_string_lossy();
        c.string_rooted(&value)
    });
    c.array_rooted(&result)
});
operation!(tempdir, c, _args, {
    Ok(c.string_rooted(&super::os_facts::temporary_directory()))
});
operation!(hostname, c, args, {
    // Linux HOST_NAME_MAX plus the terminating byte.
    const HOSTNAME_BUFFER_BYTES: usize = 256;
    let mut bytes = [0u8; HOSTNAME_BUFFER_BYTES];
    if unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        let ctx = argument(c, args, 0);
        system_error(c, ctx, errno, "uv_os_gethostname")?;
        return Ok(c.undefined());
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    Ok(c.string_rooted(&String::from_utf8_lossy(&bytes[..length])))
});
/// Node's `os.loadavg()` always reports the one, five and fifteen minute averages.
const LOAD_AVERAGE_SAMPLES: usize = 3;

/// libuv leaves the caller's buffer alone when the platform read fails.
#[cfg(target_os = "linux")]
fn read_load_averages(averages: &mut [f64; LOAD_AVERAGE_SAMPLES]) -> bool {
    if let Some(parsed) = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|source| {
            source
                .split_whitespace()
                .take(averages.len())
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()
        })
        .filter(|parsed| parsed.len() == averages.len())
    {
        averages.copy_from_slice(&parsed);
    } else {
        let mut info = unsafe { std::mem::zeroed::<libc::sysinfo>() };
        if unsafe { libc::sysinfo(&mut info) } == 0 {
            const LINUX_LOAD_FIXED_POINT_SCALE: f64 = 65536.0;
            for (average, value) in averages.iter_mut().zip(info.loads) {
                *average = value as f64 / LINUX_LOAD_FIXED_POINT_SCALE;
            }
        }
    }
    true
}

/// Darwin exposes the same fixed-point averages libuv reads through
/// `CTL_VM`/`VM_LOADAVG`; `getloadavg` is the portable spelling of that read.
#[cfg(target_os = "macos")]
fn read_load_averages(averages: &mut [f64; LOAD_AVERAGE_SAMPLES]) -> bool {
    const LOAD_AVERAGE_SAMPLES_ARGUMENT: libc::c_int = LOAD_AVERAGE_SAMPLES as libc::c_int;
    let filled = unsafe { libc::getloadavg(averages.as_mut_ptr(), LOAD_AVERAGE_SAMPLES_ARGUMENT) };
    filled == LOAD_AVERAGE_SAMPLES_ARGUMENT
}

operation!(loadavg, c, args, {
    let mut averages = [0.0; LOAD_AVERAGE_SAMPLES];
    if read_load_averages(&mut averages) {
        let array = argument(c, args, 0);
        for (index, value) in averages.into_iter().enumerate() {
            let key = c.string_rooted(&index.to_string());
            let value = c.number(value);
            c.set_property_rooted(array, key, value, array)?;
        }
    }
    Ok(c.undefined())
});
// The CLI runs its logical main JavaScript thread on a large-stack worker.
// pid zero still denotes the CLI process, including calls using process.pid.
fn priority_pid(c: &mut Context<'_>, value: RootId) -> libc::id_t {
    let pid = c
        .rooted_value(value)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as i32;
    if pid == 0 {
        unsafe { libc::getpid() as libc::id_t }
    } else {
        pid as libc::id_t
    }
}
/// libuv distinguishes a legitimate -1 nice value from a failed call by
/// clearing errno first; each libc exposes that thread-local separately.
#[cfg(target_os = "linux")]
fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}
#[cfg(target_os = "macos")]
fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__error() }
}
operation!(get_priority, c, args, {
    let pid = argument(c, args, 0);
    let pid = priority_pid(c, pid);
    // -1 is a valid nice value; errno distinguishes it from a failed call.
    unsafe {
        *errno_location() = 0;
    }
    let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) };
    let errno = unsafe { *errno_location() };
    if errno != 0 {
        let ctx = argument(c, args, 1);
        system_error(c, ctx, errno, "uv_os_getpriority")?;
        return Ok(c.undefined());
    }
    Ok(c.number(f64::from(priority)))
});
operation!(set_priority, c, args, {
    let pid = argument(c, args, 0);
    let pid = priority_pid(c, pid);
    let priority = argument(c, args, 1);
    let priority = c
        .rooted_value(priority)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as i32;
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, priority) } != 0 {
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        let ctx = argument(c, args, 2);
        system_error(c, ctx, errno, "uv_os_setpriority")?;
        return Ok(c.number(-f64::from(errno)));
    }
    if pid == unsafe { libc::getpid() as libc::id_t } {
        // Keep future guest-created children on the logical process's niceness.
        // This worker is an implementation detail; only the requested kernel
        // operation determines the public result.
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, 0, priority);
        }
    }
    Ok(c.number(0.0))
});
operation!(home, c, args, {
    use std::os::unix::ffi::OsStrExt;
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(c.string_rooted(&String::from_utf8_lossy(home.as_bytes())));
    }
    match super::os_facts::user_info() {
        Ok(user) => Ok(c.string_rooted(&String::from_utf8_lossy(&user.homedir))),
        Err(errno) => {
            let ctx = argument(c, args, 0);
            system_error(c, ctx, errno, "uv_os_homedir")?;
            Ok(c.undefined())
        }
    }
});
operation!(user_info, c, args, {
    let options = argument(c, args, 0);
    let encoding = if c.is_object_rooted(options)? {
        let key = c.string_rooted("encoding");
        let value = c.get_property_rooted(options, key)?;
        c.string_text(value)?
    } else {
        None
    };
    let encoding = encoding
        .as_deref()
        .and_then(|name| {
            if name.eq_ignore_ascii_case("buffer") {
                Some("buffer")
            } else {
                super::buffer_codec::canonical_encoding(name)
            }
        })
        .unwrap_or("utf8");
    let user = match super::os_facts::user_info() {
        Ok(user) => user,
        Err(errno) => {
            let ctx = argument(c, args, 1);
            system_error(c, ctx, errno, "uv_os_get_passwd")?;
            return Ok(c.undefined());
        }
    };
    let result = c.null_object_rooted()?;
    number(c, result, "uid", f64::from(user.uid))?;
    number(c, result, "gid", f64::from(user.gid))?;
    for (key, bytes) in [
        ("username", user.username),
        ("homedir", user.homedir),
        ("shell", user.shell),
    ] {
        let value = if encoding == "buffer" {
            super::buffer_shared_vm::from_bytes(c, &bytes)?
        } else {
            let units = super::buffer_codec::decode_units(&bytes, encoding);
            c.string_units_rooted(&units)
        };
        put(c, result, key, value)?;
    }
    Ok(result)
});
/// libuv reads `/proc/uptime`, then falls back to the boot-time clock.
#[cfg(target_os = "linux")]
fn system_uptime() -> Result<f64, i32> {
    if let Some(value) = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|source| {
            source
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
        })
    {
        return Ok(value);
    }
    let mut time = unsafe { std::mem::zeroed::<libc::timespec>() };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO));
    }
    Ok(time.tv_sec as f64)
}

/// libuv reports whole seconds elapsed since `kern.boottime`.
#[cfg(target_os = "macos")]
fn system_uptime() -> Result<f64, i32> {
    let mut boot = unsafe { std::mem::zeroed::<libc::timeval>() };
    let mut size = std::mem::size_of_val(&boot);
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::addr_of_mut!(boot).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO));
    }
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    Ok((now - boot.tv_sec) as f64)
}

operation!(uptime, c, args, {
    match system_uptime() {
        Ok(seconds) => Ok(c.number(seconds)),
        Err(errno) => {
            let ctx = argument(c, args, 0);
            system_error(c, ctx, errno, "uv_uptime")?;
            Ok(c.undefined())
        }
    }
});

struct InterfaceList(*mut libc::ifaddrs);
impl Drop for InterfaceList {
    fn drop(&mut self) {
        unsafe { libc::freeifaddrs(self.0) };
    }
}

/// libuv exposes exactly the six Ethernet address bytes.
const LINK_LAYER_ADDRESS_BYTES: usize = 6;
/// libuv reports only interfaces that are up and have allocated resources.
const REQUIRED_INTERFACE_FLAGS: libc::c_uint =
    (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_uint;
/// Node omits `scopeid` for addresses that carry no IPv6 scope.
const NO_INTERFACE_SCOPE: f64 = -1.0;

/// One interface address in Node's flat seven-field record order.
struct InterfaceAddress<'a> {
    name: &'a [u8],
    address: String,
    netmask: String,
    family: &'static str,
    mac: [u8; LINK_LAYER_ADDRESS_BYTES],
    internal: bool,
    scope: f64,
}

fn interface_name(entry: &libc::ifaddrs) -> &[u8] {
    unsafe { CStr::from_ptr(entry.ifa_name) }.to_bytes()
}

fn interface_family(entry: &libc::ifaddrs) -> libc::c_int {
    unsafe { (*entry.ifa_addr).sa_family as libc::c_int }
}

fn interface_is_reportable(entry: &libc::ifaddrs) -> bool {
    entry.ifa_flags & REQUIRED_INTERFACE_FLAGS == REQUIRED_INTERFACE_FLAGS
}

fn interface_is_internal(entry: &libc::ifaddrs) -> bool {
    entry.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint != 0
}

unsafe fn ipv4_text(interface: *const libc::sockaddr) -> String {
    if interface.is_null() {
        return std::net::Ipv4Addr::UNSPECIFIED.to_string();
    }
    let interface = unsafe { &*interface.cast::<libc::sockaddr_in>() };
    std::net::Ipv4Addr::from(interface.sin_addr.s_addr.to_ne_bytes()).to_string()
}

unsafe fn ipv6_text(interface: *const libc::sockaddr) -> String {
    if interface.is_null() {
        return std::net::Ipv6Addr::UNSPECIFIED.to_string();
    }
    let interface = unsafe { &*interface.cast::<libc::sockaddr_in6>() };
    std::net::Ipv6Addr::from(interface.sin6_addr.s6_addr).to_string()
}

fn mac_text(mac: [u8; LINK_LAYER_ADDRESS_BYTES]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Node's `os.networkInterfaces()` reads these records seven fields at a time.
fn push_interface_address(
    c: &mut Context<'_>,
    result: &mut Vec<RootId>,
    entry: InterfaceAddress<'_>,
) -> Result<(), RootedError> {
    result.extend([
        c.string_rooted(&String::from_utf8_lossy(entry.name)),
        c.string_rooted(&entry.address),
        c.string_rooted(&entry.netmask),
        c.string_rooted(entry.family),
        c.string_rooted(&mac_text(entry.mac)),
        c.boolean(entry.internal),
        c.number(entry.scope),
    ]);
    Ok(())
}

fn interface_list(
    c: &mut Context<'_>,
    args: &[RootId],
) -> Result<Option<InterfaceList>, RootedError> {
    let mut head = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        let ctx = argument(c, args, 0);
        system_error(c, ctx, errno, "uv_interface_addresses")?;
        return Ok(None);
    }
    Ok(Some(InterfaceList(head)))
}

#[cfg(target_os = "linux")]
operation!(interfaces, c, args, {
    let Some(list) = interface_list(c, args)? else {
        return Ok(c.undefined());
    };
    let head = list.0;
    let _list = list;
    let mut macs = std::collections::HashMap::new();
    let mut item = head;
    while let Some(entry) = unsafe { item.as_ref() } {
        item = entry.ifa_next;
        if entry.ifa_addr.is_null() || interface_family(entry) != libc::AF_PACKET {
            continue;
        }
        let address = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_ll>() };
        let mut mac = [0u8; LINK_LAYER_ADDRESS_BYTES];
        mac.copy_from_slice(&address.sll_addr[..LINK_LAYER_ADDRESS_BYTES]);
        macs.insert(interface_name(entry).to_vec(), mac);
    }
    let mut result = Vec::new();
    item = head;
    while let Some(entry) = unsafe { item.as_ref() } {
        item = entry.ifa_next;
        if entry.ifa_addr.is_null()
            || entry.ifa_netmask.is_null()
            || !interface_is_reportable(entry)
        {
            continue;
        }
        let (address, netmask, family, scope) = match interface_family(entry) {
            libc::AF_INET => (
                unsafe { ipv4_text(entry.ifa_addr) },
                unsafe { ipv4_text(entry.ifa_netmask) },
                "IPv4",
                NO_INTERFACE_SCOPE,
            ),
            libc::AF_INET6 => (
                unsafe { ipv6_text(entry.ifa_addr) },
                unsafe { ipv6_text(entry.ifa_netmask) },
                "IPv6",
                f64::from(unsafe {
                    (*entry.ifa_addr.cast::<libc::sockaddr_in6>()).sin6_scope_id
                }),
            ),
            _ => continue,
        };
        let name = interface_name(entry);
        push_interface_address(
            c,
            &mut result,
            InterfaceAddress {
                name,
                address,
                netmask,
                family,
                mac: macs.get(name).copied().unwrap_or_default(),
                internal: interface_is_internal(entry),
                scope,
            },
        )?;
    }
    c.array_rooted(&result)
});

/// Darwin reports link-layer addresses as their own `AF_LINK` entries, where
/// `LLADDR` addresses the bytes that follow the interface name in `sdl_data`.
#[cfg(target_os = "macos")]
fn link_layer_address(link: &libc::sockaddr_dl) -> [u8; LINK_LAYER_ADDRESS_BYTES] {
    const SOCKADDR_DL_DATA_OFFSET: usize = std::mem::offset_of!(libc::sockaddr_dl, sdl_data);
    let mut mac = [0u8; LINK_LAYER_ADDRESS_BYTES];
    let recorded = usize::from(link.sdl_len)
        .saturating_sub(SOCKADDR_DL_DATA_OFFSET + usize::from(link.sdl_nlen));
    let copied = recorded.min(LINK_LAYER_ADDRESS_BYTES);
    unsafe {
        std::ptr::copy_nonoverlapping(
            link.sdl_data
                .as_ptr()
                .cast::<u8>()
                .add(usize::from(link.sdl_nlen)),
            mac.as_mut_ptr(),
            copied,
        );
    }
    mac
}

#[cfg(target_os = "macos")]
operation!(interfaces, c, args, {
    let Some(list) = interface_list(c, args)? else {
        return Ok(c.undefined());
    };
    let head = list.0;
    let _list = list;
    let mut macs = std::collections::HashMap::new();
    let mut item = head;
    while let Some(entry) = unsafe { item.as_ref() } {
        item = entry.ifa_next;
        if entry.ifa_addr.is_null() || interface_family(entry) != libc::AF_LINK {
            continue;
        }
        let link = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_dl>() };
        macs.insert(interface_name(entry).to_vec(), link_layer_address(link));
    }
    let mut result = Vec::new();
    item = head;
    while let Some(entry) = unsafe { item.as_ref() } {
        item = entry.ifa_next;
        if entry.ifa_addr.is_null() || !interface_is_reportable(entry) {
            continue;
        }
        let (address, netmask, family, scope) = match interface_family(entry) {
            libc::AF_INET => (
                unsafe { ipv4_text(entry.ifa_addr) },
                unsafe { ipv4_text(entry.ifa_netmask) },
                "IPv4",
                NO_INTERFACE_SCOPE,
            ),
            libc::AF_INET6 => (
                unsafe { ipv6_text(entry.ifa_addr) },
                unsafe { ipv6_text(entry.ifa_netmask) },
                "IPv6",
                f64::from(unsafe {
                    (*entry.ifa_addr.cast::<libc::sockaddr_in6>()).sin6_scope_id
                }),
            ),
            // `AF_LINK` entries only carry the physical address materialised above.
            _ => continue,
        };
        let name = interface_name(entry);
        push_interface_address(
            c,
            &mut result,
            InterfaceAddress {
                name,
                address,
                netmask,
                family,
                mac: macs.get(name).copied().unwrap_or_default(),
                internal: interface_is_internal(entry),
                scope,
            },
        )?;
    }
    c.array_rooted(&result)
});

fn build_constants(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let result = c.null_object_rooted()?;
    number(
        c,
        result,
        "UV_UDP_REUSEADDR",
        f64::from(super::util_system_error_shared_vm::UV_UDP_REUSEADDR),
    )?;
    let dlopen = c.null_object_rooted()?;
    for (name, value) in [
        ("RTLD_LAZY", libc::RTLD_LAZY),
        ("RTLD_NOW", libc::RTLD_NOW),
        ("RTLD_GLOBAL", libc::RTLD_GLOBAL),
        ("RTLD_LOCAL", libc::RTLD_LOCAL),
        // glibc's deep binding flag has no Darwin equivalent.
        #[cfg(target_os = "linux")]
        ("RTLD_DEEPBIND", libc::RTLD_DEEPBIND),
    ] {
        number(c, dlopen, name, f64::from(value))?;
    }
    put(c, result, "dlopen", dlopen)?;
    let errno = c.null_object_rooted()?;
    // Node's OS constant inventory differs from libuv's error-name inventory.
    macro_rules! errno_constants {
        ($($name:ident),* $(,)?) => { $(number(c, errno, stringify!($name), f64::from(libc::$name))?;)* };
    }
    errno_constants![
        E2BIG,
        EACCES,
        EADDRINUSE,
        EADDRNOTAVAIL,
        EAFNOSUPPORT,
        EAGAIN,
        EALREADY,
        EBADF,
        EBADMSG,
        EBUSY,
        ECANCELED,
        ECHILD,
        ECONNABORTED,
        ECONNREFUSED,
        ECONNRESET,
        EDEADLK,
        EDESTADDRREQ,
        EDOM,
        EDQUOT,
        EEXIST,
        EFAULT,
        EFBIG,
        EHOSTUNREACH,
        EIDRM,
        EILSEQ,
        EINPROGRESS,
        EINTR,
        EINVAL,
        EIO,
        EISCONN,
        EISDIR,
        ELOOP,
        EMFILE,
        EMLINK,
        EMSGSIZE,
        EMULTIHOP,
        ENAMETOOLONG,
        ENETDOWN,
        ENETRESET,
        ENETUNREACH,
        ENFILE,
        ENOBUFS,
        ENODATA,
        ENODEV,
        ENOENT,
        ENOEXEC,
        ENOLCK,
        ENOLINK,
        ENOMEM,
        ENOMSG,
        ENOPROTOOPT,
        ENOSPC,
        ENOSR,
        ENOSTR,
        ENOSYS,
        ENOTCONN,
        ENOTDIR,
        ENOTEMPTY,
        ENOTSOCK,
        ENOTSUP,
        ENOTTY,
        ENXIO,
        EOPNOTSUPP,
        EOVERFLOW,
        EPERM,
        EPIPE,
        EPROTO,
        EPROTONOSUPPORT,
        EPROTOTYPE,
        ERANGE,
        EROFS,
        ESPIPE,
        ESRCH,
        ESTALE,
        ETIME,
        ETIMEDOUT,
        ETXTBSY,
        EWOULDBLOCK,
        EXDEV,
    ];
    put(c, result, "errno", errno)?;
    let signals = super::os_shared_vm::signal_constants(c, SIGNAL_CONSTANTS)?;
    put(c, result, "signals", signals)?;
    let priority = c.null_object_rooted()?;
    const PRIORITIES: &[(&str, i32)] = &[
        ("PRIORITY_LOW", 19),
        ("PRIORITY_BELOW_NORMAL", 10),
        ("PRIORITY_NORMAL", 0),
        ("PRIORITY_ABOVE_NORMAL", -7),
        ("PRIORITY_HIGH", -14),
        ("PRIORITY_HIGHEST", -20),
    ];
    for &(name, value) in PRIORITIES {
        number(c, priority, name, f64::from(value))?;
    }
    put(c, result, "priority", priority)?;
    Ok(result)
}
