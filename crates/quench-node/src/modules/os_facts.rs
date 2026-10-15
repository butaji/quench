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
