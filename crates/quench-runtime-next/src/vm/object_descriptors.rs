use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    fn own_data_descriptor(
        &mut self,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<Value, JsError> {
        let descriptor = self.object();
        for (name, value) in [
            ("value", value),
            ("writable", Self::integrity_bool(writable)),
            ("enumerable", Self::integrity_bool(enumerable)),
            ("configurable", Self::integrity_bool(configurable)),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, value)?;
        }
        Ok(descriptor)
    }

    pub(super) fn canonical_typed_array_index(key: &str) -> Option<usize> {
        let index = key.parse::<usize>().ok()?;
        (index.to_string() == key).then_some(index)
    }

    pub(super) fn define_typed_array_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        index: usize,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        let getter = self.descriptor_field(p, descriptor, "get")?;
        let setter = self.descriptor_field(p, descriptor, "set")?;
        let value = self.descriptor_field(p, descriptor, "value")?;
        let writable = self.descriptor_field(p, descriptor, "writable")?;
        let enumerable = self.descriptor_field(p, descriptor, "enumerable")?;
        let configurable = self.descriptor_field(p, descriptor, "configurable")?;
        let invalid_kind = getter.is_some() || setter.is_some();
        let invalid_attributes = writable.is_some_and(|value| !self.truthy(value))
            || enumerable.is_some_and(|value| !self.truthy(value))
            || configurable.is_some_and(|value| !self.truthy(value));
        if invalid_kind || invalid_attributes {
            return Err(self.type_error(
                p,
                "cannot define incompatible typed array index descriptor".into(),
            ));
        }
        if self
            .typed_array_length(target)
            .is_none_or(|length| index >= length)
        {
            return Err(self.type_error(
                p,
                "cannot define a property on an out-of-bounds typed array".into(),
            ));
        }
        if let Some(value) = value
            && !self.typed_array_set(p, target, index, value)?
        {
            return Err(self.type_error(
                p,
                "cannot define a property on an out-of-bounds typed array".into(),
            ));
        }
        Ok(target)
    }

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
                let key =
                    self.to_property_key(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let result = self.call_value(p, trap, handler, &[target, key])?;
                if result.is_undefined() {
                    let target_descriptor =
                        self.object_get_own_property_descriptor(p, &[target, key])?;
                    self.validate_proxy_get_own_property_descriptor(
                        p,
                        target,
                        target_descriptor,
                        None,
                    )?;
                    return Ok(Value::UNDEFINED);
                }
                if self.object_data(result).is_none() {
                    return Err(self.type_error(
                        p,
                        "proxy getOwnPropertyDescriptor trap must return an object or undefined"
                            .into(),
                    ));
                }
                let target_descriptor =
                    self.object_get_own_property_descriptor(p, &[target, key])?;
                self.validate_proxy_get_own_property_descriptor(
                    p,
                    target,
                    target_descriptor,
                    Some(result),
                )?;
                return Ok(result);
            }
            if trap.is_undefined() || trap.is_null() {
                return self.object_get_own_property_descriptor(
                    p,
                    &[target, args.get(1).copied().unwrap_or(Value::UNDEFINED)],
                );
            }
            return Err(self.type_error(
                p,
                "proxy getOwnPropertyDescriptor trap is not callable".into(),
            ));
        }
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        let target = self.box_object(target)?;
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
            let Some(value) = self.symbol_property(target, key_value) else {
                return Ok(Value::UNDEFINED);
            };
            let attributes = self
                .property_attributes(target, PropertyKey::symbol(key_value))
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
        let key = self.to_property_key(p, key_value)?;
        if matches!(self.heap.get(key), Some(Cell::Symbol(_))) {
            let Some(value) = self.symbol_property(target, key) else {
                return Ok(Value::UNDEFINED);
            };
            let attributes = self
                .property_attributes(target, PropertyKey::symbol(key))
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
            let descriptor = self.object();
            let fields = if attributes.accessor {
                vec![
                    ("get", attributes.getter.unwrap_or(Value::UNDEFINED)),
                    ("set", attributes.setter.unwrap_or(Value::UNDEFINED)),
                ]
            } else {
                vec![
                    ("value", value),
                    ("writable", Self::integrity_bool(attributes.writable)),
                ]
            };
            for (name, value) in fields.into_iter().chain([
                ("enumerable", Self::integrity_bool(attributes.enumerable)),
                (
                    "configurable",
                    Self::integrity_bool(attributes.configurable),
                ),
            ]) {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        let Some(Cell::String(key)) = self.heap.get(key).cloned() else {
            unreachable!("ToPropertyKey returns a string or symbol")
        };
        let atom = self.intern_js_atom(&key);
        self.evaluate_deferred_namespace_for_key(p, target, Some(PropertyKey::string(atom)))?;
        if key.host_string() == "length"
            && let Some(Cell::Array { elements, .. }) = self.heap.get(target)
            && !self
                .object_data(target)
                .is_some_and(Object::is_arguments_object)
        {
            let length = self.heap.sparse_length(target).unwrap_or(elements.len());
            let length_attributes = self
                .descriptors
                .get(&(target, PropertyKey::string(self.length_atom)))
                .copied()
                .unwrap_or(PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                });
            let descriptor = self.object();
            for (name, value) in [
                ("value", Value::number(length as f64)),
                ("writable", Self::integrity_bool(length_attributes.writable)),
                ("enumerable", Value::FALSE),
                (
                    "configurable",
                    Self::integrity_bool(length_attributes.configurable),
                ),
            ] {
                let atom = self.intern_atom(name);
                self.set_property(descriptor, atom, value)?;
            }
            return Ok(descriptor);
        }
        if matches!(self.heap.get(target), Some(Cell::TypedArray { .. }))
            && let Some(index) = Self::canonical_typed_array_index(key.host_string())
            && self
                .typed_array_length(target)
                .is_some_and(|length| index < length)
        {
            let value = self
                .typed_array_get(target, index)
                .unwrap_or(Value::UNDEFINED);
            return self.own_data_descriptor(value, true, true, true);
        }
        if let Some(index) =
            super::object_static::array_index(key.host_string()).map(|index| index as usize)
        {
            let atom = self.intern_js_atom(&key);
            let attributes = self
                .property_attributes(target, PropertyKey::string(atom))
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
        let module_binding = self.module_binding_value(target, atom);
        if module_binding.is_some_and(Value::is_deleted) {
            return Err(self.reference_error(
                p,
                format!(
                    "Cannot access '{}' before initialization",
                    self.atom_name(atom)
                ),
            ));
        }
        let Some(value) = module_binding.or_else(|| self.own_property(target, atom)) else {
            return Ok(Value::UNDEFINED);
        };
        let attributes = self
            .property_attributes(target, PropertyKey::string(atom))
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let writable = self
            .object_data(target)
            .is_some_and(Object::is_module_namespace)
            || attributes.writable;
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
            ("writable", Self::integrity_bool(writable)),
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

    fn validate_proxy_get_own_property_descriptor(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        target_descriptor: Value,
        result: Option<Value>,
    ) -> Result<(), JsError> {
        let extensible = self.object_is_extensible(p, &[target])?;
        let extensible = self.truthy(extensible);
        let Some(result) = result else {
            if !target_descriptor.is_undefined()
                && (!extensible || !self.descriptor_flag(target_descriptor, "configurable"))
            {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap cannot hide a target property".into(),
                ));
            }
            return Ok(());
        };
        let result_configurable = self.descriptor_field(p, result, "configurable")?;
        if target_descriptor.is_undefined() {
            if !extensible || result_configurable.is_some_and(|value| !self.truthy(value)) {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap added an incompatible property".into(),
                ));
            }
            return Ok(());
        }
        let target_configurable = self.descriptor_flag(target_descriptor, "configurable");
        if result_configurable.is_some_and(|value| !self.truthy(value)) && target_configurable {
            return Err(self.type_error(
                p,
                "proxy getOwnPropertyDescriptor trap made a property non-configurable".into(),
            ));
        }
        if !target_configurable {
            if result_configurable.is_some_and(|value| self.truthy(value)) {
                return Err(self.type_error(
                    p,
                    "proxy getOwnPropertyDescriptor trap changed configurability".into(),
                ));
            }
            for field in ["value", "writable", "get", "set", "enumerable"] {
                let Some(expected) = self.descriptor_field(p, target_descriptor, field)? else {
                    continue;
                };
                if let Some(actual) = self.descriptor_field(p, result, field)?
                    && !self.same_value(expected, actual)
                {
                    return Err(self.type_error(
                        p,
                        "proxy getOwnPropertyDescriptor trap changed a non-configurable property"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
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
