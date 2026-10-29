use super::object_descriptors::{PropertyDescriptorRecord, TypedArrayIndexKey};
use super::property_key::PropertyKey;
use super::*;

#[derive(Clone, Copy)]
pub(super) enum PropertyDefinitionKind {
    Object,
    Reflect,
}

pub(super) struct RootedPropertyDescriptor {
    value: Option<RootId>,
    getter: Option<RootId>,
    setter: Option<RootId>,
    writable: Option<bool>,
    enumerable: Option<bool>,
    configurable: Option<bool>,
}

impl RootedPropertyDescriptor {
    pub(super) fn new(heap: &mut Heap, record: PropertyDescriptorRecord) -> Self {
        Self {
            value: record.value.map(|v| heap.root(v)),
            getter: record.getter.map(|v| heap.root(v)),
            setter: record.setter.map(|v| heap.root(v)),
            writable: record.writable,
            enumerable: record.enumerable,
            configurable: record.configurable,
        }
    }
    pub(super) fn resolve(&self, heap: &Heap) -> PropertyDescriptorRecord {
        PropertyDescriptorRecord {
            value: self.value.map(|r| heap.root_value(r).unwrap()),
            getter: self.getter.map(|r| heap.root_value(r).unwrap()),
            setter: self.setter.map(|r| heap.root_value(r).unwrap()),
            writable: self.writable,
            enumerable: self.enumerable,
            configurable: self.configurable,
        }
    }
    pub(super) fn release(self, heap: &mut Heap) {
        for root in [self.value, self.getter, self.setter].into_iter().flatten() {
            heap.release_root(root);
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn define_property_from_descriptor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: PropertyDefinitionKind,
    ) -> Result<Value, JsError> {
        let [target, key, descriptor] =
            [0, 1, 2].map(|index| args.get(index).copied().unwrap_or(Value::UNDEFINED));
        // The heap does not move values: the call roots keep the arguments, and the key once
        // converted, across the conversions' guest code.
        let start = self.active_call_roots.len();
        self.active_call_roots.extend([target, key, descriptor]);
        let outcome = (|| {
            if !self.is_object_like(target) {
                return Err(self.type_error(p, "defineProperty target is not an object".into()));
            }
            let key = self.to_property_key(p, key)?;
            self.active_call_roots.push(key);
            let record = self.to_property_descriptor(p, descriptor)?;
            match kind {
                PropertyDefinitionKind::Reflect => {
                    let accepted = self.define_own_property_record(p, target, key, record)?;
                    Ok(Self::integrity_bool(accepted))
                }
                PropertyDefinitionKind::Object => {
                    self.define_property_or_throw(p, target, key, record)?;
                    Ok(target)
                }
            }
        })();
        self.active_call_roots.truncate(start);
        outcome
    }

    pub(super) fn define_property_or_throw(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<(), JsError> {
        if self.define_own_property_record(p, target, key, descriptor)? {
            Ok(())
        } else {
            Err(self.type_error(p, "cannot define property".into()))
        }
    }

    pub(super) fn define_own_property_record(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        record: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        if self.is_wasm_gc_object(target) {
            return Err(self.wasm_gc_opaque_error(p));
        }
        if matches!(self.heap.get(target), Some(Cell::Proxy { .. })) {
            if self.is_host_private_symbol(key) {
                return Ok(false);
            }
            // A trap runs guest code, and the definition is read back through these roots.
            let target = self.heap.root(target);
            let key = self.heap.root(key);
            let descriptor = RootedPropertyDescriptor::new(&mut self.heap, record);
            let outcome = self.define_proxy_property_record(p, target, key, &descriptor);
            descriptor.release(&mut self.heap);
            self.heap.release_root(key);
            self.heap.release_root(target);
            return outcome;
        }
        // The heap does not move values, so the call roots keep each of them, unchanged,
        // across whatever guest code a deferred namespace or a lazy member runs.
        let start = self.active_call_roots.len();
        self.active_call_roots.extend(
            [target, key].into_iter().chain(record.value).chain(record.getter).chain(record.setter),
        );
        let outcome = self.define_ordinary_own_property(p, target, key, record);
        self.active_call_roots.truncate(start);
        outcome
    }

    /// `define_own_property_record` of a target that is no proxy, its values kept by the
    /// caller's call roots.
    fn define_ordinary_own_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let property = match self.string_key_atom(key) {
            Some(atom) => PropertyKey::string(atom),
            None if matches!(self.heap.get(key), Some(Cell::Symbol(_))) => PropertyKey::symbol(key),
            _ => unreachable!("DefineOwnProperty receives a property key"),
        };
        if let PropertyKey::String(atom) = property {
            if matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) {
                match Self::typed_array_index_key(self.atom_name(atom)) {
                    TypedArrayIndexKey::Index(index) => {
                        return self.define_typed_array_property(
                            p,
                            object,
                            index,
                            descriptor,
                        );
                    }
                    TypedArrayIndexKey::Invalid => return Ok(false),
                    TypedArrayIndexKey::NotCanonical => {}
                }
            }
            if self.heap.array(object).is_some() {
                if atom == self.length_atom
                    && !self
                        .object_data(object)
                        .is_some_and(Object::is_arguments_object)
                {
                    return self.define_array_length(p, object, descriptor);
                }
            }
            self.evaluate_deferred_namespace_for_key(p, object, Some(property))?;
            // A lazy data property is configurable with the attributes its data property
            // will have, so only a descriptor that keeps more of it needs its value.
            if !descriptor.replaces_kind_fields() {
                self.materialize_lazy_data_property(p, object, property)?;
            }
            if self.heap.array(object).is_some()
                && let Some(index) = super::object_static::array_index(self.atom_name(atom))
            {
                return self.define_array_property(
                    object,
                    index as usize,
                    descriptor,
                );
            }
            if self
                .object_data(object)
                .is_some_and(Object::is_module_namespace)
            {
                let Some(current) = self.module_namespace_value(p, object, atom)? else {
                    return Ok(false);
                };
                let record = descriptor;
                return Ok(record.configurable != Some(true)
                    && record.enumerable != Some(false)
                    && record.writable != Some(false)
                    && !record.has_accessor_fields()
                    && record.value.is_none_or(|v| self.same_value(current, v)));
            }
        }
        // A watched member (one a binding stands for, or one its initializer redefines)
        // materializes before any definition, as before a write.
        if self.watched_lazy_member(object, property)
            || (matches!(property, PropertyKey::Symbol(_))
                && !descriptor.replaces_kind_fields())
        {
            self.materialize_lazy_data_property(p, object, property)?;
        }
        self.define_ordinary_property_key(object, property, descriptor)
    }

    fn define_proxy_property_record(
        &mut self,
        p: &ResidualProgram,
        proxy: RootId,
        key: RootId,
        record: &RootedPropertyDescriptor,
    ) -> Result<bool, JsError> {
        let proxy = self.heap.root_value(proxy).unwrap();
        let Some((target, handler)) = self.proxy_parts(proxy) else {
            unreachable!("proxy definition dispatch")
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let mut descriptor_root = None;
        let outcome = (|| {
            let atom = self.intern_atom("defineProperty");
            let object = self.heap.root_value(handler).unwrap();
            let trap = self.get_property(p, object, atom)?;
            if trap.is_undefined() || trap.is_null() {
                let object = self.heap.root_value(target).unwrap();
                let property = self.heap.root_value(key).unwrap();
                return self.define_own_property_record(
                    p,
                    object,
                    property,
                    record.resolve(&self.heap),
                );
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy defineProperty trap is not callable".into()));
            }
            let trap = self.heap.root(trap);
            let result = (|| {
                let descriptor = self.from_property_descriptor(record.resolve(&self.heap))?;
                let descriptor = self.heap.root(descriptor);
                descriptor_root = Some(descriptor);
                let callee = self.heap.root_value(trap).unwrap();
                let receiver = self.heap.root_value(handler).unwrap();
                let object = self.heap.root_value(target).unwrap();
                let property = self.heap.root_value(key).unwrap();
                let view = self.heap.root_value(descriptor).unwrap();
                let result = self.call_value(p, callee, receiver, &[object, property, view])?;
                if !self.truthy(result) {
                    return Ok(false);
                }
                self.validate_proxy_define_property_record(p, target, key, record)?;
                Ok(true)
            })();
            self.heap.release_root(trap);
            result
        })();
        for root in [Some(target), Some(handler), descriptor_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }
}
