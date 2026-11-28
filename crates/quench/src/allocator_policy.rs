//! The allocator settings a process's memory policy selects (`shared_run::select_memory_policy`).

use quench_runtime::MemoryPolicy;

/// Under the throughput policy (`MemoryPolicy::Throughput`), glibc's trim threshold is raised
/// from its 128 KiB default, which pinning the mmap threshold (quench's `main`) also pins:
/// once that much is free at the top of an arena it is returned (`MADV_DONTNEED` in a thread's
/// arena) and faulted back in by the next allocations. A server freeing and reallocating
/// stream chunks then spent its time in page faults (video route, 10 connections, 2026-10-22:
/// ~600K minor faults per 6 s; express 38 -> 58 and fastify 36 -> 67 requests/s with this
/// threshold, peak VmHWM within 1-4 MB). Freed memory below it stays in the arena for reuse.
/// At 16 MiB a run that compiles and frees as it goes kept that much of freed compiler memory
/// resident at its peak in the worker's arena: JetStream 3 no-JIT (2026-11-15) Sunspider 25.8
/// -> 22.6 MB and json-parse-inspector 55.0 -> 48.1 MB at 1 MiB, other rows and scores flat;
/// a 64 KiB-chunk HTTP stream (10 connections) shows no fault or time change at 1 MiB.
/// The memory-first policy keeps the default, which returns it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_BYTES: libc::c_int = 1024 * 1024;
/// The glibc variable that sets the trim threshold instead.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_VARIABLE: &str = "MALLOC_TRIM_THRESHOLD_";

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
    if policy == MemoryPolicy::Throughput && !quench_runtime::vm::tier_policy::memory_mix_has("alloc") {
        quench_alloc::QuenchAlloc::enable();
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if policy == MemoryPolicy::Throughput
        && !quench_runtime::vm::tier_policy::memory_mix_has("trim")
        && std::env::var_os(TRIM_THRESHOLD_VARIABLE).is_none()
    {
        // SAFETY: mallopt only adjusts allocator parameters.
        unsafe {
            libc::mallopt(libc::M_TRIM_THRESHOLD, TRIM_THRESHOLD_BYTES);
        }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = policy;
}
