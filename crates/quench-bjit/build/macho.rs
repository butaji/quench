//! A minimal Mach-O 64-bit arm64 reader (Apple Silicon): the functions of the `__text` section,
//! each ending where the next symbol starts, and its ARM64 relocations mapped to hole classes.
//! Mach-O has no `MOVW` relocation, so a 64-bit value hole is a literal word
//! (`ARM64_RELOC_UNSIGNED`, 8 bytes) and a successor is `ARM64_RELOC_BRANCH26`; a conditional
//! branch to a hole has no relocation, so LLVM branches over an unconditional one.

use crate::bytes::{ObjectReloc, ObjectRow, c_string, fixed_name, is_row_name, u32_at, u64_at};
use crate::class::{ABS64, BRANCH26};
use object::LittleEndian;
use std::mem::{offset_of, size_of};

/// The Mach-O structures this reader walks, in the spec's field order (`mach_header_64`,
/// `load_command`, `segment_command_64`, `section_64`, `symtab_command`, `nlist_64`,
/// `relocation_info`); their offsets are taken with `offset_of!`, their sizes with `size_of`.
/// The layouts are `object`'s.
type MachHeader64 = object::macho::MachHeader64<LittleEndian>;
type LoadCommand = object::macho::LoadCommand<LittleEndian>;
type SegmentCommand64 = object::macho::SegmentCommand64<LittleEndian>;
type Section64 = object::macho::Section64<LittleEndian>;
type SymtabCommand = object::macho::SymtabCommand<LittleEndian>;
type Nlist64 = object::macho::Nlist64<LittleEndian>;
type RelocationInfo = object::macho::Relocation<LittleEndian>;

const MACH_MAGIC_64: u32 = object::macho::MH_MAGIC_64 as u32;
const CPU_TYPE_ARM64: u32 = object::macho::CPU_TYPE_ARM64 as u32;
const CPU_TYPE_AT: usize = offset_of!(MachHeader64, cputype);
const HEADER_SIZE: usize = size_of::<MachHeader64>();
const COMMAND_COUNT_AT: usize = offset_of!(MachHeader64, ncmds);
const COMMAND_SIZE_AT: usize = offset_of!(LoadCommand, cmdsize);
const LC_SEGMENT_64: u32 = object::macho::LC_SEGMENT_64 as u32;
const LC_SYMTAB: u32 = object::macho::LC_SYMTAB as u32;
const SEGMENT_SECTION_COUNT_AT: usize = offset_of!(SegmentCommand64, nsects);
const SEGMENT_HEADER_SIZE: usize = size_of::<SegmentCommand64>();
const SECTION_SIZE: usize = size_of::<Section64>();
/// The section name is the first field and fills its 16 bytes up to `segname`.
const SECTION_NAME_BYTES: usize = offset_of!(Section64, segname);
const TEXT_SECTION: &str = "__text";
const SYMTAB_SYMBOL_OFFSET: usize = offset_of!(SymtabCommand, symoff);
const SYMTAB_SYMBOL_COUNT: usize = offset_of!(SymtabCommand, nsyms);
const SYMTAB_STRING_OFFSET: usize = offset_of!(SymtabCommand, stroff);
const NLIST_SIZE: usize = size_of::<Nlist64>();
const NLIST_TYPE: usize = offset_of!(Nlist64, n_type);
const NLIST_SECTION: usize = offset_of!(Nlist64, n_sect);
const NLIST_VALUE: usize = offset_of!(Nlist64, n_value);
const N_STAB: u8 = object::macho::N_STAB as u8;
const N_TYPE: u8 = object::macho::N_TYPE as u8;
const N_SECT: u8 = object::macho::N_SECT as u8;
const N_EXT: u8 = object::macho::N_EXT as u8;
/// `relocation_info`'s `r_info` is a bitfield: symbol number, pc-relative, length, extern, type.
const RELOC_SIZE: usize = size_of::<RelocationInfo>();
const RELOC_WORD: usize = offset_of!(RelocationInfo, r_word1);
const RELOC_SYMBOL_MASK: u32 = 0x00ff_ffff;
const RELOC_PCREL_SHIFT: u32 = 24;
const RELOC_LENGTH_SHIFT: u32 = 25;
const RELOC_LENGTH_MASK: u32 = 0x3;
const RELOC_EXTERN_SHIFT: u32 = 27;
const RELOC_TYPE_SHIFT: u32 = 28;
const ARM64_RELOC_UNSIGNED: u32 = object::macho::ARM64_RELOC_UNSIGNED as u32;
const ARM64_RELOC_BRANCH26: u32 = object::macho::ARM64_RELOC_BRANCH26 as u32;
/// `length` of an 8-byte field (log2 of its bytes).
const LENGTH_8_BYTES: u32 = 3;
const LEADING_UNDERSCORE: &str = "_";

struct Text {
    index: usize,
    addr: usize,
    size: usize,
    offset: usize,
    relocs: usize,
    reloc_count: usize,
}

struct Symbol {
    name: String,
    kind: u8,
    value: usize,
}

/// The `__text` section and the symbol table's offsets, found by walking the load commands.
fn commands(data: &[u8]) -> (Text, [usize; 3]) {
    let (mut at, mut text, mut symtab) = (HEADER_SIZE, None, None);
    let mut section_index = 0;
    for _ in 0..u32_at(data, COMMAND_COUNT_AT) {
        let (cmd, size) = (u32_at(data, at), u32_at(data, at + COMMAND_SIZE_AT) as usize);
        if cmd == LC_SYMTAB {
            let word = |field: usize| u32_at(data, at + field) as usize;
            symtab = Some([word(SYMTAB_SYMBOL_OFFSET), word(SYMTAB_SYMBOL_COUNT), word(SYMTAB_STRING_OFFSET)]);
        }
        if cmd == LC_SEGMENT_64 {
            for k in 0..u32_at(data, at + SEGMENT_SECTION_COUNT_AT) as usize {
                let section = at + SEGMENT_HEADER_SIZE + k * SECTION_SIZE;
                section_index += 1;
                if fixed_name(&data[section..section + SECTION_NAME_BYTES]) == TEXT_SECTION {
                    let field = |f: usize| u32_at(data, section + f) as usize;
                    text = Some(Text {
                        index: section_index,
                        addr: u64_at(data, section + offset_of!(Section64, addr)) as usize,
                        size: u64_at(data, section + offset_of!(Section64, size)) as usize,
                        offset: field(offset_of!(Section64, offset)),
                        relocs: field(offset_of!(Section64, reloff)),
                        reloc_count: field(offset_of!(Section64, nreloc)),
                    });
                }
            }
        }
        at += size;
    }
    (text.expect("a __text section"), symtab.expect("a symbol table"))
}

fn symbols(data: &[u8], [offset, count, strings]: [usize; 3]) -> Vec<(Symbol, usize)> {
    (0..count)
        .map(|k| {
            let at = offset + k * NLIST_SIZE;
            let name = c_string(data, strings + u32_at(data, at) as usize);
            let symbol = Symbol { name: name.strip_prefix(LEADING_UNDERSCORE).unwrap_or(&name).to_string(), kind: data[at + NLIST_TYPE], value: u64_at(data, at + NLIST_VALUE) as usize };
            (symbol, data[at + NLIST_SECTION] as usize)
        })
        .collect()
}

/// The relocations of `text` inside `[start, end)` as the row's, offsets relative to `start`.
fn relocations(data: &[u8], text: &Text, names: &[(Symbol, usize)], row: &str, (start, end): (usize, usize)) -> Vec<ObjectReloc> {
    (0..text.reloc_count)
        .filter_map(|k| {
            let at = text.relocs + k * RELOC_SIZE;
            let address = u32_at(data, at) as usize;
            let word = u32_at(data, at + RELOC_WORD);
            (start..end).contains(&address).then(|| {
                let (kind, length) = (word >> RELOC_TYPE_SHIFT, word >> RELOC_LENGTH_SHIFT & RELOC_LENGTH_MASK);
                assert!(word >> RELOC_EXTERN_SHIFT & 1 == 1, "stencil {row}: a relocation against a section");
                let pc_relative = word >> RELOC_PCREL_SHIFT & 1 == 1;
                let symbol = names[(word & RELOC_SYMBOL_MASK) as usize].0.name.clone();
                let class = match (kind, pc_relative, length) {
                    (ARM64_RELOC_BRANCH26, true, _) => BRANCH26,
                    (ARM64_RELOC_UNSIGNED, false, LENGTH_8_BYTES) => ABS64,
                    _ => panic!("stencil {row}: ARM64 relocation type {kind}, pc-relative {pc_relative}, length {length}, symbol {symbol}"),
                };
                ObjectReloc { offset: address - start, symbol, class, addend: 0 }
            })
        })
        .collect()
}

/// A symbol defined in `__text` (not a debugger entry).
fn in_text(text: &Text, (symbol, section): &(Symbol, usize)) -> bool {
    *section == text.index && symbol.kind & N_STAB == 0 && symbol.kind & N_TYPE == N_SECT
}

/// The global functions of the Mach-O arm64 object `data`: the `__text` symbols, each running to
/// the next symbol.
pub fn rows(data: &[u8]) -> Vec<ObjectRow> {
    assert!(u32_at(data, 0) == MACH_MAGIC_64 && u32_at(data, CPU_TYPE_AT) == CPU_TYPE_ARM64, "a little-endian Mach-O arm64 object");
    let (text, symtab) = commands(data);
    let names = symbols(data, symtab);
    let mut starts: Vec<usize> = names.iter().filter(|n| in_text(&text, n)).map(|(s, _)| s.value - text.addr).collect();
    starts.sort_unstable();
    names
        .iter()
        .filter(|n| in_text(&text, n) && n.0.kind & N_EXT != 0 && is_row_name(&n.0.name))
        .map(|(row, _)| {
            let start = row.value - text.addr;
            let end = starts.iter().copied().find(|s| *s > start).unwrap_or(text.size);
            ObjectRow { name: row.name.clone(), code: data[text.offset + start..text.offset + end].to_vec(), relocs: relocations(data, &text, &names, &row.name, (start, end)) }
        })
        .collect()
}
