//! The local, capture and name steps, which read and write the variables of the activation that
//! runs them (`VarOwner`) and the registers it addresses: made over a frame by the interpreter
//! (`Vm::step`) and over the window of a lane's frameless activation by its body
//! (`dispatch_fast::vm_steps::owner_step_in`), by this one definition (the catalog's `owned` column, `Op::owned_step`).
#[cfg(test)]
#[path = "owner_step_tests.rs"]
mod tests;

use super::*;
use crate::bytecode::{InstructionField, OwnedStep, WideInstruction, NO_REALM_BINDING};
use std::cell::Cell;

/// The atom and realm index slot of `declaration` (the immediate of a `LoadGlobalLexical` or
/// `StoreGlobalLexical`), a position in the root's declarations, which validation bounds.
fn global_lexical_declaration(p: &ResidualProgram, declaration: u32) -> (Atom, &Cell<u16>) {
    p.function(0).global_lexical_declaration(declaration).expect("validated global lexical index")
}

impl Vm {
    /// The step `i` (an `Op::owned_step` op) of the activation `owner` names, whose instruction
    /// resumes at `pc`, which a name lookup takes for its binding site.
    pub fn owner_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        match i.op().owned_step().expect("an owner step is a catalog `owned` row") {
            OwnedStep::LoadLocal => self.load_local_step(p, owner, i),
            OwnedStep::StoreVarBinding => self.store_var_binding_step(p, owner, i),
            OwnedStep::StoreLocal => self.store_local_step(p, owner, i),
            OwnedStep::InitializeTdz => self.initialize_tdz_step(p, owner, i),
            OwnedStep::StoreEnvLocal => self.store_env_local_step(p, owner, i),
            OwnedStep::LoadCapture => self.load_capture_step(p, owner, i),
            OwnedStep::StoreCapture => self.store_capture_step(p, owner, i),
            OwnedStep::LoadName => self.load_name_step(p, owner, i, pc),
            OwnedStep::LoadNameCall => self.load_name_call_step(p, owner, i, pc),
            OwnedStep::LoadNameTypeof => self.load_name_typeof_step(p, owner, i, pc),
            OwnedStep::StoreName => self.store_name_step(p, owner, i, pc),
        }
    }

    /// `LoadGlobalLexical` of `declaration` through cache `cache`, at the binding site `pc` names: the
    /// script-top-level binding the declaration names, read through its realm index slot while that
    /// index holds the name's initialized binding. Any other read (a temporal dead zone, a stale or
    /// unmapped index, an enclosing scope that may declare the name dynamically) takes the by-name read,
    /// which settles it, and then refreshes the slot.
    pub(super) fn global_lexical_load(&mut self, p: &ResidualProgram, owner: VarOwner, (declaration, cache): (u32, u16), pc: usize)
        -> Result<Value, JsError> {
        let (atom, slot) = global_lexical_declaration(p, declaration);
        let static_scopes = self.global_scope_names_resolve_statically(p, owner) && !self.scope_names.declared(atom);
        if static_scopes && let Some(value) = self.global_lexical_at(atom, slot.get()) {
            return Ok(value);
        }
        let value = self.at_binding_site(owner, pc, |vm| vm.load_name(p, owner, atom, Some(cache)))?;
        self.refresh_global_lexical_slot(slot, atom);
        Ok(value)
    }

    /// `StoreGlobalLexical` of `value` to `declaration` through cache `cache`, at the binding site `pc`
    /// names. An initializing store is the script's own declaration, which ends the binding's temporal
    /// dead zone: nothing in the root's scope can shadow it, and the binding exists from instantiation.
    /// Otherwise the write of an initialized, mutable binding through its realm index slot, under the
    /// same guards as the read. Any other write (a temporal dead zone, a const, a stale or unmapped
    /// index, a shadowing scope) takes the by-name store, which makes the errors, and then refreshes
    /// the slot.
    pub(super) fn global_lexical_store(&mut self, p: &ResidualProgram, owner: VarOwner, (declaration, cache, initializing): (u32, u16, bool),
        value: Value, pc: usize) -> Result<(), JsError> {
        let (atom, slot) = global_lexical_declaration(p, declaration);
        if initializing {
            self.initialize_global_lexical(atom, value);
            return Ok(());
        }
        let static_scopes = self.global_scope_names_resolve_statically(p, owner) && !self.scope_names.declared(atom);
        if static_scopes && self.global_lexical_store_at(atom, slot.get(), value) {
            return Ok(());
        }
        self.at_binding_site(owner, pc, |vm| vm.store_name(p, owner, atom, value, cache, initializing))?;
        self.refresh_global_lexical_slot(slot, atom);
        Ok(())
    }

    /// Points a declaration's realm index slot at the active realm's binding of `atom`, or at none.
    pub(super) fn refresh_global_lexical_slot(&self, slot: &Cell<u16>, atom: Atom) {
        let index = self.realm.global_lexical_bindings.get_index_of(&atom).and_then(|index| u16::try_from(index).ok());
        slot.set(index.unwrap_or(NO_REALM_BINDING));
    }

    /// `LoadLocal` and `LoadEnvLocal`: the binding of a local slot, into the result register.
    fn load_local_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.load_local_binding(p, owner, i.local_slot(), None)?;
        self.set_owner_register(owner, i.result_register(), value);
        Ok(())
    }

    /// `InitializeTdz`: the dead-zone marker written to local slot `slot`, wherever the owner keeps the binding
    /// (its frame's slots, or the environment of a captured one).
    fn initialize_tdz_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        *self
            .owner_binding_mut(p, owner, i.local_slot())
            .ok_or_else(|| JsError("invalid local slot".into()))? = Value::DELETED;
        Ok(())
    }

    fn store_var_binding_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.owner_register(owner, i.register_a());
        self.store_var_binding(p, owner, i.local_slot(), value)
    }

    /// `StoreEnvLocal`: the environment slot's store, which also defines the optional register where it initialized the binding.
    fn store_env_local_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.owner_register(owner, i.register_a());
        let initialize = i.boolean_field(InstructionField::C).unwrap_or(false);
        if self.store_env_local(p, owner, i.local_slot(), value, initialize)?
            && let Some(register) = i.optional_register_b()
        {
            self.set_owner_register(owner, register, value);
        }
        Ok(())
    }

    fn load_capture_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.capture(p, owner, i.capture_depth(), i.capture_slot())?;
        self.set_owner_register(owner, i.result_register(), value);
        Ok(())
    }

    fn store_capture_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.owner_register(owner, i.register_a());
        self.store_capture(p, owner, i.capture_depth(), i.capture_slot(), value)
    }

    fn load_name_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let function = self.owner_view(owner).function();
        self.staged_load_name(p, super::call_out::CallSite { owner, function, resume: pc }, i)
    }

    /// `LoadNameCall` at the binding site `pc` names: the callee and its `this`.
    fn load_name_call_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let (atom, cache) = (i.atom_index(), i.cache_site_index());
        let (callee, this) = self.at_binding_site(owner, pc, |vm| vm.load_name_call(p, owner, atom, Some(cache), false))?;
        self.set_owner_register(owner, i.result_register(), callee);
        self.set_owner_register(owner, i.register_b(), this);
        Ok(())
    }

    /// `LoadNameTypeof` at the binding site `pc` names.
    fn load_name_typeof_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let (atom, cache) = (i.atom_index(), i.cache_site_index());
        let value = self.at_binding_site(owner, pc, |vm| vm.load_name_typeof(p, owner, atom, cache))?;
        self.set_owner_register(owner, i.result_register(), value);
        Ok(())
    }

    /// `StoreName` at the binding site `pc` names.
    fn store_name_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction, pc: usize) -> Result<(), JsError> {
        let (atom, cache) = (i.atom_index(), i.cache_site_index());
        let value = self.owner_register(owner, i.register_a());
        let initializing = i.boolean_field(InstructionField::B).expect("validated initialization flag");
        self.at_binding_site(owner, pc, |vm| vm.store_name(p, owner, atom, value, cache, initializing))
    }

    /// `StoreLocal`: the function-name, initialization and constant rules, then the binding.
    fn store_local_step(&mut self, p: &ResidualProgram, owner: VarOwner, i: WideInstruction) -> Result<(), JsError> {
        let value = self.owner_register(owner, i.register_a());
        let slot = i.local_slot();
        let running = self.owner_view(owner).function();
        let function = &p.function(running as usize);
        let initialize = i.boolean_field(InstructionField::C);
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
            self.mapped_argument_store(p, owner, slot, value);
        }
        if let Some(register) = i.optional_register_b() {
            self.set_owner_register(owner, register, value);
        }
        Ok(())
    }
}
