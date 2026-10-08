use super::wtf16::JsString;
use super::*;
use std::hash::Hasher;

impl<H: Host> Vm<H> {
    pub(super) fn atom_hash_units(units: &[u16]) -> u64 {
        Self::atom_hash_utf16(units.len(), units.iter().copied())
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
            self.dynamic_atoms[index - self.atom_text.len()]
                .units()
                .iter()
                .copied()
                .eq(text.encode_utf16())
        }
    }

    pub(super) fn find_atom(
        &self,
        hash: u64,
        mut matches: impl FnMut(Atom) -> bool,
    ) -> Option<Atom> {
        let primary = self.atoms.get(&hash).copied()?;
        if matches(primary) {
            return Some(primary);
        }
        self.atom_collisions
            .get(&hash)?
            .iter()
            .copied()
            .find(|atom| matches(*atom))
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

    pub(super) fn atom_units_equal(&self, atom: Atom, units: &[u16]) -> bool {
        let index = atom as usize;
        if index < self.atom_text.len() {
            self.atom_text[index]
                .encode_utf16()
                .eq(units.iter().copied())
        } else {
            self.dynamic_atoms[index - self.atom_text.len()].units() == units
        }
    }
}
