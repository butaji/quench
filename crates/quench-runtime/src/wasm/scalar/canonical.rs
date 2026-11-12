//! The canonical function types: one id per structural function type, across every module a
//! thread instantiates. Two types get the same id exactly when their declarations are
//! equivalent (same recursion group shape and position) or, for an embedder's implicit type,
//! their signatures are equal, so a signature check that finds equal ids needs no walk of
//! declarations, and native code compares one word.

use super::WasmSignature;
use crate::wasm::WasmTypes;
use rustc_hash::FxHashMap;
use std::cell::RefCell;

/// The id of a canonical function type, an index of the table. The two ids at the top of the
/// range are no type: they say what a table element holds instead of a function of a module.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct CanonicalSignature(u32);

impl CanonicalSignature {
    /// What an element that is the null reference holds.
    pub(crate) const NULL: Self = Self(u32::MAX);
    /// What an element holds when its function's signature is not a canonical id: a host
    /// function, which the VM checks against the expected type.
    pub(crate) const FOREIGN: Self = Self(u32::MAX - 1);
    /// The first id that names no type.
    const RESERVED: u32 = Self::FOREIGN.0;

    /// The id as the word of a native table's element.
    pub(crate) const fn word(self) -> u64 {
        self.0 as u64
    }
}

/// A canonical type: a declaration of a module's type table, or an embedder's signature.
enum Canonical {
    Declared(WasmTypes, u32),
    Embedded(WasmSignature),
}

impl Canonical {
    /// Whether the type a new entry would hold is this one.
    fn is(&self, declaration: Option<(&WasmTypes, u32)>, shape: &WasmSignature) -> bool {
        match (self, declaration) {
            (Self::Declared(types, index), Some((other, other_index))) => {
                types.equivalent(*index as usize, other, other_index as usize)
            }
            (Self::Embedded(signature), None) => signature == shape,
            _ => false,
        }
    }
}

/// Every canonical type, found by the fingerprint of its declaration (`WasmTypes::fingerprint`;
/// none for an embedder's signature), so a type is compared only with the few of its bucket.
#[derive(Default)]
struct CanonicalSignatures {
    types: Vec<Canonical>,
    by_fingerprint: FxHashMap<Option<u64>, Vec<u32>>,
}

impl CanonicalSignatures {
    fn intern(
        &mut self,
        declaration: Option<(&WasmTypes, u32)>,
        shape: &WasmSignature,
    ) -> Option<CanonicalSignature> {
        let key = declaration.and_then(|(types, index)| types.fingerprint(index as usize));
        let bucket = self.by_fingerprint.entry(key).or_default();
        let same = bucket
            .iter()
            .copied()
            .find(|&id| self.types[id as usize].is(declaration, shape));
        if let Some(id) = same {
            return Some(CanonicalSignature(id));
        }
        let id = u32::try_from(self.types.len())
            .ok()
            .filter(|&id| id < CanonicalSignature::RESERVED)?;
        self.types.push(match declaration {
            Some((types, index)) => Canonical::Declared(types.clone(), index),
            None => Canonical::Embedded(shape.clone()),
        });
        bucket.push(id);
        Some(CanonicalSignature(id))
    }
}

thread_local! {
    /// A module's declarations are `Rc`, so the table is the thread's: every VM and pool of a
    /// thread shares it, and a pool never leaves the thread that built it.
    static TABLE: RefCell<CanonicalSignatures> = RefCell::default();
}

/// The canonical id of the function type `declaration` names in its type table, or of the
/// embedder's `shape` where there is none; none where the table is full.
pub(super) fn canonical_signature(
    declaration: Option<(&WasmTypes, u32)>,
    shape: &WasmSignature,
) -> Option<CanonicalSignature> {
    TABLE.with_borrow_mut(|table| table.intern(declaration, shape))
}

#[cfg(test)]
mod tests {
    use super::super::{WasmCallableType, WasmSignatures};
    use super::*;
    use crate::WasmType;
    use wasmparser::{FuncType, ValType};

    fn embedded(params: &[WasmType], results: &[WasmType]) -> WasmCallableType {
        WasmCallableType::Embedded(WasmSignature {
            params: params.to_vec(),
            results: results.to_vec(),
        })
    }

    fn declared(params: &[ValType], results: &[ValType]) -> (WasmTypes, WasmCallableType) {
        let types = WasmTypes::from_functions([FuncType::new(params.to_vec(), results.to_vec())]);
        (types, WasmCallableType::Declared(0))
    }

    fn pool(types: &WasmTypes, function: WasmCallableType) -> WasmSignatures {
        WasmSignatures::with_imports("canonical", &[], [function], types).unwrap()
    }

    #[test]
    fn an_embedded_signature_has_one_id_across_pools() {
        let types = WasmTypes::default();
        let first = pool(&types, embedded(&[WasmType::I32], &[WasmType::I64]));
        let second = pool(&types, embedded(&[WasmType::I32], &[WasmType::I64]));
        let other = pool(&types, embedded(&[WasmType::I64], &[WasmType::I64]));
        assert_eq!(first.function_canonical(0), second.function_canonical(0));
        assert_ne!(first.function_canonical(0), other.function_canonical(0));
        assert_eq!(first.type_canonical(0), first.function_canonical(0));
    }

    #[test]
    fn a_declared_function_type_has_one_id_across_modules() {
        let (first_types, first_type) = declared(&[ValType::I32], &[ValType::I32]);
        let (second_types, second_type) = declared(&[ValType::I32], &[ValType::I32]);
        let (other_types, other_type) = declared(&[ValType::I64], &[ValType::I32]);
        let first = pool(&first_types, first_type);
        let second = pool(&second_types, second_type);
        let other = pool(&other_types, other_type);
        assert_eq!(first.function_canonical(0), second.function_canonical(0));
        assert_ne!(first.function_canonical(0), other.function_canonical(0));
        assert!(first.accepts(0, &second, 0, true));
        assert!(!first.accepts(0, &other, 0, false));
    }

    #[test]
    fn an_implicit_type_is_not_its_declared_namesake_but_each_accepts_the_other() {
        let (types, declaration) = declared(&[ValType::I32], &[ValType::I32]);
        let from_module = pool(&types, declaration);
        let implicit = embedded(&[WasmType::I32], &[WasmType::I32]);
        let from_embedder = pool(&WasmTypes::default(), implicit);
        assert_ne!(
            from_module.function_canonical(0),
            from_embedder.function_canonical(0)
        );
        assert!(from_module.accepts(0, &from_embedder, 0, false));
        assert!(from_embedder.accepts(0, &from_module, 0, false));
    }

    #[test]
    fn no_type_has_the_id_of_a_null_or_foreign_element() {
        let types = WasmTypes::default();
        let signatures = pool(&types, embedded(&[], &[]));
        let id = signatures.function_canonical(0).unwrap();
        assert_ne!(id, CanonicalSignature::NULL);
        assert_ne!(id, CanonicalSignature::FOREIGN);
        assert_ne!(CanonicalSignature::NULL, CanonicalSignature::FOREIGN);
    }
}
