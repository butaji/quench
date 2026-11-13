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
use crate::staged::wasm_simd::{Lanes, Native, NativeOp, Simd, WasmSimd, simd};
use crate::wasm::V128_BYTES;
use crate::wasm::simd::SimdKernel;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, Signature, types};
use cranelift_frontend::Variable;

/// The machine type a vector is viewed as for a lane-wise operation: the lanes' integer type
/// repeated to fill the vector.
fn lane_type(lanes: Lanes) -> ir::Type {
    let width = lanes.width();
    match lanes {
        Lanes::Bits => VECTOR_IR,
        _ => ir::Type::int(width as u16)
            .and_then(|lane| lane.by(u128::BITS / width))
            .expect("a vector of lanes"),
    }
}

impl<H: Host> WasmSimd for Emitter<'_, '_, H> {
    type Value = ir::Value;

    /// One Cranelift instruction with exactly the row's semantics: a vector shift reduces its
    /// count modulo the lane width, as Wasm does.
    fn vector_native(
        &mut self,
        native: Native,
        left: ir::Value,
        right: Option<ir::Value>,
    ) -> ir::Value {
        let ty = lane_type(native.lanes);
        let left = self.lanes(ty, left);
        match (native.op, right) {
            (NativeOp::Not, _) => self.body.core.builder.ins().bnot(left),
            (
                NativeOp::ShiftLeft | NativeOp::ShiftRightUnsigned | NativeOp::ShiftRightSigned,
                Some(count),
            ) => self.lane_shift(native.op, left, count),
            (op, Some(right)) => {
                let right = self.lanes(ty, right);
                self.lane_binary(op, left, right)
            }
            (op, None) => unreachable!("{op:?} takes a second operand"),
        }
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
        self.store_operand(slot, SIMD_KERNEL_LEFT, left);
        if let Some(right) = right {
            self.store_operand(slot, SIMD_KERNEL_RIGHT, right);
        }
        let mut signature = Signature::new(self.call_conv);
        signature.params.push(AbiParam::new(types::I64));
        signature.params.push(AbiParam::new(types::I64));
        let signature = self.body.core.builder.import_signature(signature);
        let target = self.body.core.builder.ins().iconst(
            types::I64,
            compiled_simd_kernel as *const () as usize as i64,
        );
        let selector = self
            .body
            .core
            .builder
            .ins()
            .iconst(types::I64, i64::from(selector));
        let words = self.body.core.builder.ins().stack_addr(types::I64, slot, 0);
        self.body
            .core
            .builder
            .ins()
            .call_indirect(signature, target, &[selector, words]);
        self.load_result(slot, kernel.result_type(lane))
    }

    fn vector_shuffle(
        &mut self,
        lanes: [u8; V128_BYTES],
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        let mask = self
            .body
            .core
            .builder
            .func
            .dfg
            .immediates
            .push(ir::ConstantData::from(lanes.to_vec()));
        self.body.core.builder.ins().shuffle(left, right, mask)
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
        let value = self.lanes(VECTOR_IR, value);
        let variable = self.vector(register);
        self.known.retain(|(known, _)| *known != register);
        self.body.core.builder.def_var(variable, value);
    }

    /// `value`, a vector, viewed with the lane shape `ty`: the same bits.
    fn lanes(&mut self, ty: ir::Type, value: ir::Value) -> ir::Value {
        if self.body.core.builder.func.dfg.value_type(value) == ty {
            return value;
        }
        let flags = MemFlagsData::new().with_endianness(ir::Endianness::Little);
        self.body.core.builder.ins().bitcast(ty, flags, value)
    }

    pub(super) fn vector_constant(&mut self, bits: u128) -> ir::Value {
        let constant = self
            .body
            .core
            .builder
            .func
            .dfg
            .constants
            .insert(ir::ConstantData::from(bits.to_le_bytes().to_vec()));
        self.body.core.builder.ins().vconst(VECTOR_IR, constant)
    }

    fn lane_shift(&mut self, op: NativeOp, left: ir::Value, count: ir::Value) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match op {
            NativeOp::ShiftLeft => ins.ishl(left, count),
            NativeOp::ShiftRightUnsigned => ins.ushr(left, count),
            _ => ins.sshr(left, count),
        }
    }

    fn lane_binary(&mut self, op: NativeOp, left: ir::Value, right: ir::Value) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match op {
            NativeOp::And => ins.band(left, right),
            NativeOp::Or => ins.bor(left, right),
            NativeOp::Xor => ins.bxor(left, right),
            NativeOp::AndNot => ins.band_not(left, right),
            NativeOp::Add => ins.iadd(left, right),
            NativeOp::Subtract => ins.isub(left, right),
            _ => {
                let builder = &mut self.body.core.builder;
                let (left, right) = (builder.ins().uwiden_low(left), builder.ins().uwiden_low(right));
                builder.ins().imul(left, right)
            }
        }
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

    /// An operand into the two words at position `at`: a vector whole, a scalar in the low word.
    fn store_operand(&mut self, slot: ir::StackSlot, at: usize, value: ir::Value) {
        let word = types::I64.bytes();
        let offset = (at as u32 * word) as i32;
        let ty = self.body.core.builder.func.dfg.value_type(value);
        let (low, high) = if ty.is_vector() {
            let value = self.lanes(types::I64X2, value);
            let builder = &mut self.body.core.builder;
            (
                builder.ins().extractlane(value, 0),
                builder.ins().extractlane(value, 1),
            )
        } else {
            let builder = &mut self.body.core.builder;
            let low = if ty == types::I32 {
                builder.ins().uextend(types::I64, value)
            } else {
                value
            };
            (low, builder.ins().iconst(types::I64, 0))
        };
        let builder = &mut self.body.core.builder;
        builder.ins().stack_store(types::I64, low, slot, offset);
        builder
            .ins()
            .stack_store(types::I64, high, slot, offset + word as i32);
    }

    /// The kernel's result of type `ty`, from the result words.
    fn load_result(&mut self, slot: ir::StackSlot, ty: WasmType) -> ir::Value {
        let result = (SIMD_KERNEL_RESULT as u32 * types::I64.bytes()) as i32;
        let builder = &mut self.body.core.builder;
        if ty == WasmType::V128 {
            return builder
                .ins()
                .stack_load(types::I64, VECTOR_IR, slot, result);
        }
        let value = builder
            .ins()
            .stack_load(types::I64, types::I64, slot, result);
        match Kind::of(ty).expect("planned") {
            Kind::I32 => builder.ins().ireduce(types::I32, value),
            Kind::I64 => value,
        }
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
        let host = self.body.core.builder.ins().iadd(self.base, index);
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
                let value = self.body.core.builder.ins().load(VECTOR_IR, flags, host, 0);
                self.write_vector(a, value);
            }
            Simd::Store => {
                let (host, flags) = self.vector_access(pc, c);
                let value = self.read_vector(a);
                self.body.core.builder.ins().store(flags, value, host, 0);
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
