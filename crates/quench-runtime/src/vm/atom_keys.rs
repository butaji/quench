use super::wtf16::JsString;
use super::wtf16::Units;
use super::*;
use std::hash::Hasher;

impl<H: Host> Vm<H> {
    pub(super) fn atom_hash_units(units: Units<'_>) -> u64 {
        match units {
            Units::Ascii(bytes) => atom_hash_ascii(bytes),
            Units::Wide(units) => atom_hash_wide(units.len(), units.iter().copied()),
        }
    }

    pub(super) fn atom_hash_str(text: &str) -> u64 {
        // ASCII text is its own UTF-16: one unit per byte, hashed without transcoding.
        if text.is_ascii() {
            return atom_hash_ascii(text.as_bytes());
        }
        atom_hash_wide(text.encode_utf16().count(), text.encode_utf16())
    }

    /// The name of `index`, an atom the VM appended after its program's (`atom_text`).
    pub(super) fn dynamic_atom(&self, index: usize) -> &JsString {
        self.atom_sequence.appended(index - self.atom_text.len())
    }

    pub(super) fn atom_units_equal_str(&self, atom: Atom, text: &str) -> bool {
        let index = atom as usize;
        if index < self.atom_text.len() {
            // Both are UTF-8 text, whose UTF-16 forms are equal exactly when they are.
            &self.atom_text[index] == text
        } else {
            match self.dynamic_atom(index).units() {
                Units::Ascii(bytes) => bytes == text.as_bytes(),
                units => units.iter().eq(text.encode_utf16()),
            }
        }
    }

    pub(super) fn find_atom(
        &self,
        hash: u64,
        mut matches: impl FnMut(Atom) -> bool,
    ) -> Option<Atom> {
        self.atoms.find(hash, |&atom| matches(atom)).copied()
    }

    /// The atom for a run of code units, interned without a string when it already exists.
    pub(super) fn intern_unit_run_atom(&mut self, units: Units<'_>) -> Atom {
        let hash = Self::atom_hash_units(units);
        if let Some(atom) = self.find_atom(hash, |atom| self.atom_units_equal(atom, units)) {
            return atom;
        }
        self.insert_dynamic_atom(JsString::from_unit_run(units), hash)
    }

    pub(super) fn lookup_js_atom(&self, name: &JsString) -> Option<Atom> {
        let hash = Self::atom_hash_units(name.units());
        self.find_atom(hash, |atom| self.atom_units_equal(atom, name.units()))
    }

    /// `lookup_js_atom` of the heap string `key`, memoized by the string's identity
    /// (`NameAtomMemo::string_atom`); `None` for any other value or a name with no atom.
    #[inline]
    pub(super) fn lookup_string_atom(&self, key: Value) -> Option<Atom> {
        let epoch = self.heap.collection_epoch();
        if let Some(atom) = self.name_atom_memo.string_atom(key, epoch) {
            return Some(atom);
        }
        let atom = self.lookup_js_atom(self.heap.string(key)?)?;
        self.name_atom_memo.record_string(key, epoch, atom);
        Some(atom)
    }

    /// The text of `atom` as it is held: a host string, or the units of an atom added later.
    pub(super) fn atom_text(&self, atom: Atom) -> AtomText<'_> {
        let index = atom as usize;
        if index < self.atom_text.len() {
            AtomText::Host(&self.atom_text[index])
        } else {
            AtomText::Js(self.dynamic_atom(index))
        }
    }

    pub(super) fn atom_value(&self, atom: Atom) -> JsString {
        let index = atom as usize;
        if index < self.atom_text.len() {
            JsString::from_str(&self.atom_text[index])
        } else {
            self.dynamic_atom(index).clone()
        }
    }

    pub(super) fn atom_units_equal(&self, atom: Atom, units: Units<'_>) -> bool {
        let index = atom as usize;
        if index < self.atom_text.len() {
            match units {
                Units::Ascii(bytes) => self.atom_text[index].as_bytes() == bytes,
                units => self.atom_text[index].encode_utf16().eq(units.iter()),
            }
        } else {
            self.dynamic_atom(index).units() == units
        }
    }
}

/// Programs compiled at run time extend the VM's atoms in place: their prefix ids are the
/// VM's, read through the shared sequence instead of copied.
impl<H: Host> crate::compile::AtomPrefix for Vm<H> {
    fn len(&self) -> usize {
        self.atom_sequence.len()
    }

    fn name(&self, atom: Atom) -> &str {
        self.atom_name(atom)
    }

    fn find(&self, name: &str) -> Option<Atom> {
        self.lookup_atom(name)
    }

    fn table(&self) -> crate::bytecode::AtomTable {
        crate::bytecode::AtomTable::shared(
            self.atom_sequence.clone(),
            crate::compile::AtomPrefix::len(self),
        )
    }

    fn specialized(&self) -> bool {
        self.specialized
    }
}

/// How an atom's text is held (`Vm::atom_text`).
pub(super) enum AtomText<'a> {
    Host(&'a str),
    Js(&'a JsString),
}

/// The code units an atom hash folds into each hasher word: one unit per 16-bit lane.
const HASH_UNITS_PER_WORD: usize = (u64::BITS / u16::BITS) as usize;

/// The hash of an atom spelled by `len` UTF-16 `units`: its length, then each whole group of
/// `HASH_UNITS_PER_WORD` units as one word (the first unit in the lowest lane), then each
/// remaining unit. Every spelling of the same units hashes alike (`atom_hash_ascii`).
fn atom_hash_wide(len: usize, units: impl Iterator<Item = u16>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_usize(len);
    let (mut word, mut filled) = (0u64, 0usize);
    for unit in units {
        word |= u64::from(unit) << (filled as u32 * u16::BITS);
        filled += 1;
        if filled == HASH_UNITS_PER_WORD {
            hasher.write_u64(word);
            (word, filled) = (0, 0);
        }
    }
    for lane in 0..filled {
        hasher.write_u16((word >> (lane as u32 * u16::BITS)) as u16);
    }
    hasher.finish()
}

/// `atom_hash_wide` of ASCII `bytes`, each byte one unit, widened a word at a time.
fn atom_hash_ascii(bytes: &[u8]) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_usize(bytes.len());
    let mut words = bytes.chunks_exact(HASH_UNITS_PER_WORD);
    for word in &mut words {
        hasher.write_u64(widen_ascii_word(u32::from_le_bytes(
            word.try_into().unwrap(),
        )));
    }
    for &byte in words.remainder() {
        hasher.write_u16(u16::from(byte));
    }
    hasher.finish()
}

/// Four ASCII bytes (the first lowest) as four UTF-16 units in 16-bit lanes: the high pair
/// moves to the upper 32 bits, then each byte of each pair to its own lane.
fn widen_ascii_word(bytes: u32) -> u64 {
    /// The low 16 bits of each 32-bit half.
    const PAIR_LANES: u64 = 0x0000_FFFF_0000_FFFF;
    /// The low byte of each 16-bit lane.
    const UNIT_LANES: u64 = 0x00FF_00FF_00FF_00FF;
    let pairs = (u64::from(bytes) | (u64::from(bytes) << u16::BITS)) & PAIR_LANES;
    (pairs | (pairs << u8::BITS)) & UNIT_LANES
}

#[cfg(test)]
mod tests {
    /// ASCII text hashes as its UTF-16 units do, at every length around a word boundary.
    #[test]
    fn ascii_and_wide_hashes_agree() {
        let text = "abcdefghijklmnopq";
        for len in 0..=text.len() {
            let ascii = &text.as_bytes()[..len];
            let wide: Vec<u16> = ascii.iter().map(|&byte| u16::from(byte)).collect();
            assert_eq!(
                super::atom_hash_ascii(ascii),
                super::atom_hash_wide(len, wide.into_iter())
            );
        }
    }
}
