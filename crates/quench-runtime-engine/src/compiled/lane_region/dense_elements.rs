//! Dense element stores through a hoisted loop view in a lane region.

use super::*;

impl Lowering<'_, '_> {
    /// Store `value` at `index` of the dense array `view`: the staged element write, whose
    /// hole or miss continues at `miss`; `in_bounds` is the index's range proof against the view.
    pub(crate) fn index_write(&mut self, state: &mut State, index: ir::Value, value: ir::Value,
        view: super::super::DenseArrayView, miss: ir::Block, in_bounds: bool) {
        let step = self.step(state, frame::Edges::ValueGuarded { miss },
            quench_runtime::staged::Operation::IndexWrite { index, value,
                proof: super::super::EmitProof::Index { view, absent: miss, in_bounds } });
        debug_assert!(matches!(step, quench_runtime::staged::Step::Value(_)));
    }
}
