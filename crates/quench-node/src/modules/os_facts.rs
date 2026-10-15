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
pub(crate) fn total_memory_bytes() -> u64 {
    memory_info().total
}

/// Node's credentials binding skips empty variables and removes one trailing
/// slash. This differs from both Rust's temp_dir and libuv's uv_os_tmpdir.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub(crate) struct CpuInfo {
    pub(crate) model: String,
    pub(crate) speed: u64,
    pub(crate) times: CpuTimes,
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub(crate) struct UserInfo {
    pub(crate) uid: libc::uid_t,
    pub(crate) gid: libc::gid_t,
    pub(crate) username: Vec<u8>,
    pub(crate) homedir: Vec<u8>,
    pub(crate) shell: Vec<u8>,
}

/// Copy the reentrant libc record before releasing its caller-owned storage.
#[cfg(target_os = "linux")]
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
