//! Read-only file pages are clean: bootstrap and compilation fault in code and constant data
//! that a script's run never touches again. Releasing them from the process after compilation
//! keeps only what the run touches resident; a released page that is used again refaults from
//! the page cache. Two kinds of segment qualify: the executable's read-only segments (headers,
//! `.rodata`, `.eh_frame` and `.text`, linked at a fixed address with nothing relocated in
//! them) and every library's code segment. A library's read-only data does not: its RELRO
//! pages hold relocations the loader wrote.
//!
//! Faults on these mappings map only the touched page (`map_touched_pages_only`), so what stays
//! resident is what bootstrap, compilation and the run touch, without their cached neighbours.

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

/// A release and the refaults of the code a run touches again after it cost about 1.2 ms
/// (about 730 pages at 1.7 µs, the lodash npm scenario, x86_64 VM, 2026-10-19). Releases after
/// library loads (`release_read_only_pages_after_load`) and at compilations' phase boundaries
/// (`at_compile_phase`) each come at most once per this interval,
/// which bounds that cost to about 1% of a run's time: loads that follow each other faster run
/// the same loader and compiler code, which a release would only refault (semver scenario
/// 926 -> 741 ms). Collections keep releasing at each boundary (`at_collection_boundary`):
/// even a 10 ms interval there raised V8-v7 NavierStokes max RSS 5236 -> 5516 KiB.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const LOAD_RELEASE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// When the read-only pages were last released.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static LAST_RELEASE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Releases the read-only pages after a library load unless they were released less than
/// `LOAD_RELEASE_INTERVAL` ago.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_read_only_pages_after_load() {
    let recent = LAST_RELEASE
        .lock()
        .ok()
        .and_then(|last| *last)
        .is_some_and(|last| last.elapsed() < LOAD_RELEASE_INTERVAL);
    if !recent {
        release_read_only_pages();
    }
}

/// Unmaps the read-only pages above from the resident set; the next touch of each refaults it
/// from its file.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_read_only_pages() {
    if let Ok(mut last) = LAST_RELEASE.lock() {
        *last = Some(std::time::Instant::now());
    }
    for (start, end) in read_only_segments() {
        // SAFETY: each segment is a private file mapping that is never written, so discarding
        // its pages loses nothing.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
    // Held while releasing, so no tracked mapping is unmapped meanwhile.
    if let Ok(mappings) = TRACKED_MAPPINGS.lock() {
        for &(start, length) in mappings.iter() {
            // SAFETY: a tracked mapping is a live, read-only private file mapping.
            unsafe {
                libc::madvise(start as *mut libc::c_void, length, libc::MADV_DONTNEED);
            }
        }
    }
}

/// Read-only file mappings the host made after start-up (compile-cache entries), as start
/// and length, released with the segments above.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static TRACKED_MAPPINGS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

/// Treats the read-only private file mapping at `start` as the segments above: faults map
/// only the touched page, and releases discard its pages. Call `untrack_read_only_mapping`
/// before unmapping it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn track_read_only_mapping(start: usize, length: usize) {
    // A mapping aligned to a huge page maps a whole huge folio of a cached file on one fault
    // (a 5 MB startup snapshot was 4 MB resident after its records were first read).
    // SAFETY: advice on a live mapping the caller owns; it changes only how faults map.
    unsafe {
        libc::madvise(start as *mut libc::c_void, length, libc::MADV_NOHUGEPAGE);
    }
    register_touched_pages_only(start, length);
    if let Ok(mut mappings) = TRACKED_MAPPINGS.lock() {
        mappings.push((start, length));
    }
}

/// Releases the whole pages of `bytes` when they lie in a mapping `track_read_only_mapping`
/// tracked; other bytes are left alone.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_tracked_bytes(bytes: &[u8]) {
    let page = page_size();
    let start = (bytes.as_ptr() as usize).next_multiple_of(page);
    let end = (bytes.as_ptr() as usize + bytes.len()) / page * page;
    if start >= end {
        return;
    }
    // Held while releasing, so the mapping is not unmapped meanwhile.
    let Ok(mappings) = TRACKED_MAPPINGS.lock() else {
        return;
    };
    if mappings.iter().any(|&(mapping, length)| mapping <= start && end <= mapping + length) {
        // SAFETY: the range lies in a live, read-only private file mapping, whose discarded
        // pages fault back in from the file.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
}

/// Unmaps a mapping `track_read_only_mapping` tracked, once no release can reach it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn unmap_tracked_mapping(start: usize, length: usize) {
    let mut mappings = TRACKED_MAPPINGS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    mappings.retain(|&mapping| mapping != (start, length));
    // SAFETY: the caller owns the mapping, and the held lock keeps releases off it.
    unsafe {
        libc::munmap(start as *mut libc::c_void, length);
    }
}

/// Releases the read-only pages and, besides them, the clean pages of the read-only data
/// mappings, for a compilation's phase boundaries: a small program's peak is its entry
/// compilation, when the parser's constants and the code generator's would be resident
/// together (V8-v7 max RSS, bolted static x86_64, 2026-10-20: Crypto 3096 -> 2956 KiB,
/// Richards 2512 -> 2368, RayTrace 2968 -> 2824). Collections do not page data out: a long
/// run refaults its data between them, at its heap's peak (RegExp +60-150 KiB). A data page
/// a relocation or an early write made private stays: `MADV_PAGEOUT` reclaims only what
/// it can drop or swap.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_read_only_code_and_data() {
    release_read_only_pages();
    for (start, end) in file_mappings(&[DATA_MAPPING]) {
        // SAFETY: paging out never changes a mapping's contents.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_PAGEOUT);
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_read_only_code_and_data() {}

/// The host's action at a compilation's phase boundaries (`set_compile_phase_boundary`), each
/// at most once per `LOAD_RELEASE_INTERVAL`. Code generation pages code and data out
/// (`release_read_only_code_and_data`); compilations that follow it quickly (functions a
/// program builds at run time with `new Function`, `eval` or `vm`) run the same parser and
/// code generator, which a release would only refault, and each release reads the whole
/// mapping table (7% of the `@testing-library/dom` npm scenario's instructions). A first call
/// releases the read-only pages, timed apart from the collections' releases, which come far
/// more often: a run that reports its result once at the end first calls its reporting code
/// then, at its heap's peak, with the code it ran since the last collection resident (V8-v7
/// RegExp).
pub fn at_compile_phase(phase: quench_runtime::CompilePhase) {
    match phase {
        quench_runtime::CompilePhase::CodeGeneration => {
            release_once_per_interval(CompileRelease::CodeGeneration, release_read_only_code_and_data)
        }
        quench_runtime::CompilePhase::FirstCall => {
            release_once_per_interval(CompileRelease::FirstCall, release_read_only_pages)
        }
    }
}

/// The compile-phase releases timed apart from each other (`at_compile_phase`).
#[derive(Clone, Copy)]
enum CompileRelease {
    CodeGeneration,
    FirstCall,
}

/// When each compile-phase release last ran, indexed by `CompileRelease`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static LAST_COMPILE_RELEASES: std::sync::Mutex<[Option<std::time::Instant>; 2]> =
    std::sync::Mutex::new([None; 2]);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn release_once_per_interval(release: CompileRelease, action: fn()) {
    let Ok(mut releases) = LAST_COMPILE_RELEASES.lock() else {
        return;
    };
    let last = &mut releases[release as usize];
    if last.is_some_and(|last| last.elapsed() < LOAD_RELEASE_INTERVAL) {
        return;
    }
    *last = Some(std::time::Instant::now());
    drop(releases);
    action();
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn release_once_per_interval(_: CompileRelease, _: fn()) {}

/// Releases before the entry's compilation, the start-up peak, and lets its own code
/// generation boundary release again whatever `LOAD_RELEASE_INTERVAL` says: only the
/// parser's and compiler's pages, not the bootstrap's, need be resident across it, and the
/// parser's not across its code generation. A bootstrap compilation within the interval had
/// held that boundary (V8-v7 max RSS, entry compiled with the bootstrap cached, bolted static
/// x86_64, 2026-10-22: Crypto 3100-3276 -> 2704-3052 KiB, RayTrace 2996 -> 2764, Richards
/// 2812 -> 2484).
pub fn release_before_entry_compile() {
    release_read_only_code_and_data();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if let Ok(mut releases) = LAST_COMPILE_RELEASES.lock() {
        releases[CompileRelease::CodeGeneration as usize] = None;
    }
}

/// The page-aligned address ranges of the segments above, from the loader's own table: every
/// non-writable segment of the executable, which the table lists without a name, and every
/// library's non-writable code segment. The kernel's vDSO, whose name is not a path, is left
/// out. Each segment is released on its own: one range across the executable's segments spans
/// the unmapped page between them, and the kernel then rejects it. Releases run at every
/// collection, and reading `MAPS_PATH` for them made the kernel format, and the host parse,
/// the whole mapping table each time (about 4% of a `require('util')` run).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_only_segments() -> Vec<(usize, usize)> {
    unsafe extern "C" fn visit(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        segments: *mut libc::c_void,
    ) -> libc::c_int {
        // SAFETY: the loader passes a valid record for each object, and `segments` is the
        // vector below for the duration of the call.
        let (info, segments) = unsafe { (&*info, &mut *segments.cast::<Vec<(usize, usize)>>()) };
        // SAFETY: a non-null name is a NUL-terminated string the loader owns.
        let first = (!info.dlpi_name.is_null()).then(|| unsafe { *info.dlpi_name });
        let executable = first == Some(0);
        let library = first == Some(b'/' as libc::c_char);
        if !(executable || library) || info.dlpi_phdr.is_null() {
            return 0;
        }
        // SAFETY: `dlpi_phdr` points at `dlpi_phnum` program headers.
        let headers = unsafe { std::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum.into()) };
        let page = page_size();
        segments.extend(
            headers
                .iter()
                .filter(|header| {
                    header.p_type == libc::PT_LOAD
                        && header.p_flags & libc::PF_W == 0
                        && (executable || header.p_flags & libc::PF_X != 0)
                })
                .map(|header| {
                    let start = info.dlpi_addr as usize + header.p_vaddr as usize;
                    let end = start + header.p_memsz as usize;
                    (start - start % page, end.next_multiple_of(page))
                }),
        );
        0
    }
    let mut segments = Vec::new();
    // SAFETY: `visit` only reads the records and pushes onto `segments`.
    unsafe {
        libc::dl_iterate_phdr(Some(visit), (&raw mut segments).cast());
    }
    segments
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn page_size() -> usize {
    // SAFETY: `sysconf` reads a constant of the system.
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

/// Collections the host has seen, and the count at the last release before output.
static COLLECTIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static COLLECTIONS_AT_OUTPUT_RELEASE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// Records a collection boundary, after which the next output releases the read-only pages.
pub fn note_collection() {
    COLLECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Releases the read-only pages at a collection boundary whose count since start-up is a power
/// of two: a run's first boundaries return the start-up code it never runs again, and later
/// ones return what it stopped running, while a run that collects thousands of times does not
/// refault its own hot code after each. Releasing at every boundary cost EarleyBoyer a quarter
/// of its Score under MemoryFirst at the 2^13 young minimum (2705 against 3648 without
/// collection-time code release; 197K minor faults against 39K), while the release returned
/// 560-800 KiB of each V8-v7 fixture's max RSS (static + BOLT x86_64, 4 vCPU, 2026-10-22).
pub fn release_read_only_pages_at_collection() {
    let boundary = COLLECTIONS.load(std::sync::atomic::Ordering::Relaxed) + 1;
    if boundary.is_power_of_two() {
        release_read_only_pages();
    }
}

/// Releases the read-only pages before console output when a collection has run since the
/// last such release: a run that prints its result once at the end touches the output code
/// while its own code is resident, at the peak of its heap (V8-v7 RegExp max RSS, bolted
/// static x86_64, 2026-10-19: 6316-6524 -> 6244-6300 KiB). Output in a loop that allocates
/// little releases nothing more.
pub fn release_before_output() {
    use std::sync::atomic::Ordering::Relaxed;
    let collections = COLLECTIONS.load(Relaxed);
    if COLLECTIONS_AT_OUTPUT_RELEASE.swap(collections, Relaxed) != collections {
        release_read_only_pages();
    }
}

/// Returns the allocator's free pages to the system: a compilation frees its syntax tree
/// and passes, whose pages would otherwise stay resident beside the program.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn release_free_heap_pages() {
    // SAFETY: `malloc_trim` only advises the kernel about free allocator memory.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn release_free_heap_pages() {}

/// The address ranges of the process's file mappings (not the kernel's bracketed ones) whose
/// permissions are one of `permissions`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
/// The table is read as bytes, its fields split at spaces: a process with many compile-cache
/// entries mapped has a line for each (over 1,200 for the jsdom npm scenario), and decoding it
/// as text cost about 3,700 instructions a line.
fn file_mappings(permissions: &[&str]) -> Vec<(usize, usize)> {
    std::fs::read(MAPS_PATH).map_or_else(|_| Vec::new(), |maps| parse_file_mappings(&maps, permissions))
}

/// `file_mappings` over the table's bytes `maps`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn parse_file_mappings(maps: &[u8], permissions: &[&str]) -> Vec<(usize, usize)> {
    let hex = |digits: &[u8]| {
        digits.iter().try_fold(0usize, |value, &digit| {
            Some(value.checked_mul(16)? + char::from(digit).to_digit(16)? as usize)
        })
    };
    maps.split(|&byte| byte == b'\n')
        .filter_map(|line| {
            let mut fields = line.split(|&byte| byte == b' ').filter(|field| !field.is_empty());
            let (range, mode) = (fields.next()?, fields.next()?);
            if !permissions.iter().any(|permission| permission.as_bytes() == mode) {
                return None;
            }
            // Offset, device and inode precede the path; a bracketed name ([vdso]) is the
            // kernel's and an anonymous mapping has none.
            if fields.nth(3)?.first() != Some(&b'/') {
                return None;
            }
            let dash = range.iter().position(|&byte| byte == b'-')?;
            Some((hex(&range[..dash])?, hex(&range[dash + 1..])?))
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
    /// The registration is per process; a second one would only open another descriptor.
    static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if REGISTERED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    const UFFD_API: u64 = 0xaa;
    const UFFD_USER_MODE_ONLY: libc::c_int = 1;
    const UFFD_FEATURE_WP_ASYNC: u64 = 1 << 15;
    /// `_IOWR(UFFDIO, _UFFDIO_API, struct uffdio_api)`.
    const UFFDIO_API: libc::c_ulong = 0xc018_aa3f;
    #[repr(C)]
    struct UffdioApi {
        api: u64,
        features: u64,
        ioctls: u64,
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
        TOUCHED_PAGES_ONLY.store(fd, std::sync::atomic::Ordering::Relaxed);
    }
    for (start, end) in file_mappings(&[CODE_MAPPING, DATA_MAPPING]) {
        register_touched_pages_only(start, end - start);
    }
}

/// The userfaultfd descriptor `map_touched_pages_only` registers mappings with, or `NO_FD`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static TOUCHED_PAGES_ONLY: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(NO_FD);

/// No userfaultfd descriptor: the kernel lacks it or start-up has not opened it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const NO_FD: libc::c_int = -1;

/// Registers the mapping at `start` for write protection, which turns off fault-around in it
/// (`map_touched_pages_only`); nothing when no descriptor is open. The kernel registers whole
/// pages only, and a mapping spans its last partial page, so `length` is rounded up to it: a
/// compile-cache entry is mapped at its file's length, and with that length every
/// registration had failed, so reading one record faulted in the whole entry (V8-v7 RegExp's
/// 266 KiB, while its heap was at its peak).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn register_touched_pages_only(start: usize, length: usize) {
    let length = length.next_multiple_of(page_size());
    /// `struct uffdio_register` from `<linux/userfaultfd.h>`.
    #[repr(C)]
    struct UffdioRegister {
        start: u64,
        len: u64,
        mode: u64,
        ioctls: u64,
    }
    const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
    /// `_IOWR(UFFDIO, _UFFDIO_REGISTER, struct uffdio_register)`.
    const UFFDIO_REGISTER: libc::c_ulong = 0xc020_aa00;
    let fd = TOUCHED_PAGES_ONLY.load(std::sync::atomic::Ordering::Relaxed);
    if fd == NO_FD {
        return;
    }
    // A mapping covers whole pages, and the kernel refuses a range that does not: a file whose
    // length is no page multiple ends its mapping on a partial page.
    let length = length.next_multiple_of(page_size());
    let mut range = UffdioRegister { start: start as u64, len: length as u64, mode: UFFDIO_REGISTER_MODE_WP, ioctls: 0 };
    // SAFETY: the ioctl reads and writes only `range`; a failed registration leaves the
    // mapping with fault-around.
    unsafe {
        libc::ioctl(fd, UFFDIO_REGISTER, &mut range);
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn map_touched_pages_only() {}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn track_read_only_mapping(_start: usize, _length: usize) {}

#[cfg(all(unix, not(all(target_os = "linux", target_arch = "x86_64"))))]
pub fn unmap_tracked_mapping(start: usize, length: usize) {
    // SAFETY: the caller owns the mapping.
    unsafe {
        libc::munmap(start as *mut libc::c_void, length);
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_read_only_pages() {}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_tracked_bytes(_bytes: &[u8]) {}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_read_only_pages_after_load() {}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    /// File mappings with the asked permissions, by their ranges; anonymous and kernel
    /// mappings and other permissions are left out.
    #[test]
    fn parses_file_mappings() {
        let maps = b"55d0c8a00000-55d0c8a3f000 r--p 00000000 08:01 1234                       /usr/bin/quench node\n\
7f00aa000000-7f00aa021000 rw-p 00000000 00:00 0 \n\
7f00ab000000-7f00ab200000 r-xp 00028000 08:01 99 /lib/libc.so.6\n\
7ffd1c3f2000-7ffd1c3f4000 r--p 00000000 00:00 0                          [vvar]\n\
7f00ac000000-7f00ac001000 r--p 00001000 08:01 77 /lib/libc.so.6\n";
        assert_eq!(
            super::parse_file_mappings(maps, &["r--p"]),
            [(0x55d0c8a00000, 0x55d0c8a3f000), (0x7f00ac000000, 0x7f00ac001000)]
        );
        assert_eq!(super::parse_file_mappings(maps, &["r-xp"]), [(0x7f00ab000000, 0x7f00ab200000)]);
    }
}
