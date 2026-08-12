use std::rc::Rc;

use crate::module_bindings::ModuleBindingCell;
use crate::{ops::Op, value::Value};

use super::VmContext;

/// Reusable lexical scope for independently reduced residual programs.
#[derive(Clone)]
pub struct ExecutionScope(Rc<crate::environment::Environment>);

impl ExecutionScope {
    pub fn new() -> Self {
        Self(crate::environment::Environment::new())
    }

    pub fn execute(
        &self,
        ops: &[Op],
        registers: &mut Vec<Value>,
        context: &VmContext,
    ) -> Result<Value, crate::execute::VmError> {
        super::execute_in_environment(ops, registers, context, Rc::clone(&self.0))
    }

    /// Install a live module binding before executing a residual unit.
    pub fn bind_module(&self, name: &str, cell: ModuleBindingCell) {
        self.0.alias_module_binding(name, cell);
    }
}

impl Default for ExecutionScope {
    fn default() -> Self {
        Self::new()
    }
}
