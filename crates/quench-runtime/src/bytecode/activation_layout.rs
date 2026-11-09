//! What a function's activation layout depends on besides its captures, read alike by the
//! compiler's layout (`compile::capture_layout`) and the validator.
use super::super::{Function, Op};

impl Function {
    /// Whether no construction protocol names the function's slots: a derived constructor's
    /// `this` binding, a class initializer's, or a constructor defining instance elements,
    /// whose field initializers resolve the class scope through its environment by name.
    pub(crate) fn construction_is_private(&self) -> bool {
        !self.derived_constructor
            && !self.class_initializer
            && !(self.is_class_constructor
                && self.instructions().any(|instruction| matches!(instruction.op(),
                    Op::DefineField | Op::DefinePrivateField | Op::DefineComputedField | Op::MarkPrivateName)))
    }
}
