//! The `this` a bound run's entry proves once (`Lowering::entry_facts`).
use super::*;
use crate::compiled::regions;

impl Lowering<'_, '_> {
    /// A bound run's fixed `this`: the value its payload's slot holds, or
    /// `Value::DELETED` where the payload binds none.
    pub(crate) fn fixed_this(&mut self) -> ir::Value {
        let slot = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, fixed_this_slot));
        let present = self.raw_compare_imm(Relation::NotEqual, slot, 0);
        let bound = self.builder.create_block();
        let absent = self.builder.create_block();
        let join = self.builder.create_block();
        let bits = self.builder.append_block_param(join, types::I64);
        self.branch_if(present, (bound, &[]), (absent, &[]));
        self.builder.switch_to_block(bound);
        let value = self.raw_load(Lane::I64, regions::frame(), slot, 0);
        self.goto(join, &[value]);
        self.builder.switch_to_block(absent);
        let deleted = self.raw_const(Lane::I64, Value::DELETED.to_bits() as i64);
        self.goto(join, &[deleted]);
        self.builder.switch_to_block(join);
        bits
    }
}
