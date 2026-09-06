//! Physical plans selected from bounded canonical instruction windows.
//!
//! Selections own operand wiring and cost facts. JavaScript behavior remains
//! in the canonical instructions and their ordinary fallback handlers.

use crate::machine::{NativeBinaryPlan, NativePropertyPlan};
use crate::stencil_plan::{
    LocalBinarySelection, LocalNumericInputs, LocalPropertySelection, NumericSource,
};

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
    physical: LocalNumericPhysical,
    #[cfg(test)]
    local_read_count: u64,
}

enum LocalNumericPhysical {
    Folded,
    Binary(NativeBinaryPlan),
    AddChain(crate::machine::NativeAddChainPlan),
}

pub(crate) struct LocalPropertyExecution {
    pub output: crate::ir::Register,
    pub bits: u64,
    pub span: usize,
    pub discarded: crate::stencil_plan::DiscardedRegisters,
}

impl LocalPropertyExecution {
    pub(crate) fn commit(
        self,
        registers: &mut crate::register_file::RegisterFile,
    ) -> Option<usize> {
        registers.write_tagged_bits(usize::from(self.output), self.bits)?;
        for register in self.discarded.into_iter().flatten() {
            registers.clear_word(usize::from(register));
        }
        Some(self.span)
    }
}

pub(crate) struct NativeLocalPropertyPlan {
    selection: LocalPropertySelection,
    property: NativePropertyPlan,
    #[cfg(test)]
    local_read_count: u64,
}

impl NativeLocalPropertyPlan {
    pub(crate) fn new(
        selection: LocalPropertySelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let property = NativePropertyPlan::new_with_arena(selection.operation, policy, arena)?;
        Some(Self {
            selection,
            property,
            #[cfg(test)]
            local_read_count: 0,
        })
    }

    fn execute(
        &mut self,
        environment: &crate::environment::Environment,
        invoke: impl FnOnce(&mut NativePropertyPlan, &crate::value::Value) -> Option<u64>,
    ) -> Option<LocalPropertyExecution> {
        if environment.is_deleted_slot(self.selection.receiver_slot) {
            return None;
        }
        let receiver = environment.get(self.selection.receiver_slot);
        #[cfg(test)]
        {
            self.local_read_count = self.local_read_count.saturating_add(1);
        }
        Some(LocalPropertyExecution {
            output: self.selection.output,
            bits: invoke(&mut self.property, &receiver)?,
            span: usize::from(self.selection.span),
            discarded: self.selection.discarded,
        })
    }

    pub(crate) const fn operation_offset(&self) -> usize {
        self.selection.span as usize - 1
    }

    #[cfg(test)]
    pub(crate) const fn selection(&self) -> LocalPropertySelection {
        self.selection
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.property.native_entry_count()
    }

    #[cfg(test)]
    pub(crate) const fn local_read_count(&self) -> u64 {
        self.local_read_count
    }
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
        let physical = match selection.inputs {
            LocalNumericInputs::Folded { .. } => LocalNumericPhysical::Folded,
            LocalNumericInputs::AddChain { bindings, .. } => LocalNumericPhysical::AddChain(
                crate::machine::NativeAddChainPlan::new_embedded_with_arena(
                    policy, arena, bindings,
                )?,
            ),
            _ => LocalNumericPhysical::Binary(NativeBinaryPlan::new_with_shared(
                selection.operation,
                policy,
                arena,
            )?),
        };
        Some(Self {
            selection,
            physical,
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
        let LocalNumericPhysical::Binary(binary) = &mut self.physical else {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        };
        binary.execute(lhs, rhs)
    }

    fn read_number(
        &mut self,
        environment: &crate::environment::Environment,
        slot: u16,
    ) -> Option<f64> {
        if environment.is_deleted_slot(slot) {
            return None;
        }
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
            LocalNumericInputs::AddChain { sources, .. } => {
                let [lhs, rhs, third] = self.read_three_sources(environment, sources)?;
                let LocalNumericPhysical::AddChain(chain) = &mut self.physical else {
                    return None;
                };
                chain.execute(lhs, rhs, third).ok()?
            }
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
            LocalNumericInputs::AddChain { .. } => None,
        }
    }

    fn read_three_sources(
        &mut self,
        environment: &crate::environment::Environment,
        sources: [NumericSource; 3],
    ) -> Option<[f64; 3]> {
        let first = self.read_source(environment, sources[0])?;
        let second = if sources[1] == sources[0] {
            first
        } else {
            self.read_source(environment, sources[1])?
        };
        let third = match sources[..2].iter().position(|source| *source == sources[2]) {
            Some(0) => first,
            Some(1) => second,
            _ => self.read_source(environment, sources[2])?,
        };
        Some([first, second, third])
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
        match &self.physical {
            LocalNumericPhysical::Folded => 0,
            LocalNumericPhysical::Binary(binary) => binary.native_entry_count(),
            LocalNumericPhysical::AddChain(chain) => chain.native_entry_count(),
        }
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<crate::stencil_select::PhysicalStencilView> {
        match &self.physical {
            LocalNumericPhysical::AddChain(chain) => chain.last_native_view(),
            _ => None,
        }
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

pub(crate) fn execute_local_property(
    plan: &std::cell::RefCell<NativeLocalPropertyPlan>,
    environment: &crate::environment::Environment,
    invoke: impl FnOnce(&mut NativePropertyPlan, &crate::value::Value) -> Option<u64>,
) -> Option<LocalPropertyExecution> {
    plan.borrow_mut().execute(environment, invoke)
}
