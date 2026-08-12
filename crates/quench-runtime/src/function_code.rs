use crate::{ops::Op, value::FunctionValue};

impl FunctionValue {
    pub(crate) fn ops(&self) -> &[Op] {
        self.code.ops().unwrap_or(&[])
    }
}
