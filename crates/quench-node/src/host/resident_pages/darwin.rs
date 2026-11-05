//! Drop the main image's resident translations without replacing its backing object.

use std::sync::Mutex;

// Darwin's sys/proc_info.h and mach/vm_inherit.h. libc does not expose these names.
const PROC_PIDREGIONINFO: i32 = 7;
const VM_INHERIT_COPY: i32 = 1;
const PROC_REGION_SUBMAP: u32 = 1;
const PROC_REGION_SHARED: u32 = 2;
const TEXT_PROTECTION: i32 = libc::PROT_READ | libc::PROT_EXEC;
static RELEASE: Mutex<()> = Mutex::new(());

/// Darwin's proc_regioninfo output layout; all fields are integer values.
#[repr(C)]
#[derive(Default)]
struct Region {
    protection: u32,
    max_protection: u32,
    inheritance: u32,
    flags: u32,
    offset: u64,
    behavior: u32,
    user_wired_count: u32,
    user_tag: u32,
    pages_resident: u32,
    pages_shared_now_private: u32,
    pages_swapped_out: u32,
    pages_dirtied: u32,
    ref_count: u32,
    shadow_depth: u32,
    share_mode: u32,
    private_pages_resident: u32,
    shared_pages_resident: u32,
    object_id: u32,
    depth: u32,
    address: u64,
    size: u64,
}

unsafe extern "C" {
    fn getsegmentdata(
        header: *const libc::c_void,
        name: *const libc::c_char,
        size: *mut usize,
    ) -> *mut u8;
    fn mach_vm_remap(
        target_task: libc::mach_port_t,
        target_address: *mut u64,
        size: u64,
        mask: u64,
        flags: i32,
        source_task: libc::mach_port_t,
        source_address: u64,
        copy: i32,
        current_protection: *mut i32,
        max_protection: *mut i32,
        inheritance: i32,
    ) -> libc::kern_return_t;
}

impl Region {
    fn read(start: usize) -> Option<Self> {
        let mut region = Self::default();
        // SAFETY: the flavor and buffer describe proc_regioninfo from Darwin's SDK.
        let count = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                PROC_PIDREGIONINFO,
                start as u64,
                (&mut region as *mut Self).cast(),
                size_of::<Self>() as i32,
            )
        };
        (count as usize == size_of::<Self>()).then_some(region)
    }

    fn is_main_text(&self, start: usize, size: usize, page: usize) -> bool {
        page.is_power_of_two()
            && start != 0
            && size != 0
            && start.is_multiple_of(page)
            && size.is_multiple_of(page)
            && start.checked_add(size).is_some()
            && self.address == start as u64
            && self.size == size as u64
            && self.offset == 0
            && self.protection == TEXT_PROTECTION as u32
            && self.max_protection == TEXT_PROTECTION as u32
            && self.inheritance == VM_INHERIT_COPY as u32
            && self.flags & (PROC_REGION_SUBMAP | PROC_REGION_SHARED) == 0
            && self.user_wired_count == 0
    }
}

#[allow(deprecated)] // libc's public Darwin task/dyld declarations match the SDK.
fn remap_main_text() -> bool {
    let mut size = 0;
    // SAFETY: dyld owns the main image and getsegmentdata reads its load commands.
    let (header, start, page) = unsafe {
        let header = libc::_dyld_get_image_header(0);
        if header.is_null() {
            return false;
        }
        (
            header,
            getsegmentdata(header.cast(), c"__TEXT".as_ptr(), &mut size) as usize,
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    let Ok(page) = usize::try_from(page) else {
        return false;
    };
    if start != header as usize
        || Region::read(start).is_none_or(|region| !region.is_main_text(start, size, page))
    {
        return false;
    }
    let mut target = start as u64;
    let (mut current, mut maximum) = (0, 0);
    // SAFETY: this is one complete, unwired, private RX/RX main-image region, not a
    // range spanning independently protected entries. The kernel extracts its backing
    // before replacing the same address under the map lock. Copy mode preserves COW
    // contents and protections; no PROT_NONE interval or writable mapping is introduced.
    // Other release requests are serialized. Unsupported mappings are left alone.
    let status = unsafe {
        let task = libc::mach_task_self();
        mach_vm_remap(
            task,
            &mut target,
            size as u64,
            0,
            libc::VM_FLAGS_FIXED | libc::VM_FLAGS_OVERWRITE,
            task,
            start as u64,
            i32::from(true),
            &mut current,
            &mut maximum,
            VM_INHERIT_COPY,
        )
    };
    status == libc::KERN_SUCCESS
}

pub(super) fn release_main_text() {
    // Concurrent workers need not wait or repeat a release already in progress.
    let Ok(_release) = RELEASE.try_lock() else {
        return;
    };
    remap_main_text();
}

#[cfg(test)]
mod tests {
    use super::*;
    const VM_INHERIT_NONE: u32 = 2; // Darwin mach/vm_inherit.h.

    #[test]
    fn admission_requires_one_complete_private_unwired_text_region() {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let start = page;
        let make = || Region {
            address: start as u64,
            size: page as u64,
            protection: TEXT_PROTECTION as u32,
            max_protection: TEXT_PROTECTION as u32,
            inheritance: VM_INHERIT_COPY as u32,
            ..Region::default()
        };
        assert!(make().is_main_text(start, page, page));
        for mutate in [
            |r: &mut Region| r.size *= 2,
            |r: &mut Region| r.address += 1,
            |r: &mut Region| r.offset = 1,
            |r: &mut Region| r.protection |= libc::PROT_WRITE as u32,
            |r: &mut Region| r.max_protection |= libc::PROT_WRITE as u32,
            |r: &mut Region| r.flags |= PROC_REGION_SUBMAP,
            |r: &mut Region| r.flags |= PROC_REGION_SHARED,
            |r: &mut Region| r.user_wired_count = 1,
            |r: &mut Region| r.inheritance = VM_INHERIT_NONE,
        ] {
            let mut region = make();
            mutate(&mut region);
            assert!(!region.is_main_text(start, page, page));
        }
        assert!(!make().is_main_text(start, page, 0));
        assert!(!make().is_main_text(start + 1, page, page));
    }

    #[test]
    fn remapping_live_text_preserves_execution_and_mapping_contract() {
        let _release = RELEASE.lock().unwrap();
        assert!(
            remap_main_text(),
            "the test host must admit its live main text"
        );
        // Admission after remapping checks the complete region and unchanged permissions.
        assert!(remap_main_text());
    }
}
