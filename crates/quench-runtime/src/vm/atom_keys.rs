use super::wtf16::JsString;
use super::wtf16::{str_text_hash, table_hash, Units};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn atom_hash_units(units: Units<'_>) -> u64 {
        table_hash(units.text_hash())
    }

    pub(super) fn atom_hash_str(text: &str) -> u64 {
        table_hash(str_text_hash(text))
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

    pub(super) fn intern_js_atom(&mut self, name: &JsString) -> Atom {
        let units = name.units();
        let hash = Self::atom_hash_units(units);
        if let Some(atom) = self.find_atom(hash, |atom| self.atom_units_equal(atom, units)) {
            return atom;
        }
        self.insert_dynamic_atom(name.clone(), hash)
    }

    pub(super) fn lookup_js_atom(&self, name: &JsString) -> Option<Atom> {
        let units = name.units();
        self.find_atom(Self::atom_hash_units(units), |atom| self.atom_units_equal(atom, units))
    }

    /// `lookup_js_atom` of the heap string `key`, found once per string record and then
    /// read from it (`Heap::string_atom`); `None` for any other value or a name with no atom.
    #[inline]
    pub(super) fn lookup_string_atom(&self, key: Value) -> Option<Atom> {
        if let Some(atom) = self.heap.string_atom(key) {
            return Some(atom);
        }
        self.heap.is_string(key).then(|| self.find_string_atom(key))?
    }

    /// `lookup_string_atom` of a string with no atom recorded yet, found by its text hash.
    #[inline(never)]
    fn find_string_atom(&self, key: Value) -> Option<Atom> {
        let units = self.heap.string(key)?.units();
        let hash = table_hash(self.heap.string_hash(key)?);
        let atom = self.find_atom(hash, |atom| self.atom_units_equal(atom, units))?;
        self.heap.record_string_atom(key, atom);
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

#[cfg(test)]
mod tests {
    use crate::test_host::TestHost;
    use crate::Engine;
    use std::{cell::RefCell, rc::Rc};

    /// Equal texts held narrow and wide (an ASCII tail sliced from wide text) are one key to an
    /// indexed Map and Set, and to an object, whatever storage made them.
    #[test]
    fn equal_texts_in_any_storage_are_one_key() {
        let source = r#"
          const tail = "a".repeat(100), wide = ("é" + tail).slice(1);
          const m = new Map(), s = new Set(), o = {};
          for (let i = 0; i < 20; i++) { m.set("x" + i, i); s.add("x" + i); }
          m.set(tail, "found"); s.add(tail); o[tail] = "prop";
          print([wide === tail, m.get(wide), s.has(wide), o[wide], m.get("ü" + tail)].join());
        "#;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = super::Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&Engine::specialize(source, "string-keys.js").unwrap()).unwrap();
        assert_eq!(output.borrow().as_slice(), ["true,found,true,prop,"]);
    }

    #[test]
    fn similar_short_keys_find_their_atom_in_about_one_comparison() {
        // `d0`..`d255` differ only in late units; a weak hash shares their table control bits.
        let mut vm = super::Vm::new(crate::test_host::TestHost::default());
        let source = "var o = {}; for (var i = 0; i < 256; i++) o['d' + i] = i;";
        vm.execute(&crate::Engine::specialize(source, "keys.js").unwrap()).unwrap();
        let mut comparisons = 0;
        for index in 0..256 {
            let name = super::JsString::from_str(&format!("d{index}"));
            let hash = super::Vm::<crate::test_host::TestHost>::atom_hash_units(name.units());
            let found = vm.find_atom(hash, |atom| {
                comparisons += 1;
                vm.atom_units_equal(atom, name.units())
            });
            assert!(found.is_some(), "d{index} is interned");
        }
        assert!(comparisons < 256 * 5 / 4, "{comparisons} comparisons for 256 lookups");
    }
}
