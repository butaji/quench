//! Constructions in a lane region: `new callee(...)` by the callee's construct plan.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;

impl Lowering<'_, '_> {
    /// Whether a construction of function `function` runs as its stores alone: the static facts of
    /// the VM's own `ScriptCallTarget::initializes` (`TARGET_INITIALIZES`): `Vm::inline_constructible`,
    /// `Function::only_initializes_this_fields` under the program's constants, and no rest parameter
    /// (`!body.rest`). The VM's further gates are per resolution: an environment window and a
    /// cross-program target. Neither changes what the stores are; the helper answers the deleted marker
    /// where the receiver is not finished, and a cross-program plan is the callee's own. The predicate uses
    /// no host, so it is named at the one host type.
    pub(crate) fn initializes_on_construction(&self, function: u32) -> bool {
        let function = self.program.function(function as usize);
        !function.rest && quench_runtime::vm::Vm::inline_constructible(function)
            && function.only_initializes_this_fields(&self.program.constants)
    }

    /// `new callee(...)` by the callee's construct plan (`compiled::lane_construct`): the
    /// arguments pass where the helper reads them (`construct_arguments`); a construction the
    /// plan does not make leaves at the record, before any effect. The allocation may move
    /// the property arena (`allocated`). A construction in the caller's window is the call row
    /// (`call_rows`); one a previous body left at runs as a call does
    /// (`dispatch_fast::body_construct_helper`): the callee's plan, else its activation.
    pub(crate) fn construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let quench_runtime::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return None };
        let completes = match (instruction.returns_from_frame(), self.unit.origins.get(pc).copied()) {
            (false, _) if instruction.result_flags() == 0 => None,
            // A static region returns its source PC alone, so the frame's return stays its own.
            (true, Some(super::super::lane_unit::Origin::Caller(record))) if self.completion.status_and_pc()
                && instruction.result_flags() == quench_runtime::bytecode::RETURN_REGISTER => Some(record),
            _ => return None,
        };
        let site = self.construct_site(pc)?;
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Callee);
        let arguments = self.construct_arguments(state, window);
        let callee = self.tagged(state, instruction.register_b());
        let site = self.raw_const(Lane::I64, site as i64);
        let argc = self.raw_const(Lane::I64, i64::from(window.count));
        let payload = self.payload;
        let this = self.helper_call(LaneHelper::Construct, &[payload, callee, site, arguments, argc]);
        let built = self.raw_compare_imm(Relation::NotEqual, this, Value::DELETED.to_bits() as i64);
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
    fn construct_arguments(&mut self, state: &mut State, window: quench_runtime::bytecode::RegisterWindow) -> ir::Value {
        let registers = window.base..window.base + window.count;
        if self.completion.status_and_pc() && registers.end <= self.unit.caller_registers {
            let base = self.registers_base;
            for register in registers {
                let bits = self.tagged(state, register);
                self.store_word(regions::frame(), bits, base, register_offset(register));
            }
            return self.raw_binary_imm(Binary::Add, base, i64::from(register_offset(window.base)));
        }
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
            u32::from(window.count.max(1)) * size_of::<Value>() as u32, align_of::<Value>().trailing_zeros() as u8));
        for (index, register) in registers.enumerate() {
            let bits = self.tagged(state, register);
            self.slot_store(slot, Lane::I64, bits, (index * size_of::<Value>()) as i32);
        }
        self.slot_address(slot, 0)
    }
}
