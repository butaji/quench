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
        let source = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let key_input = self
            .heap
            .root(args.get(1).copied().unwrap_or(Value::UNDEFINED));
        let input = self
            .heap
            .root(args.get(2).copied().unwrap_or(Value::UNDEFINED));
        let mut key_root = None;
        let outcome = (|| {
            let target = self.heap.root_value(source).unwrap();
            if !self.is_object_like(target) {
                return Err(self.type_error(p, "defineProperty target is not an object".into()));
            }
            let key = self.heap.root_value(key_input).unwrap();
            let key = self.to_property_key(p, key)?;
            let key = self.heap.root(key);
            key_root = Some(key);
            let descriptor = self.heap.root_value(input).unwrap();
            let record = self.to_property_descriptor(p, descriptor)?;
            let target = self.heap.root_value(source).unwrap();
            let property = self.heap.root_value(key).unwrap();
            match kind {
                PropertyDefinitionKind::Reflect => {
                    let accepted = self.define_own_property_record(p, target, property, record)?;
                    Ok(Self::integrity_bool(accepted))
                }
                PropertyDefinitionKind::Object => {
                    self.define_property_or_throw(p, target, property, record)?;
                    Ok(self.heap.root_value(source).unwrap())
                }
            }
        })();
        for root in [Some(source), Some(key_input), Some(input), key_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
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
        let target = self.heap.root(target);
        let key = self.heap.root(key);
        let descriptor = RootedPropertyDescriptor::new(&mut self.heap, record);
        let outcome = (|| {
            let object = self.heap.root_value(target).unwrap();
            if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
                return self.define_proxy_property_record(p, target, key, &descriptor);
            }
            let property = self.heap.root_value(key).unwrap();
            let property = match self.heap.get(property).cloned() {
                Some(Cell::Symbol(_)) => PropertyKey::symbol(property),
                Some(Cell::String(name)) => PropertyKey::string(self.intern_js_atom(&name)),
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
                                descriptor.resolve(&self.heap),
                            );
                        }
                        TypedArrayIndexKey::Invalid => return Ok(false),
                        TypedArrayIndexKey::NotCanonical => {}
                    }
                }
                if matches!(self.heap.get(object), Some(Cell::Array { .. })) {
                    if atom == self.length_atom
                        && !self
                            .object_data(object)
                            .is_some_and(Object::is_arguments_object)
                    {
                        return self.define_array_length(p, object, descriptor.resolve(&self.heap));
                    }
                    if let Some(index) = super::object_static::array_index(self.atom_name(atom)) {
                        return self.define_array_property(
                            object,
                            index as usize,
                            descriptor.resolve(&self.heap),
                        );
                    }
                }
                self.evaluate_deferred_namespace_for_key(p, object, Some(property))?;
                let object = self.heap.root_value(target).unwrap();
                if self
                    .object_data(object)
                    .is_some_and(Object::is_module_namespace)
                {
                    let Some(current) = self.module_namespace_value(p, object, atom)? else {
                        return Ok(false);
                    };
                    let record = descriptor.resolve(&self.heap);
                    return Ok(record.configurable != Some(true)
                        && record.enumerable != Some(false)
                        && record.writable != Some(false)
                        && !record.has_accessor_fields()
                        && record.value.is_none_or(|v| self.same_value(current, v)));
                }
            }
            let object = self.heap.root_value(target).unwrap();
            self.define_ordinary_property_key(object, property, descriptor.resolve(&self.heap))
        })();
        descriptor.release(&mut self.heap);
        self.heap.release_root(key);
        self.heap.release_root(target);
        outcome
    }

    fn define_proxy_property_record(
        &mut self,
        p: &ResidualProgram,
        proxy: RootId,
        key: RootId,
        record: &RootedPropertyDescriptor,
    ) -> Result<bool, JsError> {
        let proxy = self.heap.root_value(proxy).unwrap();
        let Some((target, handler)) = self.proxy_parts(proxy)
        else {
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
