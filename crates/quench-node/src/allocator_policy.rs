//! The allocator settings a process's memory policy selects (`shared_run::select_memory_policy`).

use quench_runtime::MemoryPolicy;

/// Under the throughput policy (`MemoryPolicy::Throughput`), glibc's trim threshold is raised
/// from its 128 KiB default, which pinning the mmap threshold (quench-node's `main`) also pins:
/// once that much is free at the top of an arena it is returned (`MADV_DONTNEED` in a thread's
/// arena) and faulted back in by the next allocations. A server freeing and reallocating
/// stream chunks then spent its time in page faults (video route, 10 connections, 2026-10-22:
/// ~600K minor faults per 6 s; express 38 -> 58 and fastify 36 -> 67 requests/s with this
/// threshold, peak VmHWM within 1-4 MB). Freed memory below it stays in the arena for reuse.
/// The memory-first policy keeps the default, which returns it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_BYTES: libc::c_int = 16 * 1024 * 1024;
/// The glibc variable that sets the trim threshold instead.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_VARIABLE: &str = "MALLOC_TRIM_THRESHOLD_";
/// Under the throughput policy, the size from which glibc maps a request on its own: its
/// largest (`DEFAULT_MMAP_THRESHOLD_MAX` on 64-bit), so a large block freed and requested
/// again (a multi-megabyte `JSON.stringify` result per call) is reused from the arena instead
/// of mapped and faulted in each time. JetStream 3.0 --jit, back to back (x86_64 Linux VM, 4
/// cores, 2026-11-10): json-stringify-inspector 243 -> 301 with max RSS x0.78,
/// json-parse-inspector RSS x0.89, acorn/babylon/typescript-lib/web-ssr within noise, RSS
/// x1.00-1.01. The memory-first policy keeps `main`'s 1 MiB pin, which returns such blocks.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const THROUGHPUT_MMAP_THRESHOLD_BYTES: libc::c_int = 32 * 1024 * 1024;

/// The allocator setting the process's `policy` takes, applied once when it is selected
/// (`select_memory_policy`); an explicit `TRIM_THRESHOLD_VARIABLE` wins.
///
/// Throughput serves small requests from Quench's size-classed allocator, which the runtime's
/// bulk frees and reallocations of the same sizes favor: wall time on npm scenarios -4 to -15%
/// (x86_64 Linux VM, 4 cores, 2026-10-20: chalk 125 -> 107 ms, typescript 576 -> 505), and
/// JetStream bigint-noble 8.28 -> 13.87, json-parse-inspector 109.8 -> 133.1 (TOD-167).
/// Memory-first keeps the system allocator: the size classes' partly used pages raised V8-v7
/// max RSS (static x86_64, unbolted, 2026-10-18, KiB): Crypto 4272 -> 4640, Richards 3716 ->
/// 4064, RayTrace 3884 -> 4128, DeltaBlue 3840 -> 4120, RegExp 7612 -> 7980. Blocks the system
/// allocator handed out before the selection stay its own (`quench_alloc::owns`).
pub(crate) fn apply(policy: MemoryPolicy) {
    #[cfg(unix)]
    if policy == MemoryPolicy::Throughput {
        quench_alloc::QuenchAlloc::enable();
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if policy == MemoryPolicy::Throughput && std::env::var_os(TRIM_THRESHOLD_VARIABLE).is_none() {
        // SAFETY: mallopt only adjusts allocator parameters.
        unsafe {
            libc::mallopt(libc::M_TRIM_THRESHOLD, TRIM_THRESHOLD_BYTES);
        }
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if policy == MemoryPolicy::Throughput && std::env::var_os(crate::host::process_start::MMAP_THRESHOLD_VARIABLE).is_none() {
        // SAFETY: mallopt only adjusts allocator parameters.
        unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, THROUGHPUT_MMAP_THRESHOLD_BYTES) };
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = policy;
}
