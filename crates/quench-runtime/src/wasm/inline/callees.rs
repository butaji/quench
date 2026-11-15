//! The module's defined functions as the inlining plan sees them: their signatures, by index.
use super::*;

/// The defined functions' signatures and where they start in the function index space. The
/// plan keeps its own copy: lowering adds block types to the module's signature pool.
pub(super) struct Callees {
    imports: u32,
    /// Each defined function's index into `shapes`, the pool's type index.
    defined: Box<[u32]>,
    /// The shapes of the type indices defined functions have; a module has far fewer
    /// types than functions.
    shapes: Box<[Option<WasmSignature>]>,
}

impl Callees {
    pub(super) fn new(signatures: &WasmSignatures) -> Self {
        let imports = signatures.function_count() - signatures.defined_count();
        let defined: Box<[u32]> = (0..signatures.defined_count())
            .map(|index| signatures.function_type_index(imports + index).unwrap_or(u32::MAX))
            .collect();
        let mut shapes = Vec::new();
        for &ty in defined.iter().filter(|ty| **ty != u32::MAX) {
            let ty = ty as usize;
            if shapes.len() <= ty {
                shapes.resize(ty + 1, None);
            }
            if shapes[ty].is_none() {
                shapes[ty] = signatures.type_signature(ty as u32).cloned();
            }
        }
        Self { imports: imports as u32, defined, shapes: shapes.into() }
    }

    pub(super) fn defined_index(&self, function: u32) -> Option<usize> {
        let index = function.checked_sub(self.imports)? as usize;
        (index < self.defined.len()).then_some(index)
    }

    pub(super) fn defined_signature(&self, index: usize) -> Option<&WasmSignature> {
        self.shapes.get(*self.defined.get(index)? as usize)?.as_ref()
    }
}
