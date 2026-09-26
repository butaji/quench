use super::property_key::PropertyKey;
use super::*;

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
        let symbols = self.shapes[shape as usize]
            .keys
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
    ) -> Result<Option<Vec<Value>>, JsError> {
        let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(proxy).cloned()
        else {
            return Ok(None);
        };
        if handler.is_null() {
            return Err(JsError("cannot access a revoked proxy".into()));
        }
        let trap_atom = self.intern_atom("ownKeys");
        let trap = self.get_property(p, handler, trap_atom)?;
        if trap.is_undefined() || trap.is_null() {
            return Ok(None);
        }
        if !self.is_function(trap) {
            return Err(JsError("proxy ownKeys trap is not callable".into()));
        }
        let result = self.call_value(p, trap, handler, &[target])?;
        if !self.is_object_like(result) {
            return Err(JsError("proxy ownKeys trap must return an object".into()));
        }
        let listed_keys = self.call_argument_list(p, result, false)?;
        let mut keys = Vec::with_capacity(listed_keys.len());
        for key in listed_keys {
            if !matches!(self.heap.get(key), Some(Cell::String(_) | Cell::Symbol(_))) {
                return Err(JsError(
                    "proxy ownKeys result contains an invalid key".into(),
                ));
            }
            if keys
                .iter()
                .copied()
                .any(|previous| self.same_property_key(previous, key))
            {
                return Err(JsError(
                    "proxy ownKeys result contains duplicate keys".into(),
                ));
            }
            keys.push(key);
        }
        let target_keys = self.object_own_key_values(p, target)?;
        for target_key in target_keys.iter().copied() {
            let required = match self.heap.get(target_key).cloned() {
                Some(Cell::Symbol(_)) => self
                    .property_attributes(target, PropertyKey::symbol(target_key))
                    .is_some_and(|attributes| !attributes.configurable),
                Some(Cell::String(name)) => {
                    let atom = self.intern_js_atom(&name);
                    self.own_property(target, atom).is_some_and(|_| {
                        !self
                            .property_attributes(target, PropertyKey::string(atom))
                            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                            .configurable
                    })
                }
                _ => false,
            };
            if required
                && !keys
                    .iter()
                    .copied()
                    .any(|key| self.same_property_key(key, target_key))
            {
                return Err(JsError("proxy ownKeys trap omitted a required key".into()));
            }
        }
        if self
            .object_data(target)
            .is_some_and(|object| !object.is_extensible())
            && keys.iter().copied().any(|key| {
                !target_keys
                    .iter()
                    .copied()
                    .any(|target_key| self.same_property_key(key, target_key))
            })
        {
            return Err(JsError(
                "proxy ownKeys trap added a key to a sealed target".into(),
            ));
        }
        Ok(Some(keys))
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
        if source.is_null() || source.is_undefined() {
            return Ok(());
        }
        let excluded = match self.heap.get(exclusions) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        let enumerable_atom = self.intern_atom("enumerable");
        for key in self.object_own_key_values(p, source)? {
            if excluded
                .iter()
                .copied()
                .any(|excluded| self.same_property_key(excluded, key))
            {
                continue;
            }
            let descriptor = self.object_get_own_property_descriptor(p, &[source, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
            if !self.truthy(enumerable) {
                continue;
            }
            let value = self.get_index(p, source, key)?;
            let property_key = match self.heap.get(key).cloned() {
                Some(Cell::String(name)) => PropertyKey::string(self.intern_js_atom(&name)),
                Some(Cell::Symbol(_)) => PropertyKey::symbol(key),
                _ => continue,
            };
            self.set_shape_property(target, property_key, value)?;
        }
        Ok(())
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
            if let Some(keys) = self.proxy_own_keys(p, object)? {
                return Ok(self.own_keys_array(keys));
            }
            let Some(Cell::Proxy { target, .. }) = self.heap.get(object) else {
                unreachable!("proxy disappeared during own-key operation")
            };
            let target = *target;
            return self.object_own_keys(p, target);
        }
        let target = self.box_object(object)?;
        self.evaluate_deferred_namespace_for_key(p, target, None)?;
        let values = self.ordinary_own_key_values(target);
        Ok(self.own_keys_array(values))
    }
}
