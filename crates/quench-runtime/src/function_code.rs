use crate::{ops::Op, value::FunctionValue};

impl FunctionValue {
    pub(crate) fn ops(&self) -> &[Op] {
        self.code
            .as_ref()
            .and_then(crate::machine::FunctionCode::ops)
            .unwrap_or(&[])
    }
}
