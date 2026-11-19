//! A minimal ELF64 little-endian reader (x86-64 and AArch64 Linux): one function per section,
//! `SHT_RELA` relocations, mapped to hole classes.

use crate::bytes::{ObjectReloc, ObjectRow, c_string, is_row_name, u16_at, u32_at, u64_at};
use crate::class::{ABS32, ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, REL32};
use object::LittleEndian;
use object::elf::{
    FileHeader64, Rela64, SectionHeader64, Sym64, ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EM_X86_64, R_AARCH64_ABS64, R_AARCH64_CALL26, R_AARCH64_CONDBR19, R_AARCH64_JUMP26, R_AARCH64_MOVW_UABS_G0,
    R_AARCH64_MOVW_UABS_G0_NC, R_AARCH64_MOVW_UABS_G1, R_AARCH64_MOVW_UABS_G1_NC, R_AARCH64_MOVW_UABS_G2, R_AARCH64_MOVW_UABS_G2_NC, R_AARCH64_MOVW_UABS_G3, R_AARCH64_TSTBR14,
    R_X86_64_32, R_X86_64_32S, R_X86_64_64, R_X86_64_PC32, R_X86_64_PLT32, SHT_RELA, SHT_SYMTAB, STB_GLOBAL, STT_FUNC,
};
use std::mem::{offset_of, size_of};

/// The ELF64 structures this reader walks, in the spec's field order (`Elf64_Ehdr`, `Elf64_Shdr`,
/// `Elf64_Sym`, `Elf64_Rela`); their offsets are taken with `offset_of!`, their sizes with `size_of`.
/// The layouts are `object`'s, which the build already depends on for the format constants.
type Elf64Ehdr = FileHeader64<LittleEndian>;
type Elf64Shdr = SectionHeader64<LittleEndian>;
type Elf64Sym = Sym64<LittleEndian>;
type Elf64Rela = Rela64<LittleEndian>;

/// (relocation type, class) per machine.
const X86_64_CLASSES: [(u32, u8); 5] = [(R_X86_64_64, ABS64), (R_X86_64_PC32, REL32), (R_X86_64_PLT32, REL32), (R_X86_64_32, ABS32), (R_X86_64_32S, ABS32)];
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

const ELF_MAGIC: &[u8] = b"\x7fELF";
/// The class and data bytes of `e_ident` (ELF spec EI_CLASS, EI_DATA).
const CLASS_AT: usize = 4;
const DATA_AT: usize = 5;
const MACHINE_X86_64: usize = EM_X86_64 as usize;
const MACHINE_AARCH64: usize = EM_AARCH64 as usize;
/// `ELF64_ST_TYPE` and `ELF64_ST_BIND` (the symbol's `st_info` fields), `ELF64_R_SYM` and `ELF64_R_TYPE`
/// (the relocation's `r_info`).
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
    let (table, entry, count) = (
        u64_at(data, offset_of!(Elf64Ehdr, e_shoff)) as usize,
        u16_at(data, offset_of!(Elf64Ehdr, e_shentsize)),
        u16_at(data, offset_of!(Elf64Ehdr, e_shnum)),
    );
    (0..count)
        .map(|k| {
            let at = table + k * entry;
            Section {
                kind: u32_at(data, at + offset_of!(Elf64Shdr, sh_type)),
                offset: u64_at(data, at + offset_of!(Elf64Shdr, sh_offset)) as usize,
                size: u64_at(data, at + offset_of!(Elf64Shdr, sh_size)) as usize,
                link: u32_at(data, at + offset_of!(Elf64Shdr, sh_link)) as usize,
                info: u32_at(data, at + offset_of!(Elf64Shdr, sh_info)) as usize,
            }
        })
        .collect()
}

fn symbols(data: &[u8], sections: &[Section]) -> Vec<Symbol> {
    let symtab = sections.iter().find(|s| s.kind == SHT_SYMTAB).expect("a symbol table");
    let strings = &sections[symtab.link];
    (0..symtab.size / size_of::<Elf64Sym>())
        .map(|k| {
            let at = symtab.offset + k * size_of::<Elf64Sym>();
            Symbol {
                name: c_string(data, strings.offset + u32_at(data, at + offset_of!(Elf64Sym, st_name)) as usize),
                section: u16_at(data, at + offset_of!(Elf64Sym, st_shndx)),
                value: u64_at(data, at + offset_of!(Elf64Sym, st_value)) as usize,
                info: data[at + offset_of!(Elf64Sym, st_info)],
            }
        })
        .collect()
}

/// The hole class of relocation type `kind` on `machine`.
fn class_of(machine: usize, row: &str, kind: u32) -> u8 {
    let table: &[(u32, u8)] = match machine {
        MACHINE_X86_64 => &X86_64_CLASSES,
        MACHINE_AARCH64 => &AARCH64_CLASSES,
        other => panic!("ELF machine {other}"),
    };
    table.iter().find(|(k, _)| *k == kind).unwrap_or_else(|| panic!("stencil {row}: relocation kind {kind}")).1
}

fn relocations(data: &[u8], sections: &[Section], symbols: &[Symbol], row: &Symbol) -> Vec<ObjectReloc> {
    let machine = u16_at(data, offset_of!(Elf64Ehdr, e_machine));
    let mut relocs = Vec::new();
    for rela in sections.iter().filter(|s| s.kind == SHT_RELA && s.info == row.section) {
        for k in 0..rela.size / size_of::<Elf64Rela>() {
            let at = rela.offset + k * size_of::<Elf64Rela>();
            let info = u64_at(data, at + offset_of!(Elf64Rela, r_info));
            let kind = (info & RELOC_KIND_MASK) as u32;
            relocs.push(ObjectReloc {
                offset: u64_at(data, at + offset_of!(Elf64Rela, r_offset)) as usize,
                symbol: symbols[(info >> SYMBOL_INDEX_SHIFT) as usize].name.clone(),
                class: class_of(machine, &row.name, kind),
                addend: u64_at(data, at + offset_of!(Elf64Rela, r_addend)) as i64,
            });
        }
    }
    relocs
}

/// The global function symbols of the ELF64 object `data`, each with its code and relocations.
pub fn rows(data: &[u8]) -> Vec<ObjectRow> {
    assert!(data.starts_with(ELF_MAGIC) && data[CLASS_AT] == ELFCLASS64 && data[DATA_AT] == ELFDATA2LSB, "an ELF64 little-endian object");
    let sections = sections(data);
    let symbols = symbols(data, &sections);
    let is_row = |s: &&Symbol| s.info & SYMBOL_KIND_MASK == STT_FUNC && s.info >> SYMBOL_BIND_SHIFT == STB_GLOBAL && s.section != 0 && is_row_name(&s.name);
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
