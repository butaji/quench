//! A Proxy's `[[Get]]` (ECMA-262 10.5.8): the handler's `get` trap, called with the target,
//! key and receiver, and the invariant check against the target's own property, which the
//! target's shape settles without a descriptor wherever it holds the key configurable or not
//! at all.

use super::super::object::AtomClass;
use super::super::*;
use super::PropertyKey;
use crate::vm::proxy::ProxyTrap;

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
        if !matches!(property, PropertyKey::Private(_)) && self.handler_lacks_trap(handler, ProxyTrap::Get) {
            return self.proxy_get_target(p, target, receiver, Some(property), Value::UNDEFINED);
        }
        let key = self.proxy_key_value(p, property)?;
        self.proxy_get_keyed(p, target, handler, receiver, key, Some(property))
    }

    /// `proxy_get` of a key that is already a string or symbol value, as a computed read
    /// makes it: its atom, which only a handler without a `get` trap needs, is found then.
    pub(in crate::vm) fn proxy_get_key_value(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
    ) -> Result<Value, JsError> {
        self.proxy_get_keyed(p, target, handler, receiver, key, None)
    }

    fn proxy_get_keyed(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        handler: Value,
        receiver: Value,
        key: Value,
        property: Option<PropertyKey>,
    ) -> Result<Value, JsError> {
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

    /// The atom of the string key `key`.
    pub(in crate::vm) fn key_atom(&mut self, p: &ResidualProgram, key: Value) -> Result<Atom, JsError> {
        match self.string_key_atom(key) {
            Some(atom) => Ok(atom),
            None => {
                let name = self.coerce_js_string(p, key)?;
                Ok(self.intern_js_atom(&name))
            }
        }
    }

    /// `[[Get]]` of the target, for a handler without a `get` trap.
    fn proxy_get_target(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        receiver: Value,
        property: Option<PropertyKey>,
        key: Value,
    ) -> Result<Value, JsError> {
        let property = match property {
            Some(property) => property,
            None if matches!(self.heap.get(key), Some(Cell::Symbol(_))) => PropertyKey::symbol(key),
            None => PropertyKey::string(self.key_atom(p, key)?),
        };
        match property {
            PropertyKey::String(atom) => self.get_property_with_receiver(p, target, atom, receiver),
            PropertyKey::Symbol(symbol) => self.get_symbol_property_with_receiver(p, target, symbol, receiver),
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

    /// Whether `target`'s shape shows that it holds no own property `key`, or holds it
    /// configurable: then the target constrains no trap result for it, which a descriptor
    /// would show at more cost. `false` where the shape does not answer (a symbol key, see
    /// `shape_leaves_atom_unconstrained`).
    pub(in crate::vm) fn shape_leaves_key_unconstrained(&mut self, target: Value, key: Value) -> bool {
        if self.array_leaves_index_unconstrained(target, key) {
            return true;
        }
        match self.string_key_atom(key) {
            Some(atom) => self.shape_leaves_atom_unconstrained(target, atom),
            None => false,
        }
    }

    /// Whether `key` is a string naming an index of an array whose elements all have default
    /// attributes: an extensible array no descriptor was recorded an index for.
    fn array_leaves_index_unconstrained(&self, target: Value, key: Value) -> bool {
        self.heap.array(target).is_some()
            && self.object_data(target).is_some_and(|data| data.is_extensible() && !data.has_indexed_descriptors())
            && self.heap.string(key).is_some_and(|text| super::super::object_static::array_index(text.host_string()).is_some())
    }

    /// `shape_leaves_key_unconstrained` of the string key `atom`, for a target whose shape
    /// holds every own name: an ordinary object, an array (whose elements and `length` the
    /// shape lookup excludes) or a collection. `false` for an index key, an arguments
    /// object, a dictionary-shaped target or any other exotic one.
    pub(in crate::vm) fn shape_leaves_atom_unconstrained(&self, target: Value, atom: Atom) -> bool {
        if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) || !self.shape_names_own_keys(target) {
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

    /// Whether `target`'s shape holds each own name it has beyond the virtual ones
    /// `shape_property_lookup` excludes and the indices it leaves to the caller.
    fn shape_names_own_keys(&self, target: Value) -> bool {
        match self.heap.ordinary_object(target) {
            Some(data) => !data.is_arguments_object(),
            None => {
                self.heap.array(target).is_some()
                    || matches!(
                        self.heap.get(target),
                        Some(Cell::Map { .. } | Cell::Set { .. } | Cell::WeakMap { .. } | Cell::WeakSet { .. })
                    )
            }
        }
    }
}
