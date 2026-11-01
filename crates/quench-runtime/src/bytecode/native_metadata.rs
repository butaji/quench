use super::{AtomSequence, AtomTable, KeptBytes, KeptPrefix, ResidualProgram, SourceText, binary};
use std::{hash::{Hash, Hasher}, rc::Rc};

/// One compiler namespace, shared by every metadata record in a linked image.
/// The fingerprint detects mismatched image inputs; native code binding still
/// requires the caller to uphold the linked-image safety contract.
pub struct NativeProgramNamespace {
    atoms: Rc<AtomSequence>,
    fingerprint: u64,
}

impl NativeProgramNamespace {
    pub fn from_entry(entry: &ResidualProgram) -> Result<Self, String> {
        entry.validate_unless_proven()?;
        let mut hash = rustc_hash::FxHasher::default();
        for name in entry.atoms.iter() {
            hash.write(&(name.len() as u64).to_le_bytes());
            hash.write(name.as_bytes());
        }
        Ok(Self {
            atoms: Rc::new(AtomSequence::new(entry.atoms.clone())),
            fingerprint: hash.finish(),
        })
    }

    #[cfg(feature = "aot")]
    pub fn encode(&self, program: &ResidualProgram) -> Result<Vec<u8>, String> {
        self.prepare_encoding(program)?;
        let mut bytes = binary::encode_native_program(program);
        bytes.extend_from_slice(&self.fingerprint.to_le_bytes());
        Ok(bytes)
    }

    /// Native metadata whose original source lives in the image's shared asset pool.
    #[cfg(feature = "aot")]
    pub fn encode_with_external_source(&self, program: &ResidualProgram) -> Result<Vec<u8>, String> {
        self.prepare_encoding(program)?;
        let mut bytes = binary::encode_snapshot_program(program);
        bytes.extend_from_slice(&source_fingerprint(program.source_text()).to_le_bytes());
        bytes.extend_from_slice(&self.fingerprint.to_le_bytes());
        Ok(bytes)
    }

    #[cfg(feature = "aot")]
    fn prepare_encoding(&self, program: &ResidualProgram) -> Result<(), String> {
        program.validate_unless_proven()?;
        if program.atoms.len() > self.atoms.len()
            || program
                .atoms
                .iter()
                .enumerate()
                .any(|(id, name)| name != self.atoms.name(id))
        {
            return Err("native metadata belongs to a different atom namespace".into());
        }
        program.freeze_native_diagnostics();
        Ok(())
    }

    pub fn decode(&self, bytes: KeptBytes) -> Result<ResidualProgram, String> {
        self.decode_program(self.encoding(bytes)?, None)
    }

    /// Bind exact compiler text without copying it onto the process heap.
    pub fn decode_with_static_source(&self, bytes: KeptBytes, source: Option<&'static str>) -> Result<ResidualProgram, String> {
        let encoding = self.encoding(bytes)?;
        let data = encoding.as_ref().as_ref();
        let length = data.len().checked_sub(size_of::<u64>())
            .ok_or("truncated native source fingerprint")?;
        let expected = u64::from_le_bytes(data[length..].try_into().unwrap());
        if expected != source_fingerprint(source) {
            return Err("native metadata source mismatch".into());
        }
        self.decode_program(Rc::new(KeptPrefix { bytes: encoding, len: length }), source.map(SourceText::Static))
    }

    fn encoding(&self, bytes: KeptBytes) -> Result<KeptBytes, String> {
        let data = bytes.as_ref().as_ref();
        let encoding_len = data
            .len()
            .checked_sub(size_of::<u64>())
            .ok_or("truncated native metadata namespace fingerprint")?;
        let fingerprint = u64::from_le_bytes(data[encoding_len..].try_into().unwrap());
        if fingerprint != self.fingerprint {
            return Err("native metadata atom namespace mismatch".into());
        }
        Ok(Rc::new(KeptPrefix {
            bytes,
            len: encoding_len,
        }))
    }

    fn decode_program(&self, encoding: KeptBytes, source: Option<SourceText>) -> Result<ResidualProgram, String> {
        let (mut program, atom_count) = binary::decode_snapshot_program(encoding, source)?;
        if atom_count > self.atoms.len() {
            return Err("native metadata names atoms its namespace lacks".into());
        }
        program.atoms = AtomTable::shared(self.atoms.clone(), atom_count);
        if program.source.as_ref().is_some_and(|source| source.call_sites.get().is_none()) {
            return Err("native metadata has no frozen call-site descriptions".into());
        }
        program.validate()?;
        Ok(program)
    }
}

fn source_fingerprint(source: Option<&str>) -> u64 {
    let mut hash = rustc_hash::FxHasher::default();
    source.hash(&mut hash);
    hash.finish()
}

#[cfg(all(test, feature = "aot"))]
mod tests {
    use super::*;

    #[test]
    fn external_native_source_is_exact_static_and_sealed() {
        const SOURCE: &str = "function λ(value) { return value + 'λ'; }\nλ('source pool');";
        const OTHER: &str = "function λ(value) { return value + 'μ'; }\nλ('source pool');";
        let program = crate::Engine::specialize(SOURCE, "pooled.js").unwrap();
        let namespace = NativeProgramNamespace::from_entry(&program).unwrap();
        let included = namespace.encode(&program).unwrap();
        let external = namespace.encode_with_external_source(&program).unwrap();
        assert!(external.len() < included.len());
        assert!(!external.windows(SOURCE.len()).any(|bytes| bytes == SOURCE.as_bytes()));
        let decoded = namespace.decode_with_static_source(Rc::new(external.clone()), Some(SOURCE)).unwrap();
        assert_eq!(decoded.source_text(), Some(SOURCE));
        assert_eq!(decoded.source_text().unwrap().as_ptr(), SOURCE.as_ptr());
        assert_eq!(decoded.encode_trusted(), program.encode_trusted());
        assert!(namespace.decode(Rc::new(external.clone())).is_err());
        assert!(namespace.decode_with_static_source(Rc::new(external.clone()), None).is_err());
        assert!(namespace.decode_with_static_source(Rc::new(external), Some(OTHER)).is_err());
    }

    #[test]
    fn shared_metadata_preserves_prefix_ids_source_and_deferred_functions() {
        let mut compiler = crate::NativeJsCompiler::new(true);
        let first = compiler
            .compile_script(
                "function earlier() { return globalThis.earlier; }",
                "earlier.js",
                Default::default(),
            )
            .unwrap();
        let later = compiler
            .compile_script("globalThis.later = 42;", "later.js", Default::default())
            .unwrap();
        let entry = compiler.finish(later).unwrap();
        let namespace = NativeProgramNamespace::from_entry(&entry).unwrap();
        let bytes = namespace.encode(&first).unwrap();
        let decoded = namespace.decode(Rc::new(bytes)).unwrap();
        assert_eq!(
            decoded.atoms.iter().collect::<Vec<_>>(),
            first.atoms.iter().collect::<Vec<_>>()
        );
        assert!(decoded.atoms.belongs_to(&namespace.atoms));
        assert_eq!(
            &*decoded.source.as_ref().unwrap().text,
            &*first.source.as_ref().unwrap().text
        );
        assert_eq!(decoded.function_count(), first.function_count());
        assert_eq!(decoded.encode_trusted(), first.encode_trusted());
    }

    #[test]
    fn shared_metadata_rejects_foreign_namespaces_and_corruption() {
        let first = crate::Engine::specialize("globalThis.first = 1;", "first.js").unwrap();
        let other = crate::Engine::specialize("globalThis.other = 1;", "other.js").unwrap();
        let namespace = NativeProgramNamespace::from_entry(&first).unwrap();
        let foreign = NativeProgramNamespace::from_entry(&other).unwrap();
        assert!(namespace.encode(&other).is_err());
        let bytes = namespace.encode(&first).unwrap();
        assert!(foreign.decode(Rc::new(bytes.clone())).is_err());
        let mut damaged = bytes;
        damaged[0] ^= 1;
        assert!(namespace.decode(Rc::new(damaged)).is_err());
        assert!(namespace.decode(Rc::new(Vec::<u8>::new())).is_err());
    }
}
