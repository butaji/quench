//! The Wasm tier's registers, constants, bindings and globals (`staged::wasm_instance`): the
//! instance primitives as Cranelift code, and the lowering and analysis of each `Instance` step.

use super::{Analysis, AnalysisState, Emitter, Kind, Region, Slot};
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineAccess, MachineStage, MachineVector};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_instance::{Instance, Storage, WasmInstance, global_get, global_set, local_register};
use quench_runtime::wasm::conversion::ScalarConversionOperator as Conversion;
use cranelift_codegen::ir;

/// The width of a word held as `storage`.
const fn storage_width(storage: Storage) -> Width {
    match storage {
        Storage::Bits32 => Width::I32,
        Storage::Bits64 | Storage::Handle => Width::I64,
    }
}

/// A global's native slot is read and written in place: its payload is the value the tier
/// computes with, so an access is one load or one store, with no box and no callout.
impl WasmInstance for Emitter<'_, '_> {
    fn global_load(&mut self, slot: u32, storage: Storage) -> ir::Value {
        let address = self.global_address(slot);
        let flags = self.flags(Region::Globals);
        self.load_word(storage_width(storage), flags, address, 0)
    }

    /// A 32-bit payload is stored zero-extended, as the slot holds it (`Storage::narrow`).
    fn global_store(&mut self, slot: u32, storage: Storage, value: ir::Value) {
        let word = match storage {
            Storage::Bits32 => self.convert(Conversion::ExtendI32Unsigned, value),
            Storage::Bits64 => value,
            Storage::Handle => unreachable!("the analysis admits scalar globals only"),
        };
        let address = self.global_address(slot);
        let flags = self.flags(Region::Globals);
        self.store_word(flags, word, address, 0);
    }
}

impl Emitter<'_, '_> {
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
        // A binding moves as the binding (`Slot::Binding`): nothing is loaded, the reader reads its environment slot.
        if step == Instance::Move && matches!(self.plan.operand(pc), Slot::Binding(_)) {
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
                let storage = self.global_storage(slot);
                let value = global_get(self, slot, storage);
                self.write(a, kind, value);
            }
            Instance::GlobalSet => {
                let (slot, kind) = self.global_access(pc);
                let value = self.read(a, kind);
                let storage = self.global_storage(slot);
                global_set(self, slot, storage, value);
            }
            // A binding is known statically per instruction; the globals it names are reached
            // through the context. Constants were defined above.
            Instance::LoadLocal | Instance::StoreLocal => self.local_access(pc, instruction, step),
            Instance::Binding | Instance::Constant | Instance::Fill => {}
        }
    }

    /// A frame local's access: a load defines `a` as the local, a store defines the local as `a`.
    /// Both move the operand's kind (`operand_kind`), so the local's bits keep their Wasm type.
    fn local_access(&mut self, pc: usize, instruction: WideInstruction, step: Instance) {
        let kind = self.operand_kind(pc);
        let local = local_register(self.plan.registers, instruction.imm());
        let (from, to) = match step {
            Instance::StoreLocal => (instruction.a(), local),
            _ => (local, instruction.a()),
        };
        let value = self.read(from, kind);
        self.write(to, kind, value);
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

    fn global_type(&self, slot: u32) -> quench_runtime::WasmType {
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
                let value = self.vector_immediate(bits);
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

impl Analysis<'_> {
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
                    slot @ (Slot::Vector | Slot::Memory32Address | Slot::Binding(_)) => slot,
                    slot => Slot::Value(value_kind(slot)?),
                };
                state.set(a, registers, slot)
            }
            Instance::Binding => self.bind_instance(state, registers, instruction, memory),
            Instance::GlobalGet | Instance::GlobalSet => {
                self.transfer_global(state, registers, instruction, step)
            }
            Instance::LoadLocal | Instance::StoreLocal => self.transfer_local(function, instruction, state, step),
            Instance::Constant | Instance::Fill => unreachable!("defined by their registers"),
        }
    }

    /// A frame local's access: a load gives `a` the kind the local holds, a store gives the local
    /// the kind of `a`. A local lies past the function's registers, so its state is bounded by them
    /// and its locals.
    fn transfer_local(&self, function: &Function, instruction: WideInstruction, state: &mut AnalysisState, step: Instance) -> Option<()> {
        let registers = function.registers;
        let bound = registers.checked_add(function.locals)?;
        let local = local_register(registers, instruction.imm());
        let (from, to) = match step {
            Instance::StoreLocal => (instruction.a(), local),
            _ => (local, instruction.a()),
        };
        let kind = value_kind(state.get(from, bound)?)?;
        state.set(to, bound, Slot::Value(kind))
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
        let ty = self.global_type(slot)?;
        // A reference's global is `Storage::Handle`, which `global_store` admits no store to.
        if matches!(ty, quench_runtime::WasmType::Reference { .. }) {
            return None;
        }
        let kind = Kind::of(ty)?;
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
