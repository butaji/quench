/// glibc's per-thread cache keeps freed small chunks out of its bins, where they neither merge
/// with free neighbours nor serve other sizes, so the heap grows past them; it is turned off
/// (V8-v7 max RSS, static x86_64, 2026-10-18: RegExp 7344 -> 6868-7056 KiB, Crypto 3372 ->
/// 3104-3152 KiB with `top_pad` below; two entries per class kept 7024 and 3368). glibc
/// grows and trims its heap with `top_pad` bytes of slack beyond each request; none is kept.
/// glibc reads its tunables once, at the C runtime's first allocation, which precedes any
/// code of ours, so the setting has to be in the environment a run starts with.
#[cfg(all(target_os = "linux", target_env = "gnu", target_feature = "crt-static"))]
const THREAD_CACHE_SETTING: &std::ffi::CStr =
    c"GLIBC_TUNABLES=glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0";

/// Marks the environment of a run re-executed with `THREAD_CACHE_SETTING`.
#[cfg(all(target_os = "linux", target_env = "gnu", target_feature = "crt-static"))]
const THREAD_CACHE_MARKER: &std::ffi::CStr = c"QUENCH_THREAD_CACHE_LIMITED=1";

/// The environment variable whose presence leaves glibc's tunables to the user.
#[cfg(all(target_os = "linux", target_env = "gnu", target_feature = "crt-static"))]
const TUNABLES_PREFIX: &[u8] = b"GLIBC_TUNABLES=";

/// Runs from `.preinit_array`, before the Rust runtime and `main`. A run whose environment
/// sets no glibc tunables re-executes itself once with `THREAD_CACHE_SETTING` added; the
/// re-executed run removes the setting and its marker again, so the guest and the processes
/// it starts see the environment the user gave. A failed re-execution continues unchanged.
#[cfg(all(target_os = "linux", target_env = "gnu", target_feature = "crt-static"))]
extern "C" fn limit_thread_cache(
    _argc: libc::c_int,
    argv: *const *const libc::c_char,
    envp: *mut *mut libc::c_char,
) {
    use std::ffi::CStr;
    if envp.is_null() {
        return;
    }
    // SAFETY: the C runtime passes the process environment: NUL-terminated strings up to a
    // null pointer, which nothing else uses before `main`.
    let entries = unsafe {
        let mut count = 0;
        while !(*envp.add(count)).is_null() {
            count += 1;
        }
        std::slice::from_raw_parts_mut(envp, count + 1)
    };
    let text = |entry: *mut libc::c_char| unsafe { CStr::from_ptr(entry) };
    let count = entries.len() - 1;
    if entries[..count]
        .iter()
        .any(|entry| text(*entry) == THREAD_CACHE_MARKER)
    {
        // The re-executed run: remove what the first run added, in place.
        let mut kept = 0;
        for index in 0..entries.len() {
            let entry = entries[index];
            let added = !entry.is_null()
                && (text(entry) == THREAD_CACHE_MARKER || text(entry) == THREAD_CACHE_SETTING);
            if !added {
                entries[kept] = entry;
                kept += 1;
            }
        }
        return;
    }
    if entries[..count]
        .iter()
        .any(|entry| text(*entry).to_bytes().starts_with(TUNABLES_PREFIX))
    {
        return;
    }
    let mut environment: Vec<*const libc::c_char> = entries[..count]
        .iter()
        .map(|entry| entry.cast_const())
        .collect();
    environment.extend([THREAD_CACHE_SETTING.as_ptr(), THREAD_CACHE_MARKER.as_ptr()]);
    environment.push(std::ptr::null());
    // SAFETY: argv and the new environment are null-terminated arrays of C strings; on
    // success the process image is replaced, on failure this run continues as it was.
    unsafe { libc::execve(c"/proc/self/exe".as_ptr(), argv, environment.as_ptr()) };
}

#[cfg(all(target_os = "linux", target_env = "gnu", target_feature = "crt-static"))]
#[used]
#[unsafe(link_section = ".preinit_array")]
static LIMIT_THREAD_CACHE: extern "C" fn(
    libc::c_int,
    *const *const libc::c_char,
    *mut *mut libc::c_char,
) = limit_thread_cache;

quench_node::install_process_start!();

fn main() -> std::process::ExitCode {
    if let Err(error) = quench_node::host::process_start::begin_main() {
        eprintln!("{error}");
        return std::process::ExitCode::FAILURE;
    }
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let result = if arguments.first().is_some_and(|argument| argument == "compile") {
        #[cfg(feature = "aot")]
        { quench_node::aot::cli::run(arguments.into_iter().skip(1).collect()) }
        #[cfg(not(feature = "aot"))]
        { Err("compile requires an aot-enabled quench-node compiler build".into()) }
    } else {
        quench_node::shared_run::run_shared_cli(arguments)
    };
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    };
    // A parallel fixture may replace itself with process.execve(process.execPath).
    // Preserve the harness's private result channel across that replacement.
    if let Some(result_path) = std::env::var_os("QUENCH_NODE_TEST_CASE_RESULT") {
        let result = if status == std::process::ExitCode::SUCCESS {
            r#"{"outcome":"pass"}"#
        } else {
            r#"{"outcome":"fail","reason":"replacement process exited unsuccessfully"}"#
        };
        if let Err(error) = std::fs::write(result_path, result) {
            eprintln!("write replaced Node worker result: {error}");
            return std::process::ExitCode::from(2);
        }
    }
    status
}
