use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_get_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(source).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("getOwnPropertyDescriptor");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let key = if matches!(
                    self.heap
                        .get(args.get(1).copied().unwrap_or(Value::UNDEFINED)),
                    Some(Cell::Symbol(_))
                ) {
                    args.get(1).copied().unwrap_or(Value::UNDEFINED)
                } else {
                    let text =
                        self.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                    self.heap.alloc(Cell::String(text.into()))
                };
                let result = self.call_value(p, trap, handler, &[target, key])?;
                if result.is_undefined() {
                    let target_descriptor =
                        self.object_get_own_property_descriptor(p, &[target, key])?;
                    if !target_descriptor.is_undefined() {
                        let configurable = self.descriptor_flag(target_descriptor, "configurable");
                        if !configurable
                            || self
                                .object_data(target)
                                .is_some_and(|object| !object.is_extensible())
                        {
                            return Err(JsError(
                                "proxy descriptor trap cannot hide a target property".into(),
                            ));
                        }
                    }
                    return Ok(Value::UNDEFINED);
                }
                if self.object_data(result).is_none() {
                    return Err(JsError(
                        "proxy getOwnPropertyDescriptor trap must return an object or undefined"
                            .into(),
                    ));
                }
                let target_descriptor =
                    self.object_get_own_property_descriptor(p, &[target, key])?;
                if target_descriptor.is_undefined()
                    && self
                        .object_data(target)
                        .is_some_and(|object| !object.is_extensible())
                {
                    return Err(JsError(
                        "proxy descriptor trap added a property to a sealed target".into(),
                    ));
                }
                if !target_descriptor.is_undefined()
                    && !self.descriptor_flag(target_descriptor, "configurable")
                    && (self.descriptor_flag(result, "configurable")
                        || self.descriptor_flag(result, "enumerable")
                            != self.descriptor_flag(target_descriptor, "enumerable"))
                {
                    return Err(JsError(
                        "proxy descriptor trap changed a non-configurable target property".into(),
                    ));
                }
                return Ok(result);
            }
        }
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let target = self.box_object(target)?;
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
            let Some(value) = self.symbol_property(target, key_value) else {
                return Ok(Value::UNDEFINED);
            };
            let attributes = self
                .descriptors
                .get(&(target, PropertyKey::symbol(key_value)))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            let descriptor = self.object();
            if attributes.accessor {
                for (name, value) in [
                    ("get", attributes.getter.unwrap_or(Value::UNDEFINED)),
                    ("set", attributes.setter.unwrap_or(Value::UNDEFINED)),
                    ("enumerable", Self::integrity_bool(attributes.enumerable)),
                    (
                        "configurable",
                        Self::integrity_bool(attributes.configurable),
                    ),
                ] {
                    let atom = self.intern_atom(name);
                    self.set_property(descriptor, atom, value)?;
                }
                return Ok(descriptor);
            }
            for (name, value) in [
                ("value", value),
                ("writable", Self::integrity_bool(attributes.writable)),
                ("enumerable", Self::integrity_bool(attributes.enumerable)),
                (
                    "configurable",
                    Self::integrity_bool(attributes.configurable),
                ),
            ] {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        let key = self.coerce_js_string(p, key_value)?;
        if key.host_string() == "length"
            && let Some(Cell::Array { elements, .. }) = self.heap.get(target)
            && (!self.argument_objects.contains(&target)
                || self.own_property(target, self.length_atom).is_some())
        {
            let length = self.heap.sparse_length(target).unwrap_or(elements.len());
            let descriptor = self.object();
            for (name, value) in [
                ("value", Value::number(length as f64)),
                ("writable", Value::TRUE),
                ("enumerable", Value::FALSE),
                (
                    "configurable",
                    Self::integrity_bool(self.argument_objects.contains(&target)),
                ),
            ] {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        if let Some(index) =
            super::object_static::array_index(key.host_string()).map(|index| index as usize)
        {
            let atom = self.intern_js_atom(&key);
            let attributes = self
                .descriptors
                .get(&(target, PropertyKey::string(atom)))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            if attributes.accessor {
                let descriptor = self.object();
                for (name, value) in [
                    ("get", attributes.getter.unwrap_or(Value::UNDEFINED)),
                    ("set", attributes.setter.unwrap_or(Value::UNDEFINED)),
                    ("enumerable", Self::integrity_bool(attributes.enumerable)),
                    (
                        "configurable",
                        Self::integrity_bool(attributes.configurable),
                    ),
                ] {
                    let atom = self.intern_atom(name);
                    self.set_property(descriptor, atom, value)?;
                }
                return Ok(descriptor);
            }
            let value = match self.heap.get(target) {
                Some(Cell::Array { elements, .. }) => elements
                    .get(index)
                    .copied()
                    .filter(|value| !value.is_deleted()),
                _ => None,
            }
            .or_else(|| {
                self.heap
                    .sparse_get(target, index)
                    .filter(|value| !value.is_deleted())
            });
            if let Some(value) = value {
                let descriptor = self.object();
                for (name, value) in [
                    ("value", value),
                    ("writable", Self::integrity_bool(attributes.writable)),
                    ("enumerable", Self::integrity_bool(attributes.enumerable)),
                    (
                        "configurable",
                        Self::integrity_bool(attributes.configurable),
                    ),
                ] {
                    let atom = self.intern_atom(name);
                    self.set_property(descriptor, atom, value)?;
                }
                return Ok(descriptor);
            }
        }
        let atom = self.intern_js_atom(&key);
        let Some(value) = self.own_property(target, atom) else {
            return Ok(Value::UNDEFINED);
        };
        let attributes = self
            .descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let descriptor = self.object();
        if attributes.accessor {
            for (name, value) in [
                ("get", attributes.getter.unwrap_or(Value::UNDEFINED)),
                ("set", attributes.setter.unwrap_or(Value::UNDEFINED)),
                ("enumerable", Self::integrity_bool(attributes.enumerable)),
                (
                    "configurable",
                    Self::integrity_bool(attributes.configurable),
                ),
            ] {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        for (name, value) in [
            ("value", value),
            ("writable", Self::integrity_bool(attributes.writable)),
            ("enumerable", Self::integrity_bool(attributes.enumerable)),
            (
                "configurable",
                Self::integrity_bool(attributes.configurable),
            ),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, value)?;
        }
        Ok(descriptor)
    }

    pub(super) fn descriptor_flag(&mut self, descriptor: Value, name: &str) -> bool {
        let atom = self.intern_atom(name);
        self.own_property(descriptor, atom)
            .is_some_and(|value| self.truthy(value))
    }

    pub(super) fn object_get_own_property_descriptors(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. })) {
            let result = self.object();
            for key in self.object_own_key_values(p, source)? {
                let descriptor = self.object_get_own_property_descriptor(p, &[source, key])?;
                if descriptor.is_undefined() {
                    continue;
                }
                match self.heap.get(key).cloned() {
                    Some(Cell::Symbol(_)) => self.set_symbol_property(result, key, descriptor)?,
                    Some(Cell::String(name)) => {
                        let atom = self.intern_js_atom(&name);
                        self.set_property(result, atom, descriptor)?;
                    }
                    _ => unreachable!("validated own property key"),
                }
            }
            return Ok(result);
        }
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let target = self.box_object(target)?;
        let result = self.object();
        for key in self.object_own_key_values(p, target)? {
            let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            match self.heap.get(key).cloned() {
                Some(Cell::Symbol(_)) => self.set_symbol_property(result, key, descriptor)?,
                Some(Cell::String(name)) => {
                    let atom = self.intern_js_atom(&name);
                    self.set_property(result, atom, descriptor)?;
                }
                _ => unreachable!("validated own property key"),
            }
        }
        Ok(result)
    }
}
