//! The Wasm tier's registers, constants, bindings and globals (`staged::wasm_instance`): the
//! instance primitives as Cranelift code, and the lowering and analysis of each `Instance` step.

use super::{Analysis, AnalysisState, Emitter, Host, INTEGER_TAG, Kind, Region, Slot};
use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm_instance::{Instance, WasmInstance};
use cranelift_codegen::ir::{self, InstBuilder, types};

impl<H: Host> WasmInstance for Emitter<'_, '_, H> {
    fn global_load(&mut self, slot: u32) -> ir::Value {
        let address = self.global_address(slot);
        let flags = self.flags(Region::Globals);
        self.body
            .core
            .builder
            .ins()
            .load(types::I32, flags, address, 0)
    }

    /// The global's cell holds the integer's encoded value: the tag over the zero-extended bits.
    fn global_store(&mut self, slot: u32, value: ir::Value) {
        let address = self.global_address(slot);
        let wide = self.body.core.builder.ins().uextend(types::I64, value);
        let encoded = self
            .body
            .core
            .builder
            .ins()
            .bor_imm_u(wide, INTEGER_TAG as i64);
        let flags = self.flags(Region::Globals);
        self.body
            .core
            .builder
            .ins()
            .store(flags, encoded, address, 0);
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// Lowers a register, constant, binding or global step.
    pub(super) fn instance(&mut self, pc: usize, instruction: WideInstruction, step: Instance) {
        let (a, b) = (instruction.a(), instruction.b());
        if let Some(registers) = step.constant_registers(instruction) {
            let constant = self
                .analysis
                .constant(instruction.constant_index())
                .expect("planned");
            let kind = self.analysis.constant_kind(constant);
            let bits = self.analysis.constant_bits(constant);
            for register in registers {
                self.write_constant(register, kind, bits);
            }
            return;
        }
        match step {
            Instance::Move => {
                let kind = self.operand_kind(pc);
                let value = self.read(b, kind);
                self.write(a, kind, value);
            }
            Instance::GlobalGet => {
                let slot = self.binding_slot(pc);
                let value = self.global_load(slot);
                self.write(a, Kind::I32, value);
            }
            Instance::GlobalSet => {
                let slot = self.binding_slot(pc);
                let value = self.read(a, Kind::I32);
                self.global_store(slot, value);
            }
            // A binding is known statically per instruction; the globals it names are reached
            // through the context. Constants were defined above.
            Instance::Binding | Instance::Constant | Instance::Fill => {}
        }
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Checks a register, constant, binding or global step has a compiled denotation in `state`
    /// and applies its register writes. A global's value is its encoded i32 read in place; a
    /// 64-bit global's value is a box, so it stays interpreted.
    pub(super) fn transfer_instance(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        step: Instance,
        memory: (Option<u16>, &mut Option<u32>),
    ) -> Option<()> {
        let registers = function.registers;
        let (a, b) = (instruction.a(), instruction.b());
        if let Some(defined) = step.constant_registers(instruction) {
            let constant = self.constant(instruction.constant_index())?;
            let slot = self.constant_slot(constant);
            return defined
                .into_iter()
                .try_for_each(|register| state.set(register, registers, slot));
        }
        match step {
            Instance::Move => {
                let slot = match state.get(b, registers)? {
                    slot @ (Slot::Vector | Slot::Memory32Address) => slot,
                    slot => Slot::Value(value_kind(slot)?),
                };
                state.set(a, registers, slot)
            }
            Instance::Binding => self.bind_instance(state, registers, instruction, memory),
            Instance::GlobalGet | Instance::GlobalSet => {
                let Slot::Binding(slot) = state.get(b, registers)? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                if step == Instance::GlobalGet {
                    return state.set(a, registers, Slot::Value(Kind::I32));
                }
                (value_kind(state.get(a, registers)?)? == Kind::I32).then_some(())
            }
            Instance::Constant | Instance::Fill => unreachable!("defined by their registers"),
        }
    }

    /// An instance binding: the function's memory register binds its direct memory, any other
    /// register a slot a later access names.
    fn bind_instance(
        &self,
        state: &mut AnalysisState,
        registers: u16,
        instruction: WideInstruction,
        (memory_register, memory_slot): (Option<u16>, &mut Option<u32>),
    ) -> Option<()> {
        let slot = instruction.imm();
        if Some(instruction.a()) != memory_register {
            return state.set(instruction.a(), registers, Slot::Binding(slot));
        }
        if !self.binds_direct_memory(slot) || memory_slot.is_some_and(|known| known != slot) {
            return None;
        }
        *memory_slot = Some(slot);
        state.set(instruction.a(), registers, Slot::Memory)
    }
}

/// The machine kind a register's slot holds; a validated memory address is a 64-bit value.
pub(super) fn value_kind(slot: Slot) -> Option<Kind> {
    match slot {
        Slot::Value(kind) => Some(kind),
        Slot::Memory32Address => Some(Kind::I64),
        _ => None,
    }
}
