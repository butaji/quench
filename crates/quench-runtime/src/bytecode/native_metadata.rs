use super::{AtomSequence, AtomTable, KeptBytes, KeptPrefix, ResidualProgram, binary};
use std::{hash::Hasher, rc::Rc};

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
        let mut bytes = binary::encode_native_program(program);
        bytes.extend_from_slice(&self.fingerprint.to_le_bytes());
        Ok(bytes)
    }

    pub fn decode(&self, bytes: KeptBytes) -> Result<ResidualProgram, String> {
        let data = bytes.as_ref().as_ref();
        let encoding_len = data
            .len()
            .checked_sub(size_of::<u64>())
            .ok_or("truncated native metadata namespace fingerprint")?;
        let fingerprint = u64::from_le_bytes(data[encoding_len..].try_into().unwrap());
        if fingerprint != self.fingerprint {
            return Err("native metadata atom namespace mismatch".into());
        }
        let encoding: KeptBytes = Rc::new(KeptPrefix {
            bytes,
            len: encoding_len,
        });
        let (mut program, atom_count) = binary::decode_snapshot_program(encoding, None)?;
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

#[cfg(all(test, feature = "aot"))]
mod tests {
    use super::*;

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
