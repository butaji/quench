//! The staged rows whose slow case is the VM's own operation (`staged::CallOut`), lowered: the
//! row's words decide the common case inline, and the rest is one call of the lane's helper
//! (`dispatch_fast::vm_operation_step`) that continues in place, so no such record leaves its body.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::lane_unit::Origin;
use quench_runtime::vm::global_facts::GlobalSlot;

impl Lowering<'_, '_> {
    /// The call-out row (`Operation::call_out`) of `instruction` where this lowering makes it: in a
    /// lane body or loop region, over the unit's registers.
    fn call_out_row(&self, instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<ir::Value, EmitProof>> {
        let row = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program))?;
        let (call, result) = row.call_out()?;
        // A comparison jump decides by its own lowering (`compare_jumps`); a spread or super call
        // may return its result (`return f(...xs)`), which completes the activation after the call-out.
        (!matches!(row, quench_runtime::staged::Operation::CompareJump { .. })
            && self.completion.status_and_pc() && (instruction.result_flags() == 0 || returned_call(instruction, &row))
            && call.operands().registers(&[]).chain(result).all(|register| register < self.unit.registers))
            .then_some(row)
    }

    /// Whether `instruction` calls out and nothing else lowers it: every call-out row but a
    /// `Binary`, which `Lowering::operator` lowers natively first.
    pub(crate) fn is_call_out(&self, instruction: WideInstruction) -> bool {
        self.call_out_row(instruction).is_some_and(|row| !is_binary(&row))
    }

    /// A `Binary` the native lowering (`Lowering::operator`) leaves (a string or BigInt relation,
    /// `instanceof`, `in`, an equality of other than numbers): its row, whose slow case is the
    /// VM's operation. `None` for any other instruction.
    pub(crate) fn binary_call_out(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        self.call_out_row(instruction).filter(is_binary).map(|_| self.call_out_step(state, pc, instruction))
    }

    /// The call-out row at `pc`, stepped by its staged definition: the registers the body wrote
    /// are canonical first, and its slow case stores them to the lane's file before the helper
    /// reads them (`frame::call_out::vm_step`). A raised error leaves at `pc`; a for-in step that
    /// finds no key takes its jump. Whether control falls through.
    pub(crate) fn call_out_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        let row = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program)).expect("a call-out record decodes");
        let miss = self.guard_exit(state, pc as u32);
        self.canonicalize(state, pc);
        let cell = self.global_cell(state, pc, &row, miss);
        let site = self.call_out_site(pc, record, cell);
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::ForInNext { .. })) {
            let returns = returned_call(instruction, &row);
            let step = self.step_with(state, frame::Edges::ValueGuarded { miss }, row, StepWith { sites: sites(&site), ..StepWith::default() });
            // A row that answers its value without writing its register (`row_writes_result`: a keyed for-in read's
            // hit) leaves the write to the tier. Unwritten, the register kept its last definition: `Object.extend`'s
            // `d[p] = s[p]` stored the previous iteration's value (raytrace's "Scene rendered incorrectly").
            if let quench_runtime::staged::Step::Value(value) = step
                && !quench_runtime::staged::row_writes_result(instruction.op()) {
                self.set_tagged(state, instruction.result_register(), value);
            }
            state.call_effect();
            // A returned call's result is the activation's: the body completes with the register it defined.
            return if returns { self.complete(state, pc, instruction.result_register()) } else { true };
        }
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss }, row, StepWith { sites: sites(&site), ..StepWith::default() });
        state.call_effect();
        self.continue_branch(state, pc, instruction.jump_target() as usize, taken, fall);
        false
    }

    /// The site of the call-out record at `pc`: where the helper decodes it, the registers the body
    /// wrote, and where a status other than completion or a miss ends the body.
    pub(crate) fn call_out_site(&mut self, pc: usize, record: u32, cell: Option<(GlobalSlot, quench_runtime::staged::ShapeFact)>) -> frame::call_out::CallOutSite {
        frame::call_out::CallOutSite { record, stop: self.call_out_stop(), registers: self.registers_base,
            properties: self.entry.as_ref().expect("entry facts").receiver.properties, spill: self.spilled(pc), cell }
    }

    /// The record of the comparison jump at `pc` where the VM can decide it in place: a body with
    /// the helper, a record of its own, and registers of the unit for operands.
    pub(crate) fn decide_record(&self, pc: usize, operands: [Operand; 2]) -> Option<u32> {
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else { return None };
        (self.completion.status_and_pc() && operands.iter()
            .all(|operand| operand.register_index().is_some_and(|register| register < self.unit.registers)))
            .then_some(record)
    }

    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status: made at the first, filled once at the region's end (`fill_call_out_stop`).
    pub(crate) fn call_out_stop(&mut self) -> ir::Block {
        let builder = &mut self.builder;
        *self.call_out_stop.get_or_insert_with(|| {
            let block = super::cold_block(builder);
            builder.append_block_param(block, types::I64);
            block
        })
    }

    /// Fill the call-outs' shared stop (`call_out_stop`), once every site that reaches it is lowered.
    pub(crate) fn fill_call_out_stop(&mut self) {
        let Some(block) = self.call_out_stop else { return };
        self.switch_to(block, true);
        let status = self.builder.block_params(block)[0];
        self.stop_by_status(status);
    }
}

/// The step's sites where the lowering made a call-out's record.
fn sites(site: &frame::call_out::CallOutSite) -> frame::Sites<'_> {
    frame::Sites { call_out: Some(site), ..frame::Sites::default() }
}

/// Whether `instruction`, the spread or super call `row`, carries the return flag alone: its
/// result is the activation's, so the body completes with the register it defines (`complete`).
/// Any other row, a `Binary` included, is never a returned call.
fn returned_call(instruction: WideInstruction, row: &quench_runtime::staged::Operation<ir::Value, EmitProof>) -> bool {
    matches!(row, quench_runtime::staged::Operation::SpreadCall { .. } | quench_runtime::staged::Operation::SuperCall { .. })
        && instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER
}

/// Whether `row` is a `Binary` of two registers, equality or any other operator.
fn is_binary(row: &quench_runtime::staged::Operation<ir::Value, EmitProof>) -> bool {
    matches!(row, quench_runtime::staged::Operation::Equality { .. } | quench_runtime::staged::Operation::Binary { .. })
}
