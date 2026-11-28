//! The default lowering of a row no specialized arm makes (TOD-2307, FP1 step 1): the row's own
//! staged definition on the emit stage, as every tier steps it, so the region continues past it
//! rather than leaving at its record; a row the emit stage cannot hold makes its generic step in
//! place through the lane's helper (`LaneHelper::Step`), as the b-jit does, and continues past it; an ahead image's
//! body makes it through the same helper's image entry (`ahead_general_step`), with the same status mapping.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::{emit_js_payload_word, emit_lane_call, JsStepDispatch, LANE_CALL_BRANCHED, LANE_CALL_COMPLETED, LANE_CALL_MISSED, LANE_CALL_RAISED};
use crate::compiled::base_region::steps::STEP_WRITES;
use quench_runtime::staged::machine::{Lane, Relation};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

impl Lowering<'_, '_> {
    /// An instruction no specialized arm made: a Number row by its staged definition (`number_row`), else its
    /// generic step made in place (`stepped`), else a leave at its record. Whether control falls through.
    pub(crate) fn unlowered(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, plain: bool) -> bool {
        if let Some(row) = self.number_row(instruction) {
            let miss = self.guard_exit(state, pc as u32);
            self.step(state, frame::Edges::ValueGuarded { miss }, row);
            self.conform_declared_definitions(state, pc, instruction);
            return true;
        }
        self.note_unlowered(instruction);
        let record = self.stepped_record(pc, instruction, plain);
        if self.coverage.full() && record.is_none() && (self.completion.numeric()
            || !((matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) && plain) || instruction.returns_from_frame())) {
            self.valid = false;
            declined("an unlowered instruction in full coverage", pc);
        }
        if let Some(record) = record {
            return self.step_in_place(state, pc, instruction, record);
        }
        self.exit(state, pc as u32);
        false
    }

    /// The staged Number row of `instruction` (`~x` decodes to `BitwiseNot`) where no arm above made it: its
    /// own definition on the emit stage, over registers the unit holds, leaving at the record where it misses.
    fn number_row(&self, instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<ir::Value, EmitProof>> {
        let row = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program))?;
        let held = instruction.value_registers().chain(instruction.written_registers()).all(|register| register < self.unit.registers);
        use quench_runtime::staged::{NumberOperation as N, Operation::Number};
        let numbers = match &row {
            Number(N::BitwiseNot { source, .. } | N::Unary { source, .. } | N::Math { source, .. }) => self.number_operand_held(*source),
            Number(N::Arithmetic { left, right, .. } | N::Remainder { left, right, .. } | N::RelationValue { left, right, .. }
                | N::Bitwise { left, right, .. } | N::Extreme { left, right, .. } | N::Imul { left, right, .. }) =>
                self.number_operand_held(*left) && self.number_operand_held(*right),
            // The power leaf's address is no image's to embed where its helpers are not named by symbol (`remainder`).
            Number(N::Power { base, exponent, .. }) => self.number_operand_held(*base) && self.number_operand_held(*exponent)
                && !(self.helpers.links() && !self.helpers.names_symbols()),
            Number(N::IncDec { .. } | N::ToNumeric { .. }) => true,
            // A relation decides a branch and an int case a table's index: rows over edges, not a value.
            _ => false,
        };
        (instruction.result_flags() == 0 && held && numbers).then_some(row)
    }

    /// Whether `operand` reads as a Number on this stage: a register the unit holds, or a Number immediate.
    fn number_operand_held(&self, operand: Operand) -> bool {
        self.operand_supported(operand) && (operand.kind() != Some(OperandKind::Constant)
            || self.program.constants.get(usize::from(operand.payload())).and_then(super::super::facts::immediate).is_some_and(|value| value.as_number().is_some()))
    }

    /// The census of an instruction left unlowered: the lane's stats, the fallback site, an image's report.
    fn note_unlowered(&self, instruction: WideInstruction) {
        super::super::dispatch_fast::lane_body_stats::unlowered(instruction.op());
        if self.helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            let name = instruction.op().loads_name().then(|| self.program.atoms.get(instruction.atom_index() as usize).map(|atom| format!("({atom})"))).flatten();
            UNLOWERED.with(|unlowered| unlowered.borrow_mut().insert(format!("{:?}{}", instruction.op(), name.unwrap_or_default())));
        }
    }

    /// The caller-frame record of `instruction` at `pc` that the lane's generic step makes in place: a plain
    /// record of the generic-step catalog (`steps_in_body`, which holds no suspension, so `Await`, `Yield` and
    /// `YieldStar` leave; nor a return, call or `this` binding) writing at most `STEP_WRITES` registers, all of
    /// the unit's. A numeric region has no helper table. An ahead image's body binds no `this` in place: its step
    /// would move the receiver its rows read from the entry, which only the lane's helper republishes.
    fn stepped_record(&self, pc: usize, instruction: WideInstruction, plain: bool) -> Option<u32> {
        let Some(super::lane_unit::Origin::Caller(record)) = self.unit.origins.get(pc).copied() else { return None };
        let binds_receiver = instruction.op().binds_this() || instruction.writes_current_this();
        let steps = self.completion.status_and_pc() && self.entry.is_some() && !matches!(self.helpers, LaneHelpers::Numeric)
            && !(self.helpers == LaneHelpers::Ahead && binds_receiver);
        (steps && plain && instruction.op().steps_in_body() && instruction.written_registers().count() <= STEP_WRITES
            && instruction.written_registers().all(|register| register < self.unit.registers)).then_some(record)
    }

    /// The status of the general step of the record `record` (`LaneHelper::Step`): the lane's `(context, record)`, or an
    /// ahead image's `(payload, record)`, which its symbol names `ahead_general_step` (`LaneHelpers::Ahead`).
    fn step_call(&mut self, record: u32) -> ir::Value {
        let record = self.raw_const(Lane::I64, i64::from(record));
        if self.helpers == LaneHelpers::Ahead {
            let target = self.helpers.target(&mut self.builder, LaneHelper::Step);
            return emit_lane_call(&mut self.builder, target, &[self.payload, record]);
        }
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        self.helper_call(LaneHelper::Step, &[context, record])
    }

    /// Makes `instruction` in place through `LaneHelper::Step`, as the b-jit's helper step does: the registers it
    /// reads and writes go to the lane's file first, and the helper's status decides the successor. Completed
    /// continues past it with the registers it wrote reloaded; missed (nothing ran) leaves at the record;
    /// branched (the jump it takes) leaves at its target; raised (the error parked for the body's handlers)
    /// takes the raise path a callee's exception takes (`raised_call`); any other status stops the body with it
    /// (`stop_by_status`: a termination, or a leave in a callee the step made). Returns whether control falls through.
    fn step_in_place(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, record: u32) -> bool {
        if !state.virtuals.is_empty() || !state.virtual_arrays.is_empty() { self.materialize_virtuals(state, pc); }
        self.home_written(state, pc);
        let missed = self.guard_exit(state, pc as u32);
        let status = self.step_call(record);
        // The step may move the property arena, replace the environment and run guest code: the facts
        // retire as a call's do, and the registers it wrote are read back from the lane's file.
        state.call_effect();
        self.reload_property_arena();
        self.reload_environment();
        for register in instruction.written_registers() { self.reload_result(state, register); }
        let completed = self.builder.create_block();
        let unfinished = self.builder.create_block();
        let done = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_COMPLETED as i64);
        self.branch_if(done, (completed, &[]), (unfinished, &[]));
        self.builder.switch_to_block(unfinished);
        let is_missed = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_MISSED as i64);
        let other = self.builder.create_block();
        self.branch_if(is_missed, (missed, &[]), (other, &[]));
        self.builder.switch_to_block(other);
        if instruction.op().control_flow_layout().jumps() {
            let is_branched = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_BRANCHED as i64);
            let taken = self.guard_exit(state, instruction.jump_target());
            let rest = self.builder.create_block();
            self.branch_if(is_branched, (taken, &[]), (rest, &[]));
            self.builder.switch_to_block(rest);
        }
        let is_raised = self.raw_compare_imm(Relation::Equal, status, LANE_CALL_RAISED as i64);
        let raised = cold_block(&mut self.builder);
        let rest = self.builder.create_block();
        self.branch_if(is_raised, (raised, &[]), (rest, &[]));
        self.builder.switch_to_block(rest);
        self.stop_by_status(status);
        self.raised_call(state, pc, record, raised);
        self.builder.switch_to_block(completed);
        self.conform_declared_definitions(state, pc, instruction);
        true
    }
}
