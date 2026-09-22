use super::*;

impl<H: Host> Vm<H> {
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
        let Some(Cell::Array { elements, .. }) = self.heap.get(result) else {
            return Err(JsError("proxy ownKeys trap must return an array".into()));
        };
        let mut keys = Vec::with_capacity(elements.len());
        for key in elements.iter().copied() {
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
                    .symbol_descriptors
                    .get(&(target, target_key))
                    .is_some_and(|attributes| !attributes.configurable),
                Some(Cell::String(name)) => {
                    let atom = self.intern_atom(&name);
                    self.own_property(target, atom).is_some_and(|_| {
                        !self
                            .descriptors
                            .get(&(target, atom))
                            .copied()
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
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        })
    }

    fn same_property_key(&self, left: Value, right: Value) -> bool {
        match (self.heap.get(left), self.heap.get(right)) {
            (Some(Cell::String(left)), Some(Cell::String(right))) => left == right,
            (Some(Cell::Symbol(_)), Some(Cell::Symbol(_))) => left == right,
            _ => false,
        }
    }

    pub(super) fn symbol_property(&self, object: Value, key: Value) -> Option<Value> {
        self.symbol_properties.get(&(object, key)).copied()
    }

    pub(super) fn set_symbol_property(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if self.object_data(object).is_none() {
            return Err(JsError("property write on non-object".into()));
        }
        if !self.symbol_properties.contains_key(&(object, key))
            && self
                .object_data(object)
                .is_some_and(|object| !object.is_extensible())
        {
            return Err(JsError(
                "cannot add property to non-extensible object".into(),
            ));
        }
        if self
            .symbol_descriptors
            .get(&(object, key))
            .is_some_and(|attributes| !attributes.writable)
        {
            return Err(JsError("cannot write non-writable symbol property".into()));
        }
        let fresh = self
            .symbol_properties
            .insert((object, key), value)
            .is_none();
        if fresh {
            self.symbol_property_order
                .entry(object)
                .or_default()
                .push(key);
        }
        self.symbol_descriptors
            .entry((object, key))
            .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
        Ok(())
    }

    pub(super) fn define_symbol_property(
        &mut self,
        target: Value,
        key: Value,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        let existing = self.symbol_property(target, key);
        let mut attributes = self
            .symbol_descriptors
            .get(&(target, key))
            .copied()
            .unwrap_or(PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            });
        for (name, slot) in [
            ("writable", &mut attributes.writable),
            ("enumerable", &mut attributes.enumerable),
            ("configurable", &mut attributes.configurable),
        ] {
            let atom = self.intern_atom(name);
            if let Some(value) = self.own_property(descriptor, atom) {
                *slot = self.truthy(value);
            }
        }
        let value_atom = self.intern_atom("value");
        let value = self
            .own_property(descriptor, value_atom)
            .unwrap_or(existing.unwrap_or(Value::UNDEFINED));
        self.set_symbol_property(target, key, value)?;
        self.symbol_descriptors.insert((target, key), attributes);
        Ok(target)
    }

    pub(super) fn object_symbols(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if let Some(keys) = self.proxy_own_keys(p, object)? {
            let values = keys
                .into_iter()
                .filter(|key| matches!(self.heap.get(*key), Some(Cell::Symbol(_))))
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        let object = self.proxy_target(object);
        let object = self.box_object(object)?;
        let values = self
            .symbol_property_order
            .get(&object)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|symbol| self.symbol_property(object, *symbol).is_some())
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
        if let Some(keys) = self.proxy_own_keys(p, object)? {
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(keys),
            }));
        }
        let target = self.proxy_target(object);
        let names = self.object_names(p, target)?;
        let mut values = match self.heap.get(names) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        values.extend(
            self.symbol_property_order
                .get(&target)
                .into_iter()
                .flatten()
                .copied()
                .filter(|symbol| self.symbol_property(target, *symbol).is_some()),
        );
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
