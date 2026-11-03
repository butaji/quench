//! Genuine Unix process facts: the `process_methods` binding, and the public methods
//! Node's `wrapProcessMethods` (lib/internal/process/per_thread.js) makes over it.
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
            let function = c.host_function_named(crate::host::shared_vm::operation!("processExecve"), "execve")?;
            put(c, module, "execve", function)?;
        }
        let patch_process_object = c.host_function_named(
            crate::host::shared_vm::operation!("processMetricsPatchProcessObject"),
            "patchProcessObject",
        )?;
        put(c, module, "patchProcessObject", patch_process_object)?;
        for (name, operation) in [("cwd", "processCwd"), ("umask", "processRawUmask")] {
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
pub(crate) fn thread_rusage() -> libc::c_int {
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

/// `[user, system]` CPU time in microseconds.
fn cpu_values(who: libc::c_int) -> Result<[f64; 2], RootedError> {
    let usage = usage(who)?;
    Ok([micros(usage.ru_utime), micros(usage.ru_stime)])
}

fn cpu(c: &mut Context<'_>, args: &[RootId], who: libc::c_int) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("CPU metrics result is absent"))?;
    write_values(c, target, &cpu_values(who)?)?;
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
/// `process.resourceUsage()`'s fields, in the binding's value order.
const RESOURCE_FIELDS: [&str; 16] = [
    "userCPUTime",
    "systemCPUTime",
    "maxRSS",
    "sharedMemorySize",
    "unsharedDataSize",
    "unsharedStackSize",
    "minorPageFault",
    "majorPageFault",
    "swappedOut",
    "fsRead",
    "fsWrite",
    "ipcSent",
    "ipcReceived",
    "signalsCount",
    "voluntaryContextSwitches",
    "involuntaryContextSwitches",
];

fn resource_values() -> Result<[f64; RESOURCE_FIELDS.len()], RootedError> {
    let usage = usage(libc::RUSAGE_SELF)?;
    // Linux getrusage reports max RSS in KiB; macOS reports bytes. Node's public
    // API is KiB on every platform, so convert Darwin's value here.
    const BYTES_PER_KIB: f64 = 1024.0;
    let max_rss = if cfg!(target_os = "macos") {
        usage.ru_maxrss as f64 / BYTES_PER_KIB
    } else {
        usage.ru_maxrss as f64
    };
    Ok([
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
    ])
}

pub(crate) fn resources(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("resource metrics result is absent"))?;
    write_values(c, target, &resource_values()?)?;
    Ok(c.undefined())
}
/// The process's resident set from `/proc/self/statm`, in bytes.
#[cfg(target_os = "linux")]
pub(crate) fn resident_bytes() -> Result<f64, RootedError> {
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
pub(crate) fn resident_bytes() -> Result<f64, RootedError> {
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
    let values = memory_values(c)?;
    write_values(c, target, &values)?;
    Ok(c.undefined())
}

/// `process.memoryUsage()`'s fields, in the binding's value order.
const MEMORY_FIELDS: [&str; 5] = ["rss", "heapTotal", "heapUsed", "external", "arrayBuffers"];

fn memory_values(c: &mut Context<'_>) -> Result<[f64; MEMORY_FIELDS.len()], RootedError> {
    let heap = c.heap_memory_composition();
    let external = heap.array_buffer_bytes as f64;
    Ok([resident_bytes()?, heap.reserved_bytes() as f64, heap.used_bytes() as f64, external, external])
}

/// `{ field: value, ... }` in field order.
fn record(c: &mut Context<'_>, fields: &[&str], values: &[f64]) -> Result<RootId, RootedError> {
    let result = c.object_rooted()?;
    for (name, value) in fields.iter().zip(values) {
        let value = c.number(*value);
        put(c, result, name, value)?;
    }
    Ok(result)
}

/// `process.memoryUsage()`.
pub(crate) fn public_memory_usage(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let values = memory_values(c)?;
    record(c, &MEMORY_FIELDS, &values)
}

/// `process.resourceUsage()`.
pub(crate) fn public_resource_usage(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    record(c, &RESOURCE_FIELDS, &resource_values()?)
}

/// `process.cpuUsage()`'s and `process.threadCpuUsage()`'s fields.
const CPU_FIELDS: [&str; 2] = ["user", "system"];

/// Whether a previous CPU reading's field can be subtracted: a number in `[0, 2^53 - 1]`.
fn previous_value_is_valid(c: &Context<'_>, value: RootId) -> bool {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    c.rooted_value(value)
        .and_then(|value| value.as_number())
        .is_some_and(|number| (0.0..=MAX_SAFE_INTEGER).contains(&number))
}

/// `cpuUsage(prevValue)` over `who`'s reading: the reading, less `prevValue` when given.
fn public_cpu(c: &mut Context<'_>, args: &[RootId], who: libc::c_int) -> Result<RootId, RootedError> {
    let previous = args.first().copied().unwrap_or_else(|| c.undefined());
    let has_previous = c.truthy_rooted(previous)?;
    let mut prior = [0.0; CPU_FIELDS.len()];
    if has_previous {
        for (index, field) in CPU_FIELDS.iter().enumerate() {
            let value = if c.is_object_rooted(previous)? { get(c, previous, field)? } else { c.undefined() };
            if !previous_value_is_valid(c, value) {
                if index == 0 {
                    super::perf_hooks::validate_object(c, previous, "prevValue")?;
                }
                let name = format!("prevValue.{field}");
                if c.rooted_value(value).and_then(|value| value.as_number()).is_none() {
                    return Err(super::util_shared_vm::argument_type_error(c, value, &name, "number")?);
                }
                return Err(invalid_property_range(c, &name, value)?);
            }
            prior[index] = c.to_number(value)?;
        }
    }
    let current = cpu_values(who)?;
    let values = [current[0] - prior[0], current[1] - prior[1]];
    record(c, &CPU_FIELDS, &values)
}

/// `ERR_INVALID_ARG_VALUE.RangeError(name, value)`: "The property 'name' is invalid. Received …".
fn invalid_property_range(c: &mut Context<'_>, name: &str, value: RootId) -> Result<RootedError, RootedError> {
    let received = super::console_format_shared_vm::inspect(c, value, Default::default())?;
    let error = c.range_error_rooted(&format!("The property '{name}' is invalid. Received {received}"))?;
    super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_INVALID_ARG_VALUE")?;
    Ok(c.throw(error))
}

/// `process.cpuUsage(prevValue)`.
pub(crate) fn public_process_cpu(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    public_cpu(c, args, libc::RUSAGE_SELF)
}

/// `process.threadCpuUsage(prevValue)`.
pub(crate) fn public_thread_cpu(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    public_cpu(c, args, thread_rusage())
}

/// `process.execve(execPath[, args[, env]])`: validates, then replaces the process image.
pub(crate) fn public_execve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::process_warning_shared_vm::emit_experimental(c, "process.execve")?;
    if c.host_mut().shared_state().borrow().messaging.is_worker() {
        let error = c.type_error_rooted("Calling process.execve is not supported in a worker")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_WORKER_UNSUPPORTED_OPERATION")?;
        return Err(c.throw(error));
    }
    let undefined = c.undefined();
    let path = args.first().copied().unwrap_or(undefined);
    if c.string_text(path)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(c, path, "execPath", "string")?);
    }
    let arguments = match args.get(1).copied().filter(|value| !c.rooted_value(*value).is_some_and(|value| value.is_undefined())) {
        Some(arguments) => arguments,
        None => c.array_rooted(&[])?,
    };
    if !c.is_array_rooted(arguments)? {
        return Err(super::util_shared_vm::argument_type_error(c, arguments, "args", "Array")?);
    }
    for (index, argument) in c.array_like_values_rooted(arguments)?.into_iter().enumerate() {
        if c.string_text(argument)?.is_none_or(|text| text.contains('\0')) {
            return Err(super::util_shared_vm::argument_value_error(c, argument, &format!("args[{index}]"), "must be a string without null bytes")?);
        }
    }
    let environment = args.get(2).copied().unwrap_or(undefined);
    let mut pairs = Vec::new();
    if !c.rooted_value(environment).is_some_and(|value| value.is_undefined()) {
        super::perf_hooks::validate_object(c, environment, "env")?;
        for key in c.enumerable_own_keys_rooted(environment, false, true)? {
            let value = c.get_property_rooted(environment, key)?;
            let pair = match (c.string_text(key)?, c.string_text(value)?) {
                (Some(key), Some(value)) if !key.contains('\0') && !value.contains('\0') => format!("{key}={value}"),
                _ => {
                    return Err(super::util_shared_vm::argument_value_error(
                        c,
                        environment,
                        "env",
                        "must be an object with string keys and values without null bytes",
                    )?)
                }
            };
            pairs.push(c.string_rooted(&pair));
        }
    }
    let pairs = c.array_rooted(&pairs)?;
    super::process_execve_shared_vm::execve(c, undefined, &[path, arguments, pairs])
}

/// Defines Node's wrapped process methods on `process`.
pub(crate) fn define_methods(c: &mut Context<'_>, process: RootId) -> Result<(), RootedError> {
    for (name, operation) in WRAPPED_METHODS {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, process, name, function)?;
    }
    let memory_usage = get(c, process, "memoryUsage")?;
    let rss = c.host_function_named(crate::host::shared_vm::operation!("processMetricsRss"), "rss")?;
    put(c, memory_usage, "rss", rss)
}

/// The process methods Node's `wrapProcessMethods` makes over the native binding.
const WRAPPED_METHODS: [(&str, &str); 5] = [
    ("cpuUsage", "processCpuUsage"),
    ("threadCpuUsage", "processThreadCpuUsage"),
    ("memoryUsage", "processMemoryUsage"),
    ("resourceUsage", "processResourceUsage"),
    ("execve", "processExecvePublic"),
];
