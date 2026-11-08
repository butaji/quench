//! A Proxy's `[[Get]]` (ECMA-262 10.5.8): the handler's `get` trap, called with the target,
//! key and receiver, and the invariant check against the target's own property, which the
//! target's shape settles without a descriptor wherever it holds the key configurable or not
//! at all.

use super::super::object::AtomClass;
use super::super::*;
use super::PropertyKey;

impl<H: Host> Vm<H> {
    /// GetMethod's read of the trap `atom` from `handler`.
    pub(in crate::vm) fn proxy_trap(
        &mut self,
        p: &ResidualProgram,
        handler: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        self.get_property(p, handler, atom)
    }

    pub(in crate::vm) fn proxy_key_value(
        &mut self,
        p: &ResidualProgram,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        match property {
            PropertyKey::String(atom) if !self.is_private_name(atom) => {
                Ok(self.atom_string(atom))
            }
            PropertyKey::Symbol(key) => Ok(key),
            PropertyKey::String(_) | PropertyKey::Private(_) => {
                Err(self.type_error(p, "private member is not present on this object".into()))
            }
        }
    }

    pub(in crate::vm) fn proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        property: PropertyKey,
    ) -> Result<Value, JsError> {
        let key = self.proxy_key_value(p, property)?;
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        self.with_call_roots([target, handler, receiver, key], |vm| {
            let trap = vm.proxy_trap(p, handler, vm.runtime_atoms.get)?;
            if trap.is_null() || trap.is_undefined() {
                return vm.proxy_get_target(p, target, receiver, property, key);
            }
            if !vm.is_function(trap) {
                return Err(vm.type_error(p, "proxy get trap is not callable".into()));
            }
            let result = vm.call_value(p, trap, handler, &[target, key, receiver])?;
            vm.with_call_roots([result], |vm| vm.validate_proxy_get(p, target, key, result))?;
            Ok(result)
        })
    }

    /// `[[Get]]` of the target, for a handler without a `get` trap.
    fn proxy_get_target(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        receiver: Value,
        property: PropertyKey,
        key: Value,
    ) -> Result<Value, JsError> {
        match property {
            PropertyKey::String(atom) => self.get_property_with_receiver(p, target, atom, receiver),
            PropertyKey::Symbol(_) => self.get_symbol_property_with_receiver(p, target, key, receiver),
            PropertyKey::Private(_) => unreachable!("private keys cannot reach Proxy Get"),
        }
    }

    fn validate_proxy_get(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        result: Value,
    ) -> Result<(), JsError> {
        if self.shape_leaves_key_unconstrained(target, key) {
            return Ok(());
        }
        let Some(descriptor) = self.own_property_descriptor_record(p, target, key)? else {
            return Ok(());
        };
        if descriptor.configurable == Some(true) {
            return Ok(());
        }
        if descriptor.has_data_fields()
            && descriptor.writable == Some(false)
            && !self.same_value(descriptor.value.unwrap_or(Value::UNDEFINED), result)
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a different value for a frozen property".into(),
            ));
        }
        if descriptor.has_accessor_fields()
            && descriptor.getter.is_none_or(Value::is_undefined)
            && !result.is_undefined()
        {
            return Err(self.type_error(
                p,
                "proxy get trap returned a value for an accessor without a getter".into(),
            ));
        }
        Ok(())
    }

    /// Whether an ordinary `target`'s shape shows that it holds no own property `key`, or holds
    /// it configurable: then the target constrains no trap result for it, which a descriptor
    /// would show at more cost. `false` where the shape does not answer (an index key, an
    /// arguments object, an exotic or dictionary object, a symbol key).
    fn shape_leaves_key_unconstrained(&mut self, target: Value, key: Value) -> bool {
        let Some(atom) = self.string_key_atom(key) else {
            return false;
        };
        if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX)
            || self.heap.ordinary_object(target).is_none_or(|data| data.is_arguments_object())
        {
            return false;
        }
        let Some(data) = self.shape_property_lookup(target, atom) else {
            return false;
        };
        let shape = data.shape();
        match self.shape_slot(shape, atom) {
            None => true,
            Some(slot) => self.shape_slot_attributes(shape, slot).is_some_and(|attributes| attributes.configurable),
        }
    }
}
