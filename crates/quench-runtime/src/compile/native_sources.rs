//! Compile source units into one stable atom namespace before host initialization.

use super::{AtomPrefix, Diagnostic, HostScriptOptions, ReleasedRecords};
use crate::bytecode::{Atom, AtomSequence, AtomTable, SourceText};
use crate::{Engine, ResidualProgram};
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// A compilation's append-only namespace. All emitted images use these IDs;
/// `finish` seeds the entry with the full namespace before the host can add names.
pub struct NativeJsCompiler {
    atoms: Rc<AtomSequence>,
    index: FxHashMap<Rc<str>, Atom>,
    specialized: bool,
}

impl NativeJsCompiler {
    pub fn new(specialized: bool) -> Self {
        Self {
            atoms: Rc::new(AtomSequence::new(AtomTable::default())),
            index: FxHashMap::default(),
            specialized,
        }
    }

    pub fn compile_script(
        &mut self, source: &str, name: &str, options: HostScriptOptions,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = Engine::specialize_host_script(
            source, name, self, options, ReleasedRecords::Dropped,
        )?;
        if let (Some(source), Some((line, column))) = (&mut program.source, options.origin) {
            source.line_offset = line;
            source.column_offset = column;
        }
        self.adopt(&mut program);
        Ok(program)
    }

    /// Compile context-free host bootstrap source, matching `eval_host_script`.
    pub fn compile_host_script(
        &mut self, source: &str, name: &str,
    ) -> Result<ResidualProgram, Vec<Diagnostic>> {
        let mut program = Engine::specialize_eval_with_context(
            source, name, self, false, Default::default(), &[], &[],
        )?;
        self.adopt(&mut program);
        Ok(program)
    }

    /// Compile the same retained wrapper/body shape used by host libraries.
    pub fn compile_library_body(
        &mut self, prefix: &str, body: std::sync::Arc<str>, suffix: &str, name: &str,
    ) -> Result<ResidualProgram, String> {
        let origin = crate::api::wrapper_origin(prefix)
            .ok_or("host wrapper origin exceeds compiler positions")?;
        let base = u32::try_from(prefix.len())
            .map_err(|_| "host wrapper prefix exceeds compiler positions")?;
        let wrapped = [prefix, &body, suffix].concat();
        let mut program = self.compile_script(&wrapped, name, HostScriptOptions {
            origin: Some(origin), keep_records: true, typescript: false,
        }).map_err(|diagnostics| format!("{diagnostics:?}"))?;
        if let Some(source) = &mut program.source {
            source.text = SourceText::Shared(body);
            source.base = base;
        }
        Ok(program)
    }

    fn adopt(&mut self, program: &mut ResidualProgram) {
        program.compile_released();
        for id in self.atoms.len()..program.atoms.len() {
            let name: Rc<str> = Rc::from(&program.atoms[id]);
            self.atoms.append(crate::vm::wtf16::JsString::from_str(&name));
            self.index.insert(name, id as Atom);
        }
        program.atoms = AtomTable::shared(self.atoms.clone(), self.atoms.len());
    }

    /// Seal the namespace. The entry must have been compiled by this compiler.
    /// Emit its native image after this step; all other images retain their IDs.
    pub fn finish(self, mut entry: ResidualProgram) -> Result<ResidualProgram, String> {
        if !entry.atoms.belongs_to(&self.atoms) {
            return Err("native entry belongs to a different compiler namespace".into());
        }
        let len = self.atoms.len();
        entry.atoms = AtomTable::shared(self.atoms, len);
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_units_share_ids_and_entry_seeds_names_from_later_units() {
        let mut compiler = NativeJsCompiler::new(true);
        let options = HostScriptOptions::default();
        let first = compiler.compile_script("globalThis.first = 1;", "first.js", options).unwrap();
        let second = compiler.compile_script("globalThis.second = globalThis.first;", "second.js", options).unwrap();
        assert!(first.atoms.len() < second.atoms.len());
        assert!(first.atoms.iter().zip(second.atoms.iter()).all(|(a, b)| a == b));
        let entry = compiler.finish(first).unwrap();
        assert_eq!(entry.atoms.iter().collect::<Vec<_>>(), second.atoms.iter().collect::<Vec<_>>());
        entry.validate().unwrap();
        second.validate().unwrap();
    }

    #[test]
    fn sealing_rejects_an_independently_compiled_entry() {
        let entry = Engine::specialize("", "foreign.js").unwrap();
        assert!(NativeJsCompiler::new(true).finish(entry).is_err());
    }
}

impl AtomPrefix for NativeJsCompiler {
    fn len(&self) -> usize { self.atoms.len() }
    fn name(&self, atom: Atom) -> &str { self.atoms.name(atom as usize) }
    fn find(&self, name: &str) -> Option<Atom> { self.index.get(name).copied() }
    fn table(&self) -> AtomTable { AtomTable::shared(self.atoms.clone(), self.atoms.len()) }
    fn specialized(&self) -> bool { self.specialized }
}
