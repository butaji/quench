//! The legacy `caller` and `arguments` reads of a function that has no such own property.

use super::super::object::AtomClass;
use super::super::{Atom, Host, JsError, Value, Vm};

impl<H: Host> Vm<H> {
    /// `[[Get]]` of `caller` or `arguments` on a function without that own property, as the
    /// legacy reflection answers it; `None` for any other read.
    pub(in crate::vm) fn restricted_function_read(&mut self, object: Value, atom: Atom) -> Option<Result<Value, JsError>> {
        if !self.atom_class(atom).contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY)
            || !self.heap.is_function_value(object)
            || self.own_property(object, atom).is_some()
            || self.function_caller_is_restricted(object)
        {
            return None;
        }
        Some(match self.atom_name(atom) {
            "caller" => self.function_caller_value(object),
            "arguments" => self.function_arguments(object),
            _ => unreachable!("legacy function reflection property"),
        })
    }
}
