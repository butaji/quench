//! Dense element reads in a lane region, holes and indexes past the elements included.

use super::*;

impl Lowering<'_, '_> {
    /// Element `index` of the dense array `base` whose `view` the body holds: a present
    /// element, else what the array and its prototypes give a hole or an index past them by
    /// the general tier's rule (`general_elements::general_element_get`), else `exit`.
    pub(super) fn dense_element(&mut self, state: &mut State, base: ir::Value, view: super::super::DenseArrayView, index: ir::Value, exit: ir::Block) -> ir::Value {
        let absent = self.builder.create_block();
        let present = match self.step(state, frame::Edges::ValueGuarded { miss: absent },
            crate::staged::Operation::IndexRead { index,
                proof: super::super::EmitProof::Index { view, absent } }) {
            crate::staged::Step::Value(value) => value,
            _ => unreachable!("a dense element read yields a value or leaves at its cold path"),
        };
        let join = self.builder.create_block();
        let value = self.builder.append_block_param(join, types::I64);
        self.ins().jump(join, &[ir::BlockArg::Value(present)]);
        self.builder.switch_to_block(absent);
        self.builder.set_cold_block(absent);
        let key = int_tagged(&mut self.builder, index);
        let helper = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, element_get));
        let read = emit_lane_helper_call(&mut self.builder, self.payload, helper, &[base, key]);
        let found = self.ins().icmp_imm_u(IntCC::NotEqual, read, Value::DELETED.to_bits() as i64);
        self.ins().brif(found, join, &[ir::BlockArg::Value(read)], exit, &[]);
        self.builder.switch_to_block(join);
        value
    }
}

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
