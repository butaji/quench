//! Genuine Linux process facts; public validation stays in the configured SDK.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
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
        ] {
            let function =
                c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            put(c, module, name, function)?;
        }
        let patch_process_object = c.host_function_named(
            crate::host::shared_vm::operation("processMetricsPatchProcessObject"),
            "patchProcessObject",
        )?;
        put(c, module, "patchProcessObject", patch_process_object)?;
        for (name, operation) in [("cwd", "processCwd"), ("umask", "workerRawUmask")] {
            let function =
                c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            put(c, module, name, function)?;
        }
        Ok(module)
    })
}
pub(crate) fn patch_process_object(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // The shared host has already installed process.argv and process.execArgv.
    // Node's native method refreshes those fields and installs native process
    // title accessors; its JS caller performs the CLI path normalization.
    Ok(context.undefined())
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
    cpu(c, args, libc::RUSAGE_THREAD)
}
pub(crate) fn resources(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let usage = usage(libc::RUSAGE_SELF)?;
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
            usage.ru_maxrss as f64,
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
pub(crate) fn rss(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
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
    Ok(c.number(pages as f64 * page_size as f64))
}
pub(crate) fn install(c: &mut Context<'_>) -> Result<(), RootedError> {
    let native = binding(c)?;
    let wrapper = crate::host::shared_vm::commonjs::node_library_export(
        c,
        "internal/bootstrap/node.js",
        "internal/process/per_thread.js",
        "wrapProcessMethods",
    )?;
    let undefined = c.undefined();
    let methods = c.call_rooted(wrapper, undefined, &[native])?;
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    for name in ["cpuUsage", "threadCpuUsage", "resourceUsage"] {
        let function = get(c, methods, name)?;
        put(c, process, name, function)?;
    }
    // Only install native RSS; heap metrics require the Runtime's real accounting.
    let memory = get(c, process, "memoryUsage")?;
    let rss = get(c, native, "rss")?;
    put(c, memory, "rss", rss)?;
    Ok(())
}
