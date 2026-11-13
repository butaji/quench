//! A minimal ELF64 little-endian reader (x86-64 and AArch64 Linux): one function per section,
//! `SHT_RELA` relocations, mapped to hole classes.

use crate::bytes::{ObjectReloc, ObjectRow, c_string, is_row_name, u16_at, u32_at, u64_at};
use crate::class::{ABS32, ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, REL32};

const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_32: u32 = 10;
const R_X86_64_32S: u32 = 11;
/// (relocation type, class) per machine.
const X86_64_CLASSES: [(u32, u8); 5] = [(R_X86_64_64, ABS64), (R_X86_64_PC32, REL32), (R_X86_64_PLT32, REL32), (R_X86_64_32, ABS32), (R_X86_64_32S, ABS32)];
const R_AARCH64_ABS64: u32 = 257;
const R_AARCH64_MOVW_UABS_G0: u32 = 263;
const R_AARCH64_MOVW_UABS_G0_NC: u32 = 264;
const R_AARCH64_MOVW_UABS_G1: u32 = 265;
const R_AARCH64_MOVW_UABS_G1_NC: u32 = 266;
const R_AARCH64_MOVW_UABS_G2: u32 = 267;
const R_AARCH64_MOVW_UABS_G2_NC: u32 = 268;
const R_AARCH64_MOVW_UABS_G3: u32 = 269;
const R_AARCH64_TSTBR14: u32 = 279;
const R_AARCH64_CONDBR19: u32 = 280;
const R_AARCH64_JUMP26: u32 = 282;
const R_AARCH64_CALL26: u32 = 283;
/// `MOVW_UABS_G<n>` and `G<n>_NC` patch group n of a 64-bit value; `CONDBR19` and `TSTBR14` are the
/// conditional and test branches a `become` through a hole may lower to.
const AARCH64_CLASSES: [(u32, u8); 12] = [
    (R_AARCH64_ABS64, ABS64),
    (R_AARCH64_JUMP26, BRANCH26),
    (R_AARCH64_CALL26, BRANCH26),
    (R_AARCH64_CONDBR19, BRANCH19),
    (R_AARCH64_TSTBR14, BRANCH14),
    (R_AARCH64_MOVW_UABS_G0, MOVW),
    (R_AARCH64_MOVW_UABS_G0_NC, MOVW),
    (R_AARCH64_MOVW_UABS_G1, MOVW + 1),
    (R_AARCH64_MOVW_UABS_G1_NC, MOVW + 1),
    (R_AARCH64_MOVW_UABS_G2, MOVW + 2),
    (R_AARCH64_MOVW_UABS_G2_NC, MOVW + 2),
    (R_AARCH64_MOVW_UABS_G3, MOVW + 3),
];

// ELF64 layout: header, section header and symbol/relocation entry fields, in bytes.
const ELF_MAGIC: &[u8] = b"\x7fELF";
const CLASS_AT: usize = 4;
const DATA_AT: usize = 5;
const CLASS_64: u8 = 2;
const LITTLE_ENDIAN: u8 = 1;
const MACHINE_AT: usize = 0x12;
const EM_X86_64: usize = 62;
const EM_AARCH64: usize = 183;
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

struct Section {
    kind: u32,
    offset: usize,
    size: usize,
    link: usize,
    info: usize,
}

struct Symbol {
    name: String,
    section: usize,
    value: usize,
    info: u8,
}

fn sections(data: &[u8]) -> Vec<Section> {
    let (table, entry, count) = (u64_at(data, SECTION_TABLE_AT) as usize, u16_at(data, SECTION_ENTRY_AT), u16_at(data, SECTION_COUNT_AT));
    (0..count)
        .map(|k| {
            let at = table + k * entry;
            Section {
                kind: u32_at(data, at + SECTION_KIND),
                offset: u64_at(data, at + SECTION_OFFSET) as usize,
                size: u64_at(data, at + SECTION_SIZE) as usize,
                link: u32_at(data, at + SECTION_LINK) as usize,
                info: u32_at(data, at + SECTION_INFO) as usize,
            }
        })
        .collect()
}

fn symbols(data: &[u8], sections: &[Section]) -> Vec<Symbol> {
    let symtab = sections.iter().find(|s| s.kind == SYMTAB).expect("a symbol table");
    let strings = &sections[symtab.link];
    (0..symtab.size / SYMBOL_SIZE)
        .map(|k| {
            let at = symtab.offset + k * SYMBOL_SIZE;
            Symbol {
                name: c_string(data, strings.offset + u32_at(data, at) as usize),
                section: u16_at(data, at + SYMBOL_SECTION),
                value: u64_at(data, at + SYMBOL_VALUE) as usize,
                info: data[at + SYMBOL_INFO],
            }
        })
        .collect()
}

/// The hole class of relocation type `kind` on `machine`.
fn class_of(machine: usize, row: &str, kind: u32) -> u8 {
    let table: &[(u32, u8)] = match machine {
        EM_X86_64 => &X86_64_CLASSES,
        EM_AARCH64 => &AARCH64_CLASSES,
        other => panic!("ELF machine {other}"),
    };
    table.iter().find(|(k, _)| *k == kind).unwrap_or_else(|| panic!("stencil {row}: relocation kind {kind}")).1
}

fn relocations(data: &[u8], sections: &[Section], symbols: &[Symbol], row: &Symbol) -> Vec<ObjectReloc> {
    let machine = u16_at(data, MACHINE_AT);
    let mut relocs = Vec::new();
    for rela in sections.iter().filter(|s| s.kind == RELA && s.info == row.section) {
        for k in 0..rela.size / RELA_SIZE {
            let at = rela.offset + k * RELA_SIZE;
            let info = u64_at(data, at + RELA_INFO);
            let kind = (info & RELOC_KIND_MASK) as u32;
            relocs.push(ObjectReloc {
                offset: u64_at(data, at) as usize,
                symbol: symbols[(info >> SYMBOL_INDEX_SHIFT) as usize].name.clone(),
                class: class_of(machine, &row.name, kind),
                addend: u64_at(data, at + RELA_ADDEND) as i64,
            });
        }
    }
    relocs
}

/// The global function symbols of the ELF64 object `data`, each with its code and relocations.
pub fn rows(data: &[u8]) -> Vec<ObjectRow> {
    assert!(data.starts_with(ELF_MAGIC) && data[CLASS_AT] == CLASS_64 && data[DATA_AT] == LITTLE_ENDIAN, "an ELF64 little-endian object");
    let sections = sections(data);
    let symbols = symbols(data, &sections);
    let is_row = |s: &&Symbol| s.info & SYMBOL_KIND_MASK == SYMBOL_FUNCTION && s.info >> SYMBOL_BIND_SHIFT == SYMBOL_GLOBAL && s.section != 0 && is_row_name(&s.name);
    symbols
        .iter()
        .filter(is_row)
        .map(|row| {
            assert_eq!(row.value, 0, "{}: one function per section", row.name);
            let section = &sections[row.section];
            ObjectRow { name: row.name.clone(), code: data[section.offset..section.offset + section.size].to_vec(), relocs: relocations(data, &sections, &symbols, row) }
        })
        .collect()
}
