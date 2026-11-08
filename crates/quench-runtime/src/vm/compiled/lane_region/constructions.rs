//! Constructions in a lane region: `new callee(...)` by the callee's construct plan.

use super::*;

impl Lowering<'_, '_> {
    /// `new callee(...)` by the callee's construct plan (`compiled::lane_construct`): the
    /// arguments pass where the helper reads them (`construct_arguments`); a construction the
    /// plan does not make leaves at the record, before any effect. The allocation may move
    /// the property arena (`allocated`). A construction a previous body left at runs as a
    /// call does (`dispatch_fast::lane_body_construct`): the callee's plan, else its activation.
    pub(super) fn construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let crate::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return None };
        if self.completion == Completion::StatusAndPc && self.generic.contains(&pc)
            && matches!(self.unit.origins.get(pc), Some(super::super::lane_unit::Origin::Caller(_)))
            && window.base + window.count <= self.unit.caller_registers
            && matches!(instruction.result_flags(), 0 | crate::bytecode::RETURN_REGISTER) {
            return Some(self.call(state, pc, instruction, LaneHelper::ConstructCall));
        }
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            // A static region returns its source PC alone, so the frame's return stays its own.
            (true, Some(super::super::lane_unit::Origin::Caller(record))) if self.completion == Completion::StatusAndPc
                && instruction.result_flags() == crate::bytecode::RETURN_REGISTER => Some(record),
            _ => return None,
        };
        let exit = self.guard_exit(state, pc as u32);
        let arguments = self.construct_arguments(state, window);
        let callee = self.tagged(state, instruction.register_b());
        let argc = self.ins().iconst(types::I64, i64::from(window.count));
        let payload = self.payload;
        let this = self.helper_call(LaneHelper::Construct, &[payload, callee, arguments, argc]);
        let built = self.ins().icmp_imm_u(IntCC::NotEqual, this, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, built, exit);
        self.reshaped(state);
        if let Some(record) = completes {
            self.return_value(record, this);
            return Some(false);
        }
        self.set_tagged(state, instruction.result_register(), this);
        Some(true)
    }

    /// The address of `window`'s values for the construct helper, which only reads them. The
    /// body's register memory holds the caller's registers alone (`Lowering::call`), written
    /// back there; an inlined frame's window, or a static region's, whose registers its exits
    /// write, passes from the native stack.
    fn construct_arguments(&mut self, state: &mut State, window: crate::bytecode::RegisterWindow) -> ir::Value {
        let registers = window.base..window.base + window.count;
        if self.completion == Completion::StatusAndPc && registers.end <= self.unit.caller_registers {
            let base = self.registers_base;
            for register in registers {
                let bits = self.tagged(state, register);
                self.ins().store(MemFlagsData::trusted(), bits, base, register_offset(register));
            }
            return self.ins().iadd_imm_u(base, i64::from(register_offset(window.base)));
        }
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
            u32::from(window.count.max(1)) * size_of::<Value>() as u32, align_of::<Value>().trailing_zeros() as u8));
        for (index, register) in registers.enumerate() {
            let bits = self.tagged(state, register);
            self.ins().stack_store(types::I64, bits, slot, (index * size_of::<Value>()) as i32);
        }
        self.ins().stack_addr(types::I64, slot, 0)
    }
}
