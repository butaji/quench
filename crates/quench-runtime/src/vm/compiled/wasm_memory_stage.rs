//! The Wasm tier's linear memory: the primitives of `WasmMemory` as Cranelift code, and the
//! lowering and analysis of each `Memory` step through the staged rules, so an access traps
//! where the interpreter's does.

use super::wasm_instance_stage::value_kind;
use super::wasm_stage::infallible;
use super::{
    Analysis, AnalysisState, CONTEXT_MEMORY_LENGTH, CallContext, Emitter, Host, Kind, Region, Slot,
    Vm, compiled_memory_copy, compiled_memory_fill, refresh_memory_length, scalar_ir,
};
use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm_memory::{
    Access, Memory, WasmMemory, WasmMemoryGrow, WasmMemoryWrite, copy, fill, load, size, store,
};
use crate::wasm::INDEX_GROW_FAILURE;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, Signature, types};

/// The byte widths of the narrow accesses.
const BYTE: usize = size_of::<u8>();
const HALF: usize = size_of::<u16>();
const WORD: usize = size_of::<u32>();

impl<H: Host> WasmMemory for Emitter<'_, '_, H> {
    fn guarded(&self) -> bool {
        self.guarded
    }

    /// The length the function received, while no callout can grow the memory; else the
    /// context's, which a callout refreshes.
    fn memory_length(&mut self) -> ir::Value {
        match self.length {
            Some(length) if !self.analysis.callouts => length,
            _ => self.body.core.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.body.core.context,
                CONTEXT_MEMORY_LENGTH,
            ),
        }
    }

    fn memory_index(&mut self, address: ir::Value) -> ir::Value {
        self.zero_extend(address)
    }

    fn memory_load(&mut self, access: Access, index: ir::Value, offset: u32) -> ir::Value {
        let host = self.host_address(index, offset);
        let flags = self.load_flags();
        let kind = Kind::of(access.ty).expect("a scalar access");
        let ty = kind.ir();
        let ins = self.body.core.builder.ins();
        match (access.bytes, access.signed) {
            // A float loads straight into its view.
            _ if access.ty != kind.integer() => ins.load(scalar_ir(access.ty), flags, host, 0),
            (BYTE, false) => ins.uload8(ty, flags, host, 0),
            (BYTE, true) => ins.sload8(ty, flags, host, 0),
            (HALF, false) => ins.uload16(ty, flags, host, 0),
            (HALF, true) => ins.sload16(ty, flags, host, 0),
            (WORD, false) if kind == Kind::I64 => ins.uload32(flags, host, 0),
            (WORD, true) if kind == Kind::I64 => ins.sload32(flags, host, 0),
            _ => ins.load(ty, flags, host, 0),
        }
    }
}

impl<H: Host> WasmMemoryWrite for Emitter<'_, '_, H> {
    fn memory_store(&mut self, access: Access, index: ir::Value, offset: u32, value: ir::Value) {
        let host = self.host_address(index, offset);
        let flags = self.flags(Region::LinearMemory);
        let kind = Kind::of(access.ty).expect("a scalar access");
        let ins = self.body.core.builder.ins();
        match access.bytes {
            BYTE => ins.istore8(flags, value, host, 0),
            HALF => ins.istore16(flags, value, host, 0),
            WORD if kind == Kind::I64 => ins.istore32(flags, value, host, 0),
            _ => ins.store(flags, value, host, 0),
        };
    }

    fn memory_fill(&mut self, index: ir::Value, byte: ir::Value, length: ir::Value) {
        let address = self.body.core.builder.ins().iadd(self.base, index);
        let byte = self.body.core.builder.ins().uextend(types::I64, byte);
        self.call_leaf(
            compiled_memory_fill as *const () as usize,
            &[address, byte, length],
        );
    }

    fn memory_copy(&mut self, destination: ir::Value, source: ir::Value, length: ir::Value) {
        let destination = self.body.core.builder.ins().iadd(self.base, destination);
        let source = self.body.core.builder.ins().iadd(self.base, source);
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
unsafe extern "C" fn memory_grow_callout<H: Host>(context: *mut CallContext, pages: u64) -> u64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose VM is suspended in
    // that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    // Only the growth itself fails: the analysis admitted the context's memory binding.
    let grown = vm.wasm_memory_grow(context.memory, pages);
    refresh_memory_length(context, vm);
    grown.ok().flatten().unwrap_or(INDEX_GROW_FAILURE as u64)
}

impl<H: Host> WasmMemoryGrow for Emitter<'_, '_, H> {
    fn memory_grow(&mut self, pages: ir::Value) -> ir::Value {
        let delta = self.memory_index(pages);
        let context = self.body.core.context;
        let before = self.call_leaf(memory_grow_callout::<H> as *const () as usize, &[context, delta]);
        self.body.core.builder.ins().ireduce(types::I32, before)
    }
}

impl<H: Host> Emitter<'_, '_, H> {
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
        let host = self.body.core.builder.ins().iadd(self.base, index);
        self.body
            .core
            .builder
            .ins()
            .iadd_imm_s(host, i64::from(offset))
    }

    /// A call of a leaf: words in, a word out (the status of a checked range or a stored
    /// word, which never fails, or a growth's or a load's result).
    pub(super) fn call_leaf(&mut self, leaf: usize, arguments: &[ir::Value]) -> ir::Value {
        let mut signature = Signature::new(self.call_conv);
        for _ in arguments {
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I64));
        let signature = self.body.core.builder.import_signature(signature);
        let target = self.body.core.builder.ins().iconst(types::I64, leaf as i64);
        let call = self
            .body
            .core
            .builder
            .ins()
            .call_indirect(signature, target, arguments);
        self.body.core.builder.inst_results(call)[0]
    }

    /// Lowers a linear-memory step. An access's address is a 32-bit index plus an offset plus
    /// its width, which stays inside the guarded reservation.
    pub(super) fn memory(&mut self, instruction: WideInstruction, step: Memory) {
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
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
                let pages = size(self);
                self.write(a, Kind::I32, pages);
            }
            Memory::Grow => {
                let pages = self.read(c, Kind::I32);
                let before = self.memory_grow(pages);
                self.write(a, Kind::I32, before);
            }
            Memory::Fill => {
                let (destination, _) = instruction.register_pair();
                let destination = self.read(destination, Kind::I32);
                let byte = self.read(b, Kind::I32);
                let length = self.read(c, Kind::I32);
                infallible(fill(self, destination, byte, length));
            }
            Memory::Copy => {
                let (destination, source) = instruction.register_pair();
                let destination = self.read(destination, Kind::I32);
                let source = self.read(source, Kind::I32);
                let length = self.read(c, Kind::I32);
                infallible(copy(self, destination, source, length));
            }
        }
    }
}

impl<H: Host> Analysis<'_, H> {
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
