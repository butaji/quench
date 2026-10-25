//! Runs one command and reports its peak resident set from `wait4`, for board runners that
//! cannot read a child's resource usage themselves (a JavaScript runner on quench-node).
//!
//! ```sh
//! rss-exec [--evict PATH]... -- COMMAND [ARGUMENT]...
//! ```
//!
//! The child inherits stdin, stdout and stderr. After it exits, the last line on stderr is
//! `rss-exec maxrss_kib=N status=S`; `status` is the exit code, or 128 plus the signal that
//! ended it. `--evict PATH` drops PATH's cached pages first on Linux
//! (`POSIX_FADV_DONTNEED`), so each run faults its executable in as a cold start does.
//! Darwin has no per-file page-cache eviction: there `--evict` only marks the descriptor
//! `F_NOCACHE`, so a page an earlier run faulted in stays resident, and a Darwin board
//! must use `/usr/bin/time -l` instead of claiming eviction here.
//!
//! `ru_maxrss` counts KiB on Linux and bytes on Darwin; the report is KiB on both.
//!
//! The child is started with `posix_spawn`, which shares this process's memory until the
//! exec, so the launcher's own pages never count toward the child's peak.

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;

/// What the command line asks for.
struct Request {
    evict: Vec<OsString>,
    command: Vec<OsString>,
}

/// The exit status a shell reports for a process a signal ended.
const SIGNAL_STATUS_BASE: i32 = 128;

/// The status `rss-exec` itself exits with when it cannot run the command.
const USAGE_STATUS: i32 = 2;

fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Request, String> {
    let mut evict = Vec::new();
    let mut arguments = arguments.peekable();
    loop {
        match arguments.next() {
            Some(argument) if argument == "--evict" => {
                evict.push(arguments.next().ok_or("--evict needs a path")?);
            }
            Some(argument) if argument == "--" => break,
            Some(argument) => return Err(format!("unexpected argument {argument:?}")),
            None => return Err("missing -- COMMAND".into()),
        }
    }
    let command: Vec<OsString> = arguments.collect();
    if command.is_empty() {
        return Err("missing COMMAND after --".into());
    }
    Ok(Request { evict, command })
}

/// Drops `path`'s pages from the page cache; a path that cannot be opened is reported.
#[cfg(target_os = "linux")]
fn evict(path: &OsString) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{path:?}: {error}"))?;
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor is open for the call; advice changes no file contents.
    let result =
        unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    if result != 0 {
        return Err(format!("{path:?}: posix_fadvise failed ({result})"));
    }
    Ok(())
}

/// Darwin has no per-file page-cache eviction; `F_NOCACHE` keeps this descriptor from
/// populating the cache and leaves already-resident pages alone.
#[cfg(target_vendor = "apple")]
fn evict(path: &OsString) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path).map_err(|error| format!("{path:?}: {error}"))?;
    // SAFETY: the descriptor is open for the call; the flag changes no file contents.
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1) };
    if result < 0 {
        return Err(format!("{path:?}: F_NOCACHE failed ({})", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn c_string(argument: &OsString) -> Result<CString, String> {
    CString::new(argument.as_bytes()).map_err(|_| format!("{argument:?} contains a NUL byte"))
}

/// Spawns `command`, searching PATH, and waits for it; returns its status and peak RSS.
fn run(command: &[OsString]) -> Result<(i32, i64), String> {
    let arguments = command.iter().map(c_string).collect::<Result<Vec<_>, _>>()?;
    let mut argv: Vec<*mut libc::c_char> =
        arguments.iter().map(|argument| argument.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    unsafe extern "C" {
        static environ: *const *mut libc::c_char;
    }
    let mut pid: libc::pid_t = 0;
    // SAFETY: argv is a null-terminated array of C strings that outlive the call, and
    // `environ` is the process environment.
    let spawned = unsafe {
        libc::posix_spawnp(
            &mut pid,
            arguments[0].as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            argv.as_ptr(),
            environ,
        )
    };
    if spawned != 0 {
        return Err(format!(
            "{:?}: {}",
            command[0],
            std::io::Error::from_raw_os_error(spawned)
        ));
    }
    let mut status = 0;
    // SAFETY: an all-zero rusage is a valid value for wait4 to overwrite.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: wait4 writes the status and usage of the child it reaps.
        let reaped = unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
        if reaped == pid {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("wait4: {error}"));
        }
    }
    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        SIGNAL_STATUS_BASE + libc::WTERMSIG(status)
    };
    Ok((code, maxrss_kib(&usage)))
}

/// Bytes in one `ru_maxrss` unit: Linux counts KiB, Darwin counts bytes.
#[cfg(target_os = "linux")]
const MAXRSS_UNIT_BYTES: i64 = 1024;
#[cfg(target_vendor = "apple")]
const MAXRSS_UNIT_BYTES: i64 = 1;

/// The report's unit, KiB, which every board reads.
const BYTES_PER_KIB: i64 = 1024;

/// `usage`'s peak resident set in KiB, whatever unit the platform counts.
fn maxrss_kib(usage: &libc::rusage) -> i64 {
    usage.ru_maxrss * MAXRSS_UNIT_BYTES / BYTES_PER_KIB
}

fn main() {
    let request = match parse(std::env::args_os().skip(1)) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("rss-exec: {message}\nusage: rss-exec [--evict PATH]... -- COMMAND [ARGUMENT]...");
            std::process::exit(USAGE_STATUS);
        }
    };
    for path in &request.evict {
        if let Err(message) = evict(path) {
            eprintln!("rss-exec: evict {message}");
            std::process::exit(USAGE_STATUS);
        }
    }
    match run(&request.command) {
        Ok((status, maxrss_kib)) => {
            eprintln!("rss-exec maxrss_kib={maxrss_kib} status={status}");
            std::process::exit(status);
        }
        Err(message) => {
            eprintln!("rss-exec: {message}");
            std::process::exit(USAGE_STATUS);
        }
    }
}
