//! A Proxy's `[[Get]]` (ECMA-262 10.5.8): the handler's `get` trap, called with the target,
//! key and receiver, and the invariant check against the target's own property, which the
//! target's shape settles without a descriptor wherever it holds the key configurable or not
//! at all.

use super::super::object::AtomClass;
use super::super::*;
use super::PropertyKey;
use crate::vm::proxy::ProxyTrap;

impl Vm {
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
            PropertyKey::String(_) | PropertyKey::Private(..) => {
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
        if !matches!(property, PropertyKey::Private(..)) && self.handler_lacks_trap(handler, ProxyTrap::Get) {
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
            None if self.heap.is_symbol(key) => PropertyKey::symbol(key),
            None => PropertyKey::string(self.key_atom(p, key)?),
        };
        match property {
            PropertyKey::String(atom) => self.get_property_with_receiver(p, target, atom, receiver),
            PropertyKey::Symbol(symbol) => self.get_symbol_property_with_receiver(p, target, symbol, receiver),
            PropertyKey::Private(..) => unreachable!("private keys cannot reach Proxy Get"),
        }
    }

    pub fn validate_proxy_get(
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
            // `has_indexed_descriptors` is record data: the descriptors an index was recorded for.
            && self.heap.object(target).is_some_and(|data| data.is_extensible() && !data.record().has_indexed_descriptors())
            && self.heap.string(key).is_some_and(|text| super::super::object_static::array_index(text.host_string()).is_some())
    }

    /// `shape_leaves_key_unconstrained` of the string key `atom`, for a target whose shape
    /// holds every own name: an ordinary object, an array (whose elements and `length` the
    /// shape lookup excludes) or a collection. `false` for an index key, an arguments
    /// object, a dictionary-shaped target or any other exotic one.
    pub fn shape_leaves_atom_unconstrained(&self, target: Value, atom: Atom) -> bool {
        // The fact table's word for the target's layout says whether its shape holds the name
        // and in which slot, without a shape lookup, ahead of the array and index checks: a
        // plain named key is no index, and a non-arguments ordinary object names its own keys
        // on its shape.
        // A collection, date, regexp, promise or iterator holds its own names on its shape as
        // an ordinary object does (`Heap::ordinary_builtin_object`).
        let facts = self
            .heap
            .ordinary_object(target)
            .or_else(|| self.heap.ordinary_builtin_object(target))
            .filter(|data| !data.is_arguments_object() && !data.has_side_descriptors() && self.plain_named_key(atom))
            .and_then(|data| Some((data.layout_id(), super::super::PropertyKey::string(atom).key32()?, data)));
        if let Some((layout, key32, data)) = facts
            && let Some(word) = self.symbol_load_proofs.word(layout, key32)
        {
            use super::super::symbol_load_proof::{KIND_ABSENT, KIND_ABSENT_OWN, KIND_GETTER, KIND_GETTER_PAIR, KIND_GETTER_SLOT, KIND_INHERITED, KIND_OWN};
            match word.kind() {
                KIND_OWN => {
                    return self.shape_slot_attributes(data.shape(), word.slot()).is_some_and(|attributes| attributes.configurable);
                }
                KIND_INHERITED | KIND_ABSENT | KIND_ABSENT_OWN => return true,
                KIND_GETTER | KIND_GETTER_SLOT | KIND_GETTER_PAIR if word.hops() > 0 => return true,
                _ => {}
            }
        }
        // Record data: the arguments-object flag and the length and index descriptors.
        if let Some(array) = self.heap.array(target).and_then(|_| self.heap.object(target)).map(|object| object.record()).filter(|array| !array.is_arguments_object()) {
            // An array's `length` is non-configurable and writable unless a descriptor says
            // otherwise; its elements are configurable unless a descriptor was recorded.
            if atom == self.length_atom {
                return !array.has_length_descriptor();
            }
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
                return array.is_extensible() && !array.has_indexed_descriptors();
            }
        }
        if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) || !self.shape_names_own_keys(target) {
            return false;
        }
        let Some(data) = self.shape_property_lookup(target, atom) else {
            return false;
        };
        let shape = data.shape();
        match self.shape_slot(shape, atom) {
            None => {
                if let Some((layout, key32, _)) = facts {
                    self.symbol_load_proofs.record_absent_own(layout, key32);
                }
                true
            }
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
                    || self.heap.is_map(target)
                    || self.heap.is_set(target)
                    || matches!(
                        self.heap.get(target),
                        Some(Cell::WeakMap { .. } | Cell::WeakSet { .. })
                    )
            }
        }
    }
}
