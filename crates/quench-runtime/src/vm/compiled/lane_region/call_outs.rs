//! The staged rows whose slow case is the VM's own operation (`staged::CallOut`), lowered: the
//! row's words decide the common case inline, and the rest is one call of the lane's helper
//! (`dispatch_fast::lane_vm_step`) that continues in place, so no such record leaves its body.
use super::*;
use super::super::lane_unit::Origin;

impl Lowering<'_, '_> {
    /// The call-out row (`Operation::call_out`) of `instruction` where this lowering makes it: in a
    /// lane body or loop region, over the unit's registers.
    fn call_out_row(&self, instruction: WideInstruction) -> Option<crate::staged::Operation<ir::Value, EmitProof>> {
        let row = crate::staged::Operation::decode(instruction, |_| None)?;
        let (call, result) = row.call_out()?;
        (self.completion.status_and_pc() && instruction.result_flags() == 0
            && call.operands().into_iter().flatten().chain(result).all(|register| register < self.unit.registers))
            .then_some(row)
    }

    /// Whether `instruction` calls out and nothing else lowers it: every call-out row but a
    /// `Binary`, which `Lowering::operator` lowers natively first.
    pub(super) fn is_call_out(&self, instruction: WideInstruction) -> bool {
        self.call_out_row(instruction).is_some_and(|row| !is_binary(&row))
    }

    /// A `Binary` the native lowering (`Lowering::operator`) leaves (a string or BigInt relation,
    /// `instanceof`, `in`, an equality of other than numbers): its row, whose slow case is the
    /// VM's operation. `None` for any other instruction.
    pub(super) fn binary_call_out(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        self.call_out_row(instruction).filter(is_binary).map(|_| self.call_out_step(state, pc, instruction))
    }

    /// The call-out row at `pc`, stepped by its staged definition: the registers the body wrote
    /// are canonical first, and its slow case stores them to the lane's file before the helper
    /// reads them (`frame::call_out::vm_step`). A raised error leaves at `pc`; a for-in step that
    /// finds no key takes its jump. Whether control falls through.
    pub(super) fn call_out_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(Origin::Caller(record)) = self.unit.origins.get(pc).copied() else {
            self.exit(state, pc as u32);
            return false;
        };
        let row = crate::staged::Operation::decode(instruction, |_| None).expect("a call-out record decodes");
        let miss = self.guard_exit(state, pc as u32);
        self.canonicalize(state, pc);
        let cell = self.global_cell(state, pc, &row, miss);
        let site = frame::call_out::CallOutSite { record, stop: self.call_out_stop(), registers: self.registers_base,
            properties: self.entry.as_ref().expect("entry facts").receiver.properties, spill: self.spilled(pc), cell };
        if !matches!(row, crate::staged::Operation::ForInNext { .. }) {
            self.step_with(state, frame::Edges::ValueGuarded { miss }, None, Some(&site), row);
            state.call_effect();
            return true;
        }
        let (taken, fall) = (self.builder.create_block(), self.builder.create_block());
        self.step_with(state, frame::Edges::Branch { target: taken, following: fall, miss }, None, Some(&site), row);
        state.call_effect();
        self.continue_branch(state, pc, instruction.jump_target() as usize, taken, fall);
        false
    }

    /// The block the call-outs' statuses of a leave in a callee, or a termination, reach with the
    /// status: made at the first, filled once at the region's end (`fill_call_out_stop`).
    fn call_out_stop(&mut self) -> ir::Block {
        let builder = &mut self.builder;
        *self.call_out_stop.get_or_insert_with(|| {
            let block = builder.create_block();
            builder.set_cold_block(block);
            builder.append_block_param(block, types::I64);
            block
        })
    }

    /// Fill the call-outs' shared stop (`call_out_stop`), once every site that reaches it is lowered.
    pub(super) fn fill_call_out_stop(&mut self) {
        let Some(block) = self.call_out_stop else { return };
        self.switch_to(block, true);
        let status = self.builder.block_params(block)[0];
        self.stop_by_status(status);
    }
}

/// Whether `row` is a `Binary` of two registers, equality or any other operator.
fn is_binary(row: &crate::staged::Operation<ir::Value, EmitProof>) -> bool {
    matches!(row, crate::staged::Operation::Equality { .. } | crate::staged::Operation::Binary { .. })
}
