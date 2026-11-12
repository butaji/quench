//! Dense element stores through a hoisted loop view in a lane region.

use super::*;

impl Lowering<'_, '_> {
    /// Store `value` at `index` of the dense array `view`: the staged element write, whose
    /// hole or miss continues at `miss`.
    pub(super) fn index_write(&mut self, state: &mut State, index: ir::Value, value: ir::Value,
        view: super::super::DenseArrayView, miss: ir::Block) {
        let step = self.step(state, frame::Edges::ValueGuarded { miss },
            crate::staged::Operation::IndexWrite { index, value,
                proof: super::super::EmitProof::Index { view, absent: miss } });
        debug_assert!(matches!(step, crate::staged::Step::Value(_)));
    }
}
