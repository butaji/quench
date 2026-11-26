//! An image's runtime at rest, mapped from a dump instead of decoded from its snapshot.
//!
//! A run saved at its first wait for input (`aot::prefix::reach_wait`) resumes by decoding its
//! saved runtime (`SavedWait::rest`), most of an image's start. The build runs the image once
//! more per saved run, with every allocation in the dump arena (`quench_alloc::begin_dump`)
//! and every reservation at its fixed address (`quench_alloc::map_anonymous`), on a worker
//! whose stack, and so its thread-locals, sits at a fixed address
//! (`quench_alloc::run_on_fixed_stack`); once the snapshot is decoded it writes the process
//! memory that state lies in (`write`). The build keeps of the statics and thread-locals only
//! the runtime's own that the run changed (`finish`) and appends the dump to the image. As the
//! image starts it maps the dump back at the same addresses (`adopt`) and binds the host to
//! its own process (`NodeHost::adopt_process`), where the decode would have run.
//!
//! A start that cannot have every address, or runs under a configuration the dump did not
//! (`adoptable`), decodes the snapshot as before.

use std::fs::File;
use std::os::unix::fs::FileExt;

/// `WAIT:PATH`: the run dumps the saved run `WAIT` to `PATH` once it is restored.
pub(crate) const DUMP_VARIABLE: &str = "QUENCH_IMAGE_DUMP";
const MAGIC: [u8; 8] = *b"QNCHDMP1";
const TRAILER_MAGIC: [u8; 8] = *b"QNCHDMPT";
const PAGE: usize = 4096;
/// The arena's address.
const ARENA_BASE: u64 = quench_alloc::DUMP_BASE as u64;

/// A dump: as the dumping run writes it (`write`), its statics and thread-local block whole;
/// as the build appends it (`finish`), only the runtime's own words the run changed.
#[derive(Default)]
pub(crate) struct Dump {
    pub(crate) wait: u64,
    /// The `Box<(EntryRuntime, NativeJsProgram)>` the run leaked.
    pub(crate) root: u64,
    /// `quench_alloc::fixed_space_next` after the run.
    pub(crate) fixed_next: u64,
    /// Anonymous mappings the run made: start, length, protection.
    pub(crate) regions: Vec<(u64, u64, u64)>,
    /// Resident pages of those, of the arena and of the fixed layout table: address, bytes, and
    /// whether the page maps straight from the file (the arena's, which the run never gives
    /// back to the kernel) or is copied (a reservation's, whose released pages must read zero).
    pub(crate) runs: Vec<(u64, Vec<u8>, bool)>,
    /// Static words: address, bytes.
    pub(crate) statics: Vec<(u64, Vec<u8>)>,
    /// Thread-local words of the worker: distance below the thread pointer, bytes.
    pub(crate) tls: Vec<(u64, Vec<u8>)>,
    /// Ranges of the dumping process no dumped word may point into: start, end, name.
    pub(crate) foreign: Vec<(u64, u64, String)>,
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
    /// The dump as bytes: a header holding the statics and thread-local words, then the runs,
    /// each page aligned so a run maps straight from the file.
    pub(crate) fn encode(&self) -> Vec<u8> {
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
        // The runs' places follow the header, whose length they depend on: the table is its
        // fixed-size tail.
        let table = 8 * (1 + 2 * self.runs.len());
        let mut at = (header.len() + table).next_multiple_of(PAGE);
        let mut places = Vec::new();
        for (_, bytes, _) in &self.runs {
            places.push(at);
            at += bytes.len().next_multiple_of(PAGE);
        }
        put(&mut header, self.runs.len() as u64);
        for ((address, bytes, mapped), place) in self.runs.iter().zip(&places) {
            put(&mut header, address | (u64::from(*mapped) << 63));
            put(&mut header, ((bytes.len() as u64) << 32) | (*place as u64 / PAGE as u64));
        }
        let length = header.len() as u64;
        header[8..16].copy_from_slice(&length.to_le_bytes());
        let mut out = header;
        out.resize(out.len().next_multiple_of(PAGE), 0);
        for (_, bytes, _) in &self.runs {
            out.extend_from_slice(bytes);
            out.resize(out.len().next_multiple_of(PAGE), 0);
        }
        out
    }

    /// Reads `encode`'s bytes back.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
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

/// The saved run this process dumps (`DUMP_VARIABLE`), if it dumps one.
pub(crate) fn requested_wait() -> Option<usize> {
    let value = std::env::var(DUMP_VARIABLE).ok()?;
    value.split_once(':')?.0.parse().ok()
}

/// The anonymous mappings that held before the dump began (`begin`), which it leaves out.
static BEFORE: std::sync::Mutex<Vec<(u64, u64)>> = std::sync::Mutex::new(Vec::new());

/// Starts the dump `DUMP_VARIABLE` asks for: from here every allocation is the arena's. Call
/// first in `main`.
pub fn begin() {
    if requested_wait().is_none() {
        return;
    }
    // Read before the arena serves allocations, so the dump holds no copy of it.
    let before = maps().into_iter().filter(|mapping| mapping.anonymous()).map(|mapping| (mapping.start, mapping.end)).collect();
    *BEFORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = before;
    if !quench_alloc::begin_dump() {
        eprintln!("quench: image dump: the arena's range is taken");
        std::process::exit(2);
    }
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

fn bytes_at(start: u64, end: u64) -> Vec<u8> {
    // SAFETY: the caller names memory this process maps readable.
    unsafe { std::slice::from_raw_parts(start as *const u8, (end - start) as usize) }.to_vec()
}

/// Ends the dump `begin` started: `root` is the restored runtime and entry, leaked into the
/// arena. Writes the process memory they lie in to the dump's path and exits.
pub(crate) fn write(wait: usize, root: *mut u8) -> ! {
    let outcome = (|| -> Result<(), String> {
        let path = std::env::var(DUMP_VARIABLE).ok().and_then(|value| value.split_once(':').map(|(_, path)| path.to_owned())).ok_or("no dump path")?;
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
            dump.runs.push((start, bytes_at(start, end), true));
        }
        let table = quench_runtime::fixed_layout_table_range().map(|(start, length)| (start as u64, (start + length) as u64));
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
                    dump.runs.push((start, bytes_at(start, end), false));
                }
            }
            let process = ["[stack]", "[heap]", "[vdso]", "[vvar]"].contains(&mapping.name.as_str());
            if process || (mapping.anonymous() && !kept && !ours && !in_table && !in_image) {
                dump.foreign.push((mapping.start, mapping.end, if mapping.name.is_empty() { format!("anonymous {:#x}", mapping.start) } else { mapping.name.clone() }));
            }
        }
        dump.tls.push((tls, bytes_at(tp - tls, tp)));
        for (start, end) in writable {
            dump.statics.push((start, bytes_at(start, end)));
        }
        std::fs::write(&path, dump.encode()).map_err(|error| format!("{path}: {error}"))
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
/// stack, its C library heap, its vDSO), which the run that maps it does not have.
pub(crate) fn finish(
    raw: &[u8], keep_statics: &[(u64, u64)], keep_tls: &[(u64, u64)], initial: &dyn Fn(u64, usize) -> Vec<u8>, tls_initial: &dyn Fn(u64, usize) -> Vec<u8>,
) -> Result<Vec<u8>, String> {
    let mut dump = Dump::decode(raw)?;
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
    let mut stray: Vec<(String, usize)> = dump.foreign.iter().map(|(_, _, name)| (name.clone(), 0)).collect();
    let words = dump.runs.iter().map(|(_, bytes, _)| bytes.as_slice())
        .chain(dump.statics.iter().chain(&dump.tls).map(|(_, bytes)| bytes.as_slice()));
    for bytes in words {
        for word in bytes.chunks_exact(8) {
            let word = u64::from_le_bytes(word.try_into().unwrap_or_default());
            if let Some(index) = dump.foreign.iter().position(|&(start, end, _)| (start..end).contains(&word)) {
                stray[index].1 += 1;
            }
        }
    }
    stray.retain(|(_, count)| *count != 0);
    if !stray.is_empty() {
        return Err(format!("dumped words point into the dumping process's own memory: {stray:?}"));
    }
    dump.foreign.clear();
    Ok(dump.encode())
}

/// Appends `dumps` (`finish`'s, one per saved run) to the image at `path`, with the trailer
/// `adopt` finds them by.
pub(crate) fn append(path: &std::path::Path, dumps: &[Vec<u8>]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).map_err(|error| error.to_string())?;
    let mut at = file.metadata().map_err(|error| error.to_string())?.len().next_multiple_of(PAGE as u64);
    file.set_len(at).map_err(|error| error.to_string())?;
    let mut trailer = Vec::new();
    put(&mut trailer, dumps.len() as u64);
    for dump in dumps {
        file.write_all(dump).map_err(|error| error.to_string())?;
        put(&mut trailer, at);
        put(&mut trailer, dump.len() as u64);
        at += dump.len() as u64;
    }
    let length = trailer.len() as u64 + 16;
    put(&mut trailer, length);
    trailer.extend_from_slice(&TRAILER_MAGIC);
    file.write_all(&trailer).map_err(|error| error.to_string())
}

/// Whether this start may map a dump: it runs under the configuration the dump was made in.
fn adoptable() -> bool {
    ["QUENCH_DEV", "QUENCH_TRACE", "NODE_OPTIONS", DUMP_VARIABLE].iter().all(|name| std::env::var_os(name).is_none())
}

/// The runtime and entry of saved run `wait`, mapped from the dump this image carries; none
/// where it carries none, or any of its addresses is taken, which leaves the process as it was.
pub(crate) fn adopt(wait: usize) -> Option<*mut u8> {
    if !adoptable() {
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
    // The header and its patches lie in the dump's first pages, before its runs.
    let mut first = vec![0u8; PAGE];
    image.read_exact_at(&mut first, offset).ok()?;
    let header_length = u64::from_le_bytes(first[8..16].try_into().ok()?) as usize;
    if header_length > first.len() {
        first.resize(header_length, 0);
        image.read_exact_at(&mut first, offset).ok()?;
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
                    libc::MAP_PRIVATE | libc::MAP_FIXED, fd, (offset + place as u64) as libc::off_t) as u64 == address
            } else {
                image.read_exact_at(std::slice::from_raw_parts_mut(address as *mut u8, length), offset + place as u64).is_ok()
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
