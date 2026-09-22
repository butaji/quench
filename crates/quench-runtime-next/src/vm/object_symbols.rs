use super::*;

impl<H: Host> Vm<H> {
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

    pub(super) fn object_symbols(&mut self, object: Value) -> Result<Value, JsError> {
        let object = self.proxy_target(object);
        let object = self.box_object(object)?;
        let values = self
            .symbol_property_order
            .get(&object)
            .cloned()
            .unwrap_or_default();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn object_own_keys(&mut self, object: Value) -> Result<Value, JsError> {
        let target = self.proxy_target(object);
        let names = self.object_names(target)?;
        let mut values = match self.heap.get(names) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        values.extend(
            self.symbol_property_order
                .get(&target)
                .into_iter()
                .flatten()
                .copied(),
        );
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
