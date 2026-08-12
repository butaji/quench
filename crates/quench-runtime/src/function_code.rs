use crate::{ops::Op, value::FunctionValue};

impl FunctionValue {
    pub(crate) fn ops(&self) -> &[Op] {
        self.code.ops().unwrap_or(&[])
    }

    pub(crate) fn code_id(&self) -> crate::machine::CodeId {
        self.code.code_id()
    }
}
