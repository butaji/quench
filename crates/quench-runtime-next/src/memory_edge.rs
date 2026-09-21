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
            "{{\"kind\":\"rqj-allocator-memory\",\"phase\":\"{phase}\",\"blocks_in_use\":{},\"size_in_use\":{},\"max_size_in_use\":{},\"size_allocated\":{},\"resident_bytes\":{},\"resident_pages\":{},\"peak_resident_bytes\":{}}}",
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

#[cfg(not(target_os = "macos"))]
pub fn report_allocator_memory(_: &str) {}
