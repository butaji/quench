use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_present_indices(&self, target: Value) -> Vec<usize> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(target) else {
            return Vec::new();
        };
        let length = self.heap.sparse_length(target).unwrap_or(elements.len());
        (0..length)
            .filter(|index| {
                elements
                    .get(*index)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| self.heap.sparse_get(target, *index))
                    .is_some()
            })
            .collect()
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
        if self.own_property(descriptor, get).is_some()
            || self.own_property(descriptor, set).is_some()
        {
            return Err(JsError("array index accessors are not supported".into()));
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
}
