//! An image body's record no row lowers (`LaneHelpers::Ahead`): an image has no runtime feedback
//! and no later tier, so where the lowering lacks a fact it calls the interpreter's own step of the
//! record in place (`LaneHelper::Step`, `dispatch_fast::body_general_step`) and continues, rather
//! than leaving the body. The registers the body wrote are homed first; the registers the step
//! writes are read back, and every heap proof retires, as after a call.
use super::*;
use super::super::lane_unit::Origin;

impl Lowering<'_, '_> {
    /// The general step of `instruction` at `pc`, where an image body may make it in place: the
    /// entry function's own record (an inlined frame has no record of the running activation), one
    /// that only continues or returns its result (no jump, no other result flag, no `this` binding, no suspension) and writes
    /// registers of the unit. A raise, or a record the step does not make, leaves at `pc`; a leave
    /// in a callee, or a termination, stops the body. Whether control falls through, `None` where
    /// the record is no such step.
    pub(super) fn general_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if self.helpers != LaneHelpers::Ahead || !self.completion.status_and_pc() { return None; }
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else { return None };
        let op = instruction.op();
        // A returning record (`return a + b`) is the step that writes its result register, then the completion with it.
        let returns = instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER;
        if (instruction.result_flags() != 0 && !returns) || op.control_flow_layout().jumps() || op.binds_this() || op.suspends()
            || instruction.writes_current_this() || !(op.steps_in_body() || op.makes_script_call()) {
            return None;
        }
        let written: Vec<Register> = instruction.without_return().written_registers().collect();
        if written.iter().any(|register| *register >= self.unit.caller_registers) { return None; }
        self.canonicalize(state, pc);
        self.home_written(state, pc);
        let miss = self.guard_exit(state, pc as u32);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        let record = self.raw_const(Lane::I64, i64::from(record));
        let status = self.helper_call(LaneHelper::Step, &[context, record]);
        let (completed, incomplete) = (self.builder.create_block(), cold_block(&mut self.builder));
        let done = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_COMPLETED as i64);
        self.branch_if(done, (completed, &[]), (incomplete, &[]));
        self.builder.switch_to_block(incomplete);
        self.builder.seal_block(incomplete);
        let missed = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_MISSED as i64);
        let stop = self.call_out_stop();
        self.branch_if(missed, (miss, &[]), (stop, &[status]));
        self.builder.switch_to_block(completed);
        self.builder.seal_block(completed);
        // The step may have run guest code, allocated or reshaped any object, `this` included.
        self.reshaped(state);
        self.reload_receiver();
        for register in written { self.reload_result(state, register); }
        if returns { return Some(self.complete(state, pc, instruction.result_register())); }
        Some(true)
    }

    /// Fills `block`, a miss of the record at `pc` (a helper that made nothing), with the
    /// interpreter's step of the record (`LaneHelper::Step`): its completion continues at
    /// `resume`, a raise or a record the step does not make leaves at `pc`, and a leave in a
    /// callee, or a termination, stops the body. The registers the step reads are homed (`home_written`)
    /// on every path into `block`; `state` is the state they reach it with.
    pub(super) fn fill_stepped_miss(&mut self, state: &State, pc: usize, record: u32, block: ir::Block, resume: ir::Block) {
        self.builder.switch_to_block(block);
        let miss = self.guard_exit(state, pc as u32);
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        let record = self.raw_const(Lane::I64, i64::from(record));
        let status = self.helper_call(LaneHelper::Step, &[context, record]);
        let incomplete = cold_block(&mut self.builder);
        let done = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_COMPLETED as i64);
        self.branch_if(done, (resume, &[]), (incomplete, &[]));
        self.builder.switch_to_block(incomplete);
        self.builder.seal_block(incomplete);
        let missed = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_MISSED as i64);
        let stop = self.call_out_stop();
        self.branch_if(missed, (miss, &[]), (stop, &[status]));
    }

    /// The record of `pc` where an image body makes its misses by the interpreter's step
    /// (`fill_stepped_miss`): the entry function's own record, in a body that completes with a
    /// status, under `LaneHelpers::Ahead`.
    pub(super) fn stepped_record(&self, pc: usize) -> Option<u32> {
        if self.helpers != LaneHelpers::Ahead || !self.completion.status_and_pc() { return None; }
        match self.unit.origins.get(pc).copied() {
            Some(Origin::Caller(record)) => Some(record),
            _ => None,
        }
    }
}
