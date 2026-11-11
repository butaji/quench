//! The 32-bit hole fields of a stencil: how they are compiled (sentinels), found and patched.
use crate::{REL32_LEN, Stencil};

/// Value holes a stencil row may name (destination, first and second operand).
pub const HOLES: usize = 3;
/// Compile passes with distinct sentinels.
const PASSES: usize = 2;
/// How far from a sentinel a folded constant may move a field.
const SCAN_RANGE: u32 = 255;

/// How a stencil's 32-bit holes are compiled: as 64-bit immediates (`Wide`), or as the sentinels
/// of one of two passes whose difference locates the 32-bit fields to patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Wide,
    Sentinel(usize),
}

/// The 32-bit sentinels of each hole in each pass: every byte differs between the passes, and
/// so do the differences between holes, so a field names its hole.
const SENTINELS: [[u32; HOLES]; PASSES] = [[0x1122_3344, 0x1a2b_3c4d, 0x2b3c_4d5e], [0x5566_7788, 0x5e6f_7081, 0x6f80_91a3]];

pub(crate) fn sentinel(mode: Mode, hole: u32) -> u32 {
    match mode {
        Mode::Sentinel(pass) => SENTINELS[pass][hole as usize],
        Mode::Wide => 0,
    }
}

/// A 32-bit field of a stencil's code: hole `index`, negated when `negate`, plus `addend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Imm32 {
    pub offset: u32,
    pub index: u32,
    pub negate: bool,
    pub addend: u32,
}

impl Imm32 {
    /// The field's value for a hole of `hole`.
    pub fn value(&self, hole: u32) -> u32 {
        if self.negate { self.addend.wrapping_sub(hole) } else { hole.wrapping_add(self.addend) }
    }

    /// The field at `offset` that reads `word` where hole `index` has `sentinel`, if `word` is the
    /// sentinel, or its negation, moved by at most `SCAN_RANGE`.
    fn of(offset: usize, index: usize, word: u32, sentinel: u32) -> Option<Self> {
        let near = |delta: u32| (delta as i32).unsigned_abs() <= SCAN_RANGE;
        let (offset, index) = (offset as u32, index as u32);
        if near(word.wrapping_sub(sentinel)) {
            Some(Self { offset, index, negate: false, addend: word.wrapping_sub(sentinel) })
        } else if near(word.wrapping_add(sentinel)) {
            Some(Self { offset, index, negate: true, addend: word.wrapping_add(sentinel) })
        } else {
            None
        }
    }
}

fn word_at(code: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(code[at..at + REL32_LEN].try_into().expect("a field lies inside the code"))
}

/// The 32-bit hole fields found by their sentinel in one pass. `None` when a hole the row used
/// (`used` is a bit set) appears nowhere: it was dead, or its constant was folded away.
pub(crate) fn scan(stencil: &Stencil, used: u8) -> Option<Vec<Imm32>> {
    let code = &stencil.code;
    let (mut fields, mut at) = (Vec::new(), 0);
    while at + REL32_LEN <= code.len() {
        let word = word_at(code, at);
        match (0..HOLES).find_map(|index| Imm32::of(at, index, word, SENTINELS[0][index])) {
            Some(field) => {
                fields.push(field);
                at += REL32_LEN;
            }
            None => at += 1,
        }
    }
    let found = fields.iter().fold(0u8, |bits, field| bits | 1 << field.index);
    (found & used == used).then_some(fields)
}

/// The fields that explain every difference between two sentinel passes, or `None` when
/// something else differs (a folded constant, another shape of code).
pub(crate) fn fields(first: &Stencil, second: &Stencil) -> Option<Vec<Imm32>> {
    let key = |r: &crate::Reloc| (r.offset, r.index, r.namespace);
    let same = first.code.len() == second.code.len() && first.relocs.len() == second.relocs.len();
    if !same || first.relocs.iter().zip(&second.relocs).any(|(a, b)| key(a) != key(b)) {
        return None;
    }
    let (mut fields, mut at) = (Vec::new(), 0);
    while at + REL32_LEN <= first.code.len() {
        if first.code[at] == second.code[at] {
            at += 1;
            continue;
        }
        let (one, two) = (word_at(&first.code, at), word_at(&second.code, at));
        let field = (0..HOLES).find_map(|index| {
            let (a, b) = (SENTINELS[0][index], SENTINELS[1][index]);
            let moved = two.wrapping_sub(one);
            if moved == b.wrapping_sub(a) || moved == a.wrapping_sub(b) { Imm32::of(at, index, one, a) } else { None }
        })?;
        fields.push(field);
        at += REL32_LEN;
    }
    let mut patched = first.code.clone();
    for field in &fields {
        let value = field.value(SENTINELS[1][field.index as usize]);
        patched[field.offset as usize..field.offset as usize + REL32_LEN].copy_from_slice(&value.to_le_bytes());
    }
    (patched == second.code).then_some(fields)
}
