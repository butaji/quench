//! The `this` a bound run's entry proves once (`Lowering::entry_facts`).
use super::*;

impl Lowering<'_, '_> {
    /// A bound run's fixed `this`: the value its payload's slot holds, or
    /// `Value::DELETED` where the payload binds none.
    pub(super) fn fixed_this(&mut self) -> ir::Value {
        let slot = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, fixed_this_slot));
        let present = self.builder.ins().icmp_imm_u(IntCC::NotEqual, slot, 0);
        let bound = self.builder.create_block();
        let absent = self.builder.create_block();
        let join = self.builder.create_block();
        let bits = self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(present, bound, &[], absent, &[]);
        self.builder.switch_to_block(bound);
        let value = self.builder.ins().load(types::I64, MemFlagsData::trusted(), slot, 0);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
        self.builder.switch_to_block(absent);
        let deleted = self.builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(deleted)]);
        self.builder.switch_to_block(join);
        bits
    }
}
