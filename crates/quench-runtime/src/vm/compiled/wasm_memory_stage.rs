//! The Wasm tier's linear memory: the primitives of `WasmMemory` as Cranelift code, and the
//! lowering and analysis of each `Memory` step through the staged rules, so an access traps
//! where the interpreter's does.

use super::wasm_instance_stage::value_kind;
use super::wasm_stage::infallible;
use super::{
    Analysis, AnalysisState, CONTEXT_MEMORY_LENGTH, CallContext, Emitter, Kind, Region, Slot,
    Vm, compiled_memory_copy, compiled_memory_fill, refresh_memory_length,
};
use crate::bytecode::{Function, WideInstruction};
use crate::staged::machine::{Lane, MachineAccess, MachineStage};
use crate::staged::wasm::Width;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::integer::I64BinaryOperator as Binary;
use crate::staged::wasm_memory::{
    Access, Memory, WasmMemory, WasmMemoryGrow, WasmMemoryWrite, copy, fill, load, size, store,
};
use crate::wasm::INDEX_GROW_FAILURE;
use crate::wasm::memory::WASM_PAGE_BITS;
use cranelift_codegen::ir::{self, MemFlagsData};

impl Emitter<'_, '_> {
    /// The length a call passes after the context when the memory is unguarded (`length` is
    /// passed): the current one (`memory_length`), so a callee sees a length a growing callee
    /// refreshed. `None` where the length is not passed.
    pub(super) fn passed_length(&mut self) -> Option<ir::Value> {
        match self.length {
            Some(_) => Some(self.memory_length()),
            None => None,
        }
    }
}

impl WasmMemory for Emitter<'_, '_> {
    type Vector = ir::Value;

    fn guarded(&self) -> bool {
        self.guarded
    }

    /// The length the function received, while no callout can grow the memory, or the function
    /// itself cannot grow it (`grows`: no callee it reaches may); else the context's, which a
    /// callout refreshes.
    fn memory_length(&mut self) -> ir::Value {
        match self.length {
            Some(length) if !self.analysis.callouts || !self.grows => length,
            _ => {
                let context = self.body.core.context;
                self.load_word(Width::I64, MemFlagsData::trusted(), context, CONTEXT_MEMORY_LENGTH)
            }
        }
    }

    fn memory_index(&mut self, address: ir::Value) -> ir::Value {
        self.zero_extend(address)
    }

    fn memory_load(&mut self, access: Access, index: ir::Value, offset: u32) -> ir::Value {
        let host = self.host_address(index, offset);
        let flags = self.load_flags();
        self.load_access(access, flags, host)
    }

    fn memory_load_vector(&mut self, index: ir::Value, offset: u32) -> ir::Value {
        let host = self.host_address(index, offset);
        let flags = self.load_flags();
        self.raw_load(Lane::V128, flags, host, 0)
    }
}

impl WasmMemoryWrite for Emitter<'_, '_> {
    fn memory_store(&mut self, access: Access, index: ir::Value, offset: u32, value: ir::Value) {
        let host = self.host_address(index, offset);
        let flags = self.flags(Region::LinearMemory);
        self.store_access(access, flags, host, value);
    }

    /// A guarded vector store keeps the trap code of a load (`load_flags`): a proven access
    /// skips the explicit check, so the fault is the store's own.
    fn memory_store_vector(&mut self, index: ir::Value, offset: u32, value: ir::Value) {
        let host = self.host_address(index, offset);
        let flags = self.load_flags();
        self.store_word(flags, value, host, 0);
    }

    fn memory_fill(&mut self, index: ir::Value, byte: ir::Value, length: ir::Value) {
        let address = self.operate(Binary::Add, Width::I64, self.base, index);
        let byte = self.convert(Conversion::ExtendI32Unsigned, byte);
        self.call_leaf(
            compiled_memory_fill as *const () as usize,
            &[address, byte, length],
        );
    }

    fn memory_copy(&mut self, destination: ir::Value, source: ir::Value, length: ir::Value) {
        let destination = self.operate(Binary::Add, Width::I64, self.base, destination);
        let source = self.operate(Binary::Add, Width::I64, self.base, source);
        self.call_leaf(
            compiled_memory_copy as *const () as usize,
            &[destination, source, length],
        );
    }
}

/// The VM's growth of the context's memory by `pages` pages (`Vm::wasm_memory_grow`): its size
/// in pages before, or the failure word where it cannot grow. The memory's length in the
/// context is read again after it, the event that changes it; its base never moves, the bytes
/// being a reservation (`Analysis::callouts` is off for any other memory).
pub(super) unsafe extern "C" fn memory_grow_callout(context: *mut CallContext, pages: u64) -> u64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose VM is suspended in
    // that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm>() };
    // Only the growth itself fails: the analysis admitted the context's memory binding.
    let grown = vm.wasm_memory_grow(context.memory, pages);
    refresh_memory_length(context, vm);
    grown.ok().flatten().unwrap_or(INDEX_GROW_FAILURE as u64)
}

impl WasmMemoryGrow for Emitter<'_, '_> {
    fn memory_grow(&mut self, pages: ir::Value) -> ir::Value {
        let delta = self.memory_index(pages);
        let context = self.body.core.context;
        let roots = self.root();
        let before = self.call_leaf(memory_grow_callout as *const () as usize, &[context, delta]);
        self.unroot(roots);
        self.convert(Conversion::WrapI64, before)
    }
}

impl Emitter<'_, '_> {
    /// The flags of a linear-memory load. Where the reservation guards the access it is the bound
    /// check, so it keeps a trap code: even a load whose result is unused must fault before later
    /// effects (`vector_address`).
    fn load_flags(&self) -> MemFlagsData {
        if !self.guarded {
            return self.flags(Region::LinearMemory);
        }
        MemFlagsData::new()
            .with_trap_code(Some(ir::TrapCode::HEAP_OUT_OF_BOUNDS))
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[Region::LinearMemory as usize]))
    }

    /// The host address of the memory's byte at `index + offset`.
    fn host_address(&mut self, index: ir::Value, offset: u32) -> ir::Value {
        let host = self.operate(Binary::Add, Width::I64, self.base, index);
        let offset = self.constant(Width::I64, i64::from(offset));
        self.operate(Binary::Add, Width::I64, host, offset)
    }

    /// Lowers a linear-memory step. An access's address is a 32-bit index plus an offset plus
    /// its width, which stays inside the guarded reservation.
    pub(super) fn memory(&mut self, instruction: WideInstruction, step: Memory) {
        let (a, c) = (instruction.a(), instruction.c());
        match step {
            Memory::Load(access) => {
                let address = self.read(c, Kind::I32);
                let index = self.memory_index(address);
                let value = infallible(load(self, access, index, instruction.imm(), true));
                self.write_typed(a, access.ty, value);
            }
            Memory::Store(access) => {
                let value = self.read_typed(a, access.ty);
                let address = self.read(c, Kind::I32);
                let index = self.memory_index(address);
                infallible(store(self, access, index, instruction.imm(), value, true));
            }
            Memory::Size => {
                // Compiled memory is admitted only at the default page size
                // (`supports_compiled_memory`, `needs_compiled_memory`), so this is the bound
                // memory's page size.
                let pages = size(self, WASM_PAGE_BITS);
                self.write(a, Kind::I32, pages);
            }
            Memory::Grow => {
                let pages = self.read(c, Kind::I32);
                let before = self.memory_grow(pages);
                self.write(a, Kind::I32, before);
            }
            Memory::Fill => self.memory_fill_step(instruction),
            Memory::Copy => self.memory_copy_step(instruction),
        }
    }

    /// A fill of the linear memory: destination, byte and length registers.
    fn memory_fill_step(&mut self, instruction: WideInstruction) {
        let (destination, _) = instruction.register_pair();
        let destination = self.read(destination, Kind::I32);
        let byte = self.read(instruction.b(), Kind::I32);
        let length = self.read(instruction.c(), Kind::I32);
        infallible(fill(self, destination, byte, length));
    }

    /// A copy within the linear memory: destination, source and length registers.
    fn memory_copy_step(&mut self, instruction: WideInstruction) {
        let (destination, source) = instruction.register_pair();
        let destination = self.read(destination, Kind::I32);
        let source = self.read(source, Kind::I32);
        let length = self.read(instruction.c(), Kind::I32);
        infallible(copy(self, destination, source, length));
    }
}

impl Analysis<'_> {
    /// Checks a linear-memory step reads the function's memory through its register with
    /// 32-bit operands, and applies its register writes. `memory` is the function's memory
    /// register and the slot it binds.
    pub(super) fn transfer_memory(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        step: Memory,
        memory: (Option<u16>, Option<u32>),
    ) -> Option<()> {
        let registers = function.registers;
        let (memory_register, memory_slot) = memory;
        for named in step.memories(instruction).into_iter().flatten() {
            (Some(named) == memory_register && state.get(named, registers) == Some(Slot::Memory))
                .then_some(())?;
        }
        (!step.needs_compiled_memory()
            || memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot)))
        .then_some(())?;
        (!step.needs_callouts() || self.callouts).then_some(())?;
        for operand in step.operands(instruction).into_iter().flatten() {
            (value_kind(state.get(operand, registers)?)? == Kind::I32).then_some(())?;
        }
        match step {
            Memory::Load(access) => state.set(
                instruction.a(),
                registers,
                Slot::Value(Kind::of(access.ty)?),
            ),
            Memory::Store(access) => (value_kind(state.get(instruction.a(), registers)?)?
                == Kind::of(access.ty)?)
            .then_some(()),
            Memory::Size | Memory::Grow => {
                state.set(instruction.a(), registers, Slot::Value(Kind::I32))
            }
            Memory::Fill | Memory::Copy => Some(()),
        }
    }
}
