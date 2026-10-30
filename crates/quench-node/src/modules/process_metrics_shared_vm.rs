//! Genuine Linux process facts; public validation stays in the configured SDK.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        c,
        "\0binding:process_methods",
        create_binding,
    )
}
fn create_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let constructor = get(c, global, "Uint32Array")?;
        // The SDK uses high/low seconds and nanoseconds; its BigInt view aliases
        // the first two words of this same backing buffer.
        const HRTIME_WORDS: usize = 3;
        let length = c.number(HRTIME_WORDS as f64);
        let buffer = c.construct_rooted(constructor, constructor, &[length])?;
        let module = c.object_rooted()?;
        put(c, module, "hrtimeBuffer", buffer)?;
        for (name, operation) in [
            ("hrtime", "processMetricsHrtime"),
            ("hrtimeBigInt", "processMetricsHrtimeBigInt"),
        ] {
            let function = c.host_function_named_with_data(
                crate::host::shared_vm::operation(operation),
                name,
                buffer,
            )?;
            put(c, module, name, function)?;
        }
        for (name, operation) in [
            ("cpuUsage", "processMetricsCpu"),
            ("threadCpuUsage", "processMetricsThreadCpu"),
            ("resourceUsage", "processMetricsResources"),
            ("rss", "processMetricsRss"),
            ("memoryUsage", "processMetricsMemoryUsage"),
        ] {
            let function =
                c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            put(c, module, name, function)?;
        }
        #[cfg(unix)]
        {
            let function = c.host_function_named(crate::host::shared_vm::operation("processExecve"), "execve")?;
            put(c, module, "execve", function)?;
        }
        for (name,operation) in [("cwd","processCwd"),("umask","workerRawUmask")] {
            let function=c.host_function_named(crate::host::shared_vm::operation(operation),name)?;
            put(c,module,name,function)?;
        }
        Ok(module)
    })
}
fn write_values(c: &mut Context<'_>, target: RootId, values: &[f64]) -> Result<(), RootedError> {
    for (index, value) in values.iter().enumerate() {
        let key = c.number(index as f64);
        let value = c.number(*value);
        if !c.set_property_rooted(target, key, value, target)? {
            return Err(RootedError::host("cannot fill process metrics result"));
        }
    }
    Ok(())
}
fn os_error(operation: &str) -> RootedError {
    RootedError::host(format!("{operation}: {}", std::io::Error::last_os_error()))
}
fn monotonic() -> Result<libc::timespec, RootedError> {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // clock_gettime initializes the complete result on success.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, value.as_mut_ptr()) } != 0 {
        return Err(os_error("clock_gettime"));
    }
    Ok(unsafe { value.assume_init() })
}
pub(crate) fn hrtime(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let time = monotonic()?;
    let seconds = time.tv_sec as u64;
    let buffer = c.host_function_data()?;
    write_values(
        c,
        buffer,
        &[
            (seconds >> u32::BITS) as f64,
            seconds as u32 as f64,
            time.tv_nsec as f64,
        ],
    )?;
    Ok(c.undefined())
}
pub(crate) fn hrtime_bigint(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let time = monotonic()?;
    const NANOS_PER_SECOND: u64 = std::time::Duration::from_secs(1).as_nanos() as u64;
    let nanoseconds = (time.tv_sec as u64)
        .wrapping_mul(NANOS_PER_SECOND)
        .wrapping_add(time.tv_nsec as u64);
    let bytes = nanoseconds.to_ne_bytes();
    const WORD_BYTES: usize = std::mem::size_of::<u32>();
    let words = [
        u32::from_ne_bytes(bytes[..WORD_BYTES].try_into().unwrap()),
        u32::from_ne_bytes(bytes[WORD_BYTES..].try_into().unwrap()),
    ];
    let buffer = c.host_function_data()?;
    write_values(c, buffer, &[words[0] as f64, words[1] as f64])?;
    Ok(c.undefined())
}
fn usage(who: libc::c_int) -> Result<libc::rusage, RootedError> {
    let mut value = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage initializes every field on success.
    if unsafe { libc::getrusage(who, value.as_mut_ptr()) } != 0 {
        return Err(os_error("getrusage"));
    }
    Ok(unsafe { value.assume_init() })
}
fn micros(value: libc::timeval) -> f64 {
    const MICROS_PER_SECOND: f64 = std::time::Duration::from_secs(1).as_micros() as f64;
    value.tv_sec as f64 * MICROS_PER_SECOND + value.tv_usec as f64
}
fn thread_rusage() -> libc::c_int {
    // macOS has no RUSAGE_THREAD; the process total is the closest portable
    // approximation and is only used for the diagnostic threadCpuUsage read.
    #[cfg(target_os = "macos")]
    {
        libc::RUSAGE_SELF
    }
    #[cfg(not(target_os = "macos"))]
    {
        libc::RUSAGE_THREAD
    }
}

fn cpu(c: &mut Context<'_>, args: &[RootId], who: libc::c_int) -> Result<RootId, RootedError> {
    let usage = usage(who)?;
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("CPU metrics result is absent"))?;
    write_values(c, target, &[micros(usage.ru_utime), micros(usage.ru_stime)])?;
    Ok(c.undefined())
}
pub(crate) fn process_cpu(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    cpu(c, args, libc::RUSAGE_SELF)
}
pub(crate) fn thread_cpu(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    cpu(c, args, thread_rusage())
}
pub(crate) fn resources(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let usage = usage(libc::RUSAGE_SELF)?;
    // Linux getrusage reports max RSS in KiB; macOS reports bytes. Node's public
    // API is KiB on every platform, so convert Darwin's value here.
    let max_rss = if cfg!(target_os = "macos") {
        usage.ru_maxrss as f64 / 1024.0
    } else {
        usage.ru_maxrss as f64
    };
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("resource metrics result is absent"))?;
    // Linux getrusage already expresses max RSS in KiB, as Node's public API does.
    write_values(
        c,
        target,
        &[
            micros(usage.ru_utime),
            micros(usage.ru_stime),
            max_rss,
            usage.ru_ixrss as f64,
            usage.ru_idrss as f64,
            usage.ru_isrss as f64,
            usage.ru_minflt as f64,
            usage.ru_majflt as f64,
            usage.ru_nswap as f64,
            usage.ru_inblock as f64,
            usage.ru_oublock as f64,
            usage.ru_msgsnd as f64,
            usage.ru_msgrcv as f64,
            usage.ru_nsignals as f64,
            usage.ru_nvcsw as f64,
            usage.ru_nivcsw as f64,
        ],
    )?;
    Ok(c.undefined())
}
/// The process's resident set from `/proc/self/statm`, in bytes.
#[cfg(target_os = "linux")]
fn resident_bytes() -> Result<f64, RootedError> {
    let text = std::fs::read_to_string("/proc/self/statm")
        .map_err(|error| RootedError::host(error.to_string()))?;
    let pages = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| RootedError::host("invalid /proc/self/statm resident pages"))?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(os_error("sysconf(_SC_PAGESIZE)"));
    }
    Ok(pages as f64 * page_size as f64)
}

/// The process's resident set from Mach's basic task info, in bytes.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn resident_bytes() -> Result<f64, RootedError> {
    let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::uninit();
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: `info` points at a `mach_task_basic_info` and `count` is its
    // natural_t word count, as `task_info` requires.
    let status = unsafe {
        libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return Err(RootedError::host(format!("task_info: {status}")));
    }
    // task_info initializes the complete result on success.
    Ok(unsafe { info.assume_init() }.resident_size as f64)
}
pub(crate) fn rss(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.number(resident_bytes()?))
}
/// `binding.memoryUsage(values)` (src/node_process_methods.cc): fills `rss, heapTotal,
/// heapUsed, external, arrayBuffers`. ArrayBuffer backing stores are V8's only external
/// memory here, so `external` and `arrayBuffers` agree.
pub(crate) fn memory_usage(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("memory usage result is absent"))?;
    let heap = c.heap_memory_composition();
    let external = heap.array_buffer_bytes as f64;
    write_values(
        c,
        target,
        &[resident_bytes()?, heap.reserved_bytes() as f64, heap.used_bytes() as f64, external, external],
    )?;
    Ok(c.undefined())
}
pub(crate) fn install(c: &mut Context<'_>) -> Result<(), RootedError> {
    let process = process(c)?;
    // Node's wrappers load with `internal/process/per_thread.js` on the first read of one.
    let initializer = c.host_function(crate::host::shared_vm::operation("processMethodsInitialize"))?;
    c.define_lazy_data_properties_rooted(process, &WRAPPED_METHODS, true, initializer)?;
    Ok(())
}

/// The process methods Node's `wrapProcessMethods` makes over the native binding.
const WRAPPED_METHODS: [&str; 5] = ["cpuUsage", "threadCpuUsage", "memoryUsage", "resourceUsage", "execve"];

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))
}

/// Defines the wrapped process methods as Node's.
pub(crate) fn initialize_methods(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let native = binding(c)?;
    let wrapper = crate::host::shared_vm::commonjs::node_library_export(
        c,
        "internal/bootstrap/node.js",
        "internal/process/per_thread.js",
        "wrapProcessMethods",
    )?;
    let undefined = c.undefined();
    let methods = c.call_rooted(wrapper, undefined, &[native])?;
    let process = process(c)?;
    for name in WRAPPED_METHODS {
        let function = get(c, methods, name)?;
        put(c, process, name, function)?;
    }
    Ok(c.undefined())
}
