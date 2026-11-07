//! The signature pool's interning: one type index per equivalence class of callable types.

use super::{CallableSignature, WasmSignature, WasmSignatures};

impl WasmSignatures {
    /// The pool's type index of a callable type, added if the pool has none equivalent. Only
    /// the types of its fingerprint (`WasmTypes::fingerprint`) are compared.
    pub(super) fn interned(&mut self, declaration: Option<u32>, shape: WasmSignature) -> usize {
        let key = declaration.and_then(|index| self.declarations.fingerprint(index as usize));
        let same = self.by_fingerprint.get(&key).into_iter().flatten().copied().find(|&index| {
            match (self.types[index as usize].declaration, declaration) {
                (Some(left), Some(right)) => self.declarations.equivalent(left as usize, &self.declarations, right as usize),
                (None, None) => self.types[index as usize].shape == shape,
                _ => false,
            }
        });
        same.map_or_else(|| {
            self.types.push(CallableSignature { declaration, shape });
            self.by_fingerprint.entry(key).or_default().push(self.types.len() as u32 - 1);
            self.types.len() - 1
        }, |index| index as usize)
    }
}
