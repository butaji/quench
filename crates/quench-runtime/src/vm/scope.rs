use std::{cell::RefCell, rc::Rc};

use crate::module_bindings::ModuleBindingCell;
use crate::{ops::Op, value::Value};

use super::VmContext;

/// Reusable lexical scope for independently reduced residual programs.
#[derive(Clone)]
pub struct ExecutionScope(
    Rc<crate::environment::Environment>,
    Rc<RefCell<Option<Rc<crate::environment::Environment>>>>,
);

impl ExecutionScope {
    pub fn new() -> Self {
        Self(
            crate::environment::Environment::new(),
            Rc::new(RefCell::new(None)),
        )
    }

    pub fn execute(
        &self,
        ops: &[Op],
        registers: &mut Vec<Value>,
        context: &VmContext,
    ) -> Result<Value, crate::execute::VmError> {
        super::execute_in_environment(ops, registers, context, Rc::clone(&self.0))
    }

    pub fn execute_completion(
        &self,
        ops: &[Op],
        registers: &mut Vec<Value>,
        context: &VmContext,
    ) -> Result<crate::completion::Completion, crate::execute::VmError> {
        super::execute_frame_completion(ops, registers, context, Rc::clone(&self.0))
    }

    pub fn execute_completion_step(
        &self,
        ops: &[Op],
        registers: &mut Vec<Value>,
        context: &VmContext,
    ) -> Result<(crate::completion::Completion, usize), crate::execute::VmError> {
        let environment = self
            .1
            .borrow_mut()
            .get_or_insert_with(|| crate::environment::Environment::child(&self.0, Vec::new()))
            .clone();
        let step =
            super::execute_completion_step_in_environment(ops, registers, context, environment)?;
        Ok((step.completion, step.next))
    }

    /// Install a live module binding before executing a residual unit.
    pub fn bind_module(&self, name: &str, cell: ModuleBindingCell) {
        self.0.alias_module_binding(name, cell);
    }

    /// Install a live cell at a reducer-assigned lexical slot.
    pub fn bind_module_slot(&self, slot: u16, cell: ModuleBindingCell) {
        self.0.install_slot_cell(slot, cell.shared());
    }

    /// Obtain the live cell backing a reducer-assigned export slot.
    pub fn module_cell_slot(&self, slot: u16) -> ModuleBindingCell {
        ModuleBindingCell::from_shared(self.0.slot_cell(slot))
    }

    pub fn is_uninitialized_slot(&self, slot: u16) -> bool {
        self.0.is_uninitialized(slot)
    }
}

impl Default for ExecutionScope {
    fn default() -> Self {
        Self::new()
    }
}
