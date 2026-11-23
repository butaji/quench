//! The signature pool's interning: one type index per equivalence class of callable types.

use super::canonical::{CanonicalSignature, canonical_signature};
use super::{CallableSignature, WasmSignature, WasmSignatures};

impl WasmSignatures {
    /// The pool's type index of a callable type, added if the pool has none equivalent, with the
    /// canonical id the type has across pools; none where the canonical table is full. Only the
    /// types of its fingerprint (`WasmTypes::fingerprint`) are compared.
    pub(super) fn interned(&mut self, declaration: Option<u32>, shape: WasmSignature) -> Option<usize> {
        if let Some(index) = self.find(declaration, &shape) {
            return Some(index);
        }
        let key = declaration.and_then(|index| self.declarations.fingerprint(index as usize));
        let canonical = canonical_signature(declaration.map(|index| (&self.declarations, index)), &shape)?;
        self.types.push(CallableSignature { declaration, shape, canonical });
        self.by_fingerprint.entry(key).or_default().push(self.types.len() as u32 - 1);
        Some(self.types.len() - 1)
    }

    /// The canonical id of the pool's type `index`.
    #[inline]
    pub fn type_canonical(&self, index: u32) -> Option<CanonicalSignature> {
        Some(self.types.get(index as usize)?.canonical)
    }

    /// The canonical id of the signature of defined function `function`: what a table element
    /// holding it stores.
    pub(crate) fn defined_canonical(&self, function: u32) -> Option<CanonicalSignature> {
        self.type_canonical(self.defined_type_index(function)?)
    }

    /// The pool's type index equivalent to a callable type, if it holds one.
    pub(super) fn find(&self, declaration: Option<u32>, shape: &WasmSignature) -> Option<usize> {
        let key = declaration.and_then(|index| self.declarations.fingerprint(index as usize));
        self.by_fingerprint.get(&key).into_iter().flatten().copied().find(|&index| {
            match (self.types[index as usize].declaration, declaration) {
                (Some(left), Some(right)) => self.declarations.equivalent(left as usize, &self.declarations, right as usize),
                (None, None) => self.types[index as usize].shape == *shape,
                _ => false,
            }
        }).map(|index| index as usize)
    }
}
