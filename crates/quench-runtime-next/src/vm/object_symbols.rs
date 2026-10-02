use super::operations::ArrayLikeElementKind;
use super::property_key::PropertyKey;
use super::*;

pub(super) enum PropertyCopyKind {
    Set,
    CreateDataProperty,
}

impl<H: Host> Vm<H> {
    fn own_keys_array(&mut self, keys: Vec<Value>) -> Value {
        let roots = keys
            .iter()
            .copied()
            .map(|key| self.heap.root(key))
            .collect::<Vec<_>>();
        let keys = roots
            .iter()
            .filter_map(|root| self.heap.root_value(*root))
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(keys),
        });
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        result
    }

    fn ordinary_own_key_values(&mut self, object: Value) -> Vec<Value> {
        let mut values = self.indexed_name_keys(object).unwrap_or_default();
        let Some(data) = self.object_data(object) else {
            return values;
        };
        let shape = data.shape();
        let strings = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(atom, slot)| {
                self.heap.property_get(data, *slot).is_some()
                    && !self.atom_name(*atom).starts_with("\0rqj:")
            })
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        let symbols = self
            .shape_keys(shape)
            .iter()
            .filter_map(|key| key.symbol_value())
            .filter(|symbol| self.symbol_property(object, *symbol).is_some())
            .collect::<Vec<_>>();
        values.extend(
            strings
                .into_iter()
                .map(|atom| self.heap.alloc(Cell::String(self.atom_value(atom)))),
        );
        values.extend(symbols);
        values
    }

    pub(super) fn proxy_own_keys(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
    ) -> Result<Vec<Value>, JsError> {
        let _stack = self.enter_stack()?;
        let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(proxy).cloned()
        else {
            unreachable!("Proxy own-key dispatch");
        };
        if handler.is_null() {
            return Err(self.type_error(p, "cannot access a revoked proxy".into()));
        }
        let proxy = self.heap.root(proxy);
        let target = self.heap.root(target);
        let handler = self.heap.root(handler);
        let mut keys = Vec::new();
        let mut target_keys = Vec::new();
        let outcome = (|| {
            let trap_atom = self.intern_atom("ownKeys");
            let trap = self.get_property(p, self.heap.root_value(handler).unwrap(), trap_atom)?;
            if trap.is_undefined() || trap.is_null() {
                return self.object_own_key_values(p, self.heap.root_value(target).unwrap());
            }
            if !self.is_function(trap) {
                return Err(self.type_error(p, "proxy ownKeys trap is not callable".into()));
            }
            let result = self.call_value(
                p,
                trap,
                self.heap.root_value(handler).unwrap(),
                &[self.heap.root_value(target).unwrap()],
            )?;
            keys = self
                .create_list_from_array_like(p, result, ArrayLikeElementKind::PropertyKey)?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for (index, key) in keys.iter().enumerate() {
                if keys[..index].iter().any(|previous| {
                    self.same_property_key(
                        self.heap.root_value(*previous).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    )
                }) {
                    return Err(
                        self.type_error(p, "proxy ownKeys result contains duplicate keys".into())
                    );
                }
            }
            let extensible = self.object_is_extensible(p, &[self.heap.root_value(target).unwrap()])?;
            let extensible = self.truthy(extensible);
            target_keys = self
                .object_own_key_values(p, self.heap.root_value(target).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            let mut required = Vec::new();
            for key in &target_keys {
                let descriptor = self.object_get_own_property_descriptor(
                    p,
                    &[
                        self.heap.root_value(target).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    ],
                )?;
                if !extensible
                    || (!descriptor.is_undefined() && !self.descriptor_flag(descriptor, "configurable"))
                {
                    required.push(*key);
                }
            }
            for key in required {
                if !keys.iter().any(|listed| {
                    self.same_property_key(
                        self.heap.root_value(*listed).unwrap(),
                        self.heap.root_value(key).unwrap(),
                    )
                }) {
                    return Err(self.type_error(p, "proxy ownKeys trap omitted a required key".into()));
                }
            }
            if !extensible && keys.len() != target_keys.len() {
                return Err(self.type_error(
                    p,
                    "proxy ownKeys trap changed the keys of a sealed target".into(),
                ));
            }
            Ok(keys
                .iter()
                .map(|key| self.heap.root_value(*key).unwrap())
                .collect())
        })();
        for root in keys
            .into_iter()
            .chain(target_keys)
            .chain([proxy, target, handler])
        {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn object_own_key_values(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Vec<Value>, JsError> {
        let keys = self.object_own_keys(p, object)?;
        Ok(match self.heap.get(keys) {
            Some(Cell::Array { elements, .. }) => elements
                .iter()
                .copied()
                .filter(|key| {
                    !matches!(self.heap.get(*key), Some(Cell::String(name)) if name.host_string().starts_with("\0rqj:"))
                })
                .collect(),
            _ => Vec::new(),
        })
    }

    pub(super) fn same_property_key(&self, left: Value, right: Value) -> bool {
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::String(left)), Some(Cell::String(right))) => left == right,
            (Some(Cell::Symbol(_)), Some(Cell::Symbol(_))) => left == right,
            _ => false,
        }
    }

    pub(super) fn copy_data_properties(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        source: Value,
        exclusions: Value,
    ) -> Result<(), JsError> {
        let excluded = match self.heap.get(exclusions) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        self.copy_enumerable_properties(
            p,
            target,
            source,
            &excluded,
            PropertyCopyKind::CreateDataProperty,
        )
    }

    pub(super) fn copy_enumerable_properties(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        source: Value,
        excluded: &[Value],
        kind: PropertyCopyKind,
    ) -> Result<(), JsError> {
        if source.is_null() || source.is_undefined() {
            return Ok(());
        }
        let source = self.box_object(source)?;
        let target = self.heap.root(target);
        let source = self.heap.root(source);
        let excluded = excluded
            .iter()
            .map(|key| self.heap.root(*key))
            .collect::<Vec<_>>();
        let mut keys = Vec::new();
        let outcome = (|| {
            keys = self
                .object_own_key_values(p, self.heap.root_value(source).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for key in &keys {
                if excluded.iter().any(|excluded| {
                    self.same_property_key(
                        self.heap.root_value(*excluded).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    )
                }) {
                    continue;
                }
                let descriptor = self.object_get_own_property_descriptor(
                    p,
                    &[
                        self.heap.root_value(source).unwrap(),
                        self.heap.root_value(*key).unwrap(),
                    ],
                )?;
                if descriptor.is_undefined() || !self.descriptor_flag(descriptor, "enumerable") {
                    continue;
                }
                let value = self.get_index(
                    p,
                    self.heap.root_value(source).unwrap(),
                    self.heap.root_value(*key).unwrap(),
                )?;
                let target = self.heap.root_value(target).unwrap();
                let key = self.heap.root_value(*key).unwrap();
                match kind {
                    PropertyCopyKind::Set => self.set_index_mode(p, target, key, value, true)?,
                    PropertyCopyKind::CreateDataProperty => self.define_property_or_throw(
                        p,
                        target,
                        key,
                        super::object_descriptors::PropertyDescriptorRecord::data(value),
                    )?,
                }
            }
            Ok(())
        })();
        for root in keys.into_iter().chain(excluded).chain([target, source]) {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn symbol_property(&self, object: Value, key: Value) -> Option<Value> {
        let data = self.object_data(object)?;
        let slot = self.property_shape_slot(data.shape(), PropertyKey::symbol(key))?;
        self.heap.property_get(data, slot)
    }

    pub(super) fn set_symbol_property(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_shape_property(object, PropertyKey::symbol(key), value)
    }

    pub(super) fn set_symbol_property_with_receiver(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        value: Value,
        receiver: Value,
    ) -> Result<bool, JsError> {
        let target_root = self.heap.root(target);
        let key_root = self.heap.root(key);
        let value_root = self.heap.root(value);
        let receiver_root = self.heap.root(receiver);
        let outcome = (|| {
            let mut current = target;
            loop {
                if let Some(Cell::Proxy {
                    target, handler, ..
                }) = self.heap.get(current).cloned()
                {
                    return self.proxy_set(
                        p,
                        target,
                        handler,
                        self.heap.root_value(receiver_root).unwrap(),
                        PropertyKey::symbol(self.heap.root_value(key_root).unwrap()),
                        self.heap.root_value(value_root).unwrap(),
                    );
                }
                let descriptor = self.object_get_own_property_descriptor(p, &[current, key])?;
                if !descriptor.is_undefined() {
                    if self.descriptor_field(p, descriptor, "get")?.is_some()
                        || self.descriptor_field(p, descriptor, "set")?.is_some()
                    {
                        let setter = self.descriptor_field(p, descriptor, "set")?;
                        let Some(setter) = setter.filter(|setter| !setter.is_undefined()) else {
                            return Ok(false);
                        };
                        self.call_value(
                            p,
                            setter,
                            self.heap.root_value(receiver_root).unwrap(),
                            &[self.heap.root_value(value_root).unwrap()],
                        )?;
                        return Ok(true);
                    }
                    let writable = self
                        .descriptor_field(p, descriptor, "writable")?
                        .is_some_and(|writable| self.truthy(writable));
                    if !writable {
                        return Ok(false);
                    }
                    break;
                }
                current = self.object_get_prototype_of(p, current)?;
                if current.is_null() {
                    break;
                }
            }

            if !self.is_object_like(receiver) {
                return Ok(false);
            }
            if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
                return self.set_receiver_proxy_data_property(
                    p,
                    self.heap.root_value(receiver_root).unwrap(),
                    PropertyKey::symbol(self.heap.root_value(key_root).unwrap()),
                    self.heap.root_value(value_root).unwrap(),
                );
            }
            if let Some(attributes) = self.property_attributes(receiver, PropertyKey::symbol(key)) {
                if attributes.accessor || !attributes.writable {
                    return Ok(false);
                }
            } else if self.symbol_property(receiver, key).is_none()
                && !self
                    .object_data(receiver)
                    .is_some_and(Object::is_extensible)
            {
                return Ok(false);
            }
            self.set_symbol_property(receiver, key, value)?;
            Ok(true)
        })();
        for root in [target_root, key_root, value_root, receiver_root] {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn object_symbols(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        let values = self
            .object_own_key_values(p, object)?
            .into_iter()
            .filter(|key| matches!(self.heap.get(*key), Some(Cell::Symbol(_))))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn object_own_keys(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if matches!(self.heap.get(object), Some(Cell::Proxy { .. })) {
            let keys = self.proxy_own_keys(p, object)?;
            return Ok(self.own_keys_array(keys));
        }
        let target = self.box_object(object)?;
        self.evaluate_deferred_namespace_for_key(p, target, None)?;
        let values = self.ordinary_own_key_values(target);
        Ok(self.own_keys_array(values))
    }
}
