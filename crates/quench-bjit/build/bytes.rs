//! Little-endian field reads and the object-row types the format readers produce.

const U16_BYTES: usize = 2;
const U32_BYTES: usize = 4;
const U64_BYTES: usize = 8;

pub fn u16_at(data: &[u8], at: usize) -> usize {
    u16::from_le_bytes(data[at..at + U16_BYTES].try_into().unwrap()) as usize
}
pub fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + U32_BYTES].try_into().unwrap())
}
pub fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + U64_BYTES].try_into().unwrap())
}
pub fn c_string(data: &[u8], at: usize) -> String {
    let end = data[at..].iter().position(|b| *b == 0).expect("terminated string");
    String::from_utf8_lossy(&data[at..at + end]).into_owned()
}

/// A relocation of a row: the field's offset in the row's code, the symbol it names, its hole
/// class (`src/class.rs`, mapped by the format reader) and the addend.
pub struct ObjectReloc {
    pub offset: usize,
    pub symbol: String,
    pub class: u8,
    pub addend: i64,
}

/// A global function of the object: one stencil row.
pub struct ObjectRow {
    pub name: String,
    pub code: Vec<u8>,
    pub relocs: Vec<ObjectReloc>,
}

/// Row functions are the global functions that are not Rust-mangled helpers.
pub fn is_row_name(name: &str) -> bool {
    !name.starts_with("_R")
}
