//! `arguments` reads in a lane body: a keeping activation's count and the values it kept,
//! or an unmaterialized binding's, which the lane's own handlers answer.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

impl Lowering<'_, '_> {
    /// Whether the read at `pc` is of the entry function's own binding in a body that reports
    /// its status and pc: an inlined callee's reads its own, which no window of this
    /// activation holds.
    pub(crate) fn own_arguments_read(&self, pc: usize, instruction: WideInstruction) -> bool {
        self.completion.status_and_pc() && self.caller_record(pc) && instruction.local_slot() <= usize::from(u16::MAX)
    }

    /// `ArgumentsLength` of the entry function's own binding: the count its keeping
    /// activation kept, else the count an unmaterialized one answers
    /// (`lane_arguments_length`).
    pub(crate) fn arguments_length(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> Option<bool> {
        self.arguments_read(state, pc, instruction, LaneHelper::ArgumentsLength, &[])
    }

    /// `ArgumentGet` of that binding at an integer index a keeping activation kept or an
    /// unmaterialized binding holds; any other key leaves at the record.
    pub(crate) fn arguments_get(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> Option<bool> {
        let index = self.tagged(state, instruction.register_b());
        self.arguments_read(state, pc, instruction, LaneHelper::ArgumentGet, &[index])
    }

    /// The read `helper` answers for the binding in `instruction`'s local slot, given the lane
    /// context, the slot and `operands`. The helper's `DELETED` leaves the record to the lane,
    /// which reads the materialized object.
    fn arguments_read(&mut self, state: &mut State, pc: u32, instruction: WideInstruction, helper: LaneHelper, operands: &[ir::Value]) -> Option<bool> {
        let exit = self.guard_exit(state, pc);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        let slot = self.raw_const(Lane::I64, i64::from(instruction.local_slot() as u16));
        let mut arguments = vec![context, slot];
        arguments.extend_from_slice(operands);
        let value = self.helper_call(helper, &arguments);
        let known = self.raw_compare_imm(Relation::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, known, exit);
        self.set_tagged(state, instruction.result_register(), value);
        Some(true)
    }
}
