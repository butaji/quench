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

/// The first-call release interval under a policy that returns pages at collections
/// (memory-first). Before a run's first collections its first calls run code it runs only
/// once, and that code stayed resident until the first collection while the heap built up:
/// the max RSS of V8-v7 DeltaBlue and RayTrace without JIT, and of RegExp in both modes, fell
/// at 18-100 ms, during that warm-up. A release costs the refaults that follow, about 0.35 ms
/// (npm scenarios with 300-1800 first calls, x86_64 VM, 2026-10-22), so a release at every
/// first call slowed TypeScript's transpile 313 -> 829 ms. At 5 ms (plain x86_64 build,
/// MemoryFirst, 10 runs; `evidence/tod137-first-call-release-linux-2026-10-23`): V8-v7 max
/// RSS without JIT -124 to -516 KiB on six fixtures, with JIT -144 to +48 KiB, scores within
/// noise; npm scenarios run 7-21% longer for -3.4 to +0.1 MB. Memory-first is the opt-in
/// policy that trades speed for memory; the throughput default keeps `LOAD_RELEASE_INTERVAL`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const MEMORY_FIRST_FIRST_CALL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);

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

/// Whether a release point returns resident pages now: once the process's memory policy is
/// selected, when its `MemoryPolicy::resident_release_interval` has passed since the last
/// return. The pace starts at the first release point after the selection.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn release_due(policy: quench_runtime::MemoryPolicy) -> bool {
    let interval = policy.resident_release_interval();
    let Ok(mut last) = LAST_RELEASE.lock() else {
        return false;
    };
    match *last {
        Some(at) => at.elapsed() >= interval,
        None => {
            *last = Some(std::time::Instant::now());
            interval.is_zero()
        }
    }
}

/// `release_due` under the process's memory policy, if one is selected.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn process_release_due() -> bool {
    crate::shared_run::process_memory_policy().is_some_and(release_due)
}

/// Unmaps the read-only pages above from the resident set when a release is due
/// (`process_release_due`); the next touch of each refaults it from its file. Between due
/// releases it still unmaps the tracked mappings: the snapshot's and compile cache's records
/// a run has decoded, which it reads again only for a function's first call, unlike the code
/// a release refaults. Against keeping them until the next due release (plain x86_64 build,
/// snapshot on, 9 runs, 2026-10-23), npm scenario peak RSS jsdom 82.8 -> 80.1 MB, lodash
/// 19.1 -> 17.7, path-parse 16.8 -> 15.6, undici 32.0 -> 31.6, with as many page faults and
/// wall time within noise.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_read_only_pages() {
    if process_release_due() {
        discard_read_only_pages();
    } else {
        discard_tracked_mappings();
    }
}

/// A collection boundary's release under the runtime's `policy`: the read-only pages and the
/// allocator's free pages, which the collection's sweep just freed, when a release is due
/// (`release_due`). Every policy counts the collection for the release before output.
pub fn release_at_collection(policy: quench_runtime::MemoryPolicy) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if release_due(policy) {
        discard_read_only_pages();
        release_free_heap_pages();
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let _ = policy;
    note_collection();
}

/// Releases the read-only pages around a native compilation under every policy: the code
/// generator's pages are not the run's, which it does not touch again, so the release
/// refaults none of the run's code, and a native compilation is a short numeric program's
/// peak (V8-v7 `--jit` max RSS, TOD-185: Crypto 11.6-13.2 -> 9.2-9.7 MB, NavierStokes
/// 11.1 -> 9.1).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_at_native_compilation() {
    discard_read_only_pages();
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn release_at_native_compilation() {}

/// Unmaps the read-only pages above from the resident set; the next touch of each refaults it
/// from its file.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn discard_read_only_pages() {
    if let Ok(mut last) = LAST_RELEASE.lock() {
        *last = Some(std::time::Instant::now());
    }
    let segments = loaded_segments(ReleasedSegment::Clean);
    for &(start, end) in segments.iter().chain(unwritten_relro_pages()) {
        // SAFETY: each range is a private file mapping that is never written, so discarding
        // its pages loses nothing.
        unsafe {
            libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
        }
    }
    discard_tracked_mappings();
}

/// Unmaps the tracked mappings' pages (`TRACKED_MAPPINGS`) from the resident set.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn discard_tracked_mappings() {
    // Held while releasing, so no tracked mapping is unmapped meanwhile.
    if let Ok(mut mappings) = TRACKED_MAPPINGS.lock() {
        // Every tracked mapping is released below, deferred ranges with it.
        mappings.pending.clear();
        mappings.pending_bytes = 0;
        let spans = mappings.arena.as_ref().map(Arena::used).into_iter();
        for (start, length) in spans.chain(mappings.outside.iter().copied()) {
            // SAFETY: a tracked mapping is a live, read-only private file mapping, and the
            // arena's used span holds only such mappings and reserved, inaccessible pages.
            unsafe {
                libc::madvise(start as *mut libc::c_void, length, libc::MADV_DONTNEED);
            }
        }
    }
}

/// Read-only file mappings the host made after start-up (compile-cache entries), released with
/// the segments above. They are mapped side by side in one reserved span (`Arena`), which a
/// release discards with one call: a run that maps hundreds of entries otherwise made a call
/// for each at every release (the chalk npm scenario, x86_64 VM, 2026-10-22: 21,900 madvise
/// calls in a 78 ms run).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static TRACKED_MAPPINGS: std::sync::Mutex<TrackedMappings> =
    std::sync::Mutex::new(TrackedMappings {
        arena: None,
        inside: Vec::new(),
        outside: Vec::new(),
        pending: Vec::new(),
        pending_bytes: 0,
    });

/// The tracked mappings, as start and length: those in the arena and those mapped elsewhere
/// once it was full or could not be reserved.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct TrackedMappings {
    arena: Option<Arena>,
    inside: Vec<(usize, usize)>,
    outside: Vec<(usize, usize)>,
    /// Page ranges of tracked mappings read and not yet released (`release_tracked_bytes`),
    /// as start and end, and their bytes.
    pending: Vec<(usize, usize)>,
    pending_bytes: usize,
}

/// The ranges, and their bytes, `release_tracked_bytes` defers before releasing them all: a
/// release per decoded record was a call per function an npm package's run decodes (about
/// 6,000 for @babel/helper-module-transforms, x86_64 VM, 2026-10-22), while the deferred
/// pages stay resident no longer than this many more.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PENDING_RELEASE_RANGES: usize = 32;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PENDING_RELEASE_BYTES: usize = 256 << 10;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl TrackedMappings {
    fn contains(&self, start: usize, end: usize) -> bool {
        self.inside.iter().chain(&self.outside).any(|&(mapping, length)| mapping <= start && end <= mapping + length)
    }

    /// Releases the deferred ranges (`release_tracked_bytes`).
    fn release_pending(&mut self) {
        for &(start, end) in &self.pending {
            // SAFETY: each range lies in a live, read-only private file mapping (unmapping a
            // mapping drops its ranges), whose discarded pages fault back in from the file.
            unsafe {
                libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED);
            }
        }
        self.pending.clear();
        self.pending_bytes = 0;
    }
}

/// Address space reserved inaccessible for the tracked mappings, filled from its start: a
/// mapping replaces reserved pages, and an unmapped one is reserved again, never reused.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct Arena {
    start: usize,
    end: usize,
    next: usize,
}

/// The arena's reserved address space. Reserving costs no memory, only addresses; a process
/// that maps more falls back to mapping each entry on its own.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const ARENA_BYTES: usize = 1 << 32;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Arena {
    fn reserve() -> Option<Self> {
        // SAFETY: a fresh inaccessible anonymous reservation; nothing else uses its addresses.
        let start = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                ARENA_BYTES,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        (start != libc::MAP_FAILED).then(|| Self { start: start as usize, end: start as usize + ARENA_BYTES, next: start as usize })
    }

    /// The span its mappings have taken, as start and length.
    fn used(&self) -> (usize, usize) {
        (self.start, self.next - self.start)
    }

    fn holds(&self, address: usize) -> bool {
        (self.start..self.end).contains(&address)
    }
}

/// Maps `length` bytes of the open file `fd` read-only and private, a tracked mapping: faults
/// map only the touched page, and releases discard its pages. It lies in the arena while it has room, elsewhere otherwise. None when mapping
/// fails.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn map_read_only_file(fd: libc::c_int, length: usize) -> Option<usize> {
    let mut mappings = TRACKED_MAPPINGS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if mappings.arena.is_none() {
        mappings.arena = Arena::reserve();
    }
    let span = length.next_multiple_of(page_size());
    let slot = mappings.arena.as_ref().filter(|arena| arena.end - arena.next >= span).map(|arena| arena.next);
    // SAFETY: a read-only private mapping of an open file, either over reserved arena pages
    // nothing else uses or at an address the kernel picks.
    let address = unsafe {
        libc::mmap(
            slot.map_or(std::ptr::null_mut(), |slot| slot as *mut libc::c_void),
            length,
            libc::PROT_READ,
            libc::MAP_PRIVATE | if slot.is_some() { libc::MAP_FIXED } else { 0 },
            fd,
            0,
        )
    };
    if address == libc::MAP_FAILED {
        return None;
    }
    let address = address as usize;
    // A mapping aligned to a huge page maps a whole huge folio of a cached file on one fault
    // (a 5 MB startup snapshot was 4 MB resident after its records were first read).
    // SAFETY: advice on the live mapping just made; it changes only how faults map.
    unsafe {
        libc::madvise(address as *mut libc::c_void, length, libc::MADV_NOHUGEPAGE);
    }
    register_touched_pages_only(address, length);
    match mappings.arena.as_mut().filter(|_| slot.is_some()) {
        Some(arena) => {
            arena.next += span;
            mappings.inside.push((address, length));
        }
        None => mappings.outside.push((address, length)),
    }
    Some(address)
}

/// Releases the whole pages of `bytes` when they lie in a mapping `map_read_only_file`
/// made; other bytes are left alone.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn release_tracked_bytes(bytes: &[u8]) {
    let page = page_size();
    let start = (bytes.as_ptr() as usize).next_multiple_of(page);
    let end = (bytes.as_ptr() as usize + bytes.len()) / page * page;
    if start >= end {
        return;
    }
    // Held while releasing, so the mapping is not unmapped meanwhile.
    let Ok(mut mappings) = TRACKED_MAPPINGS.lock() else {
        return;
    };
    if !mappings.contains(start, end) {
        return;
    }
    // A read that continues the last deferred range, as a restore's sequential reads do,
    // extends it.
    match mappings.pending.last_mut() {
        Some(last) if last.0 <= end && start <= last.1 => *last = (last.0.min(start), last.1.max(end)),
        _ => mappings.pending.push((start, end)),
    }
    mappings.pending_bytes += end - start;
    if mappings.pending.len() >= PENDING_RELEASE_RANGES || mappings.pending_bytes >= PENDING_RELEASE_BYTES {
        mappings.release_pending();
    }
}

/// Unmaps a mapping `map_read_only_file` made, once no release can reach it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn unmap_tracked_mapping(start: usize, length: usize) {
    let mut mappings = TRACKED_MAPPINGS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    mappings.inside.retain(|&mapping| mapping != (start, length));
    mappings.outside.retain(|&mapping| mapping != (start, length));
    // A deferred range of the mapping must not be released once its addresses are reused.
    mappings.pending.retain(|&(pending, _)| !(start..start + length).contains(&pending));
    let in_arena = mappings.arena.as_ref().is_some_and(|arena| arena.holds(start));
    // SAFETY: the caller owns the mapping, and the held lock keeps releases off it. An arena
    // mapping's pages are reserved again, so the arena's used span stays wholly mapped.
    unsafe {
        if in_arena {
            libc::mmap(
                start as *mut libc::c_void,
                length,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_FIXED,
                -1,
                0,
            );
        } else {
            libc::munmap(start as *mut libc::c_void, length);
        }
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
    if !process_release_due() {
        return;
    }
    discard_read_only_pages();
    for (start, end) in loaded_segments(ReleasedSegment::LibraryData) {
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

impl CompileRelease {
    const COUNT: usize = 2;
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl CompileRelease {
    /// How far apart this release's runs are kept.
    fn interval(self) -> std::time::Duration {
        let memory_first = crate::shared_run::process_memory_policy() == Some(quench_runtime::MemoryPolicy::MemoryFirst);
        match self {
            Self::FirstCall if memory_first => MEMORY_FIRST_FIRST_CALL_INTERVAL,
            Self::FirstCall | Self::CodeGeneration => LOAD_RELEASE_INTERVAL,
        }
    }
}

/// When each compile-phase release last ran, indexed by `CompileRelease`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
static LAST_COMPILE_RELEASES: std::sync::Mutex<[Option<std::time::Instant>; CompileRelease::COUNT]> =
    std::sync::Mutex::new([None; CompileRelease::COUNT]);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn release_once_per_interval(release: CompileRelease, action: fn()) {
    let Ok(mut releases) = LAST_COMPILE_RELEASES.lock() else {
        return;
    };
    let last = &mut releases[release as usize];
    if last.is_some_and(|last| last.elapsed() < release.interval()) {
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

/// The loaded segments a release reaches, by the loader's program headers.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReleasedSegment {
    /// Every non-writable segment of the executable, and every library's code segment: clean
    /// file pages a release discards (`release_read_only_pages`).
    Clean,
    /// Every library's non-writable data segment, which `release_read_only_code_and_data`
    /// pages out. A library's RELRO pages lie in its writable segment, and the executable's
    /// read-only data is `Clean`. The read-only file mappings had been found by parsing the
    /// whole mapping table, a line per compile-cache entry: 4.9% of the undici npm scenario's
    /// instructions (x86_64, 2026-10-22).
    LibraryData,
}

/// The page-aligned address ranges of the segments `kind` names, from the loader's own table,
/// which names the executable with an empty string. The kernel's vDSO, whose name is not a
/// path, is left out. Each segment is released on its own: one range across the executable's segments spans
/// the unmapped page between them, and the kernel then rejects it. Releases run at every
/// collection, and reading `MAPS_PATH` for them made the kernel format, and the host parse,
/// the whole mapping table each time (about 4% of a `require('util')` run).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn loaded_segments(kind: ReleasedSegment) -> Vec<(usize, usize)> {
    unsafe extern "C" fn visit(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        found: *mut libc::c_void,
    ) -> libc::c_int {
        // SAFETY: the loader passes a valid record for each object, and `found` is the pair
        // below for the duration of the call.
        let (info, (kind, segments)) =
            unsafe { (&*info, &mut *found.cast::<(ReleasedSegment, Vec<(usize, usize)>)>()) };
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
                    let code = header.p_flags & libc::PF_X != 0;
                    header.p_type == libc::PT_LOAD
                        && header.p_flags & libc::PF_W == 0
                        && match kind {
                            ReleasedSegment::Clean => executable || code,
                            ReleasedSegment::LibraryData => library && !code,
                        }
                })
                .map(|header| {
                    let start = info.dlpi_addr as usize + header.p_vaddr as usize;
                    let end = start + header.p_memsz as usize;
                    (start - start % page, end.next_multiple_of(page))
                }),
        );
        0
    }
    let mut found = (kind, Vec::new());
    // SAFETY: `visit` only reads the records and pushes onto the pair's segments.
    unsafe {
        libc::dl_iterate_phdr(Some(visit), (&raw mut found).cast());
    }
    found.1
}

/// The page ranges of the executable's RELRO data its loader never wrote. The loader writes
/// relocations into that range and then makes it read-only, so it lies in a writable segment
/// that `ReleasedSegment::Clean` leaves out. A static executable's relocations fill only its
/// `.got.plt` (IFUNC targets), and the rest of the range stays clean file pages, which
/// start-up's reads fault in with their neighbours before `map_touched_pages_only` turns that
/// off: 196 KiB resident for an empty script, 16 KiB of them written (bolted static x86_64,
/// 2026-10-24). The kernel's page map tells written pages, now private, from file pages. Once
/// the range is read-only no page can become private, so the ranges found at the first
/// release hold for the process's life. Only the executable is read: its loader finished
/// before `main`, while a library loading on another thread may not have written its
/// relocations yet.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn unwritten_relro_pages() -> &'static [(usize, usize)] {
    static PAGES: std::sync::OnceLock<Vec<(usize, usize)>> = std::sync::OnceLock::new();
    PAGES.get_or_init(|| executable_relro().map_or_else(Vec::new, |(start, end)| file_page_runs(start, end)))
}

/// The process's own page map: one little-endian entry per virtual page.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PAGEMAP_PATH: &str = "/proc/self/pagemap";

/// The bytes of a `PAGEMAP_PATH` entry.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PAGEMAP_ENTRY_BYTES: usize = std::mem::size_of::<u64>();

/// A `PAGEMAP_PATH` entry's bits (Documentation/admin-guide/mm/pagemap.rst): the page is
/// resident, swapped out, or a file page (or shared anonymous).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PAGEMAP_PRESENT: u64 = 1 << 63;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PAGEMAP_SWAPPED: u64 = 1 << 62;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PAGEMAP_FILE: u64 = 1 << 61;

/// The runs of pages from `start` to `end` (page-aligned) that hold their file's contents:
/// resident file pages and pages never faulted in, not private copies; none when the page map
/// cannot be read.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn file_page_runs(start: usize, end: usize) -> Vec<(usize, usize)> {
    use std::os::unix::fs::FileExt;
    let page = page_size();
    let mut entries = vec![0; (end - start) / page * PAGEMAP_ENTRY_BYTES];
    let offset = (start / page * PAGEMAP_ENTRY_BYTES) as u64;
    let read = std::fs::File::open(PAGEMAP_PATH).and_then(|map| map.read_exact_at(&mut entries, offset));
    if read.is_err() {
        return Vec::new();
    }
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (index, entry) in entries.chunks_exact(PAGEMAP_ENTRY_BYTES).enumerate() {
        let entry = u64::from_le_bytes(entry.try_into().expect("a page map entry"));
        let private = entry & PAGEMAP_SWAPPED != 0
            || entry & (PAGEMAP_PRESENT | PAGEMAP_FILE) == PAGEMAP_PRESENT;
        if private {
            continue;
        }
        let address = start + index * page;
        match runs.last_mut() {
            Some(run) if run.1 == address => run.1 += page,
            _ => runs.push((address, address + page)),
        }
    }
    runs
}

/// The whole pages of the executable's RELRO range (`PT_GNU_RELRO`), whose partial first and
/// last pages it shares with its segment's other data.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn executable_relro() -> Option<(usize, usize)> {
    unsafe extern "C" fn visit(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        found: *mut libc::c_void,
    ) -> libc::c_int {
        // SAFETY: the loader passes a valid record for each object, and `found` is the option
        // below for the duration of the call.
        let (info, found) = unsafe { (&*info, &mut *found.cast::<Option<(usize, usize)>>()) };
        // SAFETY: a non-null name is a NUL-terminated string the loader owns.
        let executable = !info.dlpi_name.is_null() && unsafe { *info.dlpi_name } == 0;
        if !executable || info.dlpi_phdr.is_null() {
            return 0;
        }
        // SAFETY: `dlpi_phdr` points at `dlpi_phnum` program headers.
        let headers = unsafe { std::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum.into()) };
        let page = page_size();
        *found = headers.iter().find(|header| header.p_type == libc::PT_GNU_RELRO).map(|header| {
            let start = info.dlpi_addr as usize + header.p_vaddr as usize;
            let end = start + header.p_memsz as usize;
            (start.next_multiple_of(page), end - end % page)
        });
        // The executable is the first object; the walk stops at it.
        1
    }
    let mut found = None;
    // SAFETY: `visit` only reads the records and writes the option.
    unsafe {
        libc::dl_iterate_phdr(Some(visit), (&raw mut found).cast());
    }
    found.filter(|(start, end)| start < end)
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
fn note_collection() {
    COLLECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

#[cfg(all(unix, not(all(target_os = "linux", target_arch = "x86_64"))))]
pub fn map_read_only_file(fd: libc::c_int, length: usize) -> Option<usize> {
    // SAFETY: a fresh read-only private mapping of an open file.
    let address = unsafe { libc::mmap(std::ptr::null_mut(), length, libc::PROT_READ, libc::MAP_PRIVATE, fd, 0) };
    (address != libc::MAP_FAILED).then_some(address as usize)
}

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

    /// A private file mapping's written page is a private copy, left out of the runs; a page
    /// read and a page never touched both still hold the file's contents.
    #[test]
    fn file_page_runs_leave_out_written_pages() {
        use std::io::Write;
        let page = super::page_size();
        let pages = 3;
        let path = std::env::temp_dir().join(format!("quench-file-page-runs-{}", std::process::id()));
        std::fs::File::create(&path).and_then(|mut file| file.write_all(&vec![1; page * pages])).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        // SAFETY: a fresh private mapping of the file, unmapped below.
        let start = unsafe {
            use std::os::fd::AsRawFd;
            libc::mmap(
                std::ptr::null_mut(),
                page * pages,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        assert_ne!(start, libc::MAP_FAILED);
        let start = start as usize;
        // SAFETY: both pages lie in the mapping above.
        unsafe {
            std::ptr::write_volatile((start + page) as *mut u8, 2);
            std::ptr::read_volatile((start + 2 * page) as *const u8);
        }
        let runs = super::file_page_runs(start, start + page * pages);
        // SAFETY: the mapping above, no longer used.
        unsafe {
            libc::munmap(start as *mut libc::c_void, page * pages);
        }
        std::fs::remove_file(&path).unwrap();
        assert_eq!(runs, [(start, start + page), (start + 2 * page, start + 3 * page)]);
    }
}
