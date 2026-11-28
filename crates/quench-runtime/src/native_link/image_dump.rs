//! An image's runtime at rest, mapped from a dump instead of built or decoded at each start.
//!
//! The build runs the linked image once more with `DUMP_VARIABLE` set, with every allocation
//! in the dump arena (`quench_alloc::begin_dump`) and every reservation at its fixed address
//! (`quench_alloc::map_anonymous`), on a worker whose stack, and so its thread-locals, sits at
//! a fixed address (`quench_alloc::run_on_fixed_stack`). The run builds the state the image
//! starts from and writes the process memory that state lies in (`write`). The build keeps of
//! the statics and thread-locals only the runtime's own that the run changed (`finish`) and
//! appends the dump to the image (`append`). As the image starts it maps the dump back at the
//! same addresses (`adopt`), where the build would have run.
//!
//! This module is host-generic: it links the runtime alone, so a closed-world core image
//! and a Node image both dump and adopt through it. Each host names its own dumped roots
//! (`UNDUMPED`) and what its process binds again after an adoption.
//!
//! A start that cannot have every address (`adopt` returns none) builds its state as the
//! dumping run did. A run dumps only when its build asks it to (`DumpRequest`), by argument or,
//! for a Node image, by the environment its driver reads.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::OnceLock;

mod symbols;
pub use symbols::{dump_image, UNDUMPED};

/// The environment a Node image's dump run names its request in (`DumpRequest::parse`).
pub const DUMP_VARIABLE: &str = "QUENCH_IMAGE_DUMP";
/// The argument prefix a core image's dump run names its request in (`DumpRequest::from_arguments`).
pub const DUMP_ARGUMENT: &str = "--quench-image-dump=";

/// A run's dump request: the state of index `wait` is written to `path` once it is built.
#[derive(Debug)]
pub struct DumpRequest {
    wait: usize,
    path: PathBuf,
}

impl DumpRequest {
    /// `WAIT:PATH`, the value both the environment and the argument carry.
    pub fn parse(value: &str) -> Option<Self> {
        let (wait, path) = value.split_once(':')?;
        Some(Self { wait: wait.parse().ok()?, path: PathBuf::from(path) })
    }

    /// The request an argument list names: `--quench-image-dump=WAIT:PATH`.
    pub fn from_arguments(arguments: impl IntoIterator<Item = String>) -> Option<Self> {
        arguments.into_iter().find_map(|argument| Self::parse(argument.strip_prefix(DUMP_ARGUMENT)?))
    }
}

/// The request this process dumps, once `begin` has taken it.
static REQUEST: OnceLock<DumpRequest> = OnceLock::new();
const MAGIC: [u8; 8] = *b"QNCHDMP1";
const TRAILER_MAGIC: [u8; 8] = *b"QNCHDMPT";
const PAGE: usize = 4096;
/// The arena's address.
const ARENA_BASE: u64 = quench_alloc::DUMP_BASE as u64;

/// A dump: as the dumping run writes it (`write`), its statics and thread-local block whole;
/// as the build appends it (`finish`), only the runtime's own words the run changed.
#[derive(Default)]
pub struct Dump {
    pub wait: u64,
    /// The `Box<(EntryRuntime, NativeJsProgram)>` the run leaked.
    pub root: u64,
    /// `quench_alloc::fixed_space_next` after the run.
    pub fixed_next: u64,
    /// Anonymous mappings the run made: start, length, protection.
    pub regions: Vec<(u64, u64, u64)>,
    /// Resident pages of those, of the arena and of the fixed layout table: address, bytes, and
    /// whether the page maps straight from the file (the arena's, which the run never gives
    /// back to the kernel) or is copied (a reservation's, whose released pages must read zero).
    pub runs: Vec<(u64, Vec<u8>, bool)>,
    /// Static words: address, bytes.
    pub statics: Vec<(u64, Vec<u8>)>,
    /// Thread-local words of the worker: distance below the thread pointer, bytes.
    pub tls: Vec<(u64, Vec<u8>)>,
    /// Ranges of the dumping process no dumped word may point into: start, end, name.
    pub foreign: Vec<(u64, u64, String)>,
}

fn put(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn take(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let word = bytes.get(*at..*at + 8).ok_or("truncated image dump")?;
    *at += 8;
    Ok(u64::from_le_bytes(word.try_into().map_err(|_| "truncated image dump")?))
}

impl Dump {
    /// The dump as bytes at file offset `at`: a header holding the statics and thread-local
    /// words, then the pages no earlier dump in `pool` holds, each page aligned so a run maps
    /// straight from the file.
    pub fn encode(&self, at: u64, pool: &mut std::collections::HashMap<Vec<u8>, u64>) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&MAGIC);
        // The header's length, filled in below.
        put(&mut header, 0);
        for value in [self.wait, self.root, self.fixed_next] {
            put(&mut header, value);
        }
        put(&mut header, self.regions.len() as u64);
        for &(start, length, protection) in &self.regions {
            for value in [start, length, protection] {
                put(&mut header, value);
            }
        }
        for patches in [&self.statics, &self.tls] {
            put(&mut header, patches.len() as u64);
            for (at, bytes) in patches.iter() {
                put(&mut header, *at);
                put(&mut header, bytes.len() as u64);
                header.extend_from_slice(bytes);
                header.resize(header.len().next_multiple_of(8), 0);
            }
        }
        put(&mut header, self.foreign.len() as u64);
        for (start, end, name) in &self.foreign {
            put(&mut header, *start);
            put(&mut header, *end);
            put(&mut header, name.len() as u64);
            header.extend_from_slice(name.as_bytes());
            header.resize(header.len().next_multiple_of(8), 0);
        }
        // Each page goes where `pool` already holds the same bytes (another saved run's dump
        // in this image, or this one's), else after this dump's header: runs are cut where
        // neither address nor file place continues. Places are file offsets, from `at`, this
        // dump's own.
        enum Place {
            Pooled(u64),
            New(usize),
        }
        let mut pages: Vec<(u64, bool, Place)> = Vec::new();
        let mut fresh: Vec<&[u8]> = Vec::new();
        let mut seen: std::collections::HashMap<&[u8], usize> = std::collections::HashMap::new();
        for (address, bytes, mapped) in &self.runs {
            for (index, page) in bytes.chunks(PAGE).enumerate() {
                let place = match pool.get(page) {
                    Some(&place) => Place::Pooled(place),
                    None => Place::New(*seen.entry(page).or_insert_with(|| {
                        fresh.push(page);
                        fresh.len() - 1
                    })),
                };
                pages.push((address + (index * PAGE) as u64, *mapped, place));
            }
        }
        let place_of = |place: &Place, data: u64| match *place {
            Place::Pooled(place) => place,
            Place::New(index) => data + (index * PAGE) as u64,
        };
        let cut = |data: u64| {
            let mut runs: Vec<(u64, u64, u64, bool)> = Vec::new();
            for (address, mapped, place) in &pages {
                let place = place_of(place, data);
                match runs.last_mut() {
                    Some((start, length, at, kind)) if *kind == *mapped && *start + *length == *address && *at + *length == place => *length += PAGE as u64,
                    _ => runs.push((*address, PAGE as u64, place, *mapped)),
                }
            }
            runs
        };
        // A pooled page lies before this dump's header and a new one after it, so neither
        // continues the other: the run count, which sizes the table and so places the new
        // pages, does not depend on that place.
        let count = cut(1 << 62).len();
        let data = at + (header.len() + 8 * (1 + 2 * count)).next_multiple_of(PAGE) as u64;
        let runs = cut(data);
        debug_assert_eq!(runs.len(), count);
        put(&mut header, runs.len() as u64);
        for (address, length, place, mapped) in &runs {
            put(&mut header, address | (u64::from(*mapped) << 63));
            put(&mut header, (length << 32) | (place / PAGE as u64));
        }
        let length = header.len() as u64;
        header[8..16].copy_from_slice(&length.to_le_bytes());
        let mut out = header;
        out.resize(out.len().next_multiple_of(PAGE), 0);
        for (index, page) in fresh.iter().enumerate() {
            pool.insert(page.to_vec(), data + (index * PAGE) as u64);
            out.extend_from_slice(page);
            out.resize(out.len().next_multiple_of(PAGE), 0);
        }
        out
    }

    /// The bytes of the dump's resident pages.
    pub fn page_bytes(&self) -> usize {
        self.runs.iter().map(|run| run.1.len()).sum()
    }

    /// The bytes of the runs a start maps from the image (the dump arena's, `from_file`), which stay lazy and file-backed.
    pub fn arena_bytes(&self) -> usize {
        self.runs.iter().filter(|run| run.2).map(|run| run.1.len()).sum()
    }

    /// The bytes of the runs a start copies into anonymous reservations, plus the statics and TLS patches: the start's
    /// copy work (C5 D4's cost model, `c5-d4-heap-as-linker-data.md` A3).
    pub fn copied_bytes(&self) -> usize {
        let runs: usize = self.runs.iter().filter(|run| !run.2).map(|run| run.1.len()).sum();
        runs + self.statics.iter().chain(&self.tls).map(|patch| patch.1.len()).sum::<usize>()
    }

    /// Reads `encode`'s bytes back.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let header = Header::parse(bytes)?;
        let mut dump = Self {
            wait: header.wait, root: header.root, fixed_next: header.fixed_next, regions: header.regions,
            statics: header.statics.into_iter().map(|(at, range)| (at, bytes[range].to_vec())).collect(),
            tls: header.tls.into_iter().map(|(at, range)| (at, bytes[range].to_vec())).collect(),
            foreign: header.foreign, ..Self::default()
        };
        for (address, length, place, mapped) in header.runs {
            dump.runs.push((address, bytes.get(place..place + length).ok_or("truncated image dump")?.to_vec(), mapped));
        }
        Ok(dump)
    }
}

/// A dump's header: what the image reads before it maps anything.
struct Header {
    wait: u64,
    root: u64,
    fixed_next: u64,
    regions: Vec<(u64, u64, u64)>,
    statics: Vec<(u64, std::ops::Range<usize>)>,
    tls: Vec<(u64, std::ops::Range<usize>)>,
    foreign: Vec<(u64, u64, String)>,
    /// Address, length, offset in the dump, whether it maps from the file.
    runs: Vec<(u64, usize, usize, bool)>,
}

impl Header {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.get(..8) != Some(&MAGIC) {
            return Err("not an image dump".into());
        }
        let mut at = 16;
        let wait = take(bytes, &mut at)?;
        let root = take(bytes, &mut at)?;
        let fixed_next = take(bytes, &mut at)?;
        let mut regions = Vec::new();
        for _ in 0..take(bytes, &mut at)? {
            regions.push((take(bytes, &mut at)?, take(bytes, &mut at)?, take(bytes, &mut at)?));
        }
        let mut patches = [Vec::new(), Vec::new()];
        for list in &mut patches {
            for _ in 0..take(bytes, &mut at)? {
                let place = take(bytes, &mut at)?;
                let length = take(bytes, &mut at)? as usize;
                list.push((place, at..at + length));
                at = (at + length).next_multiple_of(8);
            }
        }
        let [statics, tls] = patches;
        let mut foreign = Vec::new();
        for _ in 0..take(bytes, &mut at)? {
            let start = take(bytes, &mut at)?;
            let end = take(bytes, &mut at)?;
            let length = take(bytes, &mut at)? as usize;
            let name = String::from_utf8_lossy(bytes.get(at..at + length).ok_or("truncated image dump")?).into_owned();
            at = (at + length).next_multiple_of(8);
            foreign.push((start, end, name));
        }
        let mut runs = Vec::new();
        for _ in 0..take(bytes, &mut at)? {
            let address = take(bytes, &mut at)?;
            let packed = take(bytes, &mut at)?;
            runs.push((address & !(1 << 63), (packed >> 32) as usize, (packed & 0xffff_ffff) as usize * PAGE, address >> 63 != 0));
        }
        if bytes.len() < at || statics.iter().chain(&tls).any(|(_, range)| range.end > bytes.len()) {
            return Err("truncated image dump".into());
        }
        Ok(Self { wait, root, fixed_next, regions, statics, tls, foreign, runs })
    }
}

/// The state index this process dumps, if it dumps one.
pub fn requested_wait() -> Option<usize> {
    REQUEST.get().map(|request| request.wait)
}

/// The anonymous mappings that held before the dump began (`begin`), which it leaves out.
static BEFORE: std::sync::Mutex<Vec<(u64, u64)>> = std::sync::Mutex::new(Vec::new());

/// Starts the dump `request` asks for (none: nothing is dumped): from here every allocation is
/// the arena's. Call first in `main`, with the request the process was started with.
pub fn begin(request: Option<DumpRequest>) {
    let Some(request) = request else { return };
    // Read before the arena serves allocations, so the dump holds no copy of it.
    let before = maps().into_iter().filter(|mapping| mapping.anonymous()).map(|mapping| (mapping.start, mapping.end)).collect();
    *BEFORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = before;
    if !quench_alloc::begin_dump() {
        eprintln!("quench: image dump: the arena's range is taken");
        std::process::exit(2);
    }
    let _ = REQUEST.set(request);
}

/// One line of `/proc/self/maps`.
struct Mapping {
    start: u64,
    end: u64,
    protection: u64,
    name: String,
    inode: u64,
}

impl Mapping {
    fn anonymous(&self) -> bool {
        self.inode == 0 && self.name.is_empty()
    }
}

fn maps() -> Vec<Mapping> {
    let text = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    text.lines().filter_map(|line| {
        let mut fields = line.split_ascii_whitespace();
        let (start, end) = fields.next()?.split_once('-')?;
        let permissions = fields.next()?.as_bytes();
        let _offset = fields.next()?;
        let _device = fields.next()?;
        let inode = fields.next()?.parse().ok()?;
        let name = fields.collect::<Vec<_>>().join(" ");
        let mut protection = 0;
        for (flag, bit) in [(b'r', libc::PROT_READ), (b'w', libc::PROT_WRITE), (b'x', libc::PROT_EXEC)] {
            if permissions.contains(&flag) {
                protection |= bit as u64;
            }
        }
        Some(Mapping { start: u64::from_str_radix(start, 16).ok()?, end: u64::from_str_radix(end, 16).ok()?, protection, name, inode })
    }).collect()
}

/// The resident pages of `[start, end)` as runs of whole pages (`/proc/self/pagemap`).
fn resident_runs(start: u64, end: u64) -> Vec<(u64, u64)> {
    let Ok(pagemap) = File::open("/proc/self/pagemap") else { return vec![(start, end)] };
    let pages = ((end - start) / PAGE as u64) as usize;
    let mut entries = vec![0u8; pages * 8];
    if pagemap.read_exact_at(&mut entries, start / PAGE as u64 * 8).is_err() {
        return vec![(start, end)];
    }
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for (index, entry) in entries.chunks_exact(8).enumerate() {
        let entry = u64::from_le_bytes(entry.try_into().unwrap_or_default());
        // Present or swapped.
        if entry & (3 << 62) == 0 {
            continue;
        }
        let page = start + (index * PAGE) as u64;
        match runs.last_mut() {
            Some(run) if run.1 == page => run.1 = page + PAGE as u64,
            _ => runs.push((page, page + PAGE as u64)),
        }
    }
    runs
}

/// The executable's thread-local block (offset below the thread pointer, length) and its
/// writable segments, from its program headers.
fn image_layout() -> (u64, Vec<(u64, u64)>) {
    struct Found {
        tls: u64,
        writable: Vec<(u64, u64)>,
        seen: bool,
    }
    unsafe extern "C" fn each(info: *mut libc::dl_phdr_info, _: libc::size_t, data: *mut libc::c_void) -> libc::c_int {
        // SAFETY: `dl_iterate_phdr` passes valid headers and `image_layout`'s `Found`.
        unsafe {
            let found = &mut *data.cast::<Found>();
            if found.seen {
                return 1;
            }
            found.seen = true;
            let info = &*info;
            for index in 0..info.dlpi_phnum as usize {
                let header = &*info.dlpi_phdr.add(index);
                if header.p_type == libc::PT_TLS {
                    let align = header.p_align.max(1);
                    found.tls = header.p_memsz.next_multiple_of(align);
                } else if header.p_type == libc::PT_LOAD && header.p_flags & libc::PF_W != 0 {
                    let start = info.dlpi_addr + header.p_vaddr;
                    found.writable.push((start, start + header.p_memsz));
                }
            }
            1
        }
    }
    let mut found = Found { tls: 0, writable: Vec::new(), seen: false };
    // SAFETY: `each` reads only what it is passed.
    unsafe { libc::dl_iterate_phdr(Some(each), (&raw mut found).cast()) };
    (found.tls, found.writable)
}

/// The calling thread's thread pointer.
fn thread_pointer() -> u64 {
    let pointer: u64;
    // SAFETY: the C library keeps the thread control block's own address at its start.
    #[cfg(target_arch = "x86_64")]
    unsafe { std::arch::asm!("mov {}, fs:0", out(reg) pointer, options(nostack, readonly, preserves_flags)) };
    #[cfg(not(target_arch = "x86_64"))]
    { pointer = 0; }
    pointer
}

/// Adds `[start, end)`'s pages to `runs`, leaving out what the run freed of the arena
/// (`quench_alloc::freed_granules`, as zeros) and every page left all zero, which the
/// region's fresh mapping reads as zero anyway.
fn push_pages(runs: &mut Vec<(u64, Vec<u8>, bool)>, start: u64, end: u64, mapped: bool) {
    let mut page = start;
    while page < end {
        let mut bytes = bytes_at(page, page + PAGE as u64);
        let freed = quench_alloc::freed_granules(page as usize);
        for (index, granule) in bytes.chunks_exact_mut(16).enumerate() {
            if freed[index / 64] >> (index % 64) & 1 != 0 {
                granule.fill(0);
            }
        }
        if bytes.iter().any(|&byte| byte != 0) {
            match runs.last_mut() {
                Some((at, run, kind)) if *kind == mapped && *at + run.len() as u64 == page => run.extend_from_slice(&bytes),
                _ => runs.push((page, bytes, mapped)),
            }
        }
        page += PAGE as u64;
    }
}

fn bytes_at(start: u64, end: u64) -> Vec<u8> {
    // SAFETY: the caller names memory this process maps readable.
    unsafe { std::slice::from_raw_parts(start as *const u8, (end - start) as usize) }.to_vec()
}

/// Ends the dump `begin` started: `root` is the restored runtime and entry, leaked into the
/// arena. Writes the process memory they lie in to the dump's path and exits.
pub fn write(wait: usize, root: *mut u8) -> ! {
    let outcome = (|| -> Result<(), String> {
        let path = REQUEST.get().map(|request| request.path.display().to_string()).ok_or("no dump path")?;
        let (_, used) = quench_alloc::end_dump();
        // Read once the arena is closed, so the dump holds no copy of it.
        let mappings = maps();
        if !quench_alloc::fixed_layout_held() {
            return Err("a reservation missed its fixed address".into());
        }
        let stack = quench_alloc::FIXED_STACK_BASE as u64;
        let tp = thread_pointer();
        if !(stack..ARENA_BASE).contains(&tp) {
            return Err("the worker does not run on the fixed stack".into());
        }
        let before = std::mem::take(&mut *BEFORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let (tls, writable) = image_layout();
        let mut dump = Dump { wait: wait as u64, root: root as u64, fixed_next: quench_alloc::fixed_space_next() as u64, ..Dump::default() };
        let arena_end = ARENA_BASE + used.next_multiple_of(PAGE) as u64;
        dump.regions.push((ARENA_BASE, arena_end - ARENA_BASE, (libc::PROT_READ | libc::PROT_WRITE) as u64));
        for (start, end) in resident_runs(ARENA_BASE, arena_end) {
            push_pages(&mut dump.runs, start, end, true);
        }
        let image = std::fs::read_link("/proc/self/exe").ok().map(|path| path.to_string_lossy().into_owned());
        let table = crate::fixed_layout_table_range().map(|(start, length)| (start as u64, (start + length) as u64));
        let arena = ARENA_BASE..ARENA_BASE + (1 << 34);
        for mapping in mappings {
            let ours = (stack..arena.end).contains(&mapping.start);
            let earlier = before.contains(&(mapping.start, mapping.end));
            // The runtime's reservations; an anonymous mapping elsewhere is the C library's
            // (the standard library's own allocations of thread-local destructor lists go
            // straight to it), which a dump never carries.
            let fixed = quench_alloc::fixed_space_range();
            let kept = mapping.anonymous() && !earlier && (fixed.start as u64..fixed.end as u64).contains(&mapping.start);
            let in_table = table.is_some_and(|(start, end)| mapping.start >= start && mapping.end <= end);
            let in_image = writable.iter().any(|&(start, end)| mapping.start < end.next_multiple_of(PAGE as u64) && mapping.end > start);
            if kept {
                dump.regions.push((mapping.start, mapping.end - mapping.start, mapping.protection));
            }
            if (kept || in_table) && mapping.protection & libc::PROT_READ as u64 != 0 {
                for (start, end) in resident_runs(mapping.start, mapping.end) {
                    push_pages(&mut dump.runs, start, end, false);
                }
            }
            let process = ["[stack]", "[heap]", "[vdso]", "[vvar]"].contains(&mapping.name.as_str())
                || (mapping.name.starts_with('/') && Some(mapping.name.as_str()) != image.as_deref());
            if process || (mapping.anonymous() && !kept && !ours && !in_table && !in_image) {
                dump.foreign.push((mapping.start, mapping.end, if mapping.name.is_empty() { format!("anonymous {:#x}", mapping.start) } else { mapping.name.clone() }));
            }
        }
        dump.tls.push((tls, bytes_at(tp - tls, tp)));
        for (start, end) in writable {
            dump.statics.push((start, bytes_at(start, end)));
        }
        std::fs::write(&path, dump.encode(0, &mut Default::default())).map_err(|error| format!("{path}: {error}"))
    })();
    let status = match outcome {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("quench: image dump: {error}");
            2
        }
    };
    // SAFETY: ends the process without running its exit handlers, whose state is the dump's.
    unsafe { libc::_exit(status) }
}

/// Keeps of a dumping run's `raw` dump (`write`) the statics and thread-locals `keep` names
/// (address ranges; thread-local ones as distances below the thread pointer) whose words the
/// run changed from the image's initial bytes (`initial`, the file's view of an address), and
/// refusing a dump any of whose words points into the dumping process's own memory (its
/// stack, its C library heap, its vDSO, files it mapped), which the run that maps it does not
/// have: such a word differs in `again`, a second dumping run's.
pub fn finish(
    raw: &[u8], again: &[u8], keep_statics: &[(u64, u64)], keep_tls: &[(u64, u64)], initial: &dyn Fn(u64, usize) -> Vec<u8>, tls_initial: &dyn Fn(u64, usize) -> Vec<u8>,
) -> Result<Dump, String> {
    let mut dump = Dump::decode(raw)?;
    // A word that points into the dumping process's own memory differs in a second dumping
    // run, whose addresses the kernel places elsewhere; an integer that happens to fall in such
    // a range does not.
    let second = Dump::decode(again)?;
    let same_layout = dump.runs.len() == second.runs.len()
        && dump.runs.iter().zip(&second.runs).all(|(a, b)| a.0 == b.0 && a.1.len() == b.1.len())
        && dump.statics.iter().zip(&second.statics).all(|(a, b)| a.0 == b.0 && a.1.len() == b.1.len())
        && dump.tls.iter().zip(&second.tls).all(|(a, b)| a.0 == b.0 && a.1.len() == b.1.len());
    if !same_layout {
        return Err("two dumping runs left different memory layouts".into());
    }
    let mut stray: Vec<(String, usize)> = dump.foreign.iter().map(|(_, _, name)| (name.clone(), 0)).collect();
    let in_keep = |address: u64, keep: &[(u64, u64)]| keep.iter().any(|&(start, end)| (start..end).contains(&address));
    let mut check = |base: u64, a: &[u8], b: &[u8], kept: &dyn Fn(u64) -> bool| {
        for (index, (x, y)) in a.chunks_exact(8).zip(b.chunks_exact(8)).enumerate() {
            if x == y || !kept(base + 8 * index as u64) {
                continue;
            }
            let word = u64::from_le_bytes(x.try_into().unwrap_or_default());
            if let Some(found) = dump.foreign.iter().position(|&(start, end, _)| (start..end).contains(&word)) {
                stray[found].1 += 1;
            }
        }
    };
    for (a, b) in dump.runs.iter().zip(&second.runs) {
        check(a.0, &a.1, &b.1, &|_| true);
    }
    for (a, b) in dump.statics.iter().zip(&second.statics) {
        check(a.0, &a.1, &b.1, &|address| in_keep(address, keep_statics));
    }
    for (a, b) in dump.tls.iter().zip(&second.tls) {
        // Offsets within the block; a kept range is distances below its end.
        let size = a.0;
        check(0, &a.1, &b.1, &|offset| keep_tls.iter().any(|&(below, above)| (size - below..size - above).contains(&offset)));
    }
    let changed = |base: u64, bytes: &[u8], keep: &[(u64, u64)], before: &dyn Fn(u64, usize) -> Vec<u8>| {
        let mut patches: Vec<(u64, Vec<u8>)> = Vec::new();
        let end = base + bytes.len() as u64;
        for &(start, stop) in keep {
            let (start, stop) = (start.max(base), stop.min(end));
            if start >= stop {
                continue;
            }
            let now = &bytes[(start - base) as usize..(stop - base) as usize];
            let then = before(start, now.len());
            for (index, (a, b)) in now.iter().zip(&then).enumerate() {
                if a == b {
                    continue;
                }
                let at = start + index as u64;
                match patches.last_mut() {
                    Some((place, patch)) if *place + patch.len() as u64 == at => patch.push(*a),
                    _ => patches.push((at, vec![*a])),
                }
            }
        }
        patches
    };
    let statics = std::mem::take(&mut dump.statics);
    dump.statics = statics.iter().flat_map(|(base, bytes)| changed(*base, bytes, keep_statics, initial)).collect();
    // The thread-local block is kept as distances below the thread pointer: as addresses of a
    // block whose base is 0, its patches' places are negated back.
    let tls = std::mem::take(&mut dump.tls);
    dump.tls = tls.iter().flat_map(|(size, bytes)| {
        let keep: Vec<_> = keep_tls.iter().map(|&(below, above)| (size - below, size - above)).collect();
        changed(0, bytes, &keep, &|start, length| tls_initial(size - start, length))
            .into_iter().map(|(at, patch)| (size - at, patch)).collect::<Vec<_>>()
    }).collect();
    stray.retain(|(_, count)| *count != 0);
    if !stray.is_empty() {
        return Err(format!("dumped words point into the dumping process's own memory: {stray:?}"));
    }
    dump.foreign.clear();
    Ok(dump)
}

/// Appends `dumps` (`finish`'s, one per saved run) to the image at `path`, with the trailer
/// `adopt` finds them by.
pub fn append(path: &std::path::Path, dumps: &[Dump]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).map_err(|error| error.to_string())?;
    let mut at = file.metadata().map_err(|error| error.to_string())?.len().next_multiple_of(PAGE as u64);
    file.set_len(at).map_err(|error| error.to_string())?;
    let mut trailer = Vec::new();
    put(&mut trailer, dumps.len() as u64);
    let mut pool = std::collections::HashMap::new();
    for dump in dumps {
        let bytes = dump.encode(at, &mut pool);
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        put(&mut trailer, at);
        put(&mut trailer, bytes.len() as u64);
        at += bytes.len() as u64;
    }
    let length = trailer.len() as u64 + 16;
    put(&mut trailer, length);
    trailer.extend_from_slice(&TRAILER_MAGIC);
    file.write_all(&trailer).map_err(|error| error.to_string())
}

/// The root the dump of state `wait` holds, mapped from the dump this image carries; none
/// where it carries none, or any of its addresses is taken, which leaves the process as it was.
pub fn adopt(wait: usize) -> Option<*mut u8> {
    // The dump's TLS patch is the fixed-stack worker's (`write` refuses any other thread); a start that fell back to an
    // ordinary thread (`run_on_fixed_stack`) builds its state instead of copying TLS to the wrong place.
    if !(quench_alloc::FIXED_STACK_BASE as u64..ARENA_BASE).contains(&thread_pointer()) {
        return None;
    }
    let image = File::open("/proc/self/exe").ok()?;
    let length = image.metadata().ok()?.len();
    let mut tail = [0u8; 16];
    image.read_exact_at(&mut tail, length.checked_sub(16)?).ok()?;
    if tail[8..] != TRAILER_MAGIC {
        return None;
    }
    let trailer_length = u64::from_le_bytes(tail[..8].try_into().ok()?);
    let mut trailer = vec![0u8; trailer_length.checked_sub(16)? as usize];
    image.read_exact_at(&mut trailer, length.checked_sub(trailer_length)?).ok()?;
    let mut at = 0;
    let count = take(&trailer, &mut at).ok()?;
    if wait as u64 >= count {
        return None;
    }
    at += wait * 16;
    let offset = take(&trailer, &mut at).ok()?;
    // The header and its patches lie in the dump's first pages, before its runs; read into
    // fresh capacity, as zeroing first would cost more than the read.
    let mut prefix = [0u8; 16];
    image.read_exact_at(&mut prefix, offset).ok()?;
    let header_length = u64::from_le_bytes(prefix[8..16].try_into().ok()?) as usize;
    let mut first = Vec::<u8>::with_capacity(header_length);
    {
        use std::os::fd::AsRawFd;
        // SAFETY: `pread` writes at most `header_length` bytes into the spare capacity.
        let read = unsafe { libc::pread(image.as_raw_fd(), first.as_mut_ptr().cast(), header_length, offset as libc::off_t) };
        if read != header_length as isize {
            return None;
        }
        // SAFETY: `pread` initialized all `header_length` bytes.
        unsafe { first.set_len(header_length) };
    }
    let header = Header::parse(&first).ok()?;
    if header.wait != wait as u64 {
        return None;
    }
    // SAFETY: the dump was made by this binary (it is part of its file), so its arena, regions
    // and words are this binary's state at the same addresses; nothing maps over a taken range.
    unsafe {
        use std::os::fd::AsRawFd;
        let fd = image.as_raw_fd();
        let mut mapped: Vec<(u64, u64)> = Vec::new();
        let undo = |mapped: &[(u64, u64)]| for &(start, length) in mapped {
            libc::munmap(start as *mut libc::c_void, length as usize);
        };
        const MAP_FIXED_NOREPLACE: libc::c_int = 0x100000;
        for &(start, length, _) in &header.regions {
            let at = libc::mmap(start as *mut libc::c_void, length as usize, libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | MAP_FIXED_NOREPLACE, -1, 0);
            if at as u64 != start {
                if at != libc::MAP_FAILED {
                    libc::munmap(at, length as usize);
                }
                undo(&mapped);
                return None;
            }
            mapped.push((start, length));
        }
        for &(address, length, place, from_file) in &header.runs {
            let copied = if from_file {
                libc::mmap(address as *mut libc::c_void, length, libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_FIXED, fd, place as libc::off_t) as u64 == address
            } else {
                image.read_exact_at(std::slice::from_raw_parts_mut(address as *mut u8, length), place as u64).is_ok()
            };
            if !copied {
                undo(&mapped);
                return None;
            }
        }
        for &(start, length, protection) in &header.regions {
            libc::mprotect(start as *mut libc::c_void, length as usize, protection as libc::c_int);
        }
        for (address, range) in &header.statics {
            std::ptr::copy_nonoverlapping(first[range.clone()].as_ptr(), *address as *mut u8, range.len());
        }
        let tp = thread_pointer();
        for (below, range) in &header.tls {
            std::ptr::copy_nonoverlapping(first[range.clone()].as_ptr(), (tp - below) as *mut u8, range.len());
        }
    }
    quench_alloc::resume_fixed_space(header.fixed_next as usize);
    Some(header.root as *mut u8)
}


/// Runs `body` on the fixed-stack worker (`quench_alloc::run_on_fixed_stack`), where a dump's
/// thread-locals lie, or on a thread of its own where that range is taken. None where the body
/// panicked.
pub fn run_on_fixed_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let writer = slot.clone();
    let work: Box<dyn FnOnce() + Send> = Box::new(move || {
        let value = body();
        *writer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(value);
    });
    let rest = match quench_alloc::run_on_fixed_stack(c"quench-image", crate::WORKER_STACK_SIZE, work) {
        Ok(_) => None,
        Err(work) => Some(work),
    };
    if let Some(work) = rest {
        std::thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(work).ok()?.join().ok()?;
    }
    let value = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    value
}
