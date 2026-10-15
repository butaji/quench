//! Read-only file pages are clean: bootstrap and compilation fault in code and constant data
//! that a script's run never touches again. Releasing them from the process after compilation
//! keeps only what the run touches resident; a released page that is used again refaults from
//! the page cache. Two ranges qualify: the executable's own read-only segments (headers,
//! `.rodata`, `.eh_frame` and `.text`, from `__executable_start` to `etext`, linked at a fixed
//! address with nothing relocated in them) and every library's code mapping. A library's
//! read-only data mappings do not: their RELRO pages hold relocations the loader wrote.
//!
//! Faults on these mappings map only the touched page (`map_touched_pages_only`), so what stays
//! resident is what bootstrap, compilation and the run touch, without their cached neighbours.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe extern "C" {
    static __executable_start: u8;
    static etext: u8;
}

/// Marks the point where the entry has compiled and its run begins. It does nothing: profiling
/// splits there (`valgrind --tool=callgrind --dump-before=quench_entry_compiled`), so the trained
/// link order can lay the functions a run executes together, ahead of start-up code.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn quench_entry_compiled() {
    std::hint::black_box(());
}

/// The process's own mapping table.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const MAPS_PATH: &str = "/proc/self/maps";

/// The permissions of a private, read-only, executable mapping in `MAPS_PATH`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CODE_MAPPING: &str = "r-xp";

/// Unmaps the read-only pages above from the resident set; the next touch of each refaults it
/// from its file.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_read_only_pages() {
    // SAFETY: the linker defines both symbols at the bounds of the read-only segments, which
    // are never written, so discarding their pages loses nothing.
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let start = &raw const __executable_start as usize;
        let end = &raw const etext as usize;
        let start = start - start % page;
        let length = (end - start) - (end - start) % page;
        libc::madvise(start as *mut libc::c_void, length, libc::MADV_DONTNEED);
    }
    for (start, end) in file_mappings(&[CODE_MAPPING]) {
        // SAFETY: a private, read-only, executable file mapping is never written.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

/// The address ranges of the process's file mappings (not the kernel's bracketed ones) whose
/// permissions are one of `permissions`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn file_mappings(permissions: &[&str]) -> Vec<(usize, usize)> {
    let Ok(maps) = std::fs::read_to_string(MAPS_PATH) else {
        return Vec::new();
    };
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (range, mode) = (fields.next()?, fields.next()?);
            // Offset, device and inode precede the path; a bracketed name ([vdso]) is the
            // kernel's and an anonymous mapping has none.
            let path = fields.nth(3)?;
            if !permissions.contains(&mode) || !path.starts_with('/') {
                return None;
            }
            let (start, end) = range.split_once('-')?;
            Some((usize::from_str_radix(start, 16).ok()?, usize::from_str_radix(end, 16).ok()?))
        })
        .collect()
}

/// The permissions of a private, read-only data mapping in `MAPS_PATH`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const DATA_MAPPING: &str = "r--p";

/// Faults on the read-only file mappings map only the touched page. A read fault on a file
/// mapping otherwise maps its cached neighbours too (fault-around, 64 KB, or whole large
/// folios), so code and constants a run never touches stay resident beside what it does. The
/// kernel skips fault-around in a mapping registered with userfaultfd for write protection;
/// these mappings are never written, and asynchronous write protection
/// (`UFFD_FEATURE_WP_ASYNC`, Linux 6.7) needs no handler, so the registration only changes
/// fault-around. The descriptor stays open for the process's life (closing it unregisters);
/// without the feature the mappings keep fault-around.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn map_touched_pages_only() {
    /// `struct uffdio_api` and `struct uffdio_register` from `<linux/userfaultfd.h>`.
    #[repr(C)]
    struct UffdioApi {
        api: u64,
        features: u64,
        ioctls: u64,
    }
    #[repr(C)]
    struct UffdioRegister {
        start: u64,
        len: u64,
        mode: u64,
        ioctls: u64,
    }
    const UFFD_API: u64 = 0xaa;
    const UFFD_USER_MODE_ONLY: libc::c_int = 1;
    const UFFD_FEATURE_WP_ASYNC: u64 = 1 << 15;
    const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
    /// `_IOWR(UFFDIO, _UFFDIO_API, struct uffdio_api)` and
    /// `_IOWR(UFFDIO, _UFFDIO_REGISTER, struct uffdio_register)`.
    const UFFDIO_API: libc::c_ulong = 0xc018_aa3f;
    const UFFDIO_REGISTER: libc::c_ulong = 0xc020_aa00;
    /// The registration is per process; a second one would only open another descriptor.
    static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if REGISTERED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // SAFETY: the descriptor is fresh; both ioctls read and write only the structs passed.
    unsafe {
        let fd = libc::syscall(
            libc::SYS_userfaultfd,
            libc::O_CLOEXEC | libc::O_NONBLOCK | UFFD_USER_MODE_ONLY,
        ) as libc::c_int;
        if fd < 0 {
            return;
        }
        let mut api = UffdioApi { api: UFFD_API, features: UFFD_FEATURE_WP_ASYNC, ioctls: 0 };
        if libc::ioctl(fd, UFFDIO_API, &mut api) != 0 {
            libc::close(fd);
            return;
        }
        for (start, end) in file_mappings(&[CODE_MAPPING, DATA_MAPPING]) {
            let mut range = UffdioRegister {
                start: start as u64,
                len: (end - start) as u64,
                mode: UFFDIO_REGISTER_MODE_WP,
                ioctls: 0,
            };
            libc::ioctl(fd, UFFDIO_REGISTER, &mut range);
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn map_touched_pages_only() {}

/// Stack below the caller's frame that a release keeps, for the calls it makes and the
/// signal frames the kernel may push.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const STACK_RELEASE_MARGIN_BYTES: usize = 64 * 1024;

/// Releases the calling thread's stack pages below its current frame. Bootstrap and
/// compilation recurse deeply once, and the pages they reached stay resident though only the
/// shallow frames above them run again; the next deep call refaults zeroed pages.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_unused_stack() {
    // SAFETY: the attribute object is initialized by pthread_getattr_np before use and
    // destroyed after; the released range lies below the live frames and their margin, which
    // nothing references.
    unsafe {
        let mut attributes = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
        if libc::pthread_getattr_np(libc::pthread_self(), attributes.as_mut_ptr()) != 0 {
            return;
        }
        let mut attributes = attributes.assume_init();
        let mut low = std::ptr::null_mut::<libc::c_void>();
        let mut size = 0usize;
        let found = libc::pthread_attr_getstack(&attributes, &mut low, &mut size) == 0;
        libc::pthread_attr_destroy(&mut attributes);
        if !found {
            return;
        }
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let frame = std::hint::black_box(&page) as *const usize as usize;
        let start = (low as usize).div_ceil(page) * page;
        let end = frame.saturating_sub(STACK_RELEASE_MARGIN_BYTES) / page * page;
        if end > start {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_unused_stack() {}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_read_only_pages() {}
