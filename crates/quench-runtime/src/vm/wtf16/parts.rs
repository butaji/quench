//! A string concatenated from other strings and a Number, written out on first read.
//!
//! The record of such a string holds its parts and nothing else: no allocation to fill,
//! none to free, and nothing for a sweep to drop. `Heap::string` writes the text over the
//! record on first read (`Heap::flatten_string`), so every reader sees flat text; a string
//! nobody reads costs its record. The parts are strings of the same heap, named by record
//! id, and a Number is kept as its bits.
use super::*;

/// A part of a concatenation: a string of the heap, or a Number.
#[derive(Clone, Copy)]
pub(crate) enum Part {
    Text(u32),
    Number(f64),
}

/// The most parts a `Parts` string holds.
pub(crate) const MAX_PARTS: usize = 3;
/// The fewest parts: one part is the string itself.
const MIN_PARTS: usize = 2;
const NO_NUMBER: u8 = 3;
/// Bytes of a string part's record id and of a Number's bits.
const ID_BYTES: usize = 4;
const NUMBER_BYTES: usize = 8;
/// The count's mask, and the shift of the Number's position, in `Parts::shape`.
const COUNT_MASK: u8 = 0b11;
const NUMBER_SHIFT: u32 = 2;

/// Up to `MAX_PARTS` parts, at most one of them a Number. Kept as bytes, so its alignment is
/// one and it sits beside a flat string's tag in the same three words: a record does not grow.
#[derive(Clone, Copy)]
pub(crate) struct Parts {
    /// The part count in the low two bits, the Number's position in the next two (`NO_NUMBER`
    /// for none).
    shape: u8,
    /// The string parts' record ids, in order, little-endian.
    ids: [u8; ID_BYTES * MAX_PARTS],
    number: [u8; NUMBER_BYTES],
}

impl Parts {
    /// The parts `parts` names; `None` for fewer than two, more than `MAX_PARTS`, or a second
    /// Number.
    pub(crate) fn new(parts: &[Part]) -> Option<Self> {
        if parts.len() < MIN_PARTS || parts.len() > MAX_PARTS {
            return None;
        }
        let (mut ids, mut texts, mut number, mut at) = ([0; ID_BYTES * MAX_PARTS], 0, [0; NUMBER_BYTES], NO_NUMBER);
        for (position, part) in parts.iter().enumerate() {
            match *part {
                Part::Text(id) => {
                    ids[ID_BYTES * texts..ID_BYTES * (texts + 1)].copy_from_slice(&id.to_le_bytes());
                    texts += 1;
                }
                Part::Number(value) if at == NO_NUMBER => {
                    number = value.to_bits().to_le_bytes();
                    at = position as u8;
                }
                Part::Number(_) => return None,
            }
        }
        Some(Self { shape: parts.len() as u8 | at << NUMBER_SHIFT, ids, number })
    }

    /// The parts, in order.
    pub(crate) fn each(&self) -> impl Iterator<Item = Part> + '_ {
        let (count, at) = (usize::from(self.shape & COUNT_MASK), self.shape >> NUMBER_SHIFT);
        let mut texts = 0;
        (0..count).map(move |position| {
            if position == usize::from(at) {
                Part::Number(f64::from_bits(u64::from_le_bytes(self.number)))
            } else {
                texts += 1;
                Part::Text(u32::from_le_bytes(self.ids[ID_BYTES * (texts - 1)..ID_BYTES * texts].try_into().expect("four bytes")))
            }
        })
    }
}

impl JsString {
    /// A string that is these parts' text, not yet written out.
    pub(crate) fn from_parts(parts: Parts) -> Self {
        Self { repr: Repr::Parts(parts) }
    }

    /// The parts of a string not yet written out (`Heap::flatten_string`).
    #[inline(always)]
    pub(crate) fn parts(&self) -> Option<&Parts> {
        match &self.repr {
            Repr::Parts(parts) => Some(parts),
            Repr::Flat(_) | Repr::Shared(_) => None,
        }
    }
}

/// What a reader that skipped `Heap::string` finds: a string whose parts were never written out.
#[cold]
pub(super) fn not_written_out() -> ! {
    unreachable!("a string of parts is written out by `Heap::string` before any read")
}
