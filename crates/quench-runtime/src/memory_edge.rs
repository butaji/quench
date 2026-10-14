#[cfg(target_os = "macos")]
pub fn report_allocator_memory(phase: &str) {
    use std::ffi::{c_int, c_uint, c_void};
    #[repr(C)]
    #[derive(Default)]
    struct MallocStatistics {
        blocks_in_use: u32,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }
    unsafe extern "C" {
        fn malloc_default_zone() -> *mut c_void;
        fn malloc_zone_statistics(zone: *mut c_void, statistics: *mut MallocStatistics);
        fn mach_task_self() -> c_uint;
        fn task_info(task: c_uint, flavor: c_uint, info: *mut c_int, count: *mut c_uint) -> c_int;
        fn getpagesize() -> c_int;
    }
    let mut malloc = MallocStatistics::default();
    let mut task = MachTaskBasicInfo::default();
    let mut count = (size_of::<MachTaskBasicInfo>() / size_of::<c_int>()) as c_uint;
    // SAFETY: all buffers use the documented macOS ABI layouts and counts;
    // the queried malloc zone and Mach task are the current process's own.
    unsafe {
        malloc_zone_statistics(malloc_default_zone(), &mut malloc);
        let _ = task_info(
            mach_task_self(),
            20,
            (&mut task as *mut MachTaskBasicInfo).cast(),
            &mut count,
        );
        let page_size = getpagesize() as u64;
        eprintln!(
            "{{\"kind\":\"quench-allocator-memory\",\"phase\":\"{phase}\",\"blocks_in_use\":{},\"size_in_use\":{},\"max_size_in_use\":{},\"size_allocated\":{},\"resident_bytes\":{},\"resident_pages\":{},\"peak_resident_bytes\":{}}}",
            malloc.blocks_in_use,
            malloc.size_in_use,
            malloc.max_size_in_use,
            malloc.size_allocated,
            task.resident_size,
            task.resident_size / page_size,
            task.resident_size_max,
        );
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu", feature = "profile-memory"))]
pub fn report_allocator_memory(phase: &str) {
    #[repr(C)]
    struct MallInfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }

    unsafe extern "C" {
        fn mallinfo2() -> MallInfo2;
    }

    fn smaps_rollup_kilobytes(field: &str) -> Option<usize> {
        let contents = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
        let line = contents.lines().find(|line| line.starts_with(field))?;
        let kilobytes = line.split_whitespace().nth(1)?.parse::<usize>().ok()?;
        Some(kilobytes * 1024)
    }

    // SAFETY: mallinfo2 returns glibc allocator counters for the current process.
    let malloc = unsafe { mallinfo2() };
    let rss_bytes = smaps_rollup_kilobytes("Rss:");
    let anonymous_bytes = smaps_rollup_kilobytes("Anonymous:");
    let private_dirty_bytes = smaps_rollup_kilobytes("Private_Dirty:");
    eprintln!(
        "{{\"kind\":\"quench-allocator-memory\",\"phase\":\"{phase}\",\"arena_bytes\":{},\"free_chunks\":{},\"small_free_chunks\":{},\"mapped_chunks\":{},\"mapped_bytes\":{},\"max_in_use_bytes\":{},\"small_free_bytes\":{},\"in_use_bytes\":{},\"free_bytes\":{},\"releasable_bytes\":{},\"rss_bytes\":{},\"anonymous_bytes\":{},\"private_dirty_bytes\":{}}}",
        malloc.arena,
        malloc.ordblks,
        malloc.smblks,
        malloc.hblks,
        malloc.hblkhd,
        malloc.usmblks,
        malloc.fsmblks,
        malloc.uordblks,
        malloc.fordblks,
        malloc.keepcost,
        json_bytes(rss_bytes),
        json_bytes(anonymous_bytes),
        json_bytes(private_dirty_bytes),
    );
}

#[cfg(all(target_os = "linux", target_env = "gnu", feature = "profile-memory"))]
fn json_bytes(bytes: Option<usize>) -> String {
    bytes.map_or_else(|| "null".to_string(), |bytes| bytes.to_string())
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct MachTaskBasicInfo {
    virtual_size: u64,
    resident_size: u64,
    resident_size_max: u64,
    user_time: TimeValue,
    system_time: TimeValue,
    policy: i32,
    suspend_count: i32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct TimeValue {
    seconds: i32,
    microseconds: i32,
}

#[cfg(not(any(
    target_os = "macos",
    all(target_os = "linux", target_env = "gnu", feature = "profile-memory")
)))]
pub fn report_allocator_memory(_: &str) {}
