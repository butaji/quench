use super::wtf16::JsString;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn atom_hash_units(units: &[u16]) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        units.hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn lookup_js_atom(&self, name: &JsString) -> Option<Atom> {
        let hash = Self::atom_hash_units(name.units());
        let primary = self.atoms.get(&hash).copied()?;
        self.atom_units_equal(primary, name.units())
            .then_some(primary)
            .or_else(|| {
                self.atom_collisions.get(&hash).and_then(|atoms| {
                    atoms
                        .iter()
                        .copied()
                        .find(|atom| self.atom_units_equal(*atom, name.units()))
                })
            })
    }

    pub(super) fn atom_value(&self, atom: Atom) -> JsString {
        let index = atom as usize;
        if index < self.atom_text.len() {
            JsString::from_str(&self.atom_text[index])
        } else {
            self.dynamic_atoms[index - self.atom_text.len()].clone()
        }
    }

    fn atom_units_equal(&self, atom: Atom, units: &[u16]) -> bool {
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
