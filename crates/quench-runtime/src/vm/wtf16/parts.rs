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
    /// The string parts' record ids by part position, little-endian; the Number's slot is unused.
    ids: [u8; ID_BYTES * MAX_PARTS],
    number: [u8; NUMBER_BYTES],
}

impl Parts {
    /// No parts yet, nor a Number: the start of a `push_text` / `push_number` sequence.
    pub(crate) const EMPTY: Self = Self { shape: NO_NUMBER << NUMBER_SHIFT, ids: [0; ID_BYTES * MAX_PARTS], number: [0; NUMBER_BYTES] };

    #[inline(always)]
    fn count(&self) -> usize {
        usize::from(self.shape & COUNT_MASK)
    }

    #[inline(always)]
    fn has_number(&self) -> bool {
        self.shape >> NUMBER_SHIFT != NO_NUMBER
    }

    /// Appends the string part `id`; `false` past `MAX_PARTS`.
    #[inline(always)]
    pub(crate) fn push_text(&mut self, id: u32) -> bool {
        let count = self.count();
        if count == MAX_PARTS {
            return false;
        }
        let at = ID_BYTES * count;
        self.ids[at..at + ID_BYTES].copy_from_slice(&id.to_le_bytes());
        self.shape += 1;
        true
    }

    /// Appends the Number `value`; `false` past `MAX_PARTS` or for a second Number.
    #[inline(always)]
    pub(crate) fn push_number(&mut self, value: f64) -> bool {
        let count = self.count();
        if count == MAX_PARTS || self.has_number() {
            return false;
        }
        self.number = value.to_bits().to_le_bytes();
        self.shape = (count as u8 + 1) | (count as u8) << NUMBER_SHIFT;
        true
    }

    /// Whether the parts pushed make a string of parts: at least `MIN_PARTS`.
    #[inline(always)]
    pub(crate) fn is_complete(&self) -> bool {
        self.count() >= MIN_PARTS
    }

    /// The string parts' record ids, in order: what a collector traces.
    #[inline(always)]
    pub(crate) fn for_each_text(&self, mut visit: impl FnMut(u32)) {
        // A plain loop: the iterator's fold stayed out of line, a call per string marked.
        let (count, at) = (usize::from(self.shape & COUNT_MASK), usize::from(self.shape >> NUMBER_SHIFT));
        for position in 0..count {
            if position != at {
                let mut id = [0; ID_BYTES];
                id.copy_from_slice(&self.ids[ID_BYTES * position..ID_BYTES * (position + 1)]);
                visit(u32::from_le_bytes(id));
            }
        }
    }

    /// The parts, in order.
    #[inline(always)]
    pub(crate) fn each(&self) -> impl Iterator<Item = Part> + '_ {
        let (count, at) = (usize::from(self.shape & COUNT_MASK), self.shape >> NUMBER_SHIFT);
        (0..count).map(move |position| {
            if position == usize::from(at) {
                Part::Number(f64::from_bits(u64::from_le_bytes(self.number)))
            } else {
                Part::Text(u32::from_le_bytes(self.ids[ID_BYTES * position..ID_BYTES * (position + 1)].try_into().expect("four bytes")))
            }
        })
    }
}

impl JsString {
    /// A string that is these parts' text, not yet written out.
    #[inline] pub(crate) fn from_parts(parts: Parts) -> Self {
        Self { repr: Repr::Flat(FlatUnits::Parts(parts)) }
    }

    /// The parts of a string not yet written out (`Heap::flatten_string`).
    #[inline(always)]
    pub(crate) fn parts(&self) -> Option<&Parts> {
        match &self.repr {
            Repr::Flat(FlatUnits::Parts(parts)) => Some(parts),
            Repr::Flat(_) | Repr::Shared(_) => None,
        }
    }
}

/// What a reader that skipped `Heap::string` finds: a string whose parts were never written out.
#[cold]
pub(super) fn not_written_out() -> ! {
    unreachable!("a string of parts is written out by `Heap::string` before any read")
}
