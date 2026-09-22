use super::property_key::PropertyKey;
use super::*;
impl<H: Host> Vm<H> {
    pub(super) fn object_delete_property(
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
            let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let key = if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
                key_value
            } else {
                let text = self.coerce_js_string(p, key_value)?;
                self.heap.alloc(Cell::String(text))
            };
            let trap_atom = self.intern_atom("deleteProperty");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target, key])?;
                if !self.truthy(result) {
                    return Ok(Value::FALSE);
                }
                let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
                if !descriptor.is_undefined() && !self.descriptor_flag(descriptor, "configurable") {
                    return Err(JsError(
                        "proxy deleteProperty trap cannot delete a non-configurable property"
                            .into(),
                    ));
                }
                return Ok(Value::TRUE);
            }
        }
        let target = self.proxy_target(source);
        if self.object_data(target).is_none() {
            return Err(JsError("delete target is not an object".into()));
        }
        let key_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(key_value), Some(Cell::Symbol(_))) {
            if self.symbol_property(target, key_value).is_none() {
                return Ok(Value::TRUE);
            }
            if self
                .descriptors
                .get(&(target, PropertyKey::symbol(key_value)))
                .is_some_and(|attributes| !attributes.configurable)
            {
                return Ok(Value::FALSE);
            }
            let property_key = PropertyKey::symbol(key_value);
            self.symbol_properties.remove(&(target, property_key));
            self.descriptors.remove(&(target, property_key));
            if let Some(keys) = self.symbol_property_order.get_mut(&target) {
                keys.retain(|candidate| *candidate != property_key);
            }
            return Ok(Value::TRUE);
        }
        let key = self.coerce_js_string(p, key_value)?;
        if let Some(index) =
            super::object_static::array_index(key.host_string()).map(|index| index as usize)
            && matches!(self.heap.get(target), Some(Cell::Array { .. }))
        {
            return Ok(self.delete_array_index(target, index));
        }
        let atom = self.intern_js_atom(&key);
        let Some(slot) = self
            .shape_slot(self.object_data(target).unwrap().shape(), atom)
            .filter(|_| self.own_property(target, atom).is_some())
        else {
            return Ok(Value::TRUE);
        };
        if !self
            .descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
            .configurable
        {
            return Ok(Value::FALSE);
        }
        self.heap.property_set(target, slot, Value::DELETED);
        self.descriptors
            .remove(&(target, PropertyKey::string(atom)));
        self.invalidate_method_caches();
        Ok(Value::TRUE)
    }

    pub(super) fn object_get_prototype_of(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(value).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("getPrototypeOf");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                if !result.is_null() && self.object_data(result).is_none() {
                    return Err(JsError(
                        "proxy getPrototypeOf trap must return an object or null".into(),
                    ));
                }
                if self
                    .object_data(target)
                    .is_some_and(|object| !object.is_extensible() && object.proto != result)
                {
                    return Err(JsError(
                        "proxy getPrototypeOf trap changed a non-extensible target".into(),
                    ));
                }
                return Ok(result);
            }
        }
        let value = self.proxy_target(value);
        self.object_data(value)
            .map(|object| object.proto)
            .ok_or_else(|| JsError("Object.getPrototypeOf target is not an object".into()))
    }

    pub(super) fn object_set_prototype_of(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        proto: Value,
    ) -> Result<Value, JsError> {
        if let Some(Cell::Proxy {
            target: underlying,
            handler,
            ..
        }) = self.heap.get(target).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("setPrototypeOf");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[underlying, proto])?;
                if !self.truthy(result) {
                    return Err(JsError("proxy setPrototypeOf trap returned false".into()));
                }
                if self
                    .object_data(underlying)
                    .is_some_and(|object| !object.is_extensible() && object.proto != proto)
                {
                    return Err(JsError(
                        "proxy setPrototypeOf trap changed a non-extensible target".into(),
                    ));
                }
                return Ok(target);
            }
        }
        let target = self.proxy_target(target);
        if !proto.is_null() && self.object_data(proto).is_none() {
            return Err(JsError("Object prototype is not an object".into()));
        }
        let Some(current_proto) = self.object_data(target).map(|object| object.proto) else {
            return Err(JsError(
                "Object.setPrototypeOf target is not an object".into(),
            ));
        };
        if self
            .object_data(target)
            .is_some_and(|object| !object.is_extensible())
            && current_proto != proto
        {
            return Err(JsError(
                "cannot change prototype of non-extensible object".into(),
            ));
        }
        let mut cursor = proto;
        while !cursor.is_null() {
            if cursor == target {
                return Err(JsError("prototype chain cycle".into()));
            }
            cursor = self
                .object_data(cursor)
                .map(|object| object.proto)
                .unwrap_or(Value::NULL);
        }
        self.object_data_mut(target)
            .expect("object validated")
            .proto = proto;
        self.invalidate_field_caches();
        self.invalidate_method_caches();
        Ok(target)
    }

    pub(super) fn integrity_bool(value: bool) -> Value {
        if value { Value::TRUE } else { Value::FALSE }
    }

    pub(super) fn check_property_write(
        &self,
        object: Value,
        atom: Atom,
        exists: bool,
    ) -> Result<(), JsError> {
        if !exists
            && self
                .object_data(object)
                .is_some_and(|object| !object.is_extensible())
        {
            return Err(JsError(
                "cannot add property to non-extensible object".into(),
            ));
        }
        if exists
            && self
                .descriptors
                .get(&(object, PropertyKey::string(atom)))
                .is_some_and(|attributes| !attributes.writable)
        {
            return Err(JsError("cannot write non-writable property".into()));
        }
        Ok(())
    }

    pub(super) fn object_prevent_extensions(
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
            let trap_atom = self.intern_atom("preventExtensions");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                if !self.truthy(result) {
                    return Err(JsError(
                        "proxy preventExtensions trap returned false".into(),
                    ));
                }
                if self.object_data(target).is_some_and(Object::is_extensible) {
                    return Err(JsError(
                        "proxy preventExtensions trap did not make target non-extensible".into(),
                    ));
                }
                return Ok(source);
            }
        }
        let target = self.proxy_target(source);
        if let Some(object) = self.object_data_mut(target) {
            object.set_extensible(false);
        }
        Ok(source)
    }

    pub(super) fn object_is_extensible(
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
            let trap_atom = self.intern_atom("isExtensible");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let result = self.call_value(p, trap, handler, &[target])?;
                let value = self.truthy(result);
                if self
                    .object_data(target)
                    .is_some_and(|object| object.is_extensible() != value)
                {
                    return Err(JsError(
                        "proxy isExtensible trap disagreed with target".into(),
                    ));
                }
                return Ok(Self::integrity_bool(value));
            }
        }
        let target = self.proxy_target(source);
        let object = self
            .object_data(target)
            .ok_or_else(|| JsError("isExtensible target is not an object".into()))?;
        Ok(Self::integrity_bool(object.is_extensible()))
    }

    pub(super) fn object_set_integrity(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        freeze: bool,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. })) {
            self.object_prevent_extensions(p, args)?;
            let configurable_atom = self.intern_atom("configurable");
            let writable_atom = self.intern_atom("writable");
            for key in self.object_own_key_values(p, source)? {
                let descriptor = self.object_get_own_property_descriptor(p, &[source, key])?;
                if descriptor.is_undefined() {
                    continue;
                }
                self.set_property(descriptor, configurable_atom, Value::FALSE)?;
                if freeze {
                    self.set_property(descriptor, writable_atom, Value::FALSE)?;
                }
                self.object_define_property(p, &[source, key, descriptor])?;
            }
            if freeze {
                let target = self.proxy_target(source);
                if let Some(object) = self.object_data_mut(target) {
                    object.set_frozen(true);
                }
            }
            return Ok(source);
        }
        let target = self.proxy_target(source);
        if self.object_data(target).is_none() {
            return Ok(target);
        }
        if let Some(object) = self.object_data_mut(target) {
            object.set_extensible(false);
            if freeze {
                object.set_frozen(true);
            }
        }
        let mut keys = self
            .object_data(target)
            .map(|data| {
                self.ordered_shape(data)
                    .into_iter()
                    .filter(|(_, slot)| self.heap.property_get(data, *slot).is_some())
                    .map(|(atom, _)| atom)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        keys.extend(self.array_integrity_atoms(target));
        for atom in keys {
            let attributes = self
                .descriptors
                .entry((target, PropertyKey::string(atom)))
                .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
        }
        let symbols = self
            .symbol_property_order
            .get(&target)
            .cloned()
            .unwrap_or_default();
        for symbol in symbols {
            let attributes = self
                .descriptors
                .entry((target, symbol))
                .or_insert(DEFAULT_PROPERTY_ATTRIBUTES);
            attributes.configurable = false;
            if freeze {
                attributes.writable = false;
            }
        }
        Ok(target)
    }

    pub(super) fn object_is_integrity_level(&self, args: &[Value], freeze: bool) -> bool {
        let target = self.proxy_target(args.first().copied().unwrap_or(Value::UNDEFINED));
        let Some(data) = self.object_data(target) else {
            return true;
        };
        if data.is_extensible() || freeze && !data.is_frozen() {
            return false;
        }
        let named_ok = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(_, slot)| self.heap.property_get(data, *slot).is_some())
            .all(|(atom, _)| {
                let attributes = self
                    .descriptors
                    .get(&(target, PropertyKey::string(atom)))
                    .copied()
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            });
        let arrays_ok = self.array_is_integrity_level(target, freeze);
        let symbols_ok = self
            .symbol_property_order
            .get(&target)
            .into_iter()
            .flatten()
            .all(|symbol| {
                let attributes = self
                    .descriptors
                    .get(&(target, *symbol))
                    .copied()
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            });
        named_ok && arrays_ok && symbols_ok
    }

    pub(super) fn validate_proxy_define_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        key: Value,
        descriptor: Value,
    ) -> Result<(), JsError> {
        let current = self.object_get_own_property_descriptor(p, &[target, key])?;
        if current.is_undefined() {
            if self
                .object_data(target)
                .is_some_and(|object| !object.is_extensible())
            {
                return Err(JsError(
                    "proxy defineProperty trap added a property to a non-extensible target".into(),
                ));
            }
            return Ok(());
        }
        if self.descriptor_flag(current, "configurable") {
            return Ok(());
        }
        let enumerable = self.intern_atom("enumerable");
        if self.descriptor_flag(descriptor, "configurable")
            || (self.own_property(descriptor, enumerable).is_some()
                && self.descriptor_flag(descriptor, "enumerable")
                    != self.descriptor_flag(current, "enumerable"))
        {
            return Err(JsError(
                "proxy defineProperty trap changed a non-configurable target property".into(),
            ));
        }
        let value = self.intern_atom("value");
        let writable = self.intern_atom("writable");
        let get = self.intern_atom("get");
        let set = self.intern_atom("set");
        let descriptor_data = self.own_property(descriptor, value).is_some()
            || self.own_property(descriptor, writable).is_some();
        let descriptor_accessor = self.own_property(descriptor, get).is_some()
            || self.own_property(descriptor, set).is_some();
        let current_data = self.own_property(current, value).is_some()
            || self.own_property(current, writable).is_some();
        if descriptor_data && !current_data || descriptor_accessor && current_data {
            return Err(JsError(
                "proxy defineProperty trap changed a non-configurable property kind".into(),
            ));
        }
        if descriptor_data && current_data {
            if !self.descriptor_flag(current, "writable")
                && (self.descriptor_flag(descriptor, "writable")
                    || self.own_property(descriptor, value).is_some_and(|next| {
                        self.own_property(current, value)
                            .is_some_and(|previous| !self.same_value(previous, next))
                    }))
            {
                return Err(JsError(
                    "proxy defineProperty trap changed a non-writable target property".into(),
                ));
            }
        } else if descriptor_accessor && !current_data {
            for (name, text) in [("get", "getter"), ("set", "setter")] {
                let atom = self.intern_atom(name);
                if let Some(next) = self.own_property(descriptor, atom)
                    && self
                        .own_property(current, atom)
                        .is_some_and(|previous| !self.same_value(previous, next))
                {
                    return Err(JsError(
                        format!(
                            "proxy defineProperty trap changed a non-configurable target {text}"
                        )
                        .into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
