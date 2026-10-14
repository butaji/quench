//! Read-only file pages are clean: bootstrap and compilation fault in code and constant data
//! that a script's run never touches again. Releasing them from the process after compilation
//! keeps only what the run touches resident; a released page that is used again refaults from
//! the page cache. Two ranges qualify: the executable's own read-only segments (headers,
//! `.rodata`, `.eh_frame` and `.text`, from `__executable_start` to `etext`, linked at a fixed
//! address with nothing relocated in them) and every library's code mapping. A library's
//! read-only data mappings do not: their RELRO pages hold relocations the loader wrote.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe extern "C" {
    static __executable_start: u8;
    static etext: u8;
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
    let Ok(maps) = std::fs::read_to_string(MAPS_PATH) else {
        return;
    };
    for line in maps.lines() {
        let mut fields = line.split_whitespace();
        let (Some(range), Some(permissions)) = (fields.next(), fields.next()) else {
            continue;
        };
        // Offset, device and inode precede the path; a bracketed name ([vdso]) is the
        // kernel's and an anonymous mapping has none.
        let Some(path) = fields.nth(3) else {
            continue;
        };
        if permissions != CODE_MAPPING || !path.starts_with('/') {
            continue;
        }
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (usize::from_str_radix(start, 16), usize::from_str_radix(end, 16))
        else {
            continue;
        };
        // SAFETY: a private, read-only, executable file mapping is never written.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub(crate) fn release_read_only_pages() {}
