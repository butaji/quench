//! The parameter flags a record keeps, which the validator checks: a rest list is never a
//! simple list, and a rest alias (`Function::rest_alias`, `compile::rest_alias`) is no
//! parameter of its function's own.
impl super::Function {
    /// Whether the parameter flags agree: no simple list has a rest parameter, and a rest
    /// alias's function has the shape its compile gives it (`rest_alias_shape`).
    pub(crate) fn parameter_flags_agree(&self) -> bool {
        !(self.rest && self.simple_parameters) && (!self.rest_alias || self.rest_alias_shape())
    }

    /// The shape a rest alias needs: an `arguments` binding, no parameters or rest list of its
    /// own, an unmapped `arguments`, no suspension, and no lexical `arguments` of an arrow.
    fn rest_alias_shape(&self) -> bool {
        self.arguments_slot.is_some()
            && self.params == 0
            && !self.rest
            && !self.simple_parameters
            && !self.is_async
            && !self.is_generator
            && !self.is_arrow
            && !self.is_class_constructor
    }
}

/// The bits of the one byte an encoding packs `unaliased_arguments` and `rest_alias` into.
pub(super) const UNALIASED_ARGUMENTS_BIT: u8 = 1 << 0;
pub(super) const REST_ALIAS_BIT: u8 = 1 << 1;
