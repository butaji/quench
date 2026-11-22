//! The locals of a lane body that live in its activation's environment window, lowered by the staged environment rows
//! (`staged::environment::environment_slot`, `environment_slot_store`, `environment_slot_define`) on the emit stage,
//! through the storage word the payload keeps (`JsStepDispatch::environment_storage`), which the unit holds in a variable
//! (`Entry::environment`) and loads again only after an operation that makes an environment
//! (`Operation::makes_environment`). The slot is a layout constant and an index; a binding in its dead zone or a slot
//! another environment shares leaves at the instruction, where the interpreter raises its `ReferenceError` or resolves
//! the slot, and only then. A store has no barrier of its own: the activation's environment lives within a lane run,
//! which collects nothing. The slots are those the b-jit makes rows of (`base_region::env_owned`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::lane_unit::Origin;
use quench_runtime::bytecode::InstructionField;
use quench_runtime::staged::environment::{environment_slot, environment_slot_define, environment_slot_open, environment_slot_store};

/// What an instruction does to an environment-owned slot.
#[derive(Clone, Copy)]
enum Access {
    /// Read into a register (`LoadLocal`).
    Read(Register),
    /// Assign (`define` false: `StoreLocal`, `StoreVarBinding`) or give a declaration's first value (`StoreLocal`), also into `result`.
    Write { source: Register, result: Option<Register>, define: bool },
    /// Open the dead zone (`InitializeTdz`).
    Open,
}

impl Lowering<'_, '_> {
    /// The payload's environment storage word.
    pub(crate) fn environment_word(&mut self) -> ir::Value {
        emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.environment_storage))
    }

    /// The storage word again, after an operation that made the environment.
    /// A numeric body (no entry facts) holds no environment word: its locals live in no window.
    pub(crate) fn reload_environment(&mut self) {
        let Some(variable) = self.entry.as_ref().map(|entry| entry.environment) else { return };
        let word = self.environment_word();
        self.builder.def_var(variable, word);
    }

    /// The slot and access of `instruction` where it reaches an environment-owned local of this body's function: a
    /// record of the caller, in a lane body (a loop region's payload names no storage), over registers the unit holds.
    fn window_access(&self, pc: usize, instruction: WideInstruction) -> Option<(u16, Access)> {
        let function = self.program.function(self.function_id as usize);
        if instruction.op().immediate_role() != quench_runtime::bytecode::ImmediateRole::LocalSlot { return None; }
        let slot = u16::try_from(instruction.local_slot()).ok()
            .filter(|slot| super::super::base_region::env_owned(function, self.function_id, usize::from(*slot)))?;
        let caller = matches!(self.unit.origins.get(pc), Some(Origin::Caller(_)));
        if !(caller && self.body_entry() && self.completion.status_and_pc() && instruction.result_flags() == 0) { return None; }
        let held = |register: Register| (register < self.unit.registers).then_some(register);
        let access = match instruction.op().class() {
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadLocal) && !instruction.writes_numeric_local() => Access::Read(held(instruction.result_register())?),
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::StoreLocal) || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::StoreVarBinding) => self.store_access(function, instruction)?,
            _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::InitializeTdz) => Access::Open,
            _ => return None,
        };
        Some((slot, access))
    }

    /// The store of `instruction` to a slot of `function`, over registers the unit holds. A function's Annex B store to its
    /// `var` binding (`StoreVarBinding`) is a store to the slot (`Function::window_reads_locals`); only a program's root
    /// stores through the global object.
    fn store_access(&self, function: &Function, instruction: WideInstruction) -> Option<Access> {
        let held = |register: Register| (register < self.unit.registers).then_some(register);
        let source = held(instruction.register_a())?;
        if function.is_self_binding_slot(instruction.local_slot()) { return None; }
        if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::StoreVarBinding) {
            return function.parent.is_some().then_some(Access::Write { source, result: None, define: false });
        }
        Some(Access::Write {
            source,
            result: instruction.optional_register_b().map_or(Some(None), |register| held(register).map(Some))?,
            define: instruction.boolean_field(InstructionField::C)?,
        })
    }

    /// Whether `instruction` at `pc` is an environment-owned local access this lowering makes.
    pub(crate) fn is_window_row(&self, pc: usize, instruction: WideInstruction) -> bool {
        self.window_access(pc, instruction).is_some()
    }

    /// The access at `pc` by the staged rows, leaving at `pc` where the row's proof fails.
    pub(crate) fn window_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let (slot, access) = self.window_access(pc, instruction).expect("a window row decodes");
        let miss = self.guard_exit(state, pc as u32);
        let storage = self.builder.use_var(self.entry.as_ref().expect("entry facts").environment);
        let written = match access {
            Access::Write { source, .. } => Some(self.tagged(state, source)),
            Access::Read(_) | Access::Open => None,
        };
        let mut stage = Emit { builder: &mut self.builder, property_values: None, gate: None , payload: None};
        let read = match (access, written) {
            (Access::Read(_), _) => environment_slot(&mut stage, storage, slot, miss),
            (Access::Write { define: false, .. }, Some(value)) => environment_slot_store(&mut stage, storage, slot, value, miss).map(|()| value),
            (Access::Write { .. }, Some(value)) => environment_slot_define(&mut stage, storage, slot, value, miss).map(|()| value),
            (_, _) => environment_slot_open(&mut stage, storage, slot, miss).map(|()| storage),
        }.expect("the emit stage continues past its guards");
        match access {
            Access::Read(destination) => self.set_tagged(state, destination, read),
            Access::Write { result: Some(result), .. } => self.set_tagged(state, result, read),
            Access::Write { result: None, .. } | Access::Open => {}
        }
        true
    }
}
