use std::rc::Rc;

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
}

impl Default for ExecutionScope {
    fn default() -> Self {
        Self::new()
    }
}
