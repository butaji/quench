//! The plain `[[Set]]` of a string key along a chain of ordinary objects: the walk proves
//! where the write lands, and that proof is the write's only lookup. The plain `[[Delete]]`
//! of a string key on one such object, decided by its shape alone.

use super::super::{Atom, Host, Object, PropertyKey, Value, Vm};

/// Where a plain `[[Set]]` lands, as the chain walk proved it.
enum PlainWriteTarget {
    /// The receiver's own writable data property at `slot`, holding `old`.
    Own { slot: usize, old: Value },
    /// An add to the receiver: no data property on the chain, or a writable inherited one.
    Add,
}

impl<H: Host> Vm<H> {
    /// The chain walk of `set_plain_named_property` for a key the caller admitted: `None` for
    /// a global, exotic, namespace or non-extensible object on the chain, a side-table
    /// descriptor of the key, or an accessor or read-only property deciding the write.
    fn plain_write_target(&self, object: Value, key: PropertyKey) -> Option<PlainWriteTarget> {
        // A global object's bindings carry declaration state beside its shape.
        if self.is_global_object(object) {
            return None;
        }
        let mut current = object;
        loop {
            let data = self.heap.ordinary_object(current)?;
            if self.plain_write_refused(current, data, key) {
                return None;
            }
            if let Some(slot) = self.property_shape_slot(data.shape(), key) {
                let attributes = self.slot_attributes(data, slot)?;
                if attributes.accessor || !attributes.writable {
                    return None;
                }
                return Some(match self.heap.property_get(data, slot).filter(|_| current == object) {
                    Some(old) => PlainWriteTarget::Own { slot, old },
                    None => PlainWriteTarget::Add,
                });
            }
            current = data.proto();
            if current.is_null() {
                return Some(PlainWriteTarget::Add);
            }
        }
    }

    /// Whether `object` on a plain write's chain leaves the write to the general path.
    fn plain_write_refused(&self, object: Value, data: &Object, key: PropertyKey) -> bool {
        !data.is_extensible()
            || data.is_module_namespace()
            || data.deferred_module().is_some()
            || (data.has_side_descriptors() && self.descriptors.contains_key(&(object, key)))
    }

    /// Overwrites `object`'s data property `key` at `slot`, proved writable, read as `old`.
    pub(in crate::vm) fn write_shape_slot(&mut self, object: Value, key: PropertyKey, slot: usize, old: Option<Value>, value: Value) {
        let invalidates_method = self.is_function(value) || old.is_some_and(|old| self.is_function(old));
        self.heap.property_set(object, slot, value);
        if invalidates_method {
            self.invalidate_method_caches_for_key(key);
        }
    }

    /// The store of `set_plain_named_property` for a key the caller admitted. `record_add`
    /// remembers an add for `keyed_add_hit` to replay.
    pub(in crate::vm) fn set_plain_key_property(&mut self, object: Value, atom: Atom, value: Value, record_add: bool) -> Option<()> {
        let key = PropertyKey::string(atom);
        if let PlainWriteTarget::Own { slot, old } = self.plain_write_target(object, key)? {
            self.write_shape_slot(object, key, slot, Some(old), value);
            return Some(());
        }
        let guard = record_add.then(|| self.keyed_add_guard(object, atom)).flatten();
        self.set_shape_property(object, key, value).ok()?;
        if let Some((shape, guard)) = guard {
            self.record_keyed_add(object, atom, shape, guard);
        }
        Some(())
    }

    /// A keyed `[[Set]]` of a string naming an existing atom, made as
    /// `set_plain_named_property` makes the named one; `false` leaves the write, and any key
    /// that function refuses, to the general path, which interns a new name.
    pub(in crate::vm) fn try_plain_string_key_set(&mut self, object: Value, key: Value, value: Value) -> bool {
        let Some(atom) = self.lookup_string_atom(key) else {
            return false;
        };
        if !self.plain_named_key(atom) {
            return false;
        }
        // An add a keyed store made before replays from its entry (`keyed_add_hit`).
        self.keyed_add_hit(object, atom, value) || self.set_plain_key_property(object, atom, value, true).is_some()
    }

    /// `delete object[key]` of a string key on an ordinary object whose access its shape
    /// describes, as `delete_property_key` decides it: no property, or a configurable data
    /// property held in the shape, which goes. `None` leaves globals, exotic objects, boxed
    /// primitives, namespaces, side-table descriptors, accessors (lazy members among them) and
    /// the keys `plain_named_key` refuses to the general path.
    pub(in crate::vm) fn plain_string_key_delete(&mut self, object: Value, key: Value) -> Option<Value> {
        let data = self.heap.ordinary_object(object).filter(|data| self.shape_holds_every_property(object, data))?;
        if !self.heap.is_string(key) {
            return None;
        }
        // A name no atom spells names no property of any shape.
        let Some(atom) = self.lookup_string_atom(key) else {
            return self.heap.string(key)?.host_string().parse::<u32>().is_err().then_some(Value::TRUE);
        };
        if !self.plain_named_key(atom) {
            return None;
        }
        let key = PropertyKey::string(atom);
        let Some(slot) = self.property_shape_slot(data.shape(), key).filter(|&slot| self.heap.property_get(data, slot).is_some())
        else {
            return Some(Value::TRUE);
        };
        let attributes = self.slot_attributes(data, slot)?;
        if attributes.accessor {
            return None;
        }
        if !attributes.configurable {
            return Some(Value::FALSE);
        }
        self.delete_shape_slot(object, slot, key);
        Some(Value::TRUE)
    }

    /// Whether `object`'s shape alone holds and describes all its properties: no global
    /// declaration state, exotic access, boxed primitive, deferred namespace or side-table
    /// descriptor beside it.
    fn shape_holds_every_property(&self, object: Value, data: &Object) -> bool {
        !(data.has_exotic_access()
            || data.has_side_descriptors()
            || data.has_indexed_descriptors()
            || data.boxed_primitive().is_some()
            || data.deferred_module().is_some()
            || self.is_global_object(object))
    }

    /// Deletes `object`'s property `key`, held at `slot` of its shape.
    pub(in crate::vm) fn delete_shape_slot(&mut self, object: Value, slot: usize, key: PropertyKey) {
        self.heap.property_set(object, slot, Value::DELETED);
        self.delete_shape_property(object, key);
    }
}
