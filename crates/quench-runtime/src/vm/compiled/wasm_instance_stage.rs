//! The Wasm tier's registers, constants, bindings and globals (`staged::wasm_instance`): the
//! instance primitives as Cranelift code, and the lowering and analysis of each `Instance` step.

use super::{
    Analysis, AnalysisState, CallContext, Emitter, Host, INTEGER_TAG, Kind, Region, Slot, Vm,
};
use crate::Value;
use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm_instance::{Instance, Storage, WasmInstance};
use crate::value::ValueLayout;
use cranelift_codegen::ir::{self, BlockArg, InstBuilder, condcodes::IntCC, types};

/// The machine type of a word held as `storage`.
const fn storage_type(storage: Storage) -> ir::Type {
    match storage {
        Storage::Bits32 => types::I32,
        Storage::Bits64 | Storage::Handle => types::I64,
    }
}

/// The global bound at `position` of the running artifact's globals (`CallContext::globals`):
/// the VM and the binding of its cell, which a callout reads and writes through the VM's own
/// access, the barrier of a store included.
///
/// # Safety
/// `context` is the running call's, whose VM is suspended in it.
unsafe fn global_binding<'a, H: Host>(
    context: *mut CallContext,
    position: u64,
) -> (&'a mut Vm<H>, Value) {
    // SAFETY: as the callouts of the tier, which pass their context.
    let context = unsafe { &*context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    let slot = unsafe { &*context.compiled }.globals[position as usize];
    let binding = vm.heap.environment_slot(context.env, slot as usize);
    (vm, binding.expect("a planned global is bound"))
}

/// The payload of the i64 or f64 global at `position`, whose cell holds a box.
unsafe extern "C" fn compiled_global_bits64<H: Host>(context: *mut CallContext, position: u64) -> u64 {
    // SAFETY: the emitter passes the running context and a planned position.
    let (vm, binding) = unsafe { global_binding::<H>(context, position) };
    let slot = vm.wasm_global_load(binding).expect("a planned global loads");
    vm.decode_bits64(slot).expect("an i64 or f64 cell holds a payload")
}

/// Stores `bits`, a payload in the tag space, into the i64 or f64 global at `position`, in a
/// box of its own.
unsafe extern "C" fn compiled_global_store_bits64<H: Host>(
    context: *mut CallContext,
    position: u64,
    bits: u64,
) -> u64 {
    // SAFETY: as `compiled_global_bits64`.
    let (vm, binding) = unsafe { global_binding::<H>(context, position) };
    let slot = vm.encode_bits64(bits);
    vm.wasm_global_store(binding, slot).expect("a planned global stores");
    0
}

/// Stores the handle `word` into the reference or vector global at `position`.
unsafe extern "C" fn compiled_global_store_handle<H: Host>(
    context: *mut CallContext,
    position: u64,
    word: u64,
) -> u64 {
    // SAFETY: as `compiled_global_bits64`.
    let (vm, binding) = unsafe { global_binding::<H>(context, position) };
    vm.wasm_global_store(binding, Value::from_bits(word))
        .expect("a planned global stores");
    0
}

impl<H: Host> WasmInstance for Emitter<'_, '_, H> {
    /// A 64-bit cell holds the payload in place unless it is in the tag space, where it holds
    /// the handle of a box the VM reads.
    fn global_load(&mut self, slot: u32, storage: Storage) -> ir::Value {
        let address = self.global_address(slot);
        let flags = self.flags(Region::Globals);
        let word = self
            .body
            .core
            .builder
            .ins()
            .load(storage_type(storage), flags, address, 0);
        match storage {
            Storage::Bits64 => self.unboxed(slot, word),
            Storage::Bits32 | Storage::Handle => word,
        }
    }

    /// A 32-bit payload's cell holds the integer's encoded value, the tag over the zero-extended
    /// bits; a 64-bit payload in the tag space is boxed by the VM; a handle is stored by the VM,
    /// whose store is the barrier.
    fn global_store(&mut self, slot: u32, storage: Storage, value: ir::Value) {
        match storage {
            Storage::Bits32 => {
                let wide = self.body.core.builder.ins().uextend(types::I64, value);
                let encoded = self
                    .body
                    .core
                    .builder
                    .ins()
                    .bor_imm_u(wide, INTEGER_TAG as i64);
                self.store_cell(slot, encoded);
            }
            Storage::Bits64 => self.store_bits64(slot, value),
            Storage::Handle => {
                let target = compiled_global_store_handle::<H> as *const () as usize;
                self.call_global(slot, target, &[value]);
            }
        }
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The flag that `word` is in the tag space: a handle, or another tagged value.
    fn in_tag_space(&mut self, word: ir::Value) -> ir::Value {
        let builder = &mut self.body.core.builder;
        let field = builder.ins().ushr_imm_u(word, i64::from(ValueLayout::TAG_SHIFT));
        let offset = builder
            .ins()
            .iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
        builder.ins().icmp_imm_u(
            IntCC::UnsignedLessThanOrEqual,
            offset,
            ValueLayout::TAG_FIELD_SPAN as i64,
        )
    }

    /// A 64-bit cell's `word` as the payload: itself in place, else the box the VM reads.
    fn unboxed(&mut self, slot: u32, word: ir::Value) -> ir::Value {
        let boxed = self.in_tag_space(word);
        let builder = &mut self.body.core.builder;
        let (read, join) = (builder.create_block(), builder.create_block());
        builder.append_block_param(join, types::I64);
        builder
            .ins()
            .brif(boxed, read, &[], join, &[BlockArg::Value(word)]);
        builder.set_cold_block(read);
        builder.switch_to_block(read);
        builder.seal_block(read);
        let payload = self.call_global(slot, compiled_global_bits64::<H> as *const () as usize, &[]);
        let builder = &mut self.body.core.builder;
        builder.ins().jump(join, &[BlockArg::Value(payload)]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        builder.block_params(join)[0]
    }

    /// Stores a 64-bit payload: in place unless it is in the tag space, where the VM boxes it.
    fn store_bits64(&mut self, slot: u32, bits: ir::Value) {
        let boxed = self.in_tag_space(bits);
        let builder = &mut self.body.core.builder;
        let (boxing, direct, done) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        builder.ins().brif(boxed, boxing, &[], direct, &[]);
        builder.set_cold_block(boxing);
        builder.switch_to_block(boxing);
        builder.seal_block(boxing);
        self.call_global(slot, compiled_global_store_bits64::<H> as *const () as usize, &[bits]);
        let builder = &mut self.body.core.builder;
        builder.ins().jump(done, &[]);
        builder.switch_to_block(direct);
        builder.seal_block(direct);
        self.store_cell(slot, bits);
        let builder = &mut self.body.core.builder;
        builder.ins().jump(done, &[]);
        builder.switch_to_block(done);
        builder.seal_block(done);
    }

    /// Writes `encoded` as the whole slot of the global bound at `slot`.
    fn store_cell(&mut self, slot: u32, encoded: ir::Value) {
        let address = self.global_address(slot);
        let flags = self.flags(Region::Globals);
        self.body
            .core
            .builder
            .ins()
            .store(flags, encoded, address, 0);
    }

    /// A call of the VM callout at `target` with the context, the global's position and
    /// `arguments`: all words, answering one.
    fn call_global(&mut self, slot: u32, target: usize, arguments: &[ir::Value]) -> ir::Value {
        let position = self.global_position(slot) as i64;
        let position = self.body.core.builder.ins().iconst(types::I64, position);
        let mut words = vec![self.body.core.context, position];
        words.extend_from_slice(arguments);
        self.call_leaf(target, &words)
    }

    /// Lowers a register, constant, binding or global step.
    pub(super) fn instance(&mut self, pc: usize, instruction: WideInstruction, step: Instance) {
        let (a, b) = (instruction.a(), instruction.b());
        if let Some(registers) = step.constant_registers(instruction) {
            self.define_constants(registers, instruction);
            return;
        }
        if step == Instance::Move && self.plan.operand(pc) == Slot::Vector {
            let value = self.read_vector(b);
            self.write_vector(a, value);
            return;
        }
        match step {
            Instance::Move => {
                let kind = self.operand_kind(pc);
                let value = self.read(b, kind);
                self.write(a, kind, value);
            }
            Instance::GlobalGet => {
                let (slot, kind) = self.global_access(pc);
                let value = self.global_load(slot, self.global_storage(slot));
                self.write(a, kind, value);
            }
            Instance::GlobalSet => {
                let (slot, kind) = self.global_access(pc);
                let value = self.read(a, kind);
                self.global_store(slot, self.global_storage(slot), value);
            }
            // A binding is known statically per instruction; the globals it names are reached
            // through the context. Constants were defined above.
            Instance::Binding | Instance::Constant | Instance::Fill => {}
        }
    }

    /// The global the access at `pc` names and the kind of the register it moves.
    fn global_access(&self, pc: usize) -> (u32, Kind) {
        let slot = self.binding_slot(pc);
        (slot, Kind::of(self.global_type(slot)).expect("planned"))
    }

    /// How the global bound at `slot` holds its value.
    fn global_storage(&self, slot: u32) -> Storage {
        Storage::of(self.global_type(slot))
    }

    fn global_type(&self, slot: u32) -> crate::WasmType {
        self.analysis.global_type(slot).expect("planned")
    }

    /// Defines each of `registers` as the instruction's constant: a vector's bits, else a scalar.
    fn define_constants(&mut self, registers: std::ops::Range<u16>, instruction: WideInstruction) {
        let constant = self
            .analysis
            .constant(instruction.constant_index())
            .expect("planned");
        if let Some(bits) = self.analysis.constant_vector(constant) {
            for register in registers {
                let value = self.vector_constant(bits);
                self.write_vector(register, value);
            }
            return;
        }
        let kind = self.analysis.constant_kind(constant);
        let bits = self.analysis.constant_bits(constant);
        for register in registers {
            self.write_constant(register, kind, bits);
        }
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Checks a register, constant, binding or global step has a compiled denotation in `state`
    /// and applies its register writes. A global is admitted where its type is a scalar the
    /// tier holds in a register: a reference or a vector stays interpreted.
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
                self.transfer_global(state, registers, instruction, step)
            }
            Instance::Constant | Instance::Fill => unreachable!("defined by their registers"),
        }
    }

    /// A global access reads the global its `b` register binds, whose type's kind is that of
    /// the register it moves.
    fn transfer_global(
        &self,
        state: &mut AnalysisState,
        registers: u16,
        instruction: WideInstruction,
        step: Instance,
    ) -> Option<()> {
        let Slot::Binding(slot) = state.get(instruction.b(), registers)? else {
            return None;
        };
        let kind = Kind::of(self.global_type(slot)?)?;
        if step == Instance::GlobalGet {
            return state.set(instruction.a(), registers, Slot::Value(kind));
        }
        (value_kind(state.get(instruction.a(), registers)?)? == kind).then_some(())
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
