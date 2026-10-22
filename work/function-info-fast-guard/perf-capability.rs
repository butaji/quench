// Linux UAPI perf_event_attr version 0 prefix and named perf_event.h values.
#[repr(C)]
struct PerfAttr {
    kind: u32, size: u32, config: u64, sample_period: u64,
    sample_type: u64, read_format: u64, flags: u64,
    wakeup_events: u32, bp_type: u32, bp_addr: u64,
}
const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
const DISABLED: u64 = 1;
const EXCLUDE_KERNEL: u64 = 1 << 5;
const EXCLUDE_HV: u64 = 1 << 6;
const PERF_FLAG_FD_CLOEXEC: u64 = 1 << 3;
fn main() {
    let attr = PerfAttr { kind: PERF_TYPE_HARDWARE,
        size: std::mem::size_of::<PerfAttr>() as u32,
        config: PERF_COUNT_HW_INSTRUCTIONS, sample_period: 0,
        sample_type: 0, read_format: 0,
        flags: DISABLED | EXCLUDE_KERNEL | EXCLUDE_HV,
        wakeup_events: 0, bp_type: 0, bp_addr: 0 };
    let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr, 0_i32, -1_i32, -1_i32, PERF_FLAG_FD_CLOEXEC) };
    if fd < 0 { println!("perf_event_open(instructions,user-only): {}", std::io::Error::last_os_error()); }
    else { println!("perf_event_open(instructions,user-only): available"); unsafe {libc::close(fd as i32); } }
}
