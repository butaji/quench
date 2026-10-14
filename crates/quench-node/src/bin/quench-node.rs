/// glibc starts serving allocations of this size and larger with their own mappings, and
/// raises the threshold each time such a mapping is freed, after which large allocations stay
/// in the heap and remain resident once freed. Pinning glibc's initial threshold keeps a freed
/// large allocation (a module's source, a compiler arena) returned to the system.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD_BYTES: libc::c_int = 128 * 1024;

/// The glibc tunable that, when set, already fixes the threshold.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD_VARIABLE: &str = "MALLOC_MMAP_THRESHOLD_";

fn main() -> std::process::ExitCode {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if std::env::var_os(MMAP_THRESHOLD_VARIABLE).is_none() {
        // SAFETY: mallopt only adjusts allocator parameters; no allocation is in flight.
        unsafe {
            libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD_BYTES);
        }
    }
    match quench_node::shared_run::run_shared_cli(std::env::args().skip(1)) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
