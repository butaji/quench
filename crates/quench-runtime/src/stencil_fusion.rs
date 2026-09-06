//! Physical plans selected from bounded canonical instruction windows.
//!
//! Selections own operand wiring and cost facts. JavaScript behavior remains
//! in the canonical instructions and their ordinary fallback handlers.

use crate::machine::NativeBinaryPlan;
use crate::stencil_plan::{LocalBinarySelection, LocalNumericInputs};

pub(crate) struct NativeLocalBinaryPlan {
    selection: LocalBinarySelection,
    binary: NativeBinaryPlan,
}

impl NativeLocalBinaryPlan {
    pub(crate) fn new(
        selection: LocalBinarySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let binary = NativeBinaryPlan::new_with_shared(selection.operation, policy, arena)?;
        Some(Self { selection, binary })
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

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.binary.native_entry_count()
    }
}

pub(crate) fn local_binary_operands(
    environment: &crate::environment::Environment,
    selection: LocalBinarySelection,
) -> Option<(f64, f64)> {
    match selection.inputs {
        LocalNumericInputs::Slots(slots) => Some((
            environment.get_number(slots[0])?,
            environment.get_number(slots[1])?,
        )),
        LocalNumericInputs::RepeatedSlot(slot) => {
            let value = environment.get_number(slot)?;
            Some((value, value))
        }
        LocalNumericInputs::SlotConstant { slot, bits } => {
            Some((environment.get_number(slot)?, f64::from_bits(bits)))
        }
    }
}

pub(crate) fn execute_local_binary(
    plan: &std::cell::RefCell<NativeLocalBinaryPlan>,
    environment: &crate::environment::Environment,
) -> Option<(crate::ir::Register, f64, usize)> {
    let selection = plan.borrow().selection();
    let (lhs, rhs) = local_binary_operands(environment, selection)?;
    let result = plan.borrow_mut().execute(lhs, rhs).ok()?;
    Some((selection.output, result, usize::from(selection.span)))
}
