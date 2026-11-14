//! The local, capture and name steps, which read and write the variables of the activation that
//! runs them (`VarOwner`) and the registers it addresses: made over a frame by the interpreter
//! (`Vm::step`) and over the window of a lane's frameless activation by its body
//! (`dispatch_fast::vm_steps::lane_owner_step_in`), by this one definition (`Op::steps_on_owner`).
#[cfg(test)]
#[path = "owner_step_tests.rs"]
mod tests;

use super::*;
use crate::bytecode::{Op, WideInstruction};

impl<H: Host> Vm<H> {
    /// The step `i` (an `Op::steps_on_owner` op) of the activation `owner` names, whose instruction
    /// resumes at `pc`, which a name lookup takes for its binding site.
    pub(super) fn owner_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        match i.op() {
            Op::LoadLocal | Op::LoadEnvLocal => {
                let value = self.load_local_binding(p, owner, i.local_slot(), None)?;
                self.set_owner_register(owner, i.result_register(), value);
            }
            Op::StoreVarBinding => {
                let value = self.owner_register(owner, i.register_a());
                self.store_var_binding(p, owner, i.local_slot(), value)?;
            }
            Op::StoreLocal => self.store_local_step(p, owner, i)?,
            Op::StoreEnvLocal => {
                let value = self.owner_register(owner, i.register_a());
                let initialize = i.boolean_field(crate::bytecode::InstructionField::C).unwrap_or(false);
                if self.store_env_local(p, owner, i.local_slot(), value, initialize)?
                    && let Some(register) = i.optional_register_b()
                {
                    self.set_owner_register(owner, register, value);
                }
            }
            Op::LoadCapture => {
                let v = self.capture(p, owner, i.capture_depth(), i.capture_slot())?;
                self.set_owner_register(owner, i.result_register(), v);
            }
            Op::StoreCapture => {
                let value = self.owner_register(owner, i.register_a());
                self.store_capture(p, owner, i.capture_depth(), i.capture_slot(), value)?;
            }
            Op::LoadName => {
                let function = self.owner_view(owner).function();
                self.staged_load_name(p, super::call_out::CallSite { owner, function, resume: pc }, i)?;
            }
            op => self.name_step_on_owner(p, owner, i, pc, op)?,
        }
        Ok(())
    }

    /// `LoadNameCall`, `LoadNameTypeof` and `StoreName` of `owner` at the binding site `pc` names.
    fn name_step_on_owner(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize, op: Op) -> Result<(), JsError> {
        let (atom, cache) = (i.atom_index(), i.cache_site_index());
        match op {
            Op::LoadNameCall => {
                let (callee, this) = self.at_binding_site(owner, pc, |vm| vm.load_name_call(p, owner, atom, Some(cache), false))?;
                self.set_owner_register(owner, i.result_register(), callee);
                self.set_owner_register(owner, i.register_b(), this);
            }
            Op::LoadNameTypeof => {
                let v = self.at_binding_site(owner, pc, |vm| vm.load_name_typeof(p, owner, atom, cache))?;
                self.set_owner_register(owner, i.result_register(), v);
            }
            Op::StoreName => {
                let value = self.owner_register(owner, i.register_a());
                let initializing = i.boolean_field(crate::bytecode::InstructionField::B).expect("validated initialization flag");
                self.at_binding_site(owner, pc, |vm| vm.store_name(p, owner, atom, value, cache, initializing))?;
            }
            _ => unreachable!("{op:?} is no owner step"),
        }
        Ok(())
    }

    /// `StoreLocal`: the function-name, initialization and constant rules, then the binding with its mirrors.
    fn store_local_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.owner_register(owner, i.register_a());
        let slot = i.local_slot();
        let running = self.owner_view(owner).function();
        let function = &p.function(running as usize);
        let initialize = i.boolean_field(crate::bytecode::InstructionField::C);
        if function.is_self_binding_slot(slot) && function.strict {
            return Err(self.type_error(p, "assignment to function name binding".into()));
        }
        if !function.is_self_binding_slot(slot) {
            self.check_local_assignment_initialized(p, owner, slot, initialize.unwrap_or(false))?;
            if running == 0 && initialize == Some(false)
                && function.local_atoms.get(slot).is_some_and(|atom| function.declares_global_immutable(*atom))
            {
                return Err(self.type_error(p, "assignment to constant binding".into()));
            }
            *self.owner_binding_mut(p, owner, slot).ok_or_else(|| JsError("invalid local slot".into()))? = value;
            self.mirror_global_lexical_binding(p, owner, slot, value);
            self.mapped_argument_store(p, owner, slot, value);
        }
        if let Some(register) = i.optional_register_b() {
            self.set_owner_register(owner, register, value);
        }
        Ok(())
    }
}
