//! Physical plans selected from bounded canonical instruction windows.
//!
//! Selections own operand wiring and cost facts. JavaScript behavior remains
//! in the canonical instructions and their ordinary fallback handlers.

use crate::machine::NativeBinaryPlan;
use crate::stencil_plan::{LocalBinarySelection, LocalNumericInputs, NumericSource};

pub(crate) struct LocalNumericExecution {
    pub output: crate::ir::Register,
    pub value: f64,
    pub span: usize,
    pub discarded: crate::stencil_plan::DiscardedRegisters,
}

impl LocalNumericExecution {
    pub(crate) fn commit(self, registers: &mut crate::register_file::RegisterFile) -> usize {
        for register in self.discarded.into_iter().flatten() {
            registers.clear_word(usize::from(register));
        }
        registers.write_number(usize::from(self.output), self.value);
        self.span
    }
}

pub(crate) struct NativeLocalBinaryPlan {
    selection: LocalBinarySelection,
    binary: Option<NativeBinaryPlan>,
    #[cfg(test)]
    local_read_count: u64,
}

impl NativeLocalBinaryPlan {
    pub(crate) fn new(
        selection: LocalBinarySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        if !policy.native_leaves {
            return None;
        }
        let binary = match selection.inputs {
            LocalNumericInputs::Folded { .. } => None,
            _ => Some(NativeBinaryPlan::new_with_shared(
                selection.operation,
                policy,
                arena,
            )?),
        };
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
        self.binary
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?
            .execute(lhs, rhs)
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
    ) -> Option<LocalNumericExecution> {
        let result = match self.selection.inputs {
            LocalNumericInputs::Folded { bits } => f64::from_bits(bits),
            _ => {
                let (lhs, rhs) = self.operands(environment)?;
                self.execute(lhs, rhs).ok()?
            }
        };
        Some(LocalNumericExecution {
            output: self.selection.output,
            value: result,
            span: usize::from(self.selection.span),
            discarded: self.selection.discarded,
        })
    }

    fn operands(&mut self, environment: &crate::environment::Environment) -> Option<(f64, f64)> {
        match self.selection.inputs {
            LocalNumericInputs::Sources(sources) => self.read_sources(environment, sources),
            LocalNumericInputs::SlotConstant { slot, bits } => {
                Some((self.read_number(environment, slot)?, f64::from_bits(bits)))
            }
            LocalNumericInputs::Folded { .. } => None,
        }
    }

    fn read_sources(
        &mut self,
        environment: &crate::environment::Environment,
        sources: [NumericSource; 2],
    ) -> Option<(f64, f64)> {
        if sources[0] == sources[1] {
            let value = self.read_source(environment, sources[0])?;
            return Some((value, value));
        }
        Some((
            self.read_source(environment, sources[0])?,
            self.read_source(environment, sources[1])?,
        ))
    }

    fn read_source(
        &mut self,
        environment: &crate::environment::Environment,
        source: NumericSource,
    ) -> Option<f64> {
        match source {
            NumericSource::Local(slot) => self.read_number(environment, slot),
            NumericSource::Constant(bits) => Some(f64::from_bits(bits)),
        }
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.binary
            .as_ref()
            .map_or(0, NativeBinaryPlan::native_entry_count)
    }

    #[cfg(test)]
    pub(crate) fn local_read_count(&self) -> u64 {
        self.local_read_count
    }
}

pub(crate) fn execute_local_binary(
    plan: &std::cell::RefCell<NativeLocalBinaryPlan>,
    environment: &crate::environment::Environment,
) -> Option<LocalNumericExecution> {
    plan.borrow_mut().execute_from_environment(environment)
}
