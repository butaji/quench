use crate::value::FunctionValue;

impl FunctionValue {
    pub(crate) fn code_id(&self) -> crate::machine::CodeId {
        self.code.code_id()
    }
}
