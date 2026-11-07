//! An opt-in census of executable-text pages mapped into this task. Darwin's ordinary
//! page queries include backing-file cache residency; footprint query mode asks its pmap
//! instead. This reports resident code bytes, neither exclusive time nor peak RSS.

use std::ffi::{c_char, c_void};

const FOOTPRINT_MODE: &std::ffi::CStr = c"vm.self_region_footprint";
static QUERY_MODE: std::sync::Mutex<()> = std::sync::Mutex::new(());

unsafe extern "C" {
    // Darwin's <mach-o/getsect.h>; the header is the loaded 64-bit main image.
    fn getsegmentdata(header: *const c_void, name: *const c_char, size: *mut usize) -> *mut u8;
    // Darwin's <mach/mach_vm.h>.
    fn mach_vm_page_query(
        task: libc::mach_port_t,
        address: u64,
        disposition: *mut i32,
        references: *mut i32,
    ) -> libc::kern_return_t;
}

/// Restores the task's query mode on every exit; QUERY_MODE serializes our collectors.
struct FootprintQuery(i32);

impl FootprintQuery {
    fn enter() -> Option<Self> {
        let mut previous = 0_i32;
        let mut size = size_of::<i32>();
        let enabled = i32::from(true);
        // SAFETY: sysctl reads/writes Darwin's integer-sized per-task query setting.
        let status = unsafe {
            libc::sysctlbyname(
                FOOTPRINT_MODE.as_ptr(),
                (&mut previous as *mut i32).cast(),
                &mut size,
                (&enabled as *const i32).cast_mut().cast(),
                size_of::<i32>(),
            )
        };
        (status == 0).then(|| Self(previous))
    }
}

impl Drop for FootprintQuery {
    fn drop(&mut self) {
        // SAFETY: restore the saved integer value, with no output buffer.
        let status = unsafe {
            libc::sysctlbyname(
                FOOTPRINT_MODE.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                (&self.0 as *const i32).cast_mut().cast(),
                size_of::<i32>(),
            )
        };
        assert_eq!(status, 0, "cannot restore memory diagnostic query mode");
    }
}

#[allow(deprecated)] // libc's dyld declaration matches Darwin's public header.
pub(super) fn report(phase: &str) {
    if !crate::trace::Channel::CodeResidency.on() {
        return;
    }
    let mut size = 0;
    // SAFETY: dyld owns the loaded image header and getsegmentdata reads its commands.
    let (header, start, page) = unsafe {
        let header = libc::_dyld_get_image_header(0);
        if header.is_null() {
            return;
        }
        let start = getsegmentdata(header.cast(), c"__TEXT".as_ptr(), &mut size);
        let page = libc::sysconf(libc::_SC_PAGESIZE);
        (header, start, page)
    };
    let Ok(page) = usize::try_from(page) else {
        return;
    };
    if start.is_null() || size == 0 || !page.is_power_of_two() || !(start as usize).is_multiple_of(page) {
        return;
    }
    let Ok(_serial) = QUERY_MODE.lock() else {
        return;
    };
    let Some(_mode) = FootprintQuery::enter() else {
        return;
    };
    let mut offsets = Vec::new();
    for offset in (0..size).step_by(page) {
        let mut disposition = 0;
        let mut references = 0;
        // SAFETY: query a page within the loaded segment of the current task.
        let status = unsafe {
            mach_vm_page_query(
                libc::mach_task_self(),
                (start as usize + offset) as u64,
                &mut disposition,
                &mut references,
            )
        };
        if status != libc::KERN_SUCCESS {
            return;
        }
        if disposition & libc::VM_PAGE_QUERY_PAGE_PRESENT != 0 {
            offsets.push(offset);
        }
    }
    eprintln!(
        "{{\"kind\":\"quench-code-residency\",\"phase\":\"{phase}\",\"image_base\":{},\"segment_start\":{},\"segment_bytes\":{size},\"page_bytes\":{page},\"resident_page_offsets\":{offsets:?}}}",
        header as usize, start as usize,
    );
}
