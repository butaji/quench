use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_present_indices(&self, target: Value) -> Vec<usize> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(target) else {
            return Vec::new();
        };
        let length = self.heap.sparse_length(target).unwrap_or(elements.len());
        let mut indices = (0..length)
            .filter(|index| {
                elements
                    .get(*index)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| {
                        self.heap
                            .sparse_get(target, *index)
                            .filter(|value| !value.is_deleted())
                    })
                    .is_some()
            })
            .collect::<Vec<usize>>();
        for (object, atom) in self.descriptors.keys() {
            if *object == target
                && let Some(index) = super::object_static::array_index(self.atom_name(*atom))
                && !indices.contains(&(index as usize))
            {
                indices.push(index as usize);
            }
        }
        indices.sort_unstable();
        indices
    }

    pub(super) fn array_descriptor(
        &self,
        target: Value,
        index: usize,
    ) -> Option<PropertyAttributes> {
        let atom = self.lookup_atom(&index.to_string())?;
        self.descriptors.get(&(target, atom)).copied()
    }

    pub(super) fn array_integrity_atoms(&mut self, target: Value) -> Vec<Atom> {
        self.array_present_indices(target)
            .into_iter()
            .map(|index| self.intern_atom(&index.to_string()))
            .collect()
    }

    pub(super) fn array_is_integrity_level(&self, target: Value, freeze: bool) -> bool {
        self.array_present_indices(target).into_iter().all(|index| {
            let Some(atom) = self.lookup_atom(&index.to_string()) else {
                return false;
            };
            let attributes = self
                .descriptors
                .get(&(target, atom))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            !attributes.configurable && (!freeze || !attributes.writable)
        })
    }

    pub(super) fn define_array_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        index: usize,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(&index.to_string());
        let existing = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted()),
            _ => None,
        }
        .or_else(|| self.heap.sparse_get(target, index));
        let is_new = existing.is_none();
        if is_new
            && self
                .object_data(target)
                .is_some_and(|object| !object.is_extensible())
        {
            return Err(JsError(
                "cannot add property to non-extensible array".into(),
            ));
        }
        let current = self
            .descriptors
            .get(&(target, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let mut attributes = if is_new {
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            }
        } else {
            current
        };
        for (name, slot) in [
            ("writable", &mut attributes.writable),
            ("enumerable", &mut attributes.enumerable),
            ("configurable", &mut attributes.configurable),
        ] {
            let field = self.intern_atom(name);
            if let Some(value) = self.own_property(descriptor, field) {
                *slot = self.truthy(value);
            }
        }
        if !is_new
            && !current.configurable
            && (attributes.configurable != current.configurable
                || attributes.enumerable != current.enumerable
                || attributes.writable && !current.writable)
        {
            return Err(JsError(
                "cannot redefine non-configurable array index".into(),
            ));
        }
        let get = self.intern_atom("get");
        let set = self.intern_atom("set");
        let descriptor_getter = self.own_property(descriptor, get);
        let descriptor_setter = self.own_property(descriptor, set);
        if descriptor_getter.is_some() || descriptor_setter.is_some() {
            let getter = descriptor_getter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.getter))
                .flatten();
            let setter = descriptor_setter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.setter))
                .flatten();
            if getter.is_some_and(|value| !self.is_function(value))
                || setter.is_some_and(|value| !self.is_function(value))
            {
                return Err(JsError("array index accessor is not callable".into()));
            }
            if !is_new && !current.configurable && !current.accessor {
                return Err(JsError(
                    "cannot change non-configurable array index kind".into(),
                ));
            }
            if !self.set_array_element(target, index, Value::DELETED) {
                return Err(JsError("cannot define array accessor".into()));
            }
            self.descriptors.insert(
                (target, atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: attributes.enumerable,
                    configurable: attributes.configurable,
                    accessor: true,
                    getter,
                    setter,
                },
            );
            return Ok(target);
        }
        if !is_new && current.accessor {
            return Err(JsError(
                "cannot change array accessor to data property".into(),
            ));
        }
        let value_atom = self.intern_atom("value");
        let next = self
            .own_property(descriptor, value_atom)
            .or(existing)
            .unwrap_or(Value::UNDEFINED);
        if !is_new
            && !current.configurable
            && !current.writable
            && self
                .own_property(descriptor, value_atom)
                .is_some_and(|value| {
                    existing.is_some_and(|current| !self.same_value(current, value))
                })
        {
            return Err(JsError("cannot write non-writable array index".into()));
        }
        if !self.set_array_element(target, index, next) {
            return Err(JsError("cannot define array index".into()));
        }
        self.descriptors.insert((target, atom), attributes);
        let _ = p;
        Ok(target)
    }

    pub(super) fn delete_array_index(&mut self, target: Value, index: usize) -> Value {
        let present = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .is_some(),
            _ => false,
        } || self.heap.sparse_get(target, index).is_some();
        let atom = self.intern_atom(&index.to_string());
        let has_descriptor = self.descriptors.contains_key(&(target, atom));
        if !present && !has_descriptor {
            return Value::TRUE;
        }
        let attributes = self
            .descriptors
            .get(&(target, atom))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        if !attributes.configurable {
            return Value::FALSE;
        }
        if present {
            if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target)
                && index < elements.len()
            {
                Rc::make_mut(elements)[index] = Value::DELETED;
            } else {
                self.heap.sparse_set(target, index, Value::DELETED);
            }
        }
        self.descriptors.remove(&(target, atom));
        Value::TRUE
    }
}
