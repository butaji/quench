//! Compiles `stencils/rows.rs` (and the Number rows `stencils/library.rs` lists, generated into
//! `OUT_DIR/instances.rs`) with the toolchain's own rustc into an object, reads its functions and
//! relocations with a minimal ELF64 reader, and writes them as static tables
//! (`OUT_DIR/rustc_stencils.rs`). A row that contains a `call` or a `ret` is a build error:
//! rows chain by `become` only, and only the `leave_*` rows return.
#[path = "stencils/library.rs"]
mod library;

use std::{env, fs, path::PathBuf, process::Command};

const SOURCE: &str = "stencils/rows.rs";
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_32: u32 = 10;
const R_X86_64_32S: u32 = 11;

struct Section {
    kind: u32,
    offset: usize,
    size: usize,
    link: usize,
    info: usize,
}

struct Elf<'a> {
    data: &'a [u8],
    sections: Vec<Section>,
}

fn u16_at(data: &[u8], at: usize) -> usize {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as usize
}
fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}
fn c_string(data: &[u8], at: usize) -> String {
    let end = data[at..].iter().position(|b| *b == 0).expect("terminated string");
    String::from_utf8_lossy(&data[at..at + end]).into_owned()
}

impl<'a> Elf<'a> {
    fn parse(data: &'a [u8]) -> Self {
        assert!(data.starts_with(b"\x7fELF") && data[4] == 2 && data[5] == 1, "an ELF64 little-endian object");
        let (table, entry, count) = (u64_at(data, 0x28) as usize, u16_at(data, 0x3a), u16_at(data, 0x3c));
        let header = |k: usize| table + k * entry;
        let sections = (0..count)
            .map(|k| {
                let at = header(k);
                Section {
                    kind: u32_at(data, at + 4),
                    offset: u64_at(data, at + 0x18) as usize,
                    size: u64_at(data, at + 0x20) as usize,
                    link: u32_at(data, at + 0x28) as usize,
                    info: u32_at(data, at + 0x2c) as usize,
                }
            })
            .collect();
        Elf { data, sections }
    }

    /// Every symbol: (name, section index, value, info byte).
    fn symbols(&self) -> Vec<(String, usize, usize, u8)> {
        let symtab = self.sections.iter().find(|s| s.kind == 2).expect("a symbol table");
        let strings = &self.sections[symtab.link];
        (0..symtab.size / 24)
            .map(|k| {
                let at = symtab.offset + k * 24;
                (c_string(self.data, strings.offset + u32_at(self.data, at) as usize), u16_at(self.data, at + 6), u64_at(self.data, at + 8) as usize, self.data[at + 4])
            })
            .collect()
    }
}

/// `jmp rel32` and `jcc rel32` are the only transfers a row may make through a relocation.
fn check_transfer(row: &str, code: &[u8], at: usize) {
    let jmp = code[at - 1] == 0xe9;
    let jcc = at >= 2 && code[at - 2] == 0x0f && (0x80..0x90).contains(&code[at - 1]);
    assert!(jmp || jcc, "stencil {row}: relocation at {at} is not a jmp or jcc (a `call`?)");
}

/// Every file whose change must recompile the stencils: the stencil sources and the runtime's
/// staged rows they include, and nothing else.
const SOURCES: [&str; 15] = [
    "stencils/rows.rs", "stencils/heap_rows.rs", "stencils/facade.rs", "stencils/layout.rs", "stencils/library.rs", "stencils/numeric.rs", "stencils/stage.rs",
    "stencils/staged.rs", "../quench-runtime/src/staged/stage.rs", "../quench-runtime/src/staged/value.rs",
    "../quench-runtime/src/staged/object.rs", "../quench-runtime/src/staged/elements.rs",
    "../quench-runtime/src/staged/environment.rs", "../quench-runtime/src/staged/entry.rs", "build.rs",
];

fn main() {
    for source in SOURCES {
        println!("cargo::rerun-if-changed={source}");
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let table = out.join("rustc_stencils.rs");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        fs::write(&table, "pub(crate) static RAW: &[RawStencil] = &[];\n").unwrap();
        return;
    }
    let instances: String = library::library().into_iter().map(|c| format!("number_row!({}, {}, {}, {}, {}, {});\n", library::name(c), c.0, c.1, c.2, c.3, c.4)).collect();
    let heap: String = library::heap_library().into_iter().map(|c| format!("heap_row!({}, {}, {}, {}, {}, {});\n", library::heap_name(c), c.0, c.1, c.2, c.3, c.4)).collect();
    fs::write(out.join("instances.rs"), instances + &heap).unwrap();
    let object = compile(&out);
    fs::write(&table, stencil_table(&fs::read(&object).unwrap())).unwrap();
}

fn compile(out: &std::path::Path) -> PathBuf {
    let object = out.join("rows.o");
    let status = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition", "2024", "--crate-type=lib", "--crate-name=quench_stencil_rows", "--emit=obj", "-C", "opt-level=3", "-C", "panic=abort", "-C", "relocation-model=static", "-C", "codegen-units=8", "-C", "debuginfo=0"])
        .arg("-o")
        .arg(&object)
        .arg(SOURCE)
        .status()
        .expect("run rustc");
    assert!(status.success(), "rustc failed on {SOURCE}");
    object
}

/// The rows of an object as the static table `RAW`, sorted by name.
fn stencil_table(data: &[u8]) -> String {
    let elf = Elf::parse(data);
    let symbols = elf.symbols();
    let mut rows: Vec<(String, String)> = Vec::new();
    for (name, section, value, _) in symbols.iter().filter(|s| s.3 & 0xf == 2 && s.3 >> 4 == 1 && s.1 != 0 && !s.0.starts_with("_R")) {
        assert_eq!(*value, 0, "{name}: one function per section");
        let section_data = &elf.sections[*section];
        let code = &data[section_data.offset..section_data.offset + section_data.size];
        let returns = name.starts_with("leave_");
        assert!(returns || !code.contains(&0xc3) || !ends_with_ret(code), "stencil {name} ends with `ret`");
        let relocs = relocations(&elf, &symbols, data, name, *section, code);
        rows.push((name.clone(), format!("    RawStencil {{ name: {name:?}, code: &{code:?}, relocs: &{relocs:?} }},\n")));
    }
    rows.sort();
    let body: String = rows.into_iter().map(|(_, row)| row).collect();
    format!("pub(crate) static RAW: &[RawStencil] = &[\n{body}];\n")
}

/// The relocations of one row as (offset, class, index, addend).
fn relocations(elf: &Elf, symbols: &[(String, usize, usize, u8)], data: &[u8], name: &str, section: usize, code: &[u8]) -> Vec<(u16, u8, u8, i32)> {
    let mut relocs = Vec::new();
    for rela in elf.sections.iter().filter(|s| s.kind == 4 && s.info == section) {
        for k in 0..rela.size / 24 {
            let at = rela.offset + k * 24;
            let (offset, info, addend) = (u64_at(data, at) as usize, u64_at(data, at + 8), u64_at(data, at + 16) as i64);
            let (symbol, kind) = (&symbols[(info >> 32) as usize].0, (info & 0xffff_ffff) as u32);
            let (class, index) = hole_of(name, symbol, kind);
            if class == 1 {
                check_transfer(name, code, offset);
            }
            relocs.push((u16::try_from(offset).expect("a stencil under 64 KiB"), class, u8::try_from(index).expect("a small hole index"), i32::try_from(addend).expect("a small addend")));
        }
    }
    relocs
}

fn ends_with_ret(code: &[u8]) -> bool {
    code.last() == Some(&0xc3)
}

/// The class (0: absolute 32-bit field, 1: pc-relative 32-bit successor, 2: absolute 64-bit) and
/// hole index of a relocation. Value hole `HOLE_V<n>` is index n; `HOLE_NEXT` 0, `HOLE_TAKEN` 1,
/// `HOLE_EXIT<k>` 2 + k (the successor indexes of `quench_bjit`).
fn hole_of(row: &str, symbol: &str, kind: u32) -> (u8, u32) {
    let rest = symbol.strip_prefix("HOLE_").unwrap_or_else(|| panic!("stencil {row}: relocation against {symbol}, not a hole"));
    let class = match kind {
        R_X86_64_32 | R_X86_64_32S => 0,
        R_X86_64_PC32 | R_X86_64_PLT32 => 1,
        R_X86_64_64 => 2,
        other => panic!("stencil {row}: relocation kind {other}"),
    };
    let number = |digits: &str| digits.parse::<u32>().unwrap_or_else(|_| panic!("stencil {row}: bad hole {symbol}"));
    match (class, rest) {
        (1, "NEXT") => (1, 0),
        (1, "TAKEN") => (1, 1),
        (1, exit) if exit.starts_with("EXIT") => (1, 2 + number(&exit[4..])),
        (_, value) if value.starts_with('V') && class != 1 => (class, number(&value[1..])),
        _ => panic!("stencil {row}: hole {symbol} used in the wrong way"),
    }
}
