#[cfg(target_os = "macos")]
use std::ffi::CStr;

pub(crate) fn type_str() -> String {
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

pub(crate) struct MemoryInfo {
    pub(crate) total: u64,
    pub(crate) available: u64,
}
pub(crate) fn memory_info() -> MemoryInfo {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    MemoryInfo {
        total: system.total_memory(),
        available: system.available_memory(),
    }
}
/// Physical memory in bytes. On Darwin `sysinfo` reads `hw.memsize`, the same
/// sysctl libuv reports as `uv_get_total_memory`.
pub(crate) fn total_memory_bytes() -> u64 {
    memory_info().total
}

/// Node's `os.freemem` follows libuv: Darwin reports the kernel's free page
/// count, Linux reports the memory sysinfo considers available.
#[cfg(target_os = "macos")]
pub(crate) fn free_memory_bytes() -> u64 {
    let Some(statistics) = darwin_vm_statistics() else {
        return 0;
    };
    // `vm_statistics64` is packed, so the counter is read without forming a reference.
    let free_count = unsafe { std::ptr::addr_of!(statistics.free_count).read_unaligned() };
    u64::from(free_count) * page_size_bytes()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn free_memory_bytes() -> u64 {
    memory_info().available
}

/// Node's credentials binding skips empty variables and removes one trailing
/// slash. This differs from both Rust's temp_dir and libuv's uv_os_tmpdir.
#[cfg(unix)]
pub(crate) fn temporary_directory() -> String {
    use std::os::unix::ffi::OsStrExt;

    let directory = ["TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty());
    let Some(directory) = directory else {
        return "/tmp".to_owned();
    };
    let mut bytes = directory.as_bytes();
    if bytes.len() > 1 && bytes.last() == Some(&b'/') {
        bytes = &bytes[..bytes.len() - 1];
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) struct CpuInfo {
    pub(crate) model: String,
    pub(crate) speed: u64,
    pub(crate) times: CpuTimes,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) struct CpuTimes {
    pub(crate) user: u64,
    pub(crate) nice: u64,
    pub(crate) sys: u64,
    pub(crate) idle: u64,
    pub(crate) irq: u64,
}

/// Linux CPU facts follow libuv's online-CPU records, frequency source and
/// millisecond tick conversion, rather than inventing zero CPU-time snapshots.
#[cfg(target_os = "linux")]
pub(crate) fn cpus() -> Vec<CpuInfo> {
    use std::collections::BTreeMap;
    const LINUX_KERNEL_MAX_CPUS: usize = 8192;
    const LIBUV_MODEL_BUFFER_BYTES: usize = 64;
    const LINUX_CPU_TICKS_PER_SECOND: u64 = 100;
    const MILLISECONDS_PER_SECOND: u64 = 1000;
    const LINUX_CPU_TICK_MILLISECONDS: u64 = MILLISECONDS_PER_SECOND / LINUX_CPU_TICKS_PER_SECOND;
    const CPU_STAT_FIELDS_THROUGH_IRQ: usize = 6;
    const KILOHERTZ_PER_MEGAHERTZ: u64 = 1000;
    let Ok(stat) = std::fs::read_to_string("/proc/stat") else {
        return Vec::new();
    };
    let mut cpus = BTreeMap::new();
    for line in stat.lines() {
        let mut fields = line.split_whitespace();
        let Some(id) = fields
            .next()
            .and_then(|field| field.strip_prefix("cpu"))
            .and_then(|id| id.parse::<usize>().ok())
            .filter(|id| *id < LINUX_KERNEL_MAX_CPUS)
        else {
            continue;
        };
        let parsed: Option<Vec<u64>> = fields
            .take(CPU_STAT_FIELDS_THROUGH_IRQ)
            .map(|field| field.parse().ok())
            .collect();
        let Some(parsed) = parsed else { break };
        let [user, nice, sys, idle, _iowait, irq] = parsed.as_slice() else {
            break;
        };
        cpus.insert(
            id,
            CpuInfo {
                model: "unknown".to_owned(),
                speed: 0,
                times: CpuTimes {
                    user: user * LINUX_CPU_TICK_MILLISECONDS,
                    nice: nice * LINUX_CPU_TICK_MILLISECONDS,
                    sys: sys * LINUX_CPU_TICK_MILLISECONDS,
                    idle: idle * LINUX_CPU_TICK_MILLISECONDS,
                    irq: irq * LINUX_CPU_TICK_MILLISECONDS,
                },
            },
        );
    }
    if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
        for block in info.split("\n\n") {
            let fields: BTreeMap<_, _> = block
                .lines()
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim(), value.trim()))
                .collect();
            let Some(cpu) = fields
                .get("processor")
                .and_then(|id| id.parse::<usize>().ok())
                .and_then(|id| cpus.get_mut(&id))
            else {
                continue;
            };
            if let Some(model) = fields
                .get("model name")
                .or_else(|| fields.get("cpu model"))
                .or_else(|| fields.get("cpu"))
            {
                let bytes = model.as_bytes();
                cpu.model = String::from_utf8_lossy(
                    &bytes[..bytes.len().min(LIBUV_MODEL_BUFFER_BYTES - 1)],
                )
                .into_owned();
            }
        }
    }
    for (id, cpu) in &mut cpus {
        cpu.speed = std::fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"
        ))
        .ok()
        .and_then(|frequency| frequency.trim().parse::<u64>().ok())
        .unwrap_or(0)
            / KILOHERTZ_PER_MEGAHERTZ;
    }
    cpus.into_values().collect()
}

#[cfg(unix)]
pub(crate) struct UserInfo {
    pub(crate) uid: libc::uid_t,
    pub(crate) gid: libc::gid_t,
    pub(crate) username: Vec<u8>,
    pub(crate) homedir: Vec<u8>,
    pub(crate) shell: Vec<u8>,
}

/// Copy the reentrant libc record before releasing its caller-owned storage.
#[cfg(unix)]
pub(crate) fn user_info() -> Result<UserInfo, i32> {
    use std::ffi::CStr;
    const INITIAL_PASSWD_BUFFER_BYTES: usize = 2000; // libuv's initial record capacity.
    let mut capacity = INITIAL_PASSWD_BUFFER_BYTES;
    loop {
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(capacity)
            .map_err(|_| libc::ENOMEM)?;
        storage.resize(capacity, 0u8);
        let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut();
        let status = loop {
            let status = unsafe {
                libc::getpwuid_r(
                    libc::geteuid(),
                    &mut record,
                    storage.as_mut_ptr().cast(),
                    storage.len(),
                    &mut result,
                )
            };
            if status != libc::EINTR {
                break status;
            }
        };
        if status == libc::ERANGE {
            capacity = capacity.checked_mul(2).ok_or(libc::ENOMEM)?;
            continue;
        }
        if status != 0 {
            return Err(status);
        }
        if result.is_null() {
            return Err(libc::ENOENT);
        }
        return Ok(UserInfo {
            uid: record.pw_uid,
            gid: record.pw_gid,
            username: unsafe { CStr::from_ptr(record.pw_name) }
                .to_bytes()
                .to_vec(),
            homedir: unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes().to_vec(),
            shell: unsafe { CStr::from_ptr(record.pw_shell) }
                .to_bytes()
                .to_vec(),
        });
    }
}

// Darwin facts follow libuv's `darwin.c`, which is the authority for every
// value Node reports on that platform.

/// Read a `sysctlbyname` value, returning the number of bytes the kernel wrote.
/// String values shrink to their length, so callers that need a fixed-size
/// record compare the answer with `size_of::<T>()`.
#[cfg(target_os = "macos")]
fn darwin_sysctl<T>(name: &CStr, out: &mut T) -> Option<usize> {
    let mut size = std::mem::size_of::<T>();
    let read = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (out as *mut T).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (read == 0).then_some(size)
}

/// libc marks the Mach ports deprecated only to point at the `mach2` crate; they
/// remain the authority for the statistics Node reports.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn mach_host_port() -> libc::mach_port_t {
    unsafe { libc::mach_host_self() }
}

/// Release the processor-info array `host_processor_info` allocated in the task.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn release_processor_info(info: libc::processor_info_array_t, entries: libc::natural_t) {
    let bytes = entries as usize * std::mem::size_of::<libc::integer_t>();
    unsafe { libc::vm_deallocate(libc::mach_task_self(), info as libc::vm_address_t, bytes) };
}

#[cfg(target_os = "macos")]
fn darwin_vm_statistics() -> Option<libc::vm_statistics64> {
    let mut statistics = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let status = unsafe {
        libc::host_statistics64(
            mach_host_port(),
            libc::HOST_VM_INFO64,
            std::ptr::addr_of_mut!(statistics).cast(),
            &mut count,
        )
    };
    (status == libc::KERN_SUCCESS).then_some(statistics)
}

/// The kernel counts pages; `sysconf` owns the page size Node multiplies by.
#[cfg(target_os = "macos")]
fn page_size_bytes() -> u64 {
    const FALLBACK_PAGE_BYTES: u64 = 4096;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page > 0 {
        page as u64
    } else {
        FALLBACK_PAGE_BYTES
    }
}

/// Darwin CPU facts use libuv's `PROCESSOR_CPU_LOAD_INFO` tick counters scaled
/// to milliseconds, the advertised brand string and published frequency.
#[cfg(target_os = "macos")]
pub(crate) fn cpus() -> Vec<CpuInfo> {
    const LIBUV_MODEL_BUFFER_BYTES: usize = 512;
    const MILLISECONDS_PER_SECOND: u64 = 1000;
    const HERTZ_PER_MEGAHERTZ: u64 = 1_000_000;
    // Apple does not publish hw.cpufrequency on Apple silicon; libuv substitutes
    // its 2.4 GHz nominal value rather than reporting zero.
    const APPLE_SILICON_NOMINAL_HERTZ: u64 = 2_400_000_000;
    const ABSENT_FREQUENCY_HERTZ: u64 = 0;
    let mut model = [0 as libc::c_char; LIBUV_MODEL_BUFFER_BYTES];
    if darwin_sysctl(c"machdep.cpu.brand_string", &mut model).is_none()
        && darwin_sysctl(c"hw.model", &mut model).is_none()
    {
        return Vec::new();
    }
    let model = unsafe { CStr::from_ptr(model.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let mut frequency = ABSENT_FREQUENCY_HERTZ;
    if darwin_sysctl(c"hw.cpufrequency", &mut frequency).is_none()
        || frequency == ABSENT_FREQUENCY_HERTZ
    {
        frequency = APPLE_SILICON_NOMINAL_HERTZ;
    }
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return Vec::new();
    }
    let tick_milliseconds = MILLISECONDS_PER_SECOND / ticks_per_second.unsigned_abs();
    let mut count = 0 as libc::natural_t;
    let mut loads: libc::processor_info_array_t = std::ptr::null_mut();
    let mut load_size = 0 as libc::mach_msg_type_number_t;
    let status = unsafe {
        libc::host_processor_info(
            mach_host_port(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            &mut loads,
            &mut load_size,
        )
    };
    if status != libc::KERN_SUCCESS {
        return Vec::new();
    }
    let mut cpus = Vec::with_capacity(count as usize);
    // `host_processor_info` returns one flat run of tick counters per CPU.
    const CPU_LOAD_FIELDS: usize = libc::CPU_STATE_MAX as usize;
    for index in 0..count as usize {
        let load = unsafe {
            *loads
                .add(index * CPU_LOAD_FIELDS)
                .cast::<libc::processor_cpu_load_info>()
        };
        let tick =
            |state: libc::c_int| u64::from(load.cpu_ticks[state as usize]) * tick_milliseconds;
        cpus.push(CpuInfo {
            model: model.clone(),
            speed: frequency / HERTZ_PER_MEGAHERTZ,
            times: CpuTimes {
                user: tick(libc::CPU_STATE_USER),
                nice: tick(libc::CPU_STATE_NICE),
                sys: tick(libc::CPU_STATE_SYSTEM),
                idle: tick(libc::CPU_STATE_IDLE),
                irq: 0,
            },
        });
    }
    release_processor_info(loads, load_size);
    cpus
}

/// libuv asks for the active, then logical, then total CPU count on Darwin.
#[cfg(target_os = "macos")]
pub(crate) fn available_parallelism() -> u64 {
    const FALLBACK_PARALLELISM: u64 = 1;
    for name in [c"hw.activecpu", c"hw.logicalcpu", c"hw.ncpu"] {
        let mut count = 0 as libc::c_int;
        // libuv accepts only a full-width, positive answer.
        if darwin_sysctl(name, &mut count) == Some(std::mem::size_of_val(&count)) && count > 0 {
            return count as u64;
        }
    }
    FALLBACK_PARALLELISM
}

/// Apply libuv's cgroup CPU quota after the scheduler affinity count.
#[cfg(target_os = "linux")]
pub(crate) fn available_parallelism() -> u64 {
    let mut set = unsafe { std::mem::zeroed::<libc::cpu_set_t>() };
    let affinity =
        if unsafe { libc::sched_getaffinity(0, std::mem::size_of_val(&set), &mut set) } == 0 {
            i64::from(unsafe { libc::CPU_COUNT(&set) })
        } else {
            unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }
        };
    let count = affinity.max(1) as u64;
    constrained_cpu()
        .filter(|quota| *quota > 0)
        .map_or(count, |quota| count.min(quota as u64))
}

#[cfg(target_os = "linux")]
fn constrained_cpu() -> Option<i64> {
    use std::path::Path;
    let source = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    if let Some(relative) = source
        .strip_prefix("0::/")
        .and_then(|value| value.lines().next())
    {
        let mount = Path::new("/sys/fs/cgroup");
        let mut path = mount.join(relative);
        std::fs::read(path.join("cgroup.controllers")).ok()?;
        let mut quota = None;
        loop {
            if let Some((limit, period)) = std::fs::read_to_string(path.join("cpu.max"))
                .ok()
                .and_then(|value| {
                    let mut fields = value.split_whitespace();
                    Some((
                        fields.next()?.parse::<i64>().ok()?,
                        fields.next()?.parse::<i64>().ok()?,
                    ))
                })
                .filter(|(_, period)| *period != 0)
            {
                // Node 26.12's libuv overwrites the quota at each finite ancestor.
                // It computes a minimum separately but returns the last quota.
                let value = limit.checked_div(period)?;
                quota = Some(if value == 0 { 1 } else { value });
            }
            if path == mount || !path.pop() {
                break;
            }
        }
        quota
    } else {
        // Preserve libuv's legacy controller lookup and file locations.
        let controller = source.split_once(":cpu,")?.1.lines().next()?;
        let root = Path::new("/sys/fs/cgroup").join(controller);
        let quota = std::fs::read_to_string(root.join("cpu.cfs_quota_us"))
            .ok()?
            .trim()
            .parse::<i64>()
            .ok()?;
        let period = std::fs::read_to_string(root.join("cpu.cfs_period_us"))
            .ok()?
            .trim()
            .parse::<i64>()
            .ok()?;
        quota.checked_div(period)
    }
}
