//! The heap's environment accessors: the bindings and slots of the environment cells.
use super::*;

impl Heap {
    pub(crate) fn environment_binding_owner(&self, environment: Value) -> Option<Value> {
        match self.get(environment)? {
            Cell::Environment { scope, .. } => match scope.dynamic_bindings() {
                EnvironmentBindings::Owned(_) => Some(environment),
                EnvironmentBindings::Shared(owner) => Some(*owner),
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings(&self, environment: Value) -> Option<&Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get(owner)? {
            Cell::Environment { scope, .. } => match scope.dynamic_bindings() {
                EnvironmentBindings::Owned(bindings) => Some(bindings),
                EnvironmentBindings::Shared(_) => None,
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings_mut(
        &mut self,
        environment: Value,
    ) -> Option<&mut Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get_mut(owner)? {
            Cell::Environment { scope, .. } => match &mut scope.make_mut().dynamic_bindings {
                EnvironmentBindings::Owned(bindings) => Some(bindings),
                EnvironmentBindings::Shared(_) => None,
            },
            _ => None,
        }
    }

    pub(crate) fn environment_slot_owner(&self, environment: Value, slot: usize) -> Option<Value> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        match slots.state(slot)? {
            EnvironmentSlotState::Owned(_) => Some(environment),
            EnvironmentSlotState::Shared(owner) => Some(owner),
        }
    }

    pub(crate) fn environment_slot(&self, environment: Value, slot: usize) -> Option<Value> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        match slots.state(slot)? {
            EnvironmentSlotState::Owned(value) => Some(value),
            // A shared slot names its owner, which holds the slot itself.
            EnvironmentSlotState::Shared(owner) => {
                let Cell::Environment { slots, .. } = self.get(owner)? else {
                    return None;
                };
                slots.owned(slot)
            }
        }
    }

    pub(crate) fn environment_slot_mut(
        &mut self,
        environment: Value,
        slot: usize,
    ) -> Option<&mut Value> {
        let owner = self.environment_slot_owner(environment, slot)?;
        let Cell::Environment { slots, .. } = self.get_mut(owner)? else {
            return None;
        };
        slots.owned_mut(slot)
    }

    /// A per-iteration copy of `environment`: the `fresh` slots own the current values,
    /// the `reset` ones own `undefined`, and the rest share the slot's owner.
    pub(crate) fn clone_environment_slots(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
    ) -> Option<EnvironmentSlots> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        slots.iteration_copy(environment, fresh, reset, |owner, index| {
            let Cell::Environment { slots, .. } = self.get(owner)? else {
                return None;
            };
            slots.owned(index)
        })
    }
}
