//! The Wasm tier's vectors: the primitives of `WasmSimd` as Cranelift vector instructions (a
//! native row) or a callout of the operator's kernel, the vector registers, and the lowering
//! and analysis of each `Simd` step. A vector access checks through the staged memory rule.

use super::wasm_instance_stage::value_kind;
use super::wasm_stage::infallible;
use super::{
    Analysis, AnalysisState, Emitter, Host, Kind, Region, SIMD_KERNEL_LEFT, SIMD_KERNEL_RESULT,
    SIMD_KERNEL_RIGHT, SIMD_KERNEL_WORDS, Slot, VECTOR_IR, compiled_simd_kernel,
};
use crate::WasmType;
use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm_memory::{Access, check_access, effective_address};
use crate::staged::machine::{Lane, MachineAccess, MachineStage, MachineVector};
use crate::staged::wasm::Width;
use crate::staged::wasm_simd::{Lanes, Native, Simd, WasmSimd, simd};
use crate::wasm::integer::I64BinaryOperator as Binary;
use crate::wasm::V128_BYTES;
use crate::wasm::simd::SimdKernel;
use cranelift_codegen::ir::{self, MemFlagsData, types};
use cranelift_frontend::Variable;

impl<H: Host> WasmSimd for Emitter<'_, '_, H> {
    type Value = ir::Value;

    /// One machine instruction with exactly the row's semantics.
    fn vector_native(
        &mut self,
        native: Native,
        left: ir::Value,
        right: Option<ir::Value>,
    ) -> ir::Value {
        self.lane_operation(native, left, right)
    }

    /// The operator's own kernel (`SimdKernel::apply_bits`), which the interpreter runs too:
    /// the operands go to words, `compiled_simd_kernel` computes, the result comes back.
    fn vector_kernel(
        &mut self,
        selector: u32,
        left: ir::Value,
        right: Option<ir::Value>,
    ) -> ir::Value {
        let (kernel, lane) = SimdKernel::of(selector).expect("an admitted selector");
        let slot = self.kernel_words();
        self.spill_operand(slot, SIMD_KERNEL_LEFT, left);
        if let Some(right) = right {
            self.spill_operand(slot, SIMD_KERNEL_RIGHT, right);
        }
        let signature = crate::vm::compiled::call_signature::typed_signature(self.call_conv, &[types::I64, types::I64], &[]);
        let signature = self.body.core.builder.import_signature(signature);
        let target = self.constant(Width::I64, compiled_simd_kernel as *const () as usize as i64);
        let selector = self.constant(Width::I64, i64::from(selector));
        let words = self.slot_address(slot, 0);
        self.call_signature(signature, target, &[selector, words]);
        self.reload_result(slot, SIMD_KERNEL_RESULT, kernel.result_type(lane))
    }

    fn vector_shuffle(
        &mut self,
        lanes: [u8; V128_BYTES],
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        self.vector_shuffle_lanes(lanes, left, right)
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The SIMD variable of `register`.
    fn vector(&mut self, register: u16) -> Variable {
        let index = usize::from(register);
        if self.vectors.len() <= index {
            self.vectors.resize(index + 1, None);
        }
        if let Some(variable) = self.vectors[index] {
            return variable;
        }
        let variable = self.body.core.builder.declare_var(VECTOR_IR);
        self.vectors[index] = Some(variable);
        variable
    }

    pub(super) fn read_vector(&mut self, register: u16) -> ir::Value {
        let variable = self.vector(register);
        self.body.core.builder.use_var(variable)
    }

    /// Define `register` as the vector `value`, of any lane shape.
    pub(super) fn write_vector(&mut self, register: u16, value: ir::Value) {
        let value = self.vector_view(Lanes::Bits, value);
        let variable = self.vector(register);
        self.known.retain(|(known, _)| *known != register);
        self.body.core.builder.def_var(variable, value);
    }

    /// The stack slot of the kernel callout's words, made at the first callout.
    fn kernel_words(&mut self) -> ir::StackSlot {
        if let Some(slot) = self.simd_words {
            return slot;
        }
        let word = types::I64.bytes();
        let slot = self
            .body
            .core
            .builder
            .create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                SIMD_KERNEL_WORDS as u32 * word,
                V128_BYTES.trailing_zeros() as u8,
            ));
        self.simd_words = Some(slot);
        slot
    }

    /// An operand of type `ty` in `register`: a vector, or a scalar in its integer view.
    fn simd_operand(&mut self, register: u16, ty: WasmType) -> ir::Value {
        match ty {
            WasmType::V128 => self.read_vector(register),
            ty => self.read(register, Kind::of(ty).expect("planned")),
        }
    }

    /// The host address and flags of a vector access at the 64-bit address in `register`. A
    /// proven memory32 address fits the guard reservation, including the vector's last byte; an
    /// access stays trapping, so even an unused load faults before later effects.
    fn vector_access(&mut self, pc: usize, register: u16) -> (ir::Value, MemFlagsData) {
        let index = self.read(register, Kind::I64);
        let proven = self.plan.operand(pc) == Slot::Memory32Address;
        infallible(check_access(self, Access::VECTOR, index, 0, proven));
        let host = self.operate(Binary::Add, Width::I64, self.base, index);
        if !(self.guarded && proven) {
            return (host, self.flags(Region::LinearMemory));
        }
        let flags = MemFlagsData::new()
            .with_trap_code(Some(ir::TrapCode::HEAP_OUT_OF_BOUNDS))
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[Region::LinearMemory as usize]));
        (host, flags)
    }

    /// Lowers a vector step.
    pub(super) fn simd(&mut self, pc: usize, instruction: WideInstruction, step: Simd) {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        match step {
            Simd::Operator { selector } => self.vector_operator(instruction, selector),
            Simd::Shuffle => {
                let lanes = self
                    .analysis
                    .shuffle_lanes(instruction.constant_index())
                    .expect("planned");
                let (left, right) = (self.read_vector(b), self.read_vector(c));
                let value = self.vector_shuffle(lanes, left, right);
                self.write_vector(a, value);
            }
            Simd::Address => {
                let offset = self
                    .analysis
                    .memory_offset(instruction.constant_index())
                    .expect("planned");
                let address = self.read(b, Kind::I32);
                let value = effective_address(self, address, offset);
                self.write(a, Kind::I64, value);
            }
            Simd::Load => {
                let (host, flags) = self.vector_access(pc, c);
                let value = self.raw_load(Lane::V128, flags, host, 0);
                self.write_vector(a, value);
            }
            Simd::Store => {
                let (host, flags) = self.vector_access(pc, c);
                let value = self.read_vector(a);
                self.store_word(flags, value, host, 0);
            }
        }
    }

    fn vector_operator(&mut self, instruction: WideInstruction, selector: u32) {
        let (kernel, lane) = SimdKernel::of(selector).expect("planned");
        let left = self.simd_operand(instruction.b(), kernel.left);
        let right = kernel
            .right
            .map(|ty| self.simd_operand(instruction.c(), ty));
        let value = simd(self, selector, left, right);
        match kernel.result_type(lane) {
            WasmType::V128 => self.write_vector(instruction.a(), value),
            ty => self.write(instruction.a(), Kind::of(ty).expect("planned"), value),
        }
    }
}

/// Checks `register` holds a scalar of `kind`.
fn holds_kind(state: &AnalysisState, registers: u16, register: u16, kind: Kind) -> Option<()> {
    (value_kind(state.get(register, registers)?)? == kind).then_some(())
}

/// Checks `register` holds a vector.
fn holds_vector(state: &AnalysisState, registers: u16, register: u16) -> Option<()> {
    (state.get(register, registers)? == Slot::Vector).then_some(())
}

/// Checks `register` holds an operand of type `ty`.
fn holds_operand(state: &AnalysisState, registers: u16, register: u16, ty: WasmType) -> Option<()> {
    match ty {
        WasmType::V128 => holds_vector(state, registers, register),
        ty => holds_kind(state, registers, register, Kind::of(ty)?),
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Checks a vector step reads vectors where it must and its memory through the function's
    /// memory, and applies its register writes.
    pub(super) fn transfer_simd(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        step: Simd,
        memory: (Option<u16>, &mut Option<u32>),
    ) -> Option<()> {
        let (memory_register, memory_slot) = memory;
        if let Some(named) = step.memory(instruction) {
            self.vector_memory(function, state, named, memory_register, memory_slot)?;
        }
        match step {
            Simd::Address | Simd::Load | Simd::Store => {
                self.transfer_access(function, instruction, state, step)
            }
            Simd::Shuffle | Simd::Operator { .. } => {
                self.transfer_lanes(function, instruction, state, step)
            }
        }
    }

    /// A vector access: a 32-bit address plus the access's offset, as 64 bits so the sum cannot
    /// wrap, which a load or store then reads as a 64-bit address.
    fn transfer_access(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        step: Simd,
    ) -> Option<()> {
        let registers = function.registers;
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        match step {
            Simd::Address => {
                self.memory_offset(instruction.constant_index())?;
                holds_kind(state, registers, b, Kind::I32)?;
                state.set(a, registers, Slot::Memory32Address)
            }
            Simd::Load => {
                holds_kind(state, registers, c, Kind::I64)?;
                state.set(a, registers, Slot::Vector)
            }
            _ => {
                holds_kind(state, registers, c, Kind::I64)?;
                holds_vector(state, registers, a)
            }
        }
    }

    /// A shuffle or an operator: operands of the types the operator's kernel names.
    fn transfer_lanes(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        step: Simd,
    ) -> Option<()> {
        let registers = function.registers;
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        let Simd::Operator { selector } = step else {
            self.shuffle_lanes(instruction.constant_index())?;
            holds_vector(state, registers, b)?;
            holds_vector(state, registers, c)?;
            return state.set(a, registers, Slot::Vector);
        };
        let (kernel, lane) = SimdKernel::of(selector)?;
        holds_operand(state, registers, b, kernel.left)?;
        if let Some(right) = kernel.right {
            holds_operand(state, registers, c, right)?;
        }
        let result = match kernel.result_type(lane) {
            WasmType::V128 => Slot::Vector,
            ty => Slot::Value(Kind::of(ty)?),
        };
        state.set(a, registers, result)
    }
}
