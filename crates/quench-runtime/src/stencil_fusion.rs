//! Physical plans selected from bounded canonical instruction windows.
//!
//! Selections own operand wiring and cost facts. JavaScript behavior remains
//! in the canonical instructions and their ordinary fallback handlers.

use crate::machine::NativeBinaryPlan;
use crate::stencil_plan::{LocalBinarySelection, LocalNumericInputs};

pub(crate) struct NativeLocalBinaryPlan {
    selection: LocalBinarySelection,
    binary: NativeBinaryPlan,
    #[cfg(test)]
    local_read_count: u64,
}

impl NativeLocalBinaryPlan {
    pub(crate) fn new(
        selection: LocalBinarySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let binary = NativeBinaryPlan::new_with_shared(selection.operation, policy, arena)?;
        Some(Self {
            selection,
            binary,
            #[cfg(test)]
            local_read_count: 0,
        })
    }

    pub(crate) const fn selection(&self) -> LocalBinarySelection {
        self.selection
    }

    pub(crate) fn execute(
        &mut self,
        lhs: f64,
        rhs: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        self.binary.execute(lhs, rhs)
    }

    fn read_number(
        &mut self,
        environment: &crate::environment::Environment,
        slot: u16,
    ) -> Option<f64> {
        #[cfg(test)]
        {
            self.local_read_count = self.local_read_count.saturating_add(1);
        }
        environment.get_number(slot)
    }

    fn execute_from_environment(
        &mut self,
        environment: &crate::environment::Environment,
    ) -> Option<(crate::ir::Register, f64, usize)> {
        let (lhs, rhs) = self.operands(environment)?;
        let result = self.execute(lhs, rhs).ok()?;
        Some((
            self.selection.output,
            result,
            usize::from(self.selection.span),
        ))
    }

    fn operands(&mut self, environment: &crate::environment::Environment) -> Option<(f64, f64)> {
        match self.selection.inputs {
            LocalNumericInputs::Slots(slots) => Some((
                self.read_number(environment, slots[0])?,
                self.read_number(environment, slots[1])?,
            )),
            LocalNumericInputs::RepeatedSlot(slot) => {
                let value = self.read_number(environment, slot)?;
                Some((value, value))
            }
            LocalNumericInputs::SlotConstant { slot, bits } => {
                Some((self.read_number(environment, slot)?, f64::from_bits(bits)))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.binary.native_entry_count()
    }

    #[cfg(test)]
    pub(crate) fn local_read_count(&self) -> u64 {
        self.local_read_count
    }
}

pub(crate) fn execute_local_binary(
    plan: &std::cell::RefCell<NativeLocalBinaryPlan>,
    environment: &crate::environment::Environment,
) -> Option<(crate::ir::Register, f64, usize)> {
    plan.borrow_mut().execute_from_environment(environment)
}
