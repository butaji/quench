//! What a dump carries of an image: its statics and thread-locals (`UNDUMPED` names those it leaves
//! out), and the dump of each state of a linked image (`dump_image`).

use super::{finish, Dump, ADOPTION_VARIABLES, DUMP_VARIABLE};
use object::{Object, ObjectSection, ObjectSymbol};
use std::path::Path;
use std::process::{Command, Stdio};

/// The statics and thread-locals a dump never carries (`dump_image`): the standard library's,
/// the allocator's, the runtime's process state (its clocks and trace switches, which the
/// environment sets) and libraries' thread pools and caches of the build machine's time zone.
/// Each is a crate root and module names under it, as v0 symbols spell them. A host adds the
/// state of its own process.
pub const UNDUMPED: &[&[&str]] = &[
    &["std"], &["core"], &["alloc"], &["quench_alloc"],
    &["quench_runtime", "trace"], &["quench_runtime", "clock"], &["quench_runtime", "vm", "date", "local_zone"],
    &["tracing_core"], &["parking_lot_core"], &["crossbeam_epoch"], &["rayon_core"], &["chrono"],
];

/// The crate root and the identifiers after it of a Rust symbol (v0 or legacy mangling).
fn rust_path(symbol: &str) -> Option<(String, String)> {
    let take_ident = |bytes: &[u8], at: &mut usize| -> Option<String> {
        let start = *at;
        while *at < bytes.len() && bytes[*at].is_ascii_digit() {
            *at += 1;
        }
        let length: usize = std::str::from_utf8(&bytes[start..*at]).ok()?.parse().ok()?;
        if bytes.get(*at) == Some(&b'_') {
            *at += 1;
        }
        let ident = std::str::from_utf8(bytes.get(*at..*at + length)?).ok()?.to_owned();
        *at += length;
        Some(ident)
    };
    let bytes = symbol.as_bytes();
    if let Some(rest) = symbol.strip_prefix("_ZN") {
        let mut at = 0;
        let crate_name = take_ident(rest.as_bytes(), &mut at)?;
        return Some((crate_name, rest[at..].to_owned()));
    }
    let mut at = 2;
    if !symbol.starts_with("_R") {
        return None;
    }
    loop {
        match *bytes.get(at)? {
            b'N' => at += 2,
            b'M' | b'X' | b'Y' => {
                at += 1;
                if bytes.get(at) == Some(&b's') {
                    while *bytes.get(at)? != b'_' {
                        at += 1;
                    }
                    at += 1;
                }
            }
            b'C' => {
                at += 1;
                if bytes.get(at) == Some(&b's') {
                    while *bytes.get(at)? != b'_' {
                        at += 1;
                    }
                    at += 1;
                }
                let crate_name = take_ident(bytes, &mut at)?;
                return Some((crate_name, symbol[at..].to_owned()));
            }
            _ => return None,
        }
    }
}

/// Whether a dump carries the static or thread-local `symbol`, given the paths `undumped` names.
fn dumped_symbol(symbol: &str, undumped: &[&[&str]]) -> bool {
    let Some((crate_name, rest)) = rust_path(symbol) else { return false };
    !undumped.iter().any(|path| path[0] == crate_name && path[1..].iter().all(|ident| rest.contains(&format!("{}{ident}", ident.len()))))
}

/// The writable statics and the thread-locals a dump keeps: address ranges, and thread-local
/// ones as distances below the block's end (`block`), which `finish` reads back.
fn kept_ranges(file: &object::File, block: u64, undumped: &[&[&str]]) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
    let mut keep_statics = Vec::new();
    let mut keep_tls = Vec::new();
    for symbol in file.symbols() {
        if symbol.size() == 0 || !symbol.name().is_ok_and(|name| dumped_symbol(name, undumped)) {
            continue;
        }
        match symbol.kind() {
            object::SymbolKind::Tls => keep_tls.push((block - symbol.address(), block - symbol.address() - symbol.size())),
            object::SymbolKind::Data => {
                let writable = symbol.section_index().and_then(|index| file.section_by_index(index).ok()).is_some_and(|section| {
                    matches!(section.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & u64::from(object::elf::SHF_WRITE) != 0)
                });
                if writable {
                    keep_statics.push((symbol.address(), symbol.address() + symbol.size()));
                }
            }
            _ => {}
        }
    }
    (keep_statics, keep_tls)
}

/// The bytes the image's file holds at `address`, zero where no section does.
fn initial_bytes(file: &object::File, address: u64, length: usize) -> Vec<u8> {
    let mut out = vec![0u8; length];
    for section in file.sections() {
        let (start, size) = (section.address(), section.size());
        if address >= start && address + length as u64 <= start + size {
            if let Ok(data) = section.data() {
                if let Some(slice) = data.get((address - start) as usize..(address - start) as usize + length) {
                    out.copy_from_slice(slice);
                }
            }
        }
    }
    out
}

/// One run of the image with the dump variable set: its dump is written to `raw_path`.
fn run_dump(image: &Path, prepare: &dyn Fn(&mut Command), index: usize, env: &[(String, String)], raw_path: &Path) -> Result<Vec<u8>, String> {
    let mut run = Command::new(image);
    prepare(&mut run);
    for name in ADOPTION_VARIABLES {
        run.env_remove(name);
    }
    for (name, value) in env {
        run.env(name, value);
    }
    run.env(DUMP_VARIABLE, format!("{index}:{}", raw_path.display())).stdin(Stdio::null()).stdout(Stdio::null());
    crate::native_link::checked_output(&mut run, "image dump")?;
    std::fs::read(raw_path).map_err(|error| error.to_string())
}

/// Dumps each state of `runs` (its environment, by index) of the linked `image`, which still has
/// its symbol table. Each runs twice, so a word that differs between the two runs is an address
/// of the run's own (`finish`). `prepare` configures each run's command (a host's environment);
/// `undumped` names the statics a dump leaves out (`UNDUMPED`, and a host's own). Returns each
/// dump with the size of its raw form, or why one cannot be dumped.
pub fn dump_image(
    image: &Path, work: &Path, runs: &[Vec<(String, String)>], undumped: &[&[&str]], prepare: &dyn Fn(&mut Command),
) -> Result<Vec<(Dump, usize)>, String> {
    let bytes = std::fs::read(image).map_err(|error| error.to_string())?;
    let file = object::File::parse(&*bytes).map_err(|error| error.to_string())?;
    // The thread-local segment: its initial image and its block's size below the thread pointer.
    let elf = object::read::elf::ElfFile64::<object::Endianness>::parse(&*bytes).map_err(|error| error.to_string())?;
    use object::read::elf::ProgramHeader;
    let endian = elf.endian();
    let header = elf.elf_program_headers().iter().find(|header| header.p_type(endian) == object::elf::PT_TLS).ok_or("the image has no thread-local segment")?;
    let (tls_offset, tls_file, tls_memory, tls_align) = (header.p_offset(endian), header.p_filesz(endian), header.p_memsz(endian), header.p_align(endian).max(1));
    let block = tls_memory.next_multiple_of(tls_align);
    let (keep_statics, keep_tls) = kept_ranges(&file, block, undumped);
    let initial = |address: u64, length: usize| initial_bytes(&file, address, length);
    let tls_initial = |below: u64, length: usize| -> Vec<u8> {
        let offset = block - below;
        (0..length as u64).map(|index| {
            let at = offset + index;
            if at < tls_file { bytes[(tls_offset + at) as usize] } else { 0 }
        }).collect()
    };
    let mut dumps = Vec::new();
    for (index, env) in runs.iter().enumerate() {
        let raw = run_dump(image, prepare, index, env, &work.join(format!("dump-{index}.raw")))?;
        let again = run_dump(image, prepare, index, env, &work.join(format!("dump-{index}.again")))?;
        let dump = finish(&raw, &again, &keep_statics, &keep_tls, &initial, &tls_initial)?;
        dumps.push((dump, raw.len()));
    }
    Ok(dumps)
}
