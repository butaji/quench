//! A Proxy's `[[Set]]` (ECMA-262 10.5.9): what a strict store raises when the `set` trap
//! returns a falsish value.

use super::super::*;
use super::PropertyKey;

impl<H: Host> Vm<H> {
    /// `[[Set]]` of the target, for a handler without a `set` trap: a proxy target runs its
    /// own, any other the ordinary set with `receiver`, whose rejection a strict store raises.
    pub(in crate::vm) fn proxy_set_target(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        receiver: Value,
        property: PropertyKey,
        value: Value,
        strict: bool,
    ) -> Result<bool, JsError> {
        if let Some((inner, handler)) = self.proxy_parts(target) {
            return self.proxy_set_mode(p, inner, handler, receiver, property, value, strict);
        }
        match property {
            PropertyKey::String(atom) => {
                let written = self.set_property_with_receiver(p, target, atom, value, receiver)?;
                if !written && strict {
                    return Err(self.rejected_assignment_error(p, target, atom, true));
                }
                Ok(written)
            }
            PropertyKey::Symbol(key) => self.set_symbol_property_with_receiver(p, target, key, value, receiver),
            PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Set"),
        }
    }

    /// The `TypeError` of a strict store of the property `key` whose `set` trap returned a
    /// falsish value.
    pub(in crate::vm) fn proxy_set_refused_error(&mut self, p: &ResidualProgram, key: Value) -> JsError {
        let property = self.detail_string(key);
        self.type_error(p, format!("'set' on proxy: trap returned falsish for property '{property}'"))
    }
}
