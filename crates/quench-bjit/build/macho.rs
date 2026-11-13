//! A minimal Mach-O 64-bit arm64 reader (Apple Silicon): the functions of the `__text` section,
//! each ending where the next symbol starts, and its ARM64 relocations mapped to hole classes.
//! Mach-O has no `MOVW` relocation, so a 64-bit value hole is a literal word
//! (`ARM64_RELOC_UNSIGNED`, 8 bytes) and a successor is `ARM64_RELOC_BRANCH26`; a conditional
//! branch to a hole has no relocation, so LLVM branches over an unconditional one.

use crate::bytes::{ObjectReloc, ObjectRow, c_string, is_row_name, u32_at, u64_at};
use crate::class::{ABS64, BRANCH26};

const MACH_MAGIC_64: u32 = 0xfeed_facf;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const CPU_TYPE_AT: usize = 4;
const HEADER_SIZE: usize = 32;
const COMMAND_COUNT_AT: usize = 16;
const COMMAND_SIZE_AT: usize = 4;
const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x2;
const SEGMENT_SECTION_COUNT_AT: usize = 64;
const SEGMENT_HEADER_SIZE: usize = 72;
const SECTION_SIZE: usize = 80;
const SECTION_NAME_BYTES: usize = 16;
const SECTION_ADDR: usize = 32;
const SECTION_BYTES: usize = 40;
const SECTION_OFFSET: usize = 48;
const SECTION_RELOC_OFFSET: usize = 56;
const SECTION_RELOC_COUNT: usize = 60;
const TEXT_SECTION: &str = "__text";
const SYMTAB_SYMBOL_OFFSET: usize = 8;
const SYMTAB_SYMBOL_COUNT: usize = 12;
const SYMTAB_STRING_OFFSET: usize = 16;
const NLIST_SIZE: usize = 16;
const NLIST_TYPE: usize = 4;
const NLIST_SECTION: usize = 5;
const NLIST_VALUE: usize = 8;
const N_STAB: u8 = 0xe0;
const N_TYPE: u8 = 0x0e;
const N_SECT: u8 = 0x0e;
const N_EXT: u8 = 0x01;
const RELOC_SIZE: usize = 8;
const RELOC_WORD: usize = 4;
const RELOC_SYMBOL_MASK: u32 = 0x00ff_ffff;
const RELOC_PCREL_SHIFT: u32 = 24;
const RELOC_LENGTH_SHIFT: u32 = 25;
const RELOC_LENGTH_MASK: u32 = 0x3;
const RELOC_EXTERN_SHIFT: u32 = 27;
const RELOC_TYPE_SHIFT: u32 = 28;
const ARM64_RELOC_UNSIGNED: u32 = 0;
const ARM64_RELOC_BRANCH26: u32 = 2;
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
                if c_string(&data[section..section + SECTION_NAME_BYTES], 0) == TEXT_SECTION {
                    let field = |f: usize| u32_at(data, section + f) as usize;
                    text = Some(Text { index: section_index, addr: u64_at(data, section + SECTION_ADDR) as usize, size: u64_at(data, section + SECTION_BYTES) as usize, offset: field(SECTION_OFFSET), relocs: field(SECTION_RELOC_OFFSET), reloc_count: field(SECTION_RELOC_COUNT) });
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
                let class = match (kind, pc_relative, length) {
                    (ARM64_RELOC_BRANCH26, true, _) => BRANCH26,
                    (ARM64_RELOC_UNSIGNED, false, LENGTH_8_BYTES) => ABS64,
                    _ => panic!("stencil {row}: ARM64 relocation type {kind}, pc-relative {pc_relative}, length {length}"),
                };
                let symbol = names[(word & RELOC_SYMBOL_MASK) as usize].0.name.clone();
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
