//! The host's side of the activation-variable (local, var binding, TDZ, argument) call-out rows (`staged::protocol_owned`): each step runs
//! the interpreter's own helper for its opcode over the running activation.
use super::*;
use crate::staged::protocol_owned::OwnedVarStep;

impl<H: Host> Vm<H> {
    /// The activation-variable (local, var binding, TDZ, argument) `step` of its operand values (`first` is the first operand register's
    /// value, `second` the second's): the value it defines, if any.
    pub(super) fn owned_var_step(&mut self, p: &ResidualProgram, site: super::call_out::CallSite, step: OwnedVarStep, first: Option<Value>, second: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let owner = site.owner;
        match step {
            OwnedVarStep::LoadLocal { slot } => self.load_local_binding(p, owner, slot, None).map(Some),
            OwnedVarStep::StoreVarBinding { slot } => {
                let value = first.unwrap_or(Value::UNDEFINED);
                self.store_var_binding(p, owner, slot, value)?;
                Ok(None)
            }
            OwnedVarStep::StoreLocal { slot, initialize } => {
                let value = first.unwrap_or(Value::UNDEFINED);
                self.store_local_value(p, owner, slot, initialize, value)?;
                Ok(Some(value))
            }
            OwnedVarStep::InitializeTdz { slot } => {
                *self.owner_binding_mut(p, owner, slot).ok_or_else(|| JsError("invalid local slot".into()))? = Value::DELETED;
                Ok(None)
            }
            OwnedVarStep::ArgumentGet { slot } => {
                let binding = self.load_local_binding(p, owner, slot, None)?;
                let key = second.unwrap_or(Value::UNDEFINED);
                let f = self.own_frame();
                self.read_arguments_binding(p, f, binding, Some(key)).map(Some)
            }
        }
    }

    /// The `StoreLocal` store of `value` to local `slot` of `owner`: the function-name, initialization and
    /// constant rules, then the binding with its mirrors. This is the body of `Vm::store_local_step` (`vm/owner_step.rs`),
    /// which takes the whole instruction and so cannot be called from a step that stores only its immediates.
    fn store_local_value(&mut self, p: &ResidualProgram, owner: VarOwner, slot: usize, initialize: Option<bool>, value: Value)
        -> Result<(), JsError> {
        let running = self.owner_view(owner).function();
        let function = &p.function(running as usize);
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
        Ok(())
    }
}
