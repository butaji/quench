//! Start-up memory policy for every quench executable: the CLI and each native image expand
//! [`install_process_start!`](crate::install_process_start) and call [`begin_main`] first, so a
//! compiled program keeps resident only what it touches, as the CLI does.

/// glibc serves allocations of this size and larger with their own mappings, and would raise
/// the threshold each time such a mapping is freed, after which large allocations stay in the
/// heap and remain resident once freed. Pinning it keeps a freed large allocation (a module's
/// source, a compiler arena) returned to the system; at glibc's initial 128 KiB each mapped
/// allocation refaulted all its pages (no-JIT x86_64, 2026-11-08, minor faults 128 KiB -> 1 MiB:
/// web-ssr 36K -> 30K, first-inspector-code-load 24K -> 18K, Sunspider 50K -> 37K, typescript-lib
/// 122K -> 119K; max RSS +0-2 MB, pdfjs 39 -> 29 MB). 4 MiB peaked 3-4 MB higher on mandreel.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD_BYTES: libc::c_int = 1 << 20;

/// The glibc tunable that, when set, already fixes the threshold.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD_VARIABLE: &str = "MALLOC_MMAP_THRESHOLD_";

/// Runs from `.init_array`, before the Rust runtime and `main`: from here on a fault on the
/// read-only file mappings maps only the touched page, and the pages the C runtime's start
/// faulted in with their neighbours are released.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub extern "C" fn fault_only_touched_pages() {
    super::resident_pages::phase_peak("load");
    super::resident_pages::map_touched_pages_only();
    super::resident_pages::release_read_only_pages();
    quench_runtime::set_compile_phase_boundary(super::resident_pages::at_compile_phase);
}

/// Reports the last phase's peak at exit (`resident_pages::phase_peak`).
extern "C" fn report_exit_peak() {
    super::resident_pages::phase_peak("run to exit");
}

/// The start of `main`: the exit report, the CLI's signal dispositions and the allocator's
/// mapping threshold.
pub fn begin_main() -> Result<(), String> {
    // SAFETY: registers a handler that only reads and writes this process's proc files.
    unsafe { libc::atexit(report_exit_peak) };
    #[cfg(target_os = "linux")]
    crate::shared_run::initialize_cli_process_signals()
        .map_err(|error| format!("initialize CLI process signals: {error}"))?;
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if std::env::var_os(MMAP_THRESHOLD_VARIABLE).is_none() {
        // SAFETY: mallopt only adjusts allocator parameters; no allocation is in flight.
        unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD_BYTES) };
    }
    Ok(())
}

/// Installs [`fault_only_touched_pages`] in the expanding executable's `.init_array`. A library
/// cannot: its hook would run in every binary that links it, test runners included.
#[macro_export]
macro_rules! install_process_start {
    () => {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        #[used]
        #[unsafe(link_section = ".init_array")]
        static FAULT_ONLY_TOUCHED_PAGES: extern "C" fn() =
            $crate::host::process_start::fault_only_touched_pages;
    };
}
