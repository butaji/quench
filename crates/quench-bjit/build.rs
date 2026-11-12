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

// ELF64 layout: header, section header and symbol/relocation entry fields, in bytes.
const ELF_MAGIC: &[u8] = b"\x7fELF";
const CLASS_AT: usize = 4;
const DATA_AT: usize = 5;
const CLASS_64: u8 = 2;
const LITTLE_ENDIAN: u8 = 1;
const SECTION_TABLE_AT: usize = 0x28;
const SECTION_ENTRY_AT: usize = 0x3a;
const SECTION_COUNT_AT: usize = 0x3c;
const SECTION_KIND: usize = 4;
const SECTION_OFFSET: usize = 0x18;
const SECTION_SIZE: usize = 0x20;
const SECTION_LINK: usize = 0x28;
const SECTION_INFO: usize = 0x2c;
const SYMTAB: u32 = 2;
const RELA: u32 = 4;
const SYMBOL_SIZE: usize = 24;
const SYMBOL_INFO: usize = 4;
const SYMBOL_SECTION: usize = 6;
const SYMBOL_VALUE: usize = 8;
const RELA_SIZE: usize = 24;
const RELA_INFO: usize = 8;
const RELA_ADDEND: usize = 16;
const SYMBOL_FUNCTION: u8 = 2;
const SYMBOL_GLOBAL: u8 = 1;
const SYMBOL_KIND_MASK: u8 = 0xf;
const SYMBOL_BIND_SHIFT: u32 = 4;
const SYMBOL_INDEX_SHIFT: u32 = 32;
const RELOC_KIND_MASK: u64 = 0xffff_ffff;
const U16_BYTES: usize = 2;
const U32_BYTES: usize = 4;
const U64_BYTES: usize = 8;
const EXIT_PREFIX: usize = 4;
const EXIT_BASE: u32 = 2;
const ABS32: u8 = 0;
const SUCCESSOR: u8 = 1;
const ABS64: u8 = 2;

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
    u16::from_le_bytes(data[at..at + U16_BYTES].try_into().unwrap()) as usize
}
fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + U32_BYTES].try_into().unwrap())
}
fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + U64_BYTES].try_into().unwrap())
}
fn c_string(data: &[u8], at: usize) -> String {
    let end = data[at..].iter().position(|b| *b == 0).expect("terminated string");
    String::from_utf8_lossy(&data[at..at + end]).into_owned()
}

impl<'a> Elf<'a> {
    fn parse(data: &'a [u8]) -> Self {
        assert!(data.starts_with(ELF_MAGIC) && data[CLASS_AT] == CLASS_64 && data[DATA_AT] == LITTLE_ENDIAN, "an ELF64 little-endian object");
        let (table, entry, count) = (u64_at(data, SECTION_TABLE_AT) as usize, u16_at(data, SECTION_ENTRY_AT), u16_at(data, SECTION_COUNT_AT));
        let header = |k: usize| table + k * entry;
        let sections = (0..count)
            .map(|k| {
                let at = header(k);
                Section {
                    kind: u32_at(data, at + SECTION_KIND),
                    offset: u64_at(data, at + SECTION_OFFSET) as usize,
                    size: u64_at(data, at + SECTION_SIZE) as usize,
                    link: u32_at(data, at + SECTION_LINK) as usize,
                    info: u32_at(data, at + SECTION_INFO) as usize,
                }
            })
            .collect();
        Elf { data, sections }
    }

    /// Every symbol: (name, section index, value, info byte).
    fn symbols(&self) -> Vec<(String, usize, usize, u8)> {
        let symtab = self.sections.iter().find(|s| s.kind == SYMTAB).expect("a symbol table");
        let strings = &self.sections[symtab.link];
        (0..symtab.size / SYMBOL_SIZE)
            .map(|k| {
                let at = symtab.offset + k * SYMBOL_SIZE;
                (c_string(self.data, strings.offset + u32_at(self.data, at) as usize), u16_at(self.data, at + SYMBOL_SECTION), u64_at(self.data, at + SYMBOL_VALUE) as usize, self.data[at + SYMBOL_INFO])
            })
            .collect()
    }
}

/// `jmp rel32` and `jcc rel32` are the only transfers a row may make through a relocation.
fn check_transfer(row: &str, code: &[u8], at: usize) {
    let before = &code[..at];
    let jmp = matches!(before, [.., 0xe9]);
    let jcc = matches!(before, [.., 0x0f, 0x80..=0x8f]);
    let call = row.starts_with("entry_") && matches!(before, [.., 0xe8]);
    assert!(jmp || jcc || call, "stencil {row}: relocation at {at} is not a jmp or jcc (a `call`?)");
}

/// Every file whose change must recompile the stencils: the stencil sources and the runtime's
/// staged rows they include, and nothing else.
const SOURCES: [&str; 12] = [
    "stencils", "../quench-runtime/src/staged/stage.rs", "../quench-runtime/src/staged/value.rs",
    "../quench-runtime/src/staged/object.rs", "../quench-runtime/src/staged/elements.rs",
    "../quench-runtime/src/staged/environment.rs", "../quench-runtime/src/staged/entry.rs", "../quench-runtime/src/staged/frame.rs",
    "../quench-runtime/src/staged/ic.rs", "../quench-runtime/src/staged/slot_store.rs", "build.rs", "src/numeric.rs",
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
    for (name, section, value, _) in symbols.iter().filter(|s| s.3 & SYMBOL_KIND_MASK == SYMBOL_FUNCTION && s.3 >> SYMBOL_BIND_SHIFT == SYMBOL_GLOBAL && s.1 != 0 && !s.0.starts_with("_R")) {
        assert_eq!(*value, 0, "{name}: one function per section");
        let section_data = &elf.sections[*section];
        let code = &data[section_data.offset..section_data.offset + section_data.size];
        let returns = name.starts_with("leave_") || name.starts_with("entry_");
        assert!(returns || !ends_with_ret(code), "stencil {name} ends with `ret`");
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
    for rela in elf.sections.iter().filter(|s| s.kind == RELA && s.info == section) {
        for k in 0..rela.size / RELA_SIZE {
            let at = rela.offset + k * RELA_SIZE;
            let (offset, info, addend) = (u64_at(data, at) as usize, u64_at(data, at + RELA_INFO), u64_at(data, at + RELA_ADDEND) as i64);
            let (symbol, kind) = (&symbols[(info >> SYMBOL_INDEX_SHIFT) as usize].0, (info & RELOC_KIND_MASK) as u32);
            let (class, index) = hole_of(name, symbol, kind);
            if class == SUCCESSOR {
                check_transfer(name, code, offset);
            }
            relocs.push((u16::try_from(offset).expect("a stencil under 64 KiB"), class, u8::try_from(index).expect("a small hole index"), i32::try_from(addend).expect("a small addend")));
        }
    }
    relocs
}

fn ends_with_ret(code: &[u8]) -> bool {
    matches!(code.last(), Some(0xc3))
}

/// The class (0: absolute 32-bit field, 1: pc-relative 32-bit successor, 2: absolute 64-bit) and
/// hole index of a relocation. Value hole `HOLE_V<n>` is index n; `HOLE_NEXT` 0, `HOLE_TAKEN` 1,
/// `HOLE_EXIT<k>` 2 + k (the successor indexes of `quench_bjit`).
fn hole_of(row: &str, symbol: &str, kind: u32) -> (u8, u32) {
    let rest = symbol.strip_prefix("HOLE_").unwrap_or_else(|| panic!("stencil {row}: relocation against {symbol}, not a hole"));
    let class = match kind {
        R_X86_64_32 | R_X86_64_32S => ABS32,
        R_X86_64_PC32 | R_X86_64_PLT32 => SUCCESSOR,
        R_X86_64_64 => ABS64,
        other => panic!("stencil {row}: relocation kind {other}"),
    };
    let number = |digits: &str| digits.parse::<u32>().unwrap_or_else(|_| panic!("stencil {row}: bad hole {symbol}"));
    match (class, rest) {
        (SUCCESSOR, "NEXT") => (SUCCESSOR, 0),
        (SUCCESSOR, "TAKEN") => (SUCCESSOR, 1),
        (SUCCESSOR, exit) if exit.starts_with("EXIT") => (SUCCESSOR, EXIT_BASE + number(&exit[EXIT_PREFIX..])),
        (_, value) if value.starts_with('V') && class != SUCCESSOR => (class, number(&value[1..])),
        _ => panic!("stencil {row}: hole {symbol} used in the wrong way"),
    }
}
