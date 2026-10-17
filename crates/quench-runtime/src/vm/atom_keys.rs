use super::wtf16::JsString;
use super::wtf16::Units;
use super::*;
use std::hash::Hasher;

impl<H: Host> Vm<H> {
    pub(super) fn atom_hash_units(units: Units<'_>) -> u64 {
        Self::atom_hash_utf16(units.len(), units.iter())
    }

    pub(super) fn atom_hash_str(text: &str) -> u64 {
        Self::atom_hash_utf16(text.encode_utf16().count(), text.encode_utf16())
    }

    fn atom_hash_utf16(units_len: usize, units: impl Iterator<Item = u16>) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        hasher.write_usize(units_len);
        for unit in units {
            hasher.write_u16(unit);
        }
        hasher.finish()
    }

    pub(super) fn atom_units_equal_str(&self, atom: Atom, text: &str) -> bool {
        let index = atom as usize;
        if index < self.atom_text.len() {
            self.atom_text[index].encode_utf16().eq(text.encode_utf16())
        } else {
            match self.dynamic_atoms[index - self.atom_text.len()].units() {
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

    pub(super) fn atom_value(&self, atom: Atom) -> JsString {
        let index = atom as usize;
        if index < self.atom_text.len() {
            JsString::from_str(&self.atom_text[index])
        } else {
            self.dynamic_atoms[index - self.atom_text.len()].clone()
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
            self.dynamic_atoms[index - self.atom_text.len()].units() == units
        }
    }
}

/// Programs compiled at run time extend the VM's atoms in place: their prefix ids are the
/// VM's, read through the shared sequence instead of copied.
impl<H: Host> crate::compile::AtomPrefix for Vm<H> {
    fn len(&self) -> usize {
        self.atom_text.len() + self.dynamic_atoms.len()
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
