//! The executable's read-only pages (headers, `.rodata`, `.eh_frame` and `.text`, from
//! `__executable_start` to `etext`) are clean file-backed pages: bootstrap and compilation
//! fault in code and data that a script's run never touches again. Releasing them from the
//! process after compilation keeps only what the run touches resident; a released page that
//! is used again refaults from the page cache.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe extern "C" {
    static __executable_start: u8;
    static etext: u8;
}

/// Unmaps the executable's read-only pages from the resident set; the next touch of each
/// refaults it from the file.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) fn release_read_only_pages() {
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
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub(crate) fn release_read_only_pages() {}
