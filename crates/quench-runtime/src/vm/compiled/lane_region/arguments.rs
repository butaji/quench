//! `arguments` reads in a lane body: a keeping activation's count and the values it kept,
//! or an unmaterialized binding's, which the lane's own handlers answer.

use super::*;

impl Lowering<'_, '_> {
    /// `ArgumentsLength` of the entry function's own binding: the count its keeping
    /// activation kept, else the count an unmaterialized one answers
    /// (`lane_arguments_length`). The helper's `DELETED` leaves the record to the lane, which
    /// reads the materialized object.
    pub(super) fn arguments_length(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> Option<bool> {
        let exit = self.guard_exit(state, pc);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let slot = self.ins().iconst(types::I64, i64::from(instruction.local_slot() as u16));
        let value = self.helper_call(LaneHelper::ArgumentsLength, &[context, slot]);
        let known = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, known, exit);
        self.set_tagged(state, instruction.result_register(), value);
        Some(true)
    }

    /// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
    /// unmaterialized binding holds (`lane_argument_get`); any other key leaves at the record.
    pub(super) fn arguments_get(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> Option<bool> {
        let exit = self.guard_exit(state, pc);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let slot = self.ins().iconst(types::I64, i64::from(instruction.local_slot() as u16));
        let index = self.tagged(state, instruction.register_b());
        let value = self.helper_call(LaneHelper::ArgumentGet, &[context, slot, index]);
        let known = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, known, exit);
        self.set_tagged(state, instruction.result_register(), value);
        Some(true)
    }
}
